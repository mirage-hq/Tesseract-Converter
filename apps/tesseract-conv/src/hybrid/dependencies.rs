//! Close source references before replacing contiguous root picture scopes.
use super::owners::{masks, Owners};
use anyhow::{ensure, Context};
use fx_schema::effect::{EffectData, EffectPayload, LayerEffect};
use fx_schema::{BlendMode, EditableFxCompositionDocument, Layer, LayerData};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) struct GraphDependencies {
    neighbors: Vec<BTreeSet<usize>>,
}

impl GraphDependencies {
    pub(super) fn new(
        document: &EditableFxCompositionDocument,
        owners: &Owners,
    ) -> anyhow::Result<Self> {
        let roots = document.composition().layers();
        let mut graph = Self {
            neighbors: vec![BTreeSet::new(); roots.len()],
        };
        let mut backdrop_component_started = false;
        for (root, layer) in roots.iter().enumerate() {
            graph.references(layer, root, owners, false)?;
            let needs_backdrop = needs_backdrop(layer);
            debug_assert!(
                !matches!(layer.data(), LayerData::Audio(_)) || !needs_backdrop,
                "standalone audio cannot require a picture backdrop"
            );
            if needs_backdrop && !backdrop_component_started {
                for (below, backdrop) in roots.iter().enumerate().skip(root + 1) {
                    // Standalone audio contributes no backdrop pixels and stays
                    // on Premiere's independently exported sound tracks.
                    if !matches!(backdrop.data(), LayerData::Audio(_)) {
                        graph.connect(root, below);
                    }
                }
                // Every later backdrop-dependent visual root is one of the
                // lower roots just connected, so repeating its lower edges
                // cannot change connected components.
                backdrop_component_started = true;
            }
        }
        let entries = document.composition().dynamics().entries();
        ensure!(
            entries.len() <= 8192,
            "hybrid animation entry limit exceeded"
        );
        let mut incoming = BTreeMap::new();
        let mut outgoing = BTreeMap::<_, BTreeSet<_>>::new();
        for entry in entries {
            ensure!(
                incoming.insert(&entry.target, 0usize).is_none(),
                "duplicate animated target"
            );
        }
        let mut edges = 0;
        for entry in entries {
            let root = owners.target(&entry.target)?;
            for dependency in &entry.dependencies {
                edges += 1;
                ensure!(edges <= 16_384, "hybrid dependency limit exceeded");
                graph.connect(root, owners.target(dependency)?);
                if incoming.contains_key(dependency)
                    && outgoing
                        .entry(dependency)
                        .or_default()
                        .insert(&entry.target)
                {
                    *incoming
                        .get_mut(&entry.target)
                        .context("missing animation target")? += 1;
                }
            }
            for reference in entry.layer_refs.values() {
                edges += 1;
                ensure!(edges <= 16_384, "hybrid dependency limit exceeded");
                graph.connect(root, owners.layer(reference.layer_id)?);
            }
            // A random seed's literal address is not a value dependency.
        }
        let mut ready: VecDeque<_> = incoming
            .iter()
            .filter_map(|(target, n)| (*n == 0).then_some(*target))
            .collect();
        let mut visited = 0;
        while let Some(target) = ready.pop_front() {
            visited += 1;
            for dependent in outgoing.get(target).into_iter().flatten() {
                let n = incoming
                    .get_mut(dependent)
                    .context("missing animation dependency")?;
                *n -= 1;
                if *n == 0 {
                    ready.push_back(*dependent);
                }
            }
        }
        ensure!(
            visited == entries.len(),
            "cyclic animated property dependencies"
        );
        Ok(graph)
    }

    fn connect(&mut self, a: usize, b: usize) {
        self.neighbors[a].insert(b);
        self.neighbors[b].insert(a);
    }

    fn references(
        &mut self,
        layer: &Layer,
        root: usize,
        owners: &Owners,
        structurally_nested: bool,
    ) -> anyhow::Result<()> {
        // FX uses the structural container ahead of the stored parent address.
        // That container is already in this root; a copied child's stale parent
        // must neither reject conversion nor absorb an unrelated picture scope.
        if let Some(parent) = layer.parent_id().filter(|_| !structurally_nested) {
            self.connect(root, owners.layer(parent)?);
        }
        if let Some(matte) = compositing(layer).1 {
            self.connect(root, owners.layer(matte.layer)?);
        }
        for mask in masks(layer) {
            if let Some(source) = mask.layer {
                self.connect(root, owners.layer(source)?);
            }
        }
        for effect in layer.effects() {
            let payload = match effect.data() {
                EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
            };
            if let EffectPayload::Known(LayerEffect::CustomShader { texture_inputs, .. }) = payload
            {
                for input in texture_inputs {
                    if let Some(source) = input.source() {
                        self.connect(root, owners.layer(source)?);
                    }
                }
            }
        }
        for child in layer.child_layers().into_iter().flatten() {
            self.references(child, root, owners, true)?;
        }
        Ok(())
    }

    /// Close references and intervals in both FX and native packing order.
    /// Premiere defers matte providers, so a contiguous FX interval can still
    /// straddle independent pictures in the retained packing recipe.
    pub(super) fn scopes(
        &self,
        seeds: &BTreeSet<usize>,
        packing_order: &[usize],
    ) -> Vec<BTreeSet<usize>> {
        let mut scopes: Vec<BTreeSet<usize>> = Vec::new();
        for &seed in seeds {
            let mut scope = self.connected_scope(seed, packing_order);
            // A later matte scope can absorb an earlier independent seed. Plan
            // all intervals before staging so no boundary is replaced twice.
            loop {
                let previous_len = scope.len();
                scopes.retain(|existing| {
                    if existing.is_disjoint(&scope) {
                        true
                    } else {
                        scope.extend(existing);
                        false
                    }
                });
                if scope.len() == previous_len {
                    break;
                }
            }
            // Overlapping closed intervals have a closed union in both orders.
            scopes.push(scope);
        }
        scopes.sort_by_key(|scope| scope.first().copied());
        scopes
    }

    fn connected_scope(&self, seed: usize, packing_order: &[usize]) -> BTreeSet<usize> {
        let mut result = self.connected(seed);
        loop {
            let previous_len = result.len();
            let first = *result.first().expect("scope contains its seed");
            let last = *result.last().expect("scope contains its seed");
            result.extend(first..=last);
            let first_slot = packing_order.iter().position(|root| result.contains(root));
            let last_slot = packing_order.iter().rposition(|root| result.contains(root));
            if let (Some(first_slot), Some(last_slot)) = (first_slot, last_slot) {
                result.extend(&packing_order[first_slot..=last_slot]);
            }
            let roots: Vec<_> = result.iter().copied().collect();
            for root in roots {
                result.extend(self.connected(root));
            }
            if result.len() == previous_len {
                return result;
            }
        }
    }

    pub(super) fn connected(&self, seed: usize) -> BTreeSet<usize> {
        let mut result = BTreeSet::from([seed]);
        let mut pending = vec![seed];
        while let Some(root) = pending.pop() {
            for &next in &self.neighbors[root] {
                if result.insert(next) {
                    pending.push(next);
                }
            }
        }
        result
    }
}

/// A Normal container can pass a child's blend against roots underneath it.
/// Treat all nested containers as potentially pass-through: effects, opacity,
/// and their animation may change isolation, so over-capture is safer than
/// silently dropping a picture the linked composition needs.
fn needs_backdrop(layer: &Layer) -> bool {
    let (blend, _) = compositing(layer);
    blend != BlendMode::Normal
        || matches!(layer.data(), LayerData::Adjustment(_))
        || layer
            .child_layers()
            .is_some_and(|children| children.iter().any(needs_backdrop))
}

/// Owners whose native parent or matte depends on rejected media sources.
pub(super) fn media_dependents(
    layers: &[Layer],
    sources: &BTreeSet<fx_schema::LayerId>,
) -> BTreeSet<fx_schema::LayerId> {
    fn visit(
        layers: &[Layer],
        affected: &mut BTreeSet<fx_schema::LayerId>,
        parent_affected: bool,
        structurally_nested: bool,
    ) {
        for layer in layers {
            if parent_affected
                || layer
                    .parent_id()
                    .filter(|_| !structurally_nested)
                    .is_some_and(|id| affected.contains(&id))
                || compositing(layer)
                    .1
                    .is_some_and(|matte| affected.contains(&matte.layer))
            {
                affected.insert(layer.id());
            }
            if let Some(children) = layer.child_layers() {
                let parent_affected = affected.contains(&layer.id());
                visit(children, affected, parent_affected, true);
            }
        }
    }
    let mut affected = sources.clone();
    loop {
        let count = affected.len();
        visit(layers, &mut affected, false, false);
        if affected.len() == count {
            return affected;
        }
    }
}

fn compositing(layer: &Layer) -> (BlendMode, Option<&fx_schema::layer::TrackMatte>) {
    match layer.data() {
        LayerData::Media(v) => (v.blend_mode, v.track_matte.as_ref()),
        LayerData::Video(v) => (v.blend_mode, v.track_matte.as_ref()),
        LayerData::Image(v) => (v.blend_mode, v.track_matte.as_ref()),
        LayerData::Text(v) => (v.blend_mode, v.track_matte.as_ref()),
        LayerData::Rect(v) => (v.blend_mode, v.track_matte.as_ref()),
        LayerData::Shape(v) => (v.blend_mode, v.track_matte.as_ref()),
        LayerData::Group(v) => (v.blend_mode, v.track_matte.as_ref()),
        LayerData::BooleanOperation(v) => (v.blend_mode, v.track_matte.as_ref()),
        LayerData::Adjustment(v) => (v.blend_mode, v.track_matte.as_ref()),
        _ => (BlendMode::Normal, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn rect(id: u64, blend: &str) -> Value {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../../../crates/aftereffects_file/tests/fixtures/hybrid/rect-identity.fx.json"
        ))
        .unwrap();
        let mut layer = value["composition"]["layers"][0].take();
        layer["id"] = json!(id);
        layer["blendMode"] = json!(blend);
        layer
    }

    fn group(id: u64, children: Vec<Value>) -> Value {
        json!({
            "type": "Group", "id": id, "name": "nested group", "parent": null,
            "playback": {
                "type": "windowed", "inputRange": {"start": 0, "duration": 2000},
                "mapping": {"type": "linear", "input": {"start": 0, "duration": 2000},
                    "output": {"start": 0, "duration": 2000}},
                "inputOffsetMs": 0
            },
            "transform": rect(999, "normal")["transform"],
            "layers": children,
        })
    }

    fn document(roots: Vec<Value>, dynamics: Vec<Value>) -> EditableFxCompositionDocument {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../../../crates/aftereffects_file/tests/fixtures/hybrid/rect-identity.fx.json"
        ))
        .unwrap();
        value["composition"]["layers"] = json!(roots);
        value["composition"]["dynamics"]["entries"] = json!(dynamics);
        EditableFxCompositionDocument::from_json_value(value).unwrap()
    }

    fn closure(roots: Vec<Value>, dynamics: Vec<Value>) -> BTreeSet<usize> {
        let document = document(roots, dynamics);
        let owners = Owners::new(document.composition().layers()).unwrap();
        GraphDependencies::new(&document, &owners)
            .unwrap()
            .connected(0)
    }

    fn audio(id: u64) -> Value {
        json!({
            "type": "Audio", "id": id, "name": "Sound", "parent": null,
            "playback": {
                "type": "windowed", "inputRange": {"start": 0, "duration": 2000},
                "mapping": {"type": "linear", "input": {"start": 0, "duration": 2000},
                    "output": {"start": 0, "duration": 2000}},
                "inputOffsetMs": 0
            },
            "sourceRange": {"start": 0, "duration": 2000},
            "sourceIntrinsicDuration": 2000, "volume": 0.5,
            "source": {"assetId": "music"}
        })
    }

    #[test]
    fn rejected_media_dependencies_follow_mattes_parents_and_structural_children() {
        let mut matte = rect(2, "normal");
        matte["trackMatte"] = json!({"layer":1,"mode":"alpha"});
        let mut consumer = group(3, vec![rect(4, "normal")]);
        consumer["trackMatte"] = json!({"layer":2,"mode":"alpha"});
        let mut parented = rect(5, "normal");
        parented["parent"] = json!(4);
        let mut unrelated = rect(6, "normal");
        unrelated["parent"] = json!(1);
        let document = document(
            vec![
                parented,
                consumer,
                matte,
                rect(1, "normal"),
                group(7, vec![unrelated]),
            ],
            vec![],
        );
        assert_eq!(
            media_dependents(
                document.composition().layers(),
                &BTreeSet::from([fx_schema::LayerId::new(1)])
            ),
            (1..=5).map(fx_schema::LayerId::new).collect()
        );
    }

    #[test]
    fn normal_group_child_screen_needs_external_picture_backdrop_not_audio() {
        let mut child = rect(2, "screen");
        child["parent"] = json!(1);
        assert_eq!(
            closure(
                vec![group(1, vec![child]), audio(3), rect(4, "normal")],
                vec![]
            ),
            BTreeSet::from([0, 2])
        );
    }

    #[test]
    fn nested_normal_groups_propagate_descendant_backdrop_need() {
        let mut screen = rect(3, "screen");
        screen["parent"] = json!(2);
        let mut inner = group(2, vec![screen]);
        inner["parent"] = json!(1);
        assert_eq!(
            closure(vec![group(1, vec![inner]), rect(4, "normal")], vec![]),
            BTreeSet::from([0, 1])
        );
    }

    #[test]
    fn effect_and_animated_opacity_do_not_hide_descendant_backdrop_dependency() {
        let mut child = rect(2, "screen");
        child["parent"] = json!(1);
        let mut root = group(1, vec![child]);
        root["effects"] = json!([{
            "id": 10, "effect": {"type": "gaussianBlur", "blurriness": 8}
        }]);
        root["transform"]["opacity"] = json!(40);
        let opacity = json!({
            "target": {"kind": "layer", "layerId": 1, "propertyType": "opacity"},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                {"id": "opacity-0", "layerTime": 0,
                 "value": {"type": "float", "value": 100}, "easing": {"type": "linear"}},
                {"id": "opacity-1", "layerTime": 1000,
                 "value": {"type": "float", "value": 40}, "easing": {"type": "linear"}}
            ]},
            "dependencies": [], "layerRefs": {}
        });
        assert_eq!(
            closure(vec![root, rect(3, "normal")], vec![opacity]),
            BTreeSet::from([0, 1])
        );
    }

    #[test]
    fn isolated_container_is_conservatively_included() {
        let mut child = rect(2, "screen");
        child["parent"] = json!(1);
        let mut isolated = group(1, vec![child]);
        isolated["blendMode"] = json!("multiply");
        isolated["parent"] = json!(4);
        // Even an isolating nested group should not make us assume its
        // dynamic/effect configuration will never need the outer backdrop.
        assert_eq!(
            closure(vec![group(4, vec![isolated]), rect(3, "normal")], vec![]),
            BTreeSet::from([0, 1])
        );
    }

    #[test]
    fn structural_parents_override_stale_nested_parent_addresses() {
        for stale_parent in [99, 3] {
            let mut child = rect(2, "normal");
            child["parent"] = json!(stale_parent);
            let mut nested = group(4, vec![child]);
            nested["parent"] = json!(stale_parent);
            assert_eq!(
                closure(vec![group(1, vec![nested]), rect(3, "normal")], vec![]),
                BTreeSet::from([0]),
                "nested parent {stale_parent} is superseded by its structural container"
            );
        }
        let mut root = rect(1, "normal");
        root["parent"] = json!(3);
        assert_eq!(
            closure(vec![root, rect(3, "normal")], vec![]),
            BTreeSet::from([0, 1])
        );
        let mut dangling_root = rect(1, "normal");
        dangling_root["parent"] = json!(99);
        let document = document(vec![dangling_root], vec![]);
        let owners = Owners::new(document.composition().layers()).unwrap();
        assert!(GraphDependencies::new(&document, &owners).is_err());
    }

    #[test]
    fn normal_children_do_not_absorb_unrelated_backdrop() {
        let mut child = rect(2, "normal");
        child["parent"] = json!(1);
        assert_eq!(
            closure(vec![group(1, vec![child]), rect(3, "normal")], vec![]),
            BTreeSet::from([0])
        );
    }

    #[test]
    fn sparse_backdrop_edges_match_dense_components_for_bounded_root_combinations() {
        const ROOTS: usize = 5;
        for states in 0..3usize.pow(ROOTS as u32) {
            let mut encoded = states;
            let mut roots = Vec::with_capacity(ROOTS);
            for index in 0..ROOTS {
                roots.push(match encoded % 3 {
                    0 => rect(index as u64 + 1, "normal"),
                    1 => rect(index as u64 + 1, "screen"),
                    _ => audio(index as u64 + 1),
                });
                encoded /= 3;
            }

            let visual: Vec<_> = roots
                .iter()
                .enumerate()
                .filter_map(|(index, layer)| (layer["type"] != "Audio").then_some(index))
                .collect();
            if visual.len() >= 2 {
                let consumer = visual[0];
                let source = visual[visual.len() - 1];
                roots[consumer]["trackMatte"] =
                    json!({"mode": "alpha", "layer": source as u64 + 1});
                roots[consumer]["masks"] = json!([{
                    "id": 10_000 + states as u64,
                    "mode": "add", "inverted": false, "layer": source as u64 + 1,
                    "feather": [0, 0], "expansion": 0, "opacity": 1
                }]);
            }

            let document = document(roots, vec![]);
            let owners = Owners::new(document.composition().layers()).unwrap();
            let sparse = GraphDependencies::new(&document, &owners).unwrap();
            let mut dense = GraphDependencies {
                neighbors: vec![BTreeSet::new(); ROOTS],
            };
            for (root, layer) in document.composition().layers().iter().enumerate() {
                dense.references(layer, root, &owners, false).unwrap();
                if needs_backdrop(layer) {
                    for (below, backdrop) in document
                        .composition()
                        .layers()
                        .iter()
                        .enumerate()
                        .skip(root + 1)
                    {
                        if !matches!(backdrop.data(), LayerData::Audio(_)) {
                            dense.connect(root, below);
                        }
                    }
                }
            }
            for seed in 0..ROOTS {
                assert_eq!(
                    sparse.connected(seed),
                    dense.connected(seed),
                    "states={states}, seed={seed}"
                );
            }
        }
    }

    #[test]
    fn sparse_backdrop_stack_retains_only_linear_neighbor_count() {
        let count = 512;
        let roots = (0..count)
            .map(|index| rect(index as u64 + 1, "screen"))
            .collect();
        let document = document(roots, vec![]);
        let owners = Owners::new(document.composition().layers()).unwrap();
        let graph = GraphDependencies::new(&document, &owners).unwrap();
        assert_eq!(
            graph.neighbors.iter().map(BTreeSet::len).sum::<usize>(),
            2 * (count - 1)
        );
    }

    #[test]
    fn deferred_matte_scope_closes_packing_and_new_source_dependencies() {
        // FX order: two foreground edges, matte, consumer, backdrop.
        // Native order: backdrop, consumer, edges, deferred matte.
        let mut consumer = rect(4, "normal");
        consumer["trackMatte"] = json!({"mode": "alpha", "layer": 3});
        let document = document(
            vec![
                rect(1, "normal"),
                rect(2, "normal"),
                rect(3, "normal"),
                consumer,
                rect(5, "normal"),
            ],
            vec![],
        );
        let owners = Owners::new(document.composition().layers()).unwrap();
        let graph = GraphDependencies::new(&document, &owners).unwrap();
        assert_eq!(
            graph.connected_scope(3, &[4, 3, 1, 0, 2]),
            BTreeSet::from([0, 1, 2, 3])
        );

        // The foreground seed is planned first but must not be replaced twice.
        assert_eq!(
            graph.scopes(&BTreeSet::from([0, 3, 4]), &[4, 3, 1, 0, 2]),
            vec![BTreeSet::from([0, 1, 2, 3]), BTreeSet::from([4])]
        );

        // An edge's external parent must also be captured after interval growth.
        let mut roots = document
            .composition()
            .layers()
            .iter()
            .map(|layer| serde_json::to_value(layer).unwrap())
            .collect::<Vec<_>>();
        roots[0]["parent"] = json!(5);
        let document = self::document(roots, vec![]);
        let owners = Owners::new(document.composition().layers()).unwrap();
        let graph = GraphDependencies::new(&document, &owners).unwrap();
        assert_eq!(
            graph.connected_scope(3, &[4, 3, 1, 0, 2]),
            BTreeSet::from([0, 1, 2, 3, 4])
        );
    }

    #[test]
    fn root_adjustment_still_needs_backdrop() {
        let mut adjustment: Value = serde_json::from_str(include_str!(
            "../../../../crates/aftereffects_file/tests/fixtures/adjustment/fx_export/adjustment-keys.fx.json"
        ))
        .unwrap();
        let adjustment = adjustment["composition"]["layers"][1].take();
        assert_eq!(adjustment["type"], "Adjustment");
        assert_eq!(
            closure(vec![adjustment, rect(3, "normal")], vec![]),
            BTreeSet::from([0, 1])
        );
    }
}
