//! Native numeric snapshots. No cache value is substituted for a dependency error.

use fx_schema::animator::PropertyKeyframeEasing;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

use crate::{
    effects::{definitions, native},
    expression_samples::{PropertyIdentity, ShapePathSegment},
    properties::{self, NumericProperty},
    rifx::Chunk,
    structure::{Composition, ItemKind, Layer, ProjectItem},
};

use super::syntax;

#[derive(Serialize)]
pub(super) struct Model {
    pub comps: Vec<Comp>,
    pub properties: Vec<Property>,
    pub allow_foreign: bool,
}
#[derive(Serialize)]
pub(super) struct Comp {
    pub id: u32,
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub duration: f64,
    pub fps: f64,
    pub display_start: f64,
    pub layers: Vec<LayerModel>,
    /// A camera layer exists; 3D layer space then needs its projection.
    pub has_camera: bool,
}
#[derive(Serialize)]
pub(super) struct LayerModel {
    pub id: u32,
    pub name: String,
    pub index: usize,
    pub three_d: bool,
    pub in_point: Option<f64>,
    pub out_point: Option<f64>,
    pub transform: BTreeMap<String, usize>,
    /// Separated Position dimension slots; `transform.position` composes them.
    pub separated_position: Option<Vec<usize>>,
    pub effects: Vec<Effect>,
    /// Source dimensions (`width`/`height`), visibility and parent layer ID.
    pub width: f64,
    pub height: f64,
    /// AE's Anchor Point when the leaf is omitted: the source center for
    /// sourced/null layers, otherwise the layer origin.
    pub default_anchor: [f64; 2],
    pub enabled: bool,
    pub parent: Option<u32>,
    pub masks: Vec<MaskModel>,
    /// Shape Contents tree for `content(name)` lookups.
    pub content: Vec<ShapeNode>,
    /// Source Text slot when it carries an enabled expression.
    pub source_text: Option<usize>,
}
#[derive(Serialize)]
pub(super) struct MaskModel {
    pub name: String,
    pub index: u32,
    /// `maskFeather` / `maskOpacity` / `maskExpansion` slots.
    pub properties: BTreeMap<String, usize>,
}
#[derive(Serialize)]
pub(super) struct ShapeNode {
    pub name: String,
    pub match_name: String,
    pub index: u32,
    /// Expression-API member name for numeric leaves, e.g. `strokeWidth`.
    pub alias: Option<String>,
    pub slot: Option<usize>,
    pub children: Vec<ShapeNode>,
}
#[derive(Serialize)]
pub(super) struct Effect {
    pub name: String,
    pub match_name: String,
    pub index: usize,
    pub parameters: Vec<Parameter>,
}
#[derive(Serialize)]
pub(super) struct Parameter {
    pub name: String,
    pub match_name: String,
    pub index: Option<u32>,
    pub slot: usize,
}
#[derive(Serialize)]
pub(super) struct Property {
    pub comp_id: u32,
    pub layer_id: u32,
    pub identity: PropertyIdentity,
    pub initial: Vec<f64>,
    pub keys: Vec<Key>,
    pub expression: Option<String>,
    pub error: Option<String>,
    pub expression_enabled: bool,
    /// Approximations applied to the native keys before evaluation.
    #[serde(skip)]
    pub notes: Vec<String>,
    /// AE's legal value range `[min, max]` (`None` = unbounded) applied to
    /// expression results, e.g. opacities clamp to 0..100.
    pub range: Option<[Option<f64>; 2]>,
    /// Pre-expression Source Text; `Some` makes this a string property.
    pub text: Option<String>,
}
#[derive(Serialize)]
pub(super) struct Key {
    pub time: f64,
    pub value: Vec<f64>,
    pub hold: bool,
    /// Incoming cubic easing `[x1, y1, x2, y2]` per component; `None` is Linear.
    pub ease: Vec<Option<[f64; 4]>>,
}

impl Model {
    pub fn new(
        items: &HashMap<u32, &ProjectItem>,
        current_id: u32,
        current: &Composition,
        allow_foreign: bool,
    ) -> Self {
        let mut model = Self {
            comps: Vec::new(),
            properties: Vec::new(),
            allow_foreign,
        };
        let current_name = items.get(&current_id).map_or("", |item| item.name.as_str());
        model.add_comp(current_id, current_name, current, items);
        // Only foreign comp() expressions need a project-wide dependency snapshot.
        if !model.properties.iter().any(|p| {
            p.expression
                .as_ref()
                .is_some_and(|code| code.contains("comp("))
        }) {
            return model;
        }
        let mut ids: Vec<_> = items
            .keys()
            .copied()
            .filter(|id| *id != current_id)
            .collect();
        ids.sort_unstable();
        for id in ids {
            let item = items[&id];
            let comp = if id == current_id {
                current
            } else if let ItemKind::Composition(comp) = &item.kind {
                comp
            } else {
                continue;
            };
            model.add_comp(id, &item.name, comp, items);
        }
        model
    }
    fn add_comp(
        &mut self,
        id: u32,
        name: &str,
        comp: &Composition,
        items: &HashMap<u32, &ProjectItem>,
    ) {
        let layers = comp
            .layers
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                let source = items.get(&layer.record.source_id()).copied();
                let size = crate::structure_document::source_anchor_dimensions(source, layer)
                    .map(f64::from);
                self.add_layer(
                    id,
                    index + 1,
                    layer,
                    size,
                    layer.record.source_id() != 0 || layer.record.flags().null_layer,
                    [f64::from(comp.width), f64::from(comp.height)],
                )
            })
            .collect();
        self.comps.push(Comp {
            id,
            name: name.to_owned(),
            width: comp.width,
            height: comp.height,
            duration: comp.duration_secs,
            fps: comp.frame_rate,
            display_start: comp.display_start_secs,
            layers,
            has_camera: comp
                .layers
                .iter()
                .any(|layer| layer.record.layer_type() == 2),
        });
    }
    fn add_layer(
        &mut self,
        comp_id: u32,
        index: usize,
        layer: &Layer,
        size: [f64; 2],
        source_relative_anchor: bool,
        comp_size: [f64; 2],
    ) -> LayerModel {
        let mut transform = BTreeMap::new();
        let mut position_separated = false;
        if let Ok(root) = properties::root_runs(&layer.content)
            && let Ok(group) = unique_run(&root, "ADBE Transform Group")
                .and_then(|run| properties::unique_list(run, *b"tdgp"))
            && let Ok(runs) = properties::runs(group)
        {
            for (name, run) in runs {
                let Some(alias) = transform_alias(name) else {
                    continue;
                };
                let orientation = name == "ADBE Orientation";
                let leaf = if orientation {
                    properties::unique_list(run, *b"otst")
                        .and_then(|wrapper| properties::unique_list(wrapper, *b"tdbs"))
                        .ok()
                } else {
                    properties::unique_list(run, *b"tdbs").ok()
                };
                let numeric = if orientation {
                    properties::read_orientation(run)
                } else {
                    leaf.ok_or(properties::PropertyError::Layout("Transform leaf missing"))
                        .and_then(properties::read_numeric)
                };
                if name == "ADBE Position"
                    && numeric
                        .as_ref()
                        .is_ok_and(|numeric| numeric.dimensions_separated)
                {
                    position_separated = true;
                }
                let numeric = numeric.and_then(|mut numeric| {
                    // Native static AV anchors are source-relative in cdat;
                    // the expression API exposes pixels. Animated/expression
                    // anchors already use the native keyed/expression units.
                    if name == "ADBE Anchor Point"
                        && source_relative_anchor
                        && !numeric.animated
                        && !numeric.expression_enabled
                    {
                        if size.iter().any(|v| !v.is_finite() || *v <= 0.0)
                            || numeric.values.len() < 2
                        {
                            return Err(properties::PropertyError::Layout(
                                "unresolved source-relative Anchor dimensions",
                            ));
                        }
                        numeric.values[0] *= size[0];
                        numeric.values[1] *= size[1];
                    }
                    Ok(numeric)
                });
                let slot = self.push_property(
                    comp_id,
                    layer,
                    PropertyIdentity::Transform {
                        match_name: name.to_owned(),
                    },
                    numeric,
                    leaf,
                );
                transform.insert(alias.to_owned(), slot);
            }
        }
        // AE omits Transform leaves at their defaults; expressions still read them.
        let default_anchor = if source_relative_anchor {
            [size[0] / 2.0, size[1] / 2.0]
        } else {
            [0.0, 0.0]
        };
        for (alias, match_name, values) in [
            (
                "anchorPoint",
                "ADBE Anchor Point",
                vec![default_anchor[0], default_anchor[1], 0.0],
            ),
            (
                "position",
                "ADBE Position",
                vec![comp_size[0] / 2.0, comp_size[1] / 2.0, 0.0],
            ),
            ("scale", "ADBE Scale", vec![100.0, 100.0, 100.0]),
            ("rotation", "ADBE Rotate Z", vec![0.0]),
            ("opacity", "ADBE Opacity", vec![100.0]),
        ] {
            if transform.contains_key(alias) || (alias == "position" && position_separated) {
                continue;
            }
            let slot = self.properties.len();
            self.properties.push(Property {
                comp_id,
                layer_id: layer.record.id(),
                identity: PropertyIdentity::Transform {
                    match_name: match_name.to_owned(),
                },
                initial: values,
                keys: Vec::new(),
                expression: None,
                error: None,
                expression_enabled: false,
                notes: Vec::new(),
                range: None,
                text: None,
            });
            transform.insert(alias.to_owned(), slot);
        }
        let dimensions = if layer.record.flags().three_d_layer {
            &["xPosition", "yPosition", "zPosition"][..]
        } else {
            &["xPosition", "yPosition"][..]
        };
        let separated_position = position_separated
            .then(|| {
                dimensions
                    .iter()
                    .map(|alias| transform.get(*alias).copied())
                    .collect::<Option<Vec<_>>>()
            })
            .flatten();
        let mut content = Vec::new();
        if let Ok(root) = properties::root_runs(&layer.content) {
            for (name, run) in &root {
                if *name == "ADBE Root Vectors Group"
                    && let Some(node) = self.add_shape_properties(
                        comp_id,
                        layer,
                        run,
                        vec![ShapePathSegment {
                            index: 2,
                            match_name: (*name).to_owned(),
                        }],
                        0,
                    )
                {
                    content = node.children;
                }
            }
        }
        let masks = self.add_mask_properties(comp_id, layer);
        let source_text = self.add_source_text(comp_id, layer);
        for (animator, leaves) in crate::structure_document::text::animator_property_leaves(layer) {
            for (name, run) in leaves {
                let leaf = properties::unique_list(run, *b"tdbs").ok();
                let Some(numeric) = leaf.map(properties::read_numeric) else {
                    continue;
                };
                if numeric.is_err() {
                    continue;
                }
                self.push_property(
                    comp_id,
                    layer,
                    PropertyIdentity::TextAnimator {
                        animator,
                        match_name: name.to_owned(),
                    },
                    numeric,
                    leaf,
                );
            }
        }
        let (decoded, _) = native::read_effects(&layer.content, size);
        let source_runs = properties::root_runs(&layer.content)
            .and_then(|root| {
                unique_run(&root, "ADBE Effect Parade")
                    .and_then(|parade| properties::unique_list(parade, *b"tdgp"))
                    .and_then(properties::runs)
            })
            .unwrap_or_default();
        let mut effects = Vec::new();
        for effect in decoded {
            let Ok(effect_index) = u32::try_from(effect.index) else {
                continue;
            };
            let Some((_, run)) = source_runs.get(effect.index - 1) else {
                continue;
            };
            let Ok(plugin) = properties::unique_list(run, *b"sspc") else {
                continue;
            };
            let Ok(body) = properties::unique_list(plugin, *b"tdgp") else {
                continue;
            };
            let explicit = properties::runs(body).unwrap_or_default();
            let declarations = properties::unique_list(plugin, *b"parT")
                .and_then(properties::runs)
                .unwrap_or_default();
            let canonical = definitions::definition(&effect.match_name);
            let name = display_name(body).unwrap_or(&effect.match_name).to_owned();
            let mut parameters = Vec::new();
            for parameter in effect.parameters {
                let leaf = unique_run(&explicit, &parameter.match_name)
                    .and_then(|run| properties::unique_list(run, *b"tdbs"))
                    .ok();
                let label = leaf
                    .and_then(display_name)
                    .or_else(|| {
                        declarations
                            .iter()
                            .find(|(name, _)| *name == parameter.match_name)
                            .and_then(|(_, run)| properties::data(run, *b"pard").ok())
                            .and_then(|bytes| bytes.get(16..48))
                            .and_then(|bytes| {
                                std::str::from_utf8(
                                    bytes.split(|b| *b == 0).next().unwrap_or_default(),
                                )
                                .ok()
                            })
                    })
                    .or_else(|| {
                        canonical.and_then(|effect| {
                            effect
                                .parameters
                                .iter()
                                .find(|p| p.match_name == parameter.match_name)
                                .map(|p| p.label.as_str())
                        })
                    })
                    .unwrap_or(&parameter.match_name)
                    .to_owned();
                // Admit native numerical selectors only when the match-name slot and
                // the complete local declaration ordering agree. Sparse ordinary
                // control profiles have independently pinned one-value slots.
                let suffix: Option<u32> = parameter
                    .match_name
                    .rsplit_once('-')
                    .and_then(|(_, suffix)| suffix.parse().ok());
                let local_index = declarations
                    .iter()
                    .position(|(name, _)| *name == parameter.match_name)
                    .and_then(|index| u32::try_from(index).ok());
                let ordinary = matches!(
                    effect.match_name.as_str(),
                    "ADBE Slider Control"
                        | "ADBE Color Control"
                        | "ADBE Point Control"
                        | "ADBE Angle Control"
                        | "ADBE Checkbox Control"
                );
                let native_index = suffix.filter(|index| ordinary || local_index == Some(*index));
                let identity = PropertyIdentity::Effect {
                    index: effect_index,
                    match_name: parameter.match_name.clone(),
                };
                let slot = self.push_property(comp_id, layer, identity, parameter.numeric, leaf);
                parameters.push(Parameter {
                    name: label,
                    match_name: parameter.match_name,
                    index: native_index,
                    slot,
                });
            }
            effects.push(Effect {
                name,
                match_name: effect.match_name,
                index: effect.index,
                parameters,
            });
        }
        LayerModel {
            id: layer.record.id(),
            name: layer.name.to_string(),
            index,
            three_d: layer.record.flags().three_d_layer,
            in_point: layer
                .record
                .in_point()
                .zip(layer.record.start_time())
                .zip(layer.record.stretch())
                .map(|((local, start), stretch)| start + local * stretch),
            out_point: layer
                .record
                .out_point()
                .zip(layer.record.start_time())
                .zip(layer.record.stretch())
                .map(|((local, start), stretch)| start + local * stretch),
            transform,
            separated_position,
            effects,
            width: size[0],
            height: size[1],
            default_anchor,
            enabled: layer.record.flags().enabled,
            parent: Some(layer.record.parent_id()).filter(|id| *id != 0),
            masks,
            content,
            source_text,
        }
    }
    /// The layer's Source Text with an enabled expression, as a string slot.
    fn add_source_text(&mut self, comp_id: u32, layer: &Layer) -> Option<usize> {
        let (text, leaf) = crate::structure_document::text::source_text_expression_input(layer)?;
        let expression = properties::data(leaf, *b"Utf8")
            .map_err(|_| "Source Text expression source is missing".to_owned())
            .and_then(|bytes| {
                std::str::from_utf8(bytes).map_err(|_| "expression is not UTF-8".to_owned())
            })
            .and_then(|code| syntax::compile(code).map_err(|error| error.to_string()));
        let slot = self.properties.len();
        let (expression, error) = match expression {
            Ok(code) => (Some(code), None),
            Err(error) => (None, Some(error)),
        };
        self.properties.push(Property {
            comp_id,
            layer_id: layer.record.id(),
            identity: PropertyIdentity::SourceText {},
            initial: Vec::new(),
            keys: Vec::new(),
            expression,
            error,
            expression_enabled: true,
            notes: Vec::new(),
            range: None,
            text: Some(text),
        });
        Some(slot)
    }
    /// Feather, Opacity and Expansion of every Mask Atom, one-based as mask import.
    fn add_mask_properties(&mut self, comp_id: u32, layer: &Layer) -> Vec<MaskModel> {
        let mut masks = Vec::new();
        let Some(atoms) = properties::root_runs(&layer.content).ok().and_then(|root| {
            unique_run(&root, "ADBE Mask Parade")
                .and_then(|parade| properties::unique_list(parade, *b"tdgp"))
                .and_then(properties::runs)
                .ok()
        }) else {
            return masks;
        };
        for (index, (_, atom)) in atoms
            .into_iter()
            .filter(|(name, _)| *name == "ADBE Mask Atom")
            .enumerate()
        {
            let Ok(index) = u32::try_from(index + 1) else {
                continue;
            };
            let Ok(group) = properties::unique_list(atom, *b"tdgp") else {
                continue;
            };
            let Ok(leaves) = properties::runs(group) else {
                continue;
            };
            let mut mask = MaskModel {
                name: display_name(group)
                    .or_else(|| display_name(atom))
                    .map_or_else(|| format!("Mask {index}"), str::to_owned),
                index,
                properties: BTreeMap::new(),
            };
            for (name, run) in leaves {
                if !matches!(
                    name,
                    "ADBE Mask Feather" | "ADBE Mask Opacity" | "ADBE Mask Offset"
                ) {
                    continue;
                }
                let leaf = properties::unique_list(run, *b"tdbs").ok();
                let Some(leaf_chunks) = leaf else {
                    continue;
                };
                // Native Feather uses the Point-control layout (as mask import).
                let numeric = if name == "ADBE Mask Feather" {
                    properties::read_effect_point(leaf_chunks)
                        .or_else(|_| properties::read_numeric(leaf_chunks))
                } else {
                    properties::read_numeric(leaf_chunks)
                };
                let slot = self.push_property(
                    comp_id,
                    layer,
                    PropertyIdentity::Mask {
                        index,
                        match_name: name.to_owned(),
                    },
                    numeric,
                    leaf,
                );
                let alias = match name {
                    "ADBE Mask Feather" => "maskFeather",
                    "ADBE Mask Opacity" => "maskOpacity",
                    _ => "maskExpansion",
                };
                mask.properties.insert(alias.to_owned(), slot);
            }
            masks.push(mask);
        }
        masks
    }
    /// Models every numeric Shape leaf and returns the named Contents tree.
    /// `ordinal` numbers same-kind siblings for AE's default display names.
    fn add_shape_properties(
        &mut self,
        comp_id: u32,
        layer: &Layer,
        run: &[Chunk],
        path: Vec<ShapePathSegment>,
        ordinal: usize,
    ) -> Option<ShapeNode> {
        if path.len() > 64 {
            return None;
        }
        let segment = path.last()?.clone();
        if let Ok(leaf) = properties::unique_list(run, *b"tdbs")
            && let Ok(numeric) = properties::read_numeric(leaf)
        {
            let slot = self.push_property(
                comp_id,
                layer,
                PropertyIdentity::Shape { path },
                Ok(numeric),
                Some(leaf),
            );
            return Some(ShapeNode {
                name: display_name(leaf)
                    .or_else(|| display_name(run))
                    .unwrap_or(&segment.match_name)
                    .to_owned(),
                alias: shape_alias(&segment.match_name).map(str::to_owned),
                match_name: segment.match_name,
                index: segment.index,
                slot: Some(slot),
                children: Vec::new(),
            });
        }
        let group = properties::unique_list(run, *b"tdgp").ok()?;
        let runs = properties::runs(group).ok()?;
        let mut children = Vec::new();
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for (index, (name, child)) in runs.iter().enumerate() {
            // Keep the Adobe-captured Scale identities; every other leaf uses
            // its native one-based position inside the parent group.
            let Some(native_index) =
                properties::shape_scale_index(&segment.match_name, name, index)
                    .or_else(|| u32::try_from(index + 1).ok())
            else {
                continue;
            };
            let same_kind = kinds.entry((*name).to_owned()).or_default();
            *same_kind += 1;
            let mut child_path = path.clone();
            child_path.push(ShapePathSegment {
                index: native_index,
                match_name: (*name).to_owned(),
            });
            if let Some(node) =
                self.add_shape_properties(comp_id, layer, child, child_path, *same_kind)
            {
                children.push(node);
            }
        }
        let name = display_name(group)
            .filter(|name| *name != "-_0_/-")
            .map(str::to_owned)
            .or_else(|| {
                default_shape_name(&segment.match_name).map(|base| format!("{base} {ordinal}"))
            })
            .unwrap_or_else(|| segment.match_name.clone());
        Some(ShapeNode {
            name,
            alias: shape_alias(&segment.match_name).map(str::to_owned),
            match_name: segment.match_name,
            index: segment.index,
            slot: None,
            children,
        })
    }
    fn push_property(
        &mut self,
        comp_id: u32,
        layer: &Layer,
        identity: PropertyIdentity,
        numeric: Result<NumericProperty, properties::PropertyError>,
        leaf: Option<&[Chunk]>,
    ) -> usize {
        let slot = self.properties.len();
        let enabled = numeric.as_ref().is_ok_and(|p| p.expression_enabled);
        let range = legal_range(&identity_name(&identity));
        let mut property = Property {
            comp_id,
            layer_id: layer.record.id(),
            identity,
            initial: Vec::new(),
            keys: Vec::new(),
            expression: None,
            error: None,
            expression_enabled: enabled,
            notes: Vec::new(),
            range,
            text: None,
        };
        match numeric {
            Err(error) => property.error = Some(error.to_string()),
            Ok(mut numeric) => {
                // Only Effect controls (checkbox/popup) are discrete integers; Shape leaves
                // such as Trim carry the integer flag yet keyed import interpolates them.
                let discrete = matches!(property.identity, PropertyIdentity::Effect { .. });
                property.error = validate_numeric(&numeric, layer, discrete).err();
                let mut easings = Vec::new();
                if property.error.is_none() && !numeric.keyframes.is_empty() {
                    let name = identity_name(&property.identity);
                    let spatial = match &property.identity {
                        PropertyIdentity::Transform { match_name } => match_name == "ADBE Position",
                        PropertyIdentity::Shape { path } => path
                            .last()
                            .is_some_and(|leaf| leaf.match_name == "ADBE Vector Position"),
                        _ => false,
                    };
                    match crate::structure_document::editable_native_keys(
                        &name,
                        &numeric,
                        layer,
                        spatial,
                        &mut property.notes,
                    ) {
                        Ok((prepared, eases)) => {
                            numeric = prepared;
                            easings = eases;
                        }
                        Err(error) => property.error = Some(error),
                    }
                    property.notes.sort();
                    property.notes.dedup();
                }
                // AEP Transform percentages use fractions; the AE expression API uses 100.
                if matches!(&property.identity, PropertyIdentity::Transform { match_name } if matches!(match_name.as_str(), "ADBE Scale" | "ADBE Opacity"))
                    || matches!(&property.identity, PropertyIdentity::Mask { match_name, .. } if match_name == "ADBE Mask Opacity")
                {
                    for value in numeric
                        .values
                        .iter_mut()
                        .chain(numeric.keyframes.iter_mut().flat_map(|key| &mut key.values))
                    {
                        *value *= 100.0;
                    }
                }
                property.initial = numeric.values.clone();
                if property.error.is_none() {
                    let start = layer.record.start_time().unwrap_or(0.0);
                    let stretch = layer.record.stretch().unwrap_or(1.0);
                    property.keys = numeric
                        .keyframes
                        .iter()
                        .zip(easings)
                        .map(|(key, eases)| Key {
                            time: start + key.time_secs * stretch,
                            value: key.values.clone(),
                            hold: key.out_interpolation == 3,
                            ease: eases.iter().map(cubic_controls).collect(),
                        })
                        .collect();
                    if property.initial.is_empty()
                        && let Some(key) = property.keys.first()
                    {
                        property.initial = key.value.clone();
                    }
                }
                if enabled {
                    let compiled = leaf
                        .ok_or("expression source is missing")
                        .and_then(|leaf| {
                            properties::data(leaf, *b"Utf8")
                                .map_err(|_| "expression source is missing")
                        })
                        .and_then(|bytes| {
                            std::str::from_utf8(bytes).map_err(|_| "expression is not UTF-8")
                        });
                    match compiled
                        .map_err(str::to_owned)
                        .and_then(|text| syntax::compile(text).map_err(|error| error.to_string()))
                    {
                        Ok(code) => property.expression = Some(code),
                        Err(error) => property.error = Some(error),
                    }
                }
            }
        }
        self.properties.push(property);
        slot
    }
}

fn validate_numeric(
    numeric: &NumericProperty,
    layer: &Layer,
    discrete: bool,
) -> Result<(), String> {
    let start = layer.record.start_time().ok_or("invalid property clock")?;
    let stretch = layer.record.stretch().ok_or("invalid property clock")?;
    if !start.is_finite() || !stretch.is_finite() || stretch <= 0.0 {
        return Err("nonpositive/invalid native clock is not admitted".into());
    }
    if numeric.dimensions_separated {
        return Err("separated Position is read through its dimension properties".into());
    }
    if numeric.animated && numeric.keyframes.is_empty() {
        return Err("native keys could not be decoded".into());
    }
    let dim = numeric
        .keyframes
        .first()
        .map_or(numeric.values.len(), |key| key.values.len());
    if !(1..=4).contains(&dim) || numeric.values.iter().any(|v| !v.is_finite()) {
        return Err("invalid numeric dimensions/value".into());
    }
    for (index, key) in numeric.keyframes.iter().enumerate() {
        if (discrete
            && numeric.value_kind == properties::NumericValueKind::Integer
            && index + 1 < numeric.keyframes.len()
            && key.out_interpolation != 3)
            || key.values.len() != dim
            || key.values.iter().any(|v| !v.is_finite())
            || !key.time_secs.is_finite()
            || (index > 0 && key.time_secs <= numeric.keyframes[index - 1].time_secs)
            || !matches!(key.in_interpolation, 1..=3)
            || !matches!(key.out_interpolation, 1..=3)
            || key
                .spatial_in
                .iter()
                .chain(&key.spatial_out)
                .any(|v| !v.is_finite())
        {
            return Err(
                "only finite ordered Linear/Bezier/Hold keys with finite tangents are admitted"
                    .into(),
            );
        }
    }
    Ok(())
}

/// Linear and Hold segments need no controls; Hold is carried by the key.
fn cubic_controls(easing: &PropertyKeyframeEasing) -> Option<[f64; 4]> {
    match *easing {
        PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => Some([x1, y1, x2, y2]),
        PropertyKeyframeEasing::Linear | PropertyKeyframeEasing::Hold => None,
    }
}

fn identity_name(identity: &PropertyIdentity) -> String {
    match identity {
        PropertyIdentity::Transform { match_name } => match_name.clone(),
        PropertyIdentity::Effect { index, match_name } => format!("effect {index} {match_name}"),
        PropertyIdentity::Shape { path } => path
            .last()
            .map_or_else(|| "Shape".to_owned(), |leaf| leaf.match_name.clone()),
        PropertyIdentity::Mask { index, match_name } => format!("Mask {index} {match_name}"),
        PropertyIdentity::TextAnimator {
            animator,
            match_name,
        } => format!("Text Animator {animator} {match_name}"),
        PropertyIdentity::SourceText {} => "Source Text".to_owned(),
    }
}
fn unique_run<'a>(
    runs: &[(&str, &'a [Chunk])],
    name: &str,
) -> Result<&'a [Chunk], properties::PropertyError> {
    let mut matching = runs.iter().filter(|(candidate, _)| *candidate == name);
    let first = matching
        .next()
        .ok_or(properties::PropertyError::Layout("property group missing"))?;
    if matching.next().is_some() {
        return Err(properties::PropertyError::Layout(
            "ambiguous property group",
        ));
    }
    Ok(first.1)
}
fn display_name(chunks: &[Chunk]) -> Option<&str> {
    crate::structure_document::native_property_name(chunks)
}
fn transform_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "ADBE Position" => "position",
        "ADBE Anchor Point" => "anchorPoint",
        "ADBE Scale" => "scale",
        "ADBE Rotate Z" => "rotation",
        "ADBE Opacity" => "opacity",
        "ADBE Position_0" => "xPosition",
        "ADBE Position_1" => "yPosition",
        "ADBE Position_2" => "zPosition",
        "ADBE Rotate X" => "xRotation",
        "ADBE Rotate Y" => "yRotation",
        "ADBE Orientation" => "orientation",
        _ => return None,
    })
}

/// AE's default display-name stem for a Contents item (`Group 1`, `Ellipse Path 1`).
fn default_shape_name(match_name: &str) -> Option<&'static str> {
    Some(match match_name {
        "ADBE Vector Group" => "Group",
        "ADBE Vector Shape - Ellipse" => "Ellipse Path",
        "ADBE Vector Shape - Rect" => "Rectangle Path",
        "ADBE Vector Shape - Star" => "Polystar Path",
        "ADBE Vector Shape - Group" => "Path",
        "ADBE Vector Graphic - Fill" => "Fill",
        "ADBE Vector Graphic - Stroke" => "Stroke",
        "ADBE Vector Graphic - G-Fill" => "Gradient Fill",
        "ADBE Vector Graphic - G-Stroke" => "Gradient Stroke",
        "ADBE Vector Filter - Trim" => "Trim Paths",
        "ADBE Vector Filter - Repeater" => "Repeater",
        "ADBE Vector Filter - RC" => "Round Corners",
        "ADBE Vector Filter - Offset" => "Offset Paths",
        "ADBE Vector Filter - Merge" => "Merge Paths",
        _ => return None,
    })
}

/// Expression-API member names of Shape groups and numeric leaves.
fn shape_alias(match_name: &str) -> Option<&'static str> {
    Some(match match_name {
        "ADBE Vector Transform Group" => "transform",
        "ADBE Vectors Group" => "content",
        "ADBE Vector Anchor" => "anchorPoint",
        "ADBE Vector Position"
        | "ADBE Vector Ellipse Position"
        | "ADBE Vector Rect Position"
        | "ADBE Vector Star Position" => "position",
        "ADBE Vector Scale" => "scale",
        "ADBE Vector Rotation" | "ADBE Vector Star Rotation" => "rotation",
        "ADBE Vector Skew" => "skew",
        "ADBE Vector Skew Axis" => "skewAxis",
        "ADBE Vector Group Opacity" | "ADBE Vector Fill Opacity" | "ADBE Vector Stroke Opacity" => {
            "opacity"
        }
        "ADBE Vector Ellipse Size" | "ADBE Vector Rect Size" => "size",
        "ADBE Vector Rect Roundness" => "roundness",
        "ADBE Vector Fill Color" | "ADBE Vector Stroke Color" => "color",
        "ADBE Vector Stroke Width" => "strokeWidth",
        "ADBE Vector Stroke Miter Limit" => "miterLimit",
        "ADBE Vector Trim Start" => "start",
        "ADBE Vector Trim End" => "end",
        "ADBE Vector Trim Offset" => "offset",
        "ADBE Vector Star Points" => "points",
        "ADBE Vector Star Inner Radius" => "innerRadius",
        "ADBE Vector Star Outer Radius" => "outerRadius",
        "ADBE Vector Star Inner Roundess" => "innerRoundness",
        "ADBE Vector Star Outer Roundess" => "outerRoundness",
        "ADBE Vector RoundCorner Radius" => "radius",
        _ => return None,
    })
}

/// AE clamps these properties to their legal range, including expression results.
fn legal_range(name: &str) -> Option<[Option<f64>; 2]> {
    let leaf = name.rsplit(' ').next().unwrap_or(name);
    let percent = [Some(0.0), Some(100.0)];
    let nonnegative = [Some(0.0), None];
    if name.ends_with("Opacity") {
        return Some(percent);
    }
    match leaf {
        "Start" | "End" if name.contains("Vector Trim") => Some(percent),
        "Width" if name.contains("Stroke") => Some(nonnegative),
        "Feather" | "Size" | "Radius" => Some(nonnegative),
        _ => None,
    }
}
