//! Static AE mask import using hidden editable shape guides.

use fx_schema::animator::AnimationGraphEntry;
use fx_schema::layer::{MaskMode, PathMask, ShapeLayer};
use fx_schema::{
    Duration, FxItemId, LayerData as FxLayer, LayerId, NonNegativeProperty, PercentageProperty,
    Position, PropertyTarget, ShapeContent, Time, TimeRangeProperty, Transform,
};

use crate::{
    properties::{
        NumericProperty, PropertyError, data, read_numeric, root_runs, runs, unique_list,
    },
    rifx::Chunk,
    structure::Layer,
};

use super::{
    animation::{NumericAnimationClock, NumericAnimationTarget, numeric_entries},
    animation_budget::AnimationBudget,
    shapes::path,
};

pub(super) struct MaskImport {
    pub(super) animations: Vec<AnimationGraphEntry>,
    /// One-based native mask index to successfully imported guide layer id.
    pub(super) guide_ids: Vec<(u32, LayerId)>,
    pub(super) warnings: Vec<String>,
}

pub(super) fn apply(
    layer: &Layer,
    occurrence: &mut fx_schema::GroupLayer,
    source_size: [u32; 2],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> MaskImport {
    let mut result = MaskImport {
        warnings: Vec::new(),
        animations: Vec::new(),
        guide_ids: Vec::new(),
    };
    let roots = match root_runs(&layer.content) {
        Ok(roots) => roots,
        Err(error) => {
            result
                .warnings
                .push(format!("mask property root ignored: {error}"));
            return result;
        }
    };
    for (_, parade_run) in roots
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Mask Parade")
    {
        let parade = match unique_list(parade_run, *b"tdgp") {
            Ok(group) => group,
            Err(error) => {
                result
                    .warnings
                    .push(format!("mask parade ignored: {error}"));
                continue;
            }
        };
        let atoms = match runs(parade) {
            Ok(atoms) => atoms,
            Err(error) => {
                result
                    .warnings
                    .push(format!("mask parade ignored: {error}"));
                continue;
            }
        };
        let atoms: Vec<_> = atoms
            .into_iter()
            .filter(|(name, _)| *name == "ADBE Mask Atom")
            .collect();
        if !atoms.is_empty() && source_size.contains(&0) {
            result.warnings.push("native masks are normalized to their owning layer's source bounds, but those dimensions are zero; mask guides were omitted (composition-bounds fallback must be selected and diagnosed by the parent)".into());
            return result;
        }
        for (index, (_, atom_run)) in atoms.into_iter().enumerate() {
            import_mask(
                layer,
                atom_run,
                index + 1,
                source_size,
                MaskImportDestination {
                    occurrence,
                    next_id,
                    budget,
                },
                &mut result,
            );
        }
    }
    result
}

struct MaskImportDestination<'a> {
    occurrence: &'a mut fx_schema::GroupLayer,
    next_id: &'a mut u64,
    budget: &'a mut AnimationBudget,
}

fn import_mask(
    layer: &Layer,
    atom_run: &[Chunk],
    index: usize,
    source_size: [u32; 2],
    destination: MaskImportDestination<'_>,
    result: &mut MaskImport,
) {
    let MaskImportDestination {
        occurrence,
        next_id,
        budget,
    } = destination;
    let MaskImport {
        animations,
        guide_ids,
        warnings,
    } = result;
    let info = match data(atom_run, *b"mkif") {
        Ok(info) if info.len() == 48 => info,
        Ok(_) => {
            warnings.push(format!("Mask {index} ignored: malformed mkif layout"));
            return;
        }
        Err(error) => {
            warnings.push(format!("Mask {index} ignored: {error}"));
            return;
        }
    };
    let group = match unique_list(atom_run, *b"tdgp") {
        Ok(group) => group,
        Err(error) => {
            warnings.push(format!("Mask {index} ignored: {error}"));
            return;
        }
    };
    let leaves = match runs(group) {
        Ok(leaves) => leaves,
        Err(error) => {
            warnings.push(format!("Mask {index} ignored: {error}"));
            return;
        }
    };
    let Some(path_run) = leaves
        .iter()
        .find(|(name, _)| *name == "ADBE Mask Shape")
        .map(|(_, run)| *run)
    else {
        warnings.push(format!("Mask {index} has AE's implicit path-less default; no explicit editable path was imported"));
        return;
    };
    match mask_roto_bezier(path_run) {
        Ok(Some(true)) => {
            warnings.push(format!("Mask {index} uses RotoBezier; automatic tangents are unsupported, so the mask was omitted"));
            return;
        }
        Ok(Some(false) | None) => {}
        Err(error) => warnings.push(format!(
            "Mask {index} RotoBezier metadata is malformed; the static path is retained as an ordinary Bezier path: {error}"
        )),
    }
    let (outline, _) = match path::decode_first(
        path_run,
        [f64::from(source_size[0]), f64::from(source_size[1])],
    ) {
        Ok(value) => value,
        Err(error) => {
            warnings.push(format!("Mask {index} path ignored: {error}"));
            return;
        }
    };
    match has_variable_feather(path_run) {
        Ok(true) => warnings.push(format!("Mask {index} variable-width feather points are unsupported; uniform Feather is retained")),
        Ok(false) => {}
        Err(error) => warnings.push(format!(
            "Mask {index} variable-width feather metadata is malformed; uniform Feather is retained: {error}"
        )),
    }
    if info[3] != 0 {
        warnings.push(format!("Mask {index} uses linear feather falloff; FX supports only its default falloff, so the falloff mode is approximated"));
    }

    let feather_property = read_property(&leaves, "ADBE Mask Feather", index, warnings);
    let opacity_property = read_property(&leaves, "ADBE Mask Opacity", index, warnings);
    let expansion_property = read_property(&leaves, "ADBE Mask Offset", index, warnings);
    let feather = feather_property
        .as_ref()
        .and_then(|property| static_values(property, index, "Feather", "[0, 0]", warnings))
        .and_then(pair)
        .unwrap_or([0.0, 0.0]);
    let opacity = opacity_property
        .as_ref()
        .and_then(|property| static_values(property, index, "Opacity", "100%", warnings))
        .and_then(scalar)
        .and_then(|value| NonNegativeProperty::new(value.clamp(0.0, 1.0)))
        .unwrap_or_else(|| NonNegativeProperty::new(1.0).expect("1 is non-negative"));
    let expansion = expansion_property
        .as_ref()
        .and_then(|property| static_values(property, index, "Expansion", "0", warnings))
        .and_then(scalar)
        .unwrap_or(0.0);

    let Some(mask_raw) = super::reserve_ids(next_id, 2) else {
        warnings.push(format!(
            "Mask {index} omitted: generated layer identifier space exhausted"
        ));
        return;
    };
    let guide_raw = mask_raw + 1;
    let guide_id = LayerId::new(guide_raw);
    let guide = FxLayer::Shape(ShapeLayer {
        id: guide_id,
        name: format!("{} — Mask {index} guide", occurrence.name),
        description:
            "Editable AE mask path in source-local layer coordinates; retained visible for path resolution and consumed as a mask source"
                .into(),
        is_hidden: false,
        parent: Some(occurrence.id),
        blend_mode: Default::default(),
        track_matte: None,
        masks: Vec::new(),
        active_range: TimeRangeProperty::new(Time::ZERO, Duration::from_secs(super::MAX_TIME_SECS)),
        effects: Vec::new(),
        motion_blur: false,
        transform: identity_transform(),
        shape: ShapeContent {
            path: outline,
            fills: Vec::new(),
            strokes: Vec::new(),
            round_corners: None,
            offset_paths: None,
            trim: None,
            poly_star: None,
            ellipse: None,
        },
    });
    match fx_schema::Layer::from_data(&guide) {
        Ok(guide) => occurrence.layers.push(guide),
        Err(error) => {
            warnings.push(format!(
                "Mask {index} ignored: invalid guide record: {error}"
            ));
            return;
        }
    }
    let mask_id = FxItemId::new(mask_raw);
    let mut mask_mode = mode(u16::from_be_bytes([info[6], info[7]]), index, warnings);
    let mut inverted = info[0] != 0;
    let leading_subtract = mask_mode == MaskMode::Subtract
        && occurrence
            .masks
            .iter()
            .all(|mask| mask.mode == MaskMode::None);
    if leading_subtract
        && opacity.value() == 1.0
        && opacity_property.as_ref().is_none_or(|property| {
            !property.animated
                && property.keyframes.is_empty()
                && !property.expression_enabled
                && !property.expression_present
        })
    {
        // AE subtracts the first mask from full layer coverage. FX's PAG-compatible
        // stack instead starts with that path, so express the opaque complement
        // explicitly. Fractional/animated opacity cannot use this equivalence.
        mask_mode = MaskMode::Add;
        inverted = !inverted;
    } else if leading_subtract {
        warnings.push(format!(
            "Mask {index}: leading Subtract with non-unit or animated/expression opacity cannot use the exact opaque-complement mapping; FX starting-shape approximation retained"
        ));
    }
    occurrence.masks.push(PathMask {
        id: mask_id,
        mode: mask_mode,
        inverted,
        layer: Some(guide_id),
        legacy_path: None,
        feather: [feather[0].max(0.0), feather[1].max(0.0)],
        expansion,
        opacity,
    });
    guide_ids.push((u32::try_from(index).unwrap_or(u32::MAX), guide_id));

    let clock = match NumericAnimationClock::parent_identity(layer) {
        Ok(clock) => Some(clock),
        Err(message) => {
            warnings.push(format!(
                "Mask {index} animation, including Path keys, cannot be mapped because its outer occurrence clock is invalid; initial editable values retained: {message}"
            ));
            None
        }
    };
    if let Some(clock) = clock {
        let (entries, path_warnings) = path::entries(
            path_run,
            [f64::from(source_size[0]), f64::from(source_size[1])],
            PropertyTarget::layer(guide_id, fx_schema::PropType::ShapePath),
            clock,
            budget,
        );
        animations.extend(entries);
        warnings.extend(
            path_warnings
                .into_iter()
                .map(|message| format!("Mask {index}: {message}")),
        );
        let mut output = MaskAnimationOutput {
            animations,
            warnings,
            budget,
        };
        add_mask_entries(
            index,
            "ADBE Mask Feather",
            feather_property.as_ref(),
            &[NumericAnimationTarget::vector2(
                PropertyTarget::fx_item(mask_id, "feather"),
                [0, 1],
                [1.0, 1.0],
            )],
            clock,
            &mut output,
        );
        add_mask_entries(
            index,
            "ADBE Mask Opacity",
            opacity_property.as_ref(),
            &[NumericAnimationTarget::float(
                PropertyTarget::fx_item(mask_id, "opacity"),
                0,
                1.0,
            )],
            clock,
            &mut output,
        );
        add_mask_entries(
            index,
            "ADBE Mask Offset",
            expansion_property.as_ref(),
            &[NumericAnimationTarget::float(
                PropertyTarget::fx_item(mask_id, "expansion"),
                0,
                1.0,
            )],
            clock,
            &mut output,
        );
    }
}

fn read_property(
    leaves: &[(&str, &[Chunk])],
    name: &str,
    index: usize,
    warnings: &mut Vec<String>,
) -> Option<NumericProperty> {
    let run = leaves.iter().find(|(candidate, _)| *candidate == name)?.1;
    let list = match unique_list(run, *b"tdbs") {
        Ok(list) => list,
        Err(error) => {
            warnings.push(format!("Mask {index} {name} defaulted: {error}"));
            return None;
        }
    };
    let value = match read_numeric(list) {
        Ok(value) => value,
        Err(error) => {
            warnings.push(format!("Mask {index} {name} defaulted: {error}"));
            return None;
        }
    };
    Some(value)
}

struct MaskAnimationOutput<'a> {
    animations: &'a mut Vec<AnimationGraphEntry>,
    warnings: &'a mut Vec<String>,
    budget: &'a mut AnimationBudget,
}

fn add_mask_entries(
    index: usize,
    name: &str,
    numeric: Option<&NumericProperty>,
    targets: &[NumericAnimationTarget],
    clock: NumericAnimationClock,
    output: &mut MaskAnimationOutput<'_>,
) {
    let Some(numeric) = numeric else {
        return;
    };
    let (mut entries, property_warnings) =
        numeric_entries(name, numeric, targets, clock, output.budget);
    output.animations.append(&mut entries);
    output.warnings.extend(
        property_warnings
            .into_iter()
            .map(|warning| format!("Mask {index} {warning}")),
    );
}

pub(super) fn mask_roto_bezier(run: &[Chunk]) -> Result<Option<bool>, PropertyError> {
    let Some(value) = optional_unique_list(run, *b"om-s")? else {
        return Ok(None);
    };
    let Some(property) = optional_unique_list(value, *b"tdbs")? else {
        return Err(PropertyError::Layout("missing RotoBezier property"));
    };
    let flags = data(property, *b"tdsb")?;
    if flags.len() != 4 {
        return Err(PropertyError::Layout("RotoBezier flags length"));
    }
    Ok(Some(flags[0] != 0))
}

pub(super) fn has_variable_feather(run: &[Chunk]) -> Result<bool, PropertyError> {
    let Some(value) = optional_unique_list(run, *b"om-s")? else {
        return Ok(false);
    };
    let Some(keys) = optional_unique_list(value, *b"omks")? else {
        return Ok(false);
    };
    if keys.iter().any(|chunk| chunk.id() != *b"LIST") {
        return Err(PropertyError::Layout(
            "variable feather key-container framing",
        ));
    }
    let metadata = crate::properties::read_path_metadata(unique_list(value, *b"tdbs")?)?;
    // Ordinary animation stores one shap per timing key. Extra containers, not
    // the presence of multiple authored keys, indicate unsupported metadata.
    let expected_shapes = metadata.keyframes.len().max(1);
    Ok(keys.len() != expected_shapes
        || keys.iter().any(|chunk| chunk.list_kind() != Some(*b"shap")))
}

fn optional_unique_list(
    children: &[Chunk],
    kind: [u8; 4],
) -> Result<Option<&[Chunk]>, PropertyError> {
    if children.iter().any(|chunk| chunk.list_kind() == Some(kind)) {
        return unique_list(children, kind).map(Some);
    }
    if children.iter().any(|chunk| chunk.id() == kind) {
        return Err(PropertyError::Layout("property LIST shape"));
    }
    Ok(None)
}

fn mode(value: u16, index: usize, warnings: &mut Vec<String>) -> MaskMode {
    match value {
        0 => MaskMode::None,
        1 => MaskMode::Add,
        2 => MaskMode::Subtract,
        3 => MaskMode::Intersect,
        4 => MaskMode::Lighten,
        5 => MaskMode::Darken,
        6 => MaskMode::Difference,
        other => {
            warnings.push(format!(
                "Mask {index} mode {other} is unknown; Add was used"
            ));
            MaskMode::Add
        }
    }
}

fn static_values<'a>(
    property: &'a NumericProperty,
    index: usize,
    name: &str,
    default: &str,
    warnings: &mut Vec<String>,
) -> Option<&'a [f64]> {
    if property.expression_enabled {
        warnings.push(format!(
            "Mask {index} {name}: enabled AE expression cannot be evaluated; destination static default {default} used and animation omitted"
        ));
        return None;
    }
    if property.animated || !property.keyframes.is_empty() {
        let detail = if property.keyframes.is_empty() {
            "animated property has no supported native keys"
        } else {
            "editable native keyframes supply authored values"
        };
        warnings.push(format!(
            "Mask {index} {name}: destination static default {default} used; {detail}"
        ));
        return None;
    }
    if property.expression_present {
        warnings.push(format!(
            "Mask {index} {name}: disabled AE expression omitted; stored static value used"
        ));
    }
    Some(&property.values)
}

fn pair(values: &[f64]) -> Option<[f64; 2]> {
    (values.len() >= 2).then(|| [values[0], values[1]])
}

fn scalar(values: &[f64]) -> Option<f64> {
    values.first().copied()
}

fn identity_transform() -> Transform {
    Transform {
        anchor_point: [0.0, 0.0],
        position: Position::TwoD([0.0, 0.0]),
        scale: [100.0, 100.0],
        rotation: 0.0,
        skew: 0.0,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0, 0.0, 0.0],
        opacity: PercentageProperty::new(100.0).expect("100 is a valid percentage"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        properties::{NumericKeyframe, NumericValueKind},
        structure::{ItemKind, Layer, read_project},
    };

    fn native_mask_case() -> (Layer, [u32; 2], Vec<Chunk>) {
        let project = read_project(include_bytes!("../../tests/fixtures/masks/mask.aep"))
            .expect("pinned native mask fixture parses");
        let composition = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) if !composition.layers.is_empty() => {
                    Some(composition)
                }
                _ => None,
            })
            .expect("fixture has a composition layer");
        let layer = composition.layers[0].clone();
        let atom = {
            let roots = root_runs(&layer.content).expect("fixture property roots parse");
            let (_, parade_run) = roots
                .into_iter()
                .find(|(name, _)| *name == "ADBE Mask Parade")
                .expect("fixture has a mask parade");
            let parade = unique_list(parade_run, *b"tdgp").expect("mask parade is framed");
            runs(parade)
                .expect("mask atoms are framed")
                .into_iter()
                .find(|(name, _)| *name == "ADBE Mask Atom")
                .expect("fixture has a mask atom")
                .1
                .to_vec()
        };
        (
            layer,
            [u32::from(composition.width), u32::from(composition.height)],
            atom,
        )
    }

    fn path_value_mut(chunks: &mut [Chunk]) -> Option<&mut Vec<Chunk>> {
        for chunk in chunks {
            let is_path_value = chunk.list_kind() == Some(*b"om-s")
                && chunk.children().is_some_and(|children| {
                    children
                        .iter()
                        .any(|child| child.list_kind() == Some(*b"omks"))
                });
            if is_path_value {
                return chunk.children_mut();
            }
            if let Some(children) = chunk.children_mut()
                && let Some(value) = path_value_mut(children)
            {
                return Some(value);
            }
        }
        None
    }

    fn patch_numeric_layout(chunks: &mut [Chunk], property_name: &str, dimensions: u16) -> bool {
        let property_index = chunks.iter().position(|chunk| {
            chunk.id() == *b"tdmn"
                && chunk
                    .data_payload()
                    .and_then(|bytes| bytes.split(|byte| *byte == 0).next())
                    == Some(property_name.as_bytes())
        });
        if let Some(property_index) = property_index {
            for chunk in &mut chunks[property_index + 1..] {
                if chunk.id() == *b"tdmn" {
                    break;
                }
                if chunk.list_kind() != Some(*b"tdbs") {
                    continue;
                }
                let storage = chunk.children_mut().expect("numeric storage is parsed");
                let metadata = storage
                    .iter_mut()
                    .find(|chunk| chunk.id() == *b"tdb4")
                    .expect("numeric storage has metadata");
                let mut bytes = metadata
                    .data_payload()
                    .expect("numeric metadata is a data chunk")
                    .to_vec();
                assert_eq!(bytes.len(), 124);
                bytes[2..4].copy_from_slice(&dimensions.to_be_bytes());
                bytes[59] &= !4;
                *metadata = Chunk::data(*b"tdb4", bytes).unwrap();
                return true;
            }
        }
        chunks.iter_mut().any(|chunk| {
            chunk
                .children_mut()
                .is_some_and(|children| patch_numeric_layout(children, property_name, dimensions))
        })
    }

    fn import_atom(layer: &Layer, source_size: [u32; 2], atom: &[Chunk]) -> MaskImport {
        let mut occurrence = super::super::group(
            LayerId::new(1),
            layer.name.to_string(),
            None,
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(10.0)),
        );
        let mut result = MaskImport {
            warnings: Vec::new(),
            animations: Vec::new(),
            guide_ids: Vec::new(),
        };
        let mut budget = AnimationBudget::default();
        let mut next_id = 2;
        import_mask(
            layer,
            atom,
            1,
            source_size,
            MaskImportDestination {
                occurrence: &mut occurrence,
                next_id: &mut next_id,
                budget: &mut budget,
            },
            &mut result,
        );
        result
    }

    fn numeric_property(values: &[f64]) -> NumericProperty {
        NumericProperty {
            values: values.to_vec(),
            animated: false,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: Vec::new(),
            value_kind: NumericValueKind::Continuous,
        }
    }

    #[test]
    fn animated_and_expression_mask_storage_never_becomes_the_static_base() {
        let mut warnings = Vec::new();
        let static_property = numeric_property(&[12.0, 34.0]);
        assert_eq!(
            static_values(&static_property, 1, "Feather", "[0, 0]", &mut warnings).and_then(pair),
            Some([12.0, 34.0])
        );
        assert!(warnings.is_empty());

        let mut animated = numeric_property(&[91.0, 92.0]);
        animated.animated = true;
        animated.keyframes.push(NumericKeyframe {
            time_secs: 0.0,
            values: vec![1.0, 2.0],
            in_interpolation: 1,
            out_interpolation: 1,
            in_speed: vec![0.0; 2],
            in_influence: vec![0.0; 2],
            out_speed: vec![0.0; 2],
            out_influence: vec![0.0; 2],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        });
        assert!(static_values(&animated, 2, "Feather", "[0, 0]", &mut warnings).is_none());
        animated.keyframes.clear();
        assert!(static_values(&animated, 3, "Feather", "[0, 0]", &mut warnings).is_none());

        let mut expression = numeric_property(&[17.0]);
        expression.expression_present = true;
        expression.expression_enabled = true;
        assert!(static_values(&expression, 4, "Expansion", "0", &mut warnings).is_none());
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("Mask 2 Feather") && warning.contains("keyframes"))
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("Mask 3 Feather")
                    && warning.contains("no supported native keys"))
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("Mask 4 Expansion")
                    && warning.contains("expression"))
        );
    }

    #[test]
    fn review_text_malformed_mask_roto_bezier_is_diagnosed() {
        let (layer, source_size, mut malformed) = native_mask_case();
        let baseline = import_atom(&layer, source_size, &malformed);
        assert!(
            baseline.warnings.iter().all(|warning| {
                !(warning.contains("RotoBezier") && warning.contains("malformed"))
            }),
            "baseline warnings: {:?}",
            baseline.warnings
        );

        let value = path_value_mut(&mut malformed).expect("fixture has a native path value");
        let descriptor = value
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
            .and_then(Chunk::children_mut)
            .expect("path has descriptor storage");
        let flag = descriptor
            .iter_mut()
            .find(|chunk| chunk.id() == *b"tdsb")
            .expect("path has a RotoBezier flag");
        *flag = Chunk::data(*b"tdsb", Vec::new()).unwrap();

        let imported = import_atom(&layer, source_size, &malformed);
        assert_eq!(
            imported.guide_ids.len(),
            1,
            "the static editable mask must survive malformed RotoBezier metadata"
        );
        assert!(
            imported.warnings.iter().any(|warning| {
                warning.contains("Mask 1")
                    && warning.contains("RotoBezier")
                    && warning.contains("malformed")
            }),
            "warnings: {:?}",
            imported.warnings
        );

        let (layer, source_size, mut missing_property) = native_mask_case();
        let value = path_value_mut(&mut missing_property).expect("fixture has a native path value");
        value.retain(|chunk| chunk.list_kind() != Some(*b"tdbs"));
        let imported = import_atom(&layer, source_size, &missing_property);
        assert_eq!(
            imported.guide_ids.len(),
            1,
            "the static editable mask must survive missing RotoBezier metadata"
        );
        assert!(
            imported.warnings.iter().any(|warning| {
                warning.contains("Mask 1")
                    && warning.contains("RotoBezier")
                    && warning.contains("missing RotoBezier property")
            }),
            "warnings: {:?}",
            imported.warnings
        );
    }

    #[test]
    fn review_text_malformed_variable_feather_is_diagnosed() {
        let (layer, source_size, mut malformed) = native_mask_case();
        let baseline = import_atom(&layer, source_size, &malformed);
        assert!(
            baseline.warnings.iter().all(|warning| {
                !(warning.contains("variable-width feather") && warning.contains("malformed"))
            }),
            "baseline warnings: {:?}",
            baseline.warnings
        );

        let value = path_value_mut(&mut malformed).expect("fixture has a native path value");
        let keys = value
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"omks"))
            .and_then(Chunk::children_mut)
            .expect("path has shape-key storage");
        keys.push(Chunk::data(*b"junk", Vec::new()).unwrap());

        let imported = import_atom(&layer, source_size, &malformed);
        assert_eq!(
            imported.guide_ids.len(),
            1,
            "the static editable mask must survive malformed feather metadata"
        );
        assert!(
            imported.warnings.iter().any(|warning| {
                warning.contains("Mask 1")
                    && warning.contains("variable-width feather")
                    && warning.contains("malformed")
            }),
            "warnings: {:?}",
            imported.warnings
        );
    }

    #[test]
    fn mask_pair_requires_two_remaining_generated_ids() {
        let project = read_project(include_bytes!("../../tests/fixtures/masks/mask.aep"))
            .expect("pinned native mask fixture parses");
        let (composition, layer) = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(comp) if !comp.layers.is_empty() => {
                    Some((comp, &comp.layers[0]))
                }
                _ => None,
            })
            .expect("fixture has a composition layer");
        for (start, expected_masks, expected_cursor) in
            [(u64::MAX - 2, 1, u64::MAX), (u64::MAX - 1, 0, u64::MAX - 1)]
        {
            let mut occurrence = super::super::group(
                LayerId::new(1),
                layer.name.to_string(),
                None,
                TimeRangeProperty::new(Time::ZERO, Duration::from_secs(10.0)),
            );
            let mut next_id = start;
            let imported = apply(
                layer,
                &mut occurrence,
                [u32::from(composition.width), u32::from(composition.height)],
                &mut next_id,
                &mut AnimationBudget::default(),
            );
            assert_eq!(occurrence.masks.len(), expected_masks);
            assert_eq!(occurrence.layers.len(), expected_masks);
            assert_eq!(next_id, expected_cursor);
            if expected_masks == 0 {
                assert!(
                    imported
                        .warnings
                        .iter()
                        .any(|warning| warning.contains("identifier space exhausted"))
                );
            }
        }
    }

    #[test]
    fn leading_opaque_subtract_inverts_coverage_but_later_subtract_does_not() {
        let (layer, source_size, original) = native_mask_case();
        for native_inverted in [false, true] {
            let mut atom = original.clone();
            let info = atom
                .iter_mut()
                .find(|chunk| chunk.id() == *b"mkif")
                .unwrap();
            let mut bytes = info.data_payload().unwrap().to_vec();
            bytes[0] = u8::from(native_inverted);
            bytes[6..8].copy_from_slice(&2_u16.to_be_bytes());
            *info = Chunk::data(*b"mkif", bytes).unwrap();
            let mut occurrence = super::super::group(
                LayerId::new(1),
                layer.name.to_string(),
                None,
                TimeRangeProperty::new(Time::ZERO, Duration::from_secs(10.0)),
            );
            let mut result = MaskImport {
                warnings: Vec::new(),
                animations: Vec::new(),
                guide_ids: Vec::new(),
            };
            let mut next_id = 2;
            let mut budget = AnimationBudget::default();
            for index in 1..=2 {
                import_mask(
                    &layer,
                    &atom,
                    index,
                    source_size,
                    MaskImportDestination {
                        occurrence: &mut occurrence,
                        next_id: &mut next_id,
                        budget: &mut budget,
                    },
                    &mut result,
                );
            }
            assert_eq!(occurrence.masks.len(), 2, "{:?}", result.warnings);
            assert_eq!(occurrence.masks[0].mode, MaskMode::Add);
            assert_eq!(occurrence.masks[0].inverted, !native_inverted);
            assert_eq!(occurrence.masks[1].mode, MaskMode::Subtract);
            assert_eq!(occurrence.masks[1].inverted, native_inverted);
        }
    }

    #[test]
    #[ignore = "requires licensed local Intro source via AEP_MASK_SOURCE"]
    fn local_external_source_leading_subtract_keeps_outside_the_native_mask() {
        use sha2::{Digest, Sha256};

        let bytes =
            std::fs::read(std::env::var_os("AEP_MASK_SOURCE").expect("licensed source path"))
                .expect("licensed source is readable");
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
        );
        let project = read_project(&bytes).expect("pinned native source parses");
        let ItemKind::Composition(composition) = &project.item(1197).expect("SH06 exists").kind
        else {
            panic!("source target must be a composition")
        };
        let layer = composition
            .layers
            .iter()
            .find(|layer| layer.record.id() == 1205)
            .expect("native subtract-mask occurrence exists");
        let mut occurrence = super::super::group(
            LayerId::new(1),
            layer.name.to_string(),
            None,
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(10.0)),
        );
        let imported = apply(
            layer,
            &mut occurrence,
            [3840, 1600],
            &mut 2,
            &mut AnimationBudget::default(),
        );
        assert_eq!(occurrence.masks.len(), 1, "{:?}", imported.warnings);
        assert_eq!(occurrence.masks[0].mode, MaskMode::Add);
        assert!(occurrence.masks[0].inverted);
        assert_eq!(occurrence.masks[0].opacity.value(), 1.0);
        assert_eq!(
            occurrence.layers.len(),
            1,
            "native editable mask guide retained"
        );
    }

    #[test]
    fn native_masks_become_source_local_guides_and_path_masks() {
        let project = read_project(include_bytes!("../../tests/fixtures/masks/mask.aep"))
            .expect("pinned native mask fixture parses");
        let (composition, layer) = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(comp) if !comp.layers.is_empty() => {
                    Some((comp, &comp.layers[0]))
                }
                _ => None,
            })
            .expect("fixture has a composition layer");
        let mut occurrence = super::super::group(
            LayerId::new(1),
            layer.name.to_string(),
            None,
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(10.0)),
        );
        // Mask and guide identifiers must survive the former 10,000 cutoff.
        let mut next_id = 10_000;
        let mut budget = AnimationBudget::default();
        let imported = apply(
            layer,
            &mut occurrence,
            [u32::from(composition.width), u32::from(composition.height)],
            &mut next_id,
            &mut budget,
        );
        assert!(
            !occurrence.masks.is_empty(),
            "warnings: {:?}",
            imported.warnings
        );
        assert_eq!(occurrence.masks.len(), occurrence.layers.len());
        assert_eq!(imported.guide_ids.len(), occurrence.masks.len());
        assert_eq!(imported.guide_ids[0].0, 1);
        let FxLayer::Shape(guide) = occurrence.layers[0].data() else {
            panic!("mask guide must be an editable shape")
        };
        // Resolver eligibility and paint suppression are separate: referenced
        // guides stay visible here, while mask-source collection consumes them.
        assert!(!guide.is_hidden, "mask resolver rejects hidden guides");
        assert!(guide.description.contains("consumed as a mask source"));
        assert_eq!(occurrence.masks[0].layer, Some(guide.id));
        assert_eq!(imported.guide_ids[0].1, guide.id);
        assert!(guide.shape.path.is_finite());
        assert!(guide.shape.fills.is_empty());
        assert!(guide.shape.strokes.is_empty());
        assert!(matches!(
            guide.shape.path.commands.first(),
            Some(fx_schema::ShapePathCommand::MoveTo { x, y, .. })
                if x.is_finite() && y.is_finite()
        ));
    }

    #[test]
    fn budget_exhaustion_keeps_static_mask_guide_and_reports_omitted_numeric_track() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/masks/import_mask_controls.aep"
        ))
        .expect("pinned native mask controls fixture parses");
        let ItemKind::Composition(composition) = &project.item(178).unwrap().kind else {
            panic!("MASK_FEATHER_KEYED composition")
        };
        let mut layer = composition
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "target")
            .expect("keyed mask target layer")
            .clone();
        assert!(patch_numeric_layout(
            &mut layer.content,
            "ADBE Mask Feather",
            2
        ));
        let mut occurrence = super::super::group(
            LayerId::new(1),
            layer.name.to_string(),
            None,
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(10.0)),
        );
        let mut next_id = 2;
        let mut budget = AnimationBudget::with_limit(0);

        let imported = apply(
            &layer,
            &mut occurrence,
            [u32::from(composition.width), u32::from(composition.height)],
            &mut next_id,
            &mut budget,
        );

        assert_eq!(occurrence.masks.len(), 1);
        assert_eq!(imported.guide_ids.len(), 1);
        assert!(matches!(occurrence.layers[0].data(), FxLayer::Shape(_)));
        assert!(imported.animations.is_empty());
        assert_eq!(budget.used(), 0);
        assert!(
            imported.warnings.iter().any(|warning| {
                warning.contains("Mask 1 ADBE Mask Feather")
                    && warning.contains("animation budget")
                    && warning.contains("static values retained")
            }),
            "warnings: {:?}",
            imported.warnings
        );
    }

    #[test]
    fn zero_source_dimensions_preserve_layer_without_invalid_guides() {
        let project = read_project(include_bytes!("../../tests/fixtures/masks/mask.aep"))
            .expect("pinned native mask fixture parses");
        let layer = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(comp) => comp.layers.first(),
                _ => None,
            })
            .expect("fixture has a layer");
        let mut occurrence = super::super::group(
            LayerId::new(1),
            layer.name.to_string(),
            None,
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(10.0)),
        );
        let mut budget = AnimationBudget::default();
        let imported = apply(layer, &mut occurrence, [0, 0], &mut 2, &mut budget);
        assert!(occurrence.masks.is_empty());
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("dimensions"))
        );
    }
}
