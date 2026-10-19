use super::*;

/// Hard sectors use a union or intersection of half-planes; with a static
/// Start Angle only one boundary moves.
pub(in crate::structure_document) fn apply(
    layer: &Layer,
    comp: &Composition,
    source: Option<&ProjectItem>,
    occurrence: &mut GroupLayer,
    next_id: &mut u64,
    budget: &mut AnimationBudget,
    supported_plane: bool,
) -> Result<(Vec<AnimationGraphEntry>, Vec<String>), String> {
    let flags = layer.record.flags();
    if !supported_plane
        || flags.three_d_layer
        || flags.collapse_transformation
        || flags.adjustment_layer
        || !occurrence.masks.is_empty()
        || comp.pixel_aspect.0 != comp.pixel_aspect.1
    {
        return Err("unsupported sector source plane or authored masks".into());
    }
    let size = match source {
        Some(ProjectItem {
            kind: ItemKind::Composition(source),
            ..
        }) if source.pixel_aspect.0 == source.pixel_aspect.1 => [source.width, source.height],
        Some(ProjectItem {
            solid: Some(Ok(source)),
            ..
        }) if source.pixel_aspect.0 == source.pixel_aspect.1 => [source.width, source.height],
        _ => return Err("sector requires a solid or square-pixel composition source".into()),
    };
    if size.contains(&0) {
        return Err("empty sector source canvas".into());
    }
    let (effects, warnings) = native::read_effects(&layer.content, size.map(f64::from));
    if !warnings.is_empty()
        || effects.iter().filter(|e| e.enabled).any(|e| {
            !matches!(
                e.match_name.as_str(),
                MATCH_NAME | "ADBE Fill" | "ADBE Tint"
            )
        })
    {
        return Err(
            "sector may commute only with other wipes and alpha-preserving Fill/Tint".into(),
        );
    }
    let clock = NumericAnimationClock::parent_identity(layer).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut messages = Vec::new();
    let mut admitted = false;
    for effect in effects
        .iter()
        .filter(|e| e.enabled && e.match_name == MATCH_NAME)
    {
        let checkpoint = budget.checkpoint();
        match lower(layer, effect, size, occurrence, *next_id, budget, clock) {
            Ok(sector) => {
                occurrence.layers = vec![sector.group];
                let operation = if sector.union {
                    "union"
                } else {
                    "intersection"
                };
                entries.extend(sector.animations);
                *next_id = sector.end_id;
                admitted = true;
                messages.push(format!("Radial Wipe {}: Completion lowered to two editable half-plane guides with {operation} and Rotation keys; live linkage and native edge antialiasing are not retained", effect.index));
            }
            Err(error) => {
                budget.rollback(checkpoint);
                messages.push(format!("Radial Wipe {} omitted: {error}; other admitted wipes, content and effects retained", effect.index));
            }
        }
    }
    if !admitted {
        return Err(messages.join("; "));
    }
    Ok((entries, messages))
}

struct Sector {
    union: bool,
    group: fx_schema::Layer,
    animations: Vec<AnimationGraphEntry>,
    end_id: u64,
}

fn lower(
    layer: &Layer,
    effect: &DecodedEffect,
    size: [u16; 2],
    occurrence: &GroupLayer,
    next_id: u64,
    budget: &mut AnimationBudget,
    clock: NumericAnimationClock,
) -> Result<Sector, String> {
    let completion = parameter(effect, "0001")?;
    let values = completion
        .values
        .iter()
        .filter(|_| completion.keyframes.is_empty())
        .chain(completion.keyframes.iter().flat_map(|k| &k.values));
    let union = values.clone().any(|v| *v < 50.0);
    let range = if union { 0.0..=50.0 } else { 50.0..=100.0 };
    if completion.expression_enabled
        || values.clone().any(|v| !v.is_finite() || !range.contains(v))
        || completion.keyframes.iter().any(|k| k.values.len() != 1)
    {
        return Err("Completion must be authored, finite and confined to 0–50% or 50–100%".into());
    }
    let start = static_values(effect, "0002", 1)?[0];
    let direction = static_values(effect, "0004", 1)?[0];
    if !matches!(direction, 1.0 | 2.0) || static_values(effect, "0005", 1)? != [0.0] {
        return Err("sector requires direction 1/2 and zero feather".into());
    }
    let center = center(layer, effect, size)?;
    let path = half_plane_path(size, center)?;
    let initial = completion
        .keyframes
        .first()
        .map_or(completion.values.as_slice(), |k| k.values.as_slice());
    let [initial] = initial else {
        return Err("Completion must be scalar".into());
    };
    let (fixed, offset, scale) = if direction == 1.0 {
        (start, start - 180.0, 3.6)
    } else {
        (start + 180.0, start + 360.0, -3.6)
    };
    let mut angle = completion.clone();
    for value in angle
        .values
        .iter_mut()
        .chain(angle.keyframes.iter_mut().flat_map(|k| &mut k.values))
    {
        *value += offset / scale;
    }
    let mut end_id = next_id;
    let raw =
        super::super::reserve_ids(&mut end_id, 5).ok_or("generated identifier space exhausted")?;
    let group_id = LayerId::new(raw + 4);
    let moving_id = LayerId::new(raw + 3);
    let target = NumericAnimationTarget::float(
        PropertyTarget::layer(moving_id, fx_schema::PropType::Rotation),
        0,
        scale,
    );
    let (animations, warnings) =
        animation::numeric_entries("Radial Wipe Completion", &angle, &[target], clock, budget);
    if !warnings.is_empty() || (completion.animated && animations.is_empty()) {
        return Err(format!(
            "Completion animation not admitted: {}",
            warnings.join("; ")
        ));
    }
    for entry in &animations {
        if let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data() {
            for key in track.keyframes() {
                if let fx_schema::PropertyKeyframeEasing::CubicBezier { y1, y2, .. } = key.easing()
                    && (!(0.0..=1.0).contains(&y1) || !(0.0..=1.0).contains(&y2))
                {
                    return Err("Completion easing may leave the bounded sector range".into());
                }
            }
        }
    }
    let mut guides = Vec::with_capacity(2);
    let mut masks = Vec::with_capacity(2);
    for (index, rotation) in [fixed, offset + scale * initial].into_iter().enumerate() {
        let id_offset = u64::try_from(index).expect("two guides") * 2;
        let id = LayerId::new(raw + id_offset + 1);
        let transform = Transform {
            anchor_point: [0.0, 0.0],
            position: Position::TwoD(center),
            scale: [100.0, 100.0],
            rotation,
            skew: 0.0,
            skew_axis: 0.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            orientation: [0.0, 0.0, 0.0],
            opacity: PercentageProperty::new(100.0).expect("valid percentage"),
        };
        guides.push(
            fx_schema::Layer::from_data(&LayerData::Shape(ShapeLayer {
                id,
                parent: Some(group_id),
                name: "Radial Wipe sector boundary".into(),
                description: "Editable source-local hard-edge sector guide".into(),
                is_hidden: false,
                blend_mode: Default::default(),
                track_matte: None,
                masks: vec![],
                effects: vec![],
                motion_blur: false,
                active_range: TimeRangeProperty::new(
                    Time::ZERO,
                    Duration::from_secs(super::super::MAX_TIME_SECS),
                ),
                transform,
                shape: ShapeContent {
                    path: path.clone(),
                    fills: vec![],
                    strokes: vec![],
                    round_corners: None,
                    offset_paths: None,
                    trim: None,
                    poly_star: None,
                    ellipse: None,
                },
            }))
            .map_err(|e| e.to_string())?,
        );
        masks.push(PathMask {
            id: FxItemId::new(raw + id_offset),
            mode: if union || index == 0 {
                MaskMode::Add
            } else {
                MaskMode::Intersect
            },
            inverted: false,
            layer: Some(id),
            legacy_path: None,
            feather: [0.0, 0.0],
            expansion: 0.0,
            opacity: NonNegativeProperty::new(1.0).expect("nonnegative opacity"),
        });
    }
    // Separate stages avoid relying on repeated Intersect masks in one runtime stack.
    let mut group = super::super::group(
        group_id,
        "Radial Wipe sector".into(),
        Some(occurrence.id),
        occurrence.playback.input_range(),
    );
    group.masks = masks;
    for child in &occurrence.layers {
        let LayerData::Group(mut content) = child.data().clone() else {
            return Err("sector requires an editable source content Group".into());
        };
        content.parent = Some(group_id);
        group.layers.push(
            fx_schema::Layer::from_data(&LayerData::Group(content)).map_err(|e| e.to_string())?,
        );
    }
    group.layers.extend(guides);
    Ok(Sector {
        union,
        group: fx_schema::Layer::from_data(&LayerData::Group(group)).map_err(|e| e.to_string())?,
        animations,
        end_id,
    })
}
