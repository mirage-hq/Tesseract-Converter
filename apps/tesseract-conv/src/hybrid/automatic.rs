//! Coordinate actual converter results; no parallel capability or proof model.
use super::{dependencies::GraphDependencies, owners::Owners, package};
use aftereffects_file::{
    AepPreparationControl, AfterEffects, AfterEffectsExportOptions, StagedAfterEffectsPictureExport,
};
use anyhow::{ensure, Context};
use fx_conv::{ConversionDiagnostic, Diagnostic, DiagnosticKind};
use premiere_file::{
    AfterEffectsPicture, ExportField, ExportLossDomain, ExportLossKind, ExportLossSource,
    FrameRate, PictureReplacement, Premiere, PremiereExportOptions, StagedPicturePremiereExport,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tesseract_file::TesseractFile;

pub(super) struct StagedExport {
    pub native: StagedPicturePremiereExport,
    pub scopes: Vec<(u16, StagedAfterEffectsPictureExport)>,
    pub diagnostics: Vec<Diagnostic>,
}

pub(super) fn stage(
    archive: &TesseractFile,
    parent: &Path,
    output: &Path,
    options: &PremiereExportOptions,
    progress: fx_conv::Progress<'_>,
) -> anyhow::Result<StagedExport> {
    let document = archive.project();
    let prepared = Premiere.prepare_export_with_progress(archive, document, options, progress)?;
    if document.composition().layers().is_empty() {
        return super::empty_root::stage(archive, prepared, parent, output, options, progress);
    }
    progress.stage("plan Premiere picture scopes");
    let losses = prepared.losses();
    ensure!(
        !losses.losses_truncated,
        "hybrid export loss report exceeded its limit"
    );
    if !losses.losses.iter().any(|loss| {
        !matches!(
            loss.domain,
            ExportLossDomain::Audio | ExportLossDomain::Metadata
        ) && !matches!(loss.source, ExportLossSource::Document)
    }) {
        progress.stage("stage Premiere project");
        return Ok(StagedExport {
            native: prepared.stage_with_picture_replacements(parent, output, &[])?,
            scopes: Vec::new(),
            diagnostics: Vec::new(),
        });
    }
    let owners = Owners::new(document.composition().layers())?;
    let mut seeds = BTreeSet::new();
    let preserve_video_assets = losses
        .losses
        .iter()
        .any(|loss| loss.kind == ExportLossKind::Field(ExportField::PictureMedia));
    for loss in &losses.losses {
        if matches!(
            loss.domain,
            ExportLossDomain::Audio | ExportLossDomain::Metadata
        ) {
            continue;
        }
        let root = match &loss.source {
            ExportLossSource::Layer(layer) | ExportLossSource::LayerSubtree(layer) => {
                owners.layer(*layer)?
            }
            ExportLossSource::Property(target) => owners.target(target)?,
            // Document/preparation diagnostics remain visible; they are not a
            // request to move the entire project into After Effects.
            ExportLossSource::Document => continue,
        };
        if !matches!(
            document.composition().layers()[root].data(),
            fx_schema::LayerData::Audio(_)
        ) {
            seeds.insert(root);
        }
    }
    if seeds.is_empty() {
        progress.stage("stage Premiere project");
        return Ok(StagedExport {
            native: prepared.stage_with_picture_replacements(parent, output, &[])?,
            scopes: Vec::new(),
            diagnostics: Vec::new(),
        });
    }
    let rate = options.frame_rate.unwrap_or(FrameRate::Fps30);
    let fps = match rate {
        FrameRate::Fps24 => 24.0,
        FrameRate::Fps25 => 25.0,
        FrameRate::Fps30 => 30.0,
        FrameRate::Fps50 => 50.0,
        FrameRate::Fps60 => 60.0,
        FrameRate::Native(_) => anyhow::bail!("unsupported Premiere export sequence rate"),
        FrameRate::Fps24000Over1001 | FrameRate::Fps30000Over1001 | FrameRate::Fps60000Over1001 => {
            // These rational sequence rates are not exact AE 16.16 rates.
            // Keep the already prepared native result rather than changing
            // cadence or discarding otherwise convertible picture and sound.
            progress.stage("stage Premiere project");
            return Ok(StagedExport {
                native: prepared.stage_with_picture_replacements(parent, output, &[])?,
                scopes: Vec::new(),
                diagnostics: vec![Diagnostic {
                    code: "HYBRID-NATIVE-RETAINED",
                    kind: DiagnosticKind::Warning,
                    context: Some(format!("sequence rate {rate}")),
                    message: format!("The {rate} fps sequence clock is not exact in linked AEP scopes; retained the native Premiere result with its reported limitations. Linked scopes support exact 24, 25, 30, 50 and 60 fps."),
                }],
            });
        }
    };
    let dependencies = GraphDependencies::new(document, &owners)?;
    let recipe = prepared.packing_recipe();
    ensure!(
        recipe.is_complete(),
        "picture packing capture is incomplete"
    );
    let root = recipe
        .containers()
        .into_iter()
        .find(|c| c.token == recipe.root())
        .context("missing root picture container")?;
    let boundaries = recipe.boundaries();
    let mut boundary_layers = BTreeMap::<_, BTreeSet<_>>::new();
    for boundary in &boundaries {
        boundary_layers
            .entry(boundary.token)
            .or_default()
            .insert(boundary.layer);
    }
    let retained: BTreeSet<_> = recipe.retained_picture_boundaries(root.token).collect();
    let retained_roots: BTreeSet<_> = boundaries
        .iter()
        .filter(|boundary| retained.contains(&boundary.token))
        .map(|boundary| owners.layer(boundary.layer))
        .collect::<anyhow::Result<_>>()?;
    let mut replacements = Vec::new();
    let mut scopes = Vec::new();
    let mut diagnostics = Vec::new();
    while let Some(seed) = seeds.pop_first() {
        progress.stage("prepare linked AE scope");
        let connected = dependencies.connected(seed);
        for index in &connected {
            seeds.remove(index);
        }
        let first = *connected.first().context("empty picture scope")?;
        let last = *connected.last().context("empty picture scope")?;
        ensure!(scopes.len() < 256, "hybrid picture scope limit exceeded");
        let selected = &document.composition().layers()[first..=last];
        ensure!(selected.iter().enumerate().all(|(offset, layer)| {
            connected.contains(&(first + offset))
                || matches!(layer.data(), fx_schema::LayerData::Audio(_))
        }), "linked roots are interleaved with independent pictures; cannot extract scope {first}..={last}");
        // Audio contributes no picture boundary actions. It may lie inside
        // this interval: picture-only AE staging disables its native switches,
        // while Premiere replay retains its independent sound occurrences.
        let ids: BTreeSet<_> = selected.iter().map(fx_schema::Layer::id).collect();
        let slots = select_boundary_slots(&root.boundaries, &boundary_layers, &ids);
        ensure!(
            slots.len() == selected.len(),
            "selected roots are not independently replaceable native boundaries"
        );
        let scope = u16::try_from(scopes.len() + 1)?;
        let ae = match AfterEffects.stage_picture_layers_with_control(
            archive,
            document,
            first..last + 1,
            parent,
            &AfterEffectsExportOptions { fps },
            AepPreparationControl {
                progress,
                preserve_video_assets,
                ..Default::default()
            },
        ) {
            Ok(stage) => stage,
            Err(error) => {
                let Some(omissions) = error.picture_scope_omissions() else {
                    return Err(error.into());
                };
                for omission in omissions {
                    let mut diagnostic = omission.diagnostic();
                    diagnostic.context = Some(format!(
                        "AE fallback for roots {first}..={last}: {}",
                        diagnostic.context.unwrap_or_default()
                    ));
                    diagnostics.push(diagnostic);
                }
                diagnostics.push(Diagnostic {
                    code: "HYBRID-NATIVE-RETAINED",
                    kind: DiagnosticKind::Warning,
                    context: Some(format!("roots {first}..={last}")),
                    message: "After Effects could not convert this picture scope; retained the native Premiere result with its reported limitations.".into(),
                });
                continue;
            }
        };
        let omitted_roots: BTreeSet<_> = ae
            .omitted_layer_ids()
            .iter()
            .map(|id| owners.layer(*id))
            .collect::<anyhow::Result<_>>()?;
        if !omitted_roots.is_disjoint(&retained_roots) {
            // After Effects drops something that the native export kept.
            // Replace the native picture only when the linked scope still
            // preserves more of it than the native export does.
            let scope = &document.composition().layers()[first..=last];
            let native_lost = native_lost_leaves(scope, &losses.losses, &owners, first..=last)?;
            let linked_lost = lost_leaves(scope, ae.omitted_layer_ids());
            if !linked_scope_preserves_more(&linked_lost, &native_lost) {
                for omission in &ae.report().diagnostics {
                    let mut diagnostic = omission.diagnostic();
                    diagnostic.context = Some(format!(
                        "AE fallback for roots {first}..={last}: {}",
                        diagnostic.context.unwrap_or_default()
                    ));
                    diagnostics.push(diagnostic);
                }
                diagnostics.push(Diagnostic {
                    code: "HYBRID-NATIVE-RETAINED",
                    kind: DiagnosticKind::Warning,
                    context: Some(format!("roots {first}..={last}")),
                    message: format!("After Effects omitted a source layer from a root with retained native picture: it omits {} shown picture layers whole, while the native export omits or partly converts {}; kept the complete native scope with its reported limitations instead of deleting supported content.", linked_lost.len(), native_lost.len()),
                });
                continue;
            }
            let because = if linked_lost.len() < native_lost.len() {
                "it loses fewer layers"
            } else {
                "it loses as many layers and none that the native export kept"
            };
            diagnostics.push(Diagnostic {
                code: "HYBRID-LINKED-PREFERRED",
                kind: DiagnosticKind::Warning,
                context: Some(format!("roots {first}..={last}")),
                message: format!("The native Premiere export omits or partly converts {} shown picture layers of this scope; After Effects omits {} whole, including {} that the native export kept. The linked scope replaces the native picture because {because}; this count ignores effect and key approximations on either side, and its omissions are reported with the scope's diagnostics.", native_lost.len(), linked_lost.len(), linked_lost.difference(&native_lost).count()),
            });
        }
        let composition = ae.root_composition();
        let frames = (composition.duration_secs() * fps).round();
        ensure!(
            frames.is_finite()
                && frames > 0.0
                && frames <= (i64::MAX / rate.ticks_per_frame()) as f64,
            "AEP duration exceeds native clock"
        );
        let intrinsic = (frames as i64)
            .checked_mul(rate.ticks_per_frame())
            .context("AEP clock overflow")?;
        ensure!(
            root.timeline_end_ticks <= intrinsic,
            "AEP picture is shorter than its native container"
        );
        replacements.push(PictureReplacement {
            packing_id: recipe.id(),
            container: root.token,
            boundaries: slots,
            picture: AfterEffectsPicture {
                composition_guid: composition.dynamic_link_guid().to_owned(),
                relative_path: package::scoped_aep_path(scope)?,
                dimensions: root.dimensions,
                frame_rate: rate,
                intrinsic_duration_ticks: intrinsic,
                timeline_ticks: 0..root.timeline_end_ticks,
                source_ticks: 0..root.timeline_end_ticks,
                enabled: true,
            },
        });
        scopes.push((scope, ae));
    }
    progress.stage("stage Premiere project");
    let native = prepared.stage_with_picture_replacements(parent, output, &replacements)?;
    Ok(StagedExport {
        native,
        scopes,
        diagnostics,
    })
}

/// Picture layers that the native Premiere export of roots `scope_roots`
/// loses: every leaf under a layer whose whole subtree is omitted
/// ([`ExportLossSource::LayerSubtree`]), plus the layer that owns a reported
/// partial loss ([`ExportLossSource::Layer`] or a property) in a picture
/// domain. Audio and metadata losses are not picture.
fn native_lost_leaves(
    scope: &[fx_schema::Layer],
    losses: &[premiere_file::ExportLoss],
    owners: &Owners,
    scope_roots: std::ops::RangeInclusive<usize>,
) -> anyhow::Result<BTreeSet<fx_schema::LayerId>> {
    let shown = shown_layer_ids(scope);
    let mut subtrees = BTreeSet::new();
    let mut lost = BTreeSet::new();
    for loss in losses {
        if matches!(
            loss.domain,
            ExportLossDomain::Audio | ExportLossDomain::Metadata
        ) {
            continue;
        }
        let (root, layer) = match &loss.source {
            ExportLossSource::Document => continue,
            ExportLossSource::LayerSubtree(layer) => {
                let root = owners.layer(*layer)?;
                if scope_roots.contains(&root) {
                    subtrees.insert(*layer);
                }
                continue;
            }
            ExportLossSource::Layer(layer) => (owners.layer(*layer)?, *layer),
            ExportLossSource::Property(target) => {
                (owners.target(target)?, owners.target_layer(target)?)
            }
        };
        // A partial loss on a hidden layer, or under a hidden ancestor,
        // changes no picture either.
        if scope_roots.contains(&root) && shown.contains(&layer) {
            lost.insert(layer);
        }
    }
    lost.extend(lost_leaves(scope, &subtrees));
    Ok(lost)
}

/// Every layer of `scope` that paints: not hidden and under no hidden
/// ancestor, groups included.
fn shown_layer_ids(scope: &[fx_schema::Layer]) -> BTreeSet<fx_schema::LayerId> {
    fn visit(layers: &[fx_schema::Layer], shown: &mut BTreeSet<fx_schema::LayerId>) {
        for layer in layers {
            if is_hidden(layer) {
                continue;
            }
            shown.insert(layer.id());
            if let Some(children) = layer.child_layers() {
                visit(children, shown);
            }
        }
    }
    let mut shown = BTreeSet::new();
    visit(scope, &mut shown);
    shown
}

/// The painting leaves (shown layers without children, sound excluded) under
/// the layers of `scope` whose IDs are in `omitted`, each omitted with its
/// whole subtree. A hidden layer or subtree paints nothing, so losing it
/// loses no picture.
fn lost_leaves(
    scope: &[fx_schema::Layer],
    omitted: &BTreeSet<fx_schema::LayerId>,
) -> BTreeSet<fx_schema::LayerId> {
    fn visit(
        layers: &[fx_schema::Layer],
        omitted: &BTreeSet<fx_schema::LayerId>,
        under_omitted: bool,
        lost: &mut BTreeSet<fx_schema::LayerId>,
    ) {
        for layer in layers {
            if is_hidden(layer) {
                continue;
            }
            let dropped = under_omitted || omitted.contains(&layer.id());
            match layer.child_layers() {
                Some(children) => visit(children, omitted, dropped, lost),
                None => {
                    if dropped && !matches!(layer.data(), fx_schema::LayerData::Audio(_)) {
                        lost.insert(layer.id());
                    }
                }
            }
        }
    }
    let mut lost = BTreeSet::new();
    visit(scope, omitted, false, &mut lost);
    lost
}

fn is_hidden(layer: &fx_schema::Layer) -> bool {
    use fx_schema::LayerData;
    match layer.data() {
        LayerData::Media(v) => v.is_hidden,
        LayerData::Video(v) => v.is_hidden,
        LayerData::Image(v) => v.is_hidden,
        LayerData::Text(v) => v.is_hidden,
        LayerData::Rect(v) => v.is_hidden,
        LayerData::Shape(v) => v.is_hidden,
        LayerData::Group(v) => v.is_hidden,
        LayerData::BooleanOperation(v) => v.is_hidden,
        LayerData::Adjustment(v) => v.is_hidden,
        _ => false,
    }
}

/// Whether the linked After Effects scope keeps more of the picture than the
/// native Premiere export: it loses fewer layers, or the same number while
/// losing nothing that the native export kept. The two counts differ in
/// kind: `native_lost` holds the leaves of omitted subtrees plus the owner
/// of each partial picture loss, `linked_lost` the leaves of omitted
/// subtrees only, as AE reports approximations without typed losses. Any
/// other tie keeps the native, Premiere-editable result.
fn linked_scope_preserves_more(
    linked_lost: &BTreeSet<fx_schema::LayerId>,
    native_lost: &BTreeSet<fx_schema::LayerId>,
) -> bool {
    linked_lost.len() < native_lost.len()
        || (linked_lost.len() == native_lost.len() && linked_lost.is_subset(native_lost))
}

fn select_boundary_slots<T, L>(
    ordered_tokens: &[T],
    boundary_layers: &BTreeMap<T, BTreeSet<L>>,
    selected_layers: &BTreeSet<L>,
) -> Vec<T>
where
    T: Copy + Ord,
    L: Ord,
{
    ordered_tokens
        .iter()
        .copied()
        .filter(|token| {
            boundary_layers
                .get(token)
                .is_some_and(|layers| !layers.is_disjoint(selected_layers))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_schema::LayerId;
    use serde_json::{json, Value};

    fn rect(id: u64, hidden: bool) -> Value {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../../../crates/aftereffects_file/tests/fixtures/hybrid/rect-identity.fx.json"
        ))
        .unwrap();
        let mut layer = value["composition"]["layers"][0].take();
        layer["id"] = json!(id);
        layer["isHidden"] = json!(hidden);
        layer
    }

    fn group(id: u64, hidden: bool, children: Vec<Value>) -> Value {
        json!({
            "type": "Group", "id": id, "name": "group", "isHidden": hidden,
            "playback": {
                "type": "windowed", "inputRange": {"start": 0, "duration": 2000},
                "mapping": {"type": "linear", "input": {"start": 0, "duration": 2000},
                    "output": {"start": 0, "duration": 2000}},
                "inputOffsetMs": 0
            },
            "transform": rect(999, false)["transform"],
            "layers": children,
        })
    }

    fn scope(roots: Vec<Value>) -> fx_schema::EditableFxCompositionDocument {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../../../crates/aftereffects_file/tests/fixtures/hybrid/rect-identity.fx.json"
        ))
        .unwrap();
        value["composition"]["layers"] = json!(roots);
        fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap()
    }

    fn ids(values: &[u64]) -> BTreeSet<LayerId> {
        values.iter().copied().map(LayerId::new).collect()
    }

    #[test]
    fn hidden_layers_and_hidden_subtrees_count_for_neither_side() {
        // Root 1: a shown group with a shown rect and a hidden rect.
        // Root 2: a hidden group with a shown rect under it.
        let document = scope(vec![
            rect(1, false),
            group(10, false, vec![rect(11, false), rect(12, true)]),
            group(20, true, vec![rect(21, false)]),
        ]);
        let layers = document.composition().layers();
        assert_eq!(shown_layer_ids(layers), ids(&[1, 10, 11]));
        // Omitting the shown group loses its shown leaf only; omitting the
        // hidden group or the hidden rect loses nothing.
        assert_eq!(lost_leaves(layers, &ids(&[10])), ids(&[11]));
        assert_eq!(lost_leaves(layers, &ids(&[12, 20, 21])), ids(&[]));
        assert_eq!(lost_leaves(layers, &ids(&[1, 10, 20])), ids(&[1, 11]));
    }

    #[test]
    fn linked_scope_wins_by_fewer_losses_or_by_losing_nothing_the_native_export_kept() {
        assert!(linked_scope_preserves_more(&ids(&[1]), &ids(&[2, 3])));
        assert!(linked_scope_preserves_more(&ids(&[]), &ids(&[2])));
        assert!(linked_scope_preserves_more(&ids(&[2]), &ids(&[2])));
        assert!(!linked_scope_preserves_more(&ids(&[1]), &ids(&[2])));
        assert!(!linked_scope_preserves_more(&ids(&[1, 2]), &ids(&[2])));
    }

    fn dense_selection(
        ordered_tokens: &[u8],
        boundaries: &[(u8, u8)],
        selected_layers: &BTreeSet<u8>,
    ) -> Vec<u8> {
        ordered_tokens
            .iter()
            .copied()
            .filter(|token| {
                boundaries.iter().any(|(boundary_token, layer)| {
                    boundary_token == token && selected_layers.contains(layer)
                })
            })
            .collect()
    }

    #[test]
    fn indexed_boundary_selection_matches_dense_scan_for_bounded_inventories() {
        let ordered_tokens = [3, 1, 3, 2, 0];
        for inventory in 0..4usize.pow(4) {
            let mut encoded = inventory;
            let mut boundaries = Vec::new();
            let mut boundary_layers = BTreeMap::<_, BTreeSet<_>>::new();
            for token in 0..4u8 {
                let layer = (encoded % 4) as u8;
                encoded /= 4;
                boundaries.push((token, layer));
                boundary_layers.entry(token).or_default().insert(layer);
                if (inventory + usize::from(token)) % 3 == 0 {
                    let duplicate_layer = (layer + 1) % 4;
                    boundaries.push((token, duplicate_layer));
                    boundary_layers
                        .entry(token)
                        .or_default()
                        .insert(duplicate_layer);
                }
            }
            for selected_bits in 0..(1 << 4) {
                let selected_layers = (0..4u8)
                    .filter(|layer| selected_bits & (1 << layer) != 0)
                    .collect();
                assert_eq!(
                    select_boundary_slots(&ordered_tokens, &boundary_layers, &selected_layers),
                    dense_selection(&ordered_tokens, &boundaries, &selected_layers),
                    "inventory={inventory}, selected_bits={selected_bits}"
                );
            }
        }
    }
}
