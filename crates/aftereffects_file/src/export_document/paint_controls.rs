//! Converter-local lowering of FX paint-presence controls to native paint opacity.
//!
//! This is intentionally a two-phase helper: callers materialize the exact paint
//! view used to build a vector program, then apply tracks only to those directly
//! owned paint records. It does not sample animators or change layer/group opacity.

use fx_schema::{
    LayerData, LayerId, NonNegativeProperty, PropType, PropertyKeyframeEasing, PropertyValue,
    animator::AnimatorData,
    layer::{
        BlendMode, ShapeFillRule, ShapeFillStyle, ShapeLineCap, ShapeLineJoin, ShapePaint,
        ShapeStrokeStyle,
    },
};

use crate::writer::{
    KeyframeEasing, NumericKeyframe, NumericTrack, VectorContent, VectorPaintSpec,
};

use super::effective_constant;

const NORMALIZED_FILL_DIAGNOSTIC: &str = "FillEnabled was normalized to every owned Fill's native paint Opacity because native vector-group toggle authoring is not established; paint opacity remains per-paint, with static solid alpha combined there, never into layer/group opacity.";
const NORMALIZED_STROKE_DIAGNOSTIC: &str = "StrokeEnabled was normalized to every owned Stroke's native paint Opacity because native vector-group toggle authoring is not established; paint opacity remains per-paint, with static solid alpha combined there, never into layer/group opacity.";
const DISABLED_STROKE_DIAGNOSTIC: &str = "A statically disabled authored Stroke was omitted while sibling paint was retained; the off-state is preserved, but editable disabled-Stroke authoring is lost.";
const MISSING_RECT_STROKE_DIAGNOSTIC: &str =
    "StrokeEnabled has no Rectangle stroke color to reveal; no native Stroke paint was invented.";

#[derive(Clone, Debug)]
struct BoolControl {
    keys: Vec<BoolKey>,
}

#[derive(Clone, Copy, Debug)]
struct BoolKey {
    time_millis: i64,
    value: bool,
}

impl BoolControl {
    fn can_enable(&self) -> bool {
        self.keys.iter().any(|key| key.value)
    }

    fn opacity_track(&self, original_opacity: f64) -> NumericTrack {
        NumericTrack {
            keys: self
                .keys
                .iter()
                .map(|key| NumericKeyframe {
                    time_millis: key.time_millis,
                    values: vec![if key.value { original_opacity } else { 0.0 }],
                    easing: vec![KeyframeEasing::Hold],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
                .collect(),
        }
    }
}

/// Paints and discrete controls owned by one FX vector-paint layer.
///
/// Build native paint records from [`Self::fills`] and [`Self::strokes`], then
/// call [`Self::apply_owned_tracks`] on that program's direct contents.
#[derive(Clone, Debug)]
pub(super) struct PaintMaterialization {
    fills: Vec<ShapeFillStyle>,
    strokes: Vec<ShapeStrokeStyle>,
    fill_control: Option<BoolControl>,
    stroke_control: Option<BoolControl>,
    diagnostics: Vec<&'static str>,
}

impl PaintMaterialization {
    pub(super) fn fills(&self) -> &[ShapeFillStyle] {
        &self.fills
    }

    pub(super) fn strokes(&self) -> &[ShapeStrokeStyle] {
        &self.strokes
    }

    pub(super) fn diagnostics(&self) -> &[&'static str] {
        &self.diagnostics
    }

    /// Applies presence controls to direct, owner-level paints only.
    ///
    /// Count and static-opacity checks make an accidental application to a
    /// nested operand paint or differently materialized program fail closed.
    pub(super) fn apply_owned_tracks(
        &self,
        contents: &mut [VectorContent],
    ) -> Result<(), &'static str> {
        let fill_count = contents
            .iter()
            .filter(|content| matches!(content, VectorContent::Paint(VectorPaintSpec::Fill { .. })))
            .count();
        let stroke_count = contents
            .iter()
            .filter(|content| {
                matches!(
                    content,
                    VectorContent::Paint(VectorPaintSpec::Stroke { .. })
                )
            })
            .count();
        if fill_count != self.fills.len() || stroke_count != self.strokes.len() {
            return Err("Materialized paint count does not match direct native owned paints");
        }

        let mut fill_index = 0;
        let mut stroke_index = 0;
        for content in contents {
            match content {
                VectorContent::Paint(VectorPaintSpec::Fill {
                    opacity,
                    animations,
                    ..
                }) => {
                    let expected = self.fills[fill_index].opacity * 100.0;
                    if *opacity != expected {
                        return Err("Materialized Fill opacity changed before control application");
                    }
                    if let Some(control) = &self.fill_control {
                        animations.opacity = Some(control.opacity_track(*opacity));
                    }
                    fill_index += 1;
                }
                VectorContent::Paint(VectorPaintSpec::Stroke {
                    opacity,
                    animations,
                    ..
                }) => {
                    let expected = self.strokes[stroke_index].opacity * 100.0;
                    if *opacity != expected {
                        return Err(
                            "Materialized Stroke opacity changed before control application",
                        );
                    }
                    if let Some(control) = &self.stroke_control {
                        animations.opacity = Some(control.opacity_track(*opacity));
                    }
                    stroke_index += 1;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Creates the converter-owned paint view before native vector-program building.
///
/// Text deliberately remains on the native text-document route. Other wrong
/// layer kinds fail instead of silently consuming a paint target.
pub(super) fn materialize(
    layer: &LayerData,
    entries: &crate::export_document::AnimationIndex<'_>,
) -> Result<PaintMaterialization, &'static str> {
    let id = layer.id();
    let fill_control = bool_control(entries, id, PropType::FillEnabled)?;
    let stroke_control = bool_control(entries, id, PropType::StrokeEnabled)?;
    let mut diagnostics = Vec::new();
    if fill_control.is_some() {
        diagnostics.push(NORMALIZED_FILL_DIAGNOSTIC);
    }
    if stroke_control.is_some() {
        diagnostics.push(NORMALIZED_STROKE_DIAGNOSTIC);
    }

    let (fills, strokes) = match layer {
        LayerData::Rect(layer) => {
            let fills = if layer.rect.fill_enabled || fill_control.is_some() {
                vec![ShapeFillStyle {
                    paint: layer.rect.fill_paint.clone().unwrap_or(ShapePaint::Solid {
                        color: layer.rect.fill_color,
                    }),
                    fill_rule: ShapeFillRule::NonZeroWinding,
                    blend_mode: layer.rect.fill_blend_mode.unwrap_or_default(),
                    opacity: 1.0,
                }]
            } else {
                Vec::new()
            };
            let strokes = match layer.rect.stroke_color {
                Some(color) if layer.rect.stroke_enabled || stroke_control.is_some() => {
                    vec![ShapeStrokeStyle {
                        enabled: true,
                        paint: ShapePaint::Solid { color },
                        width: layer.rect.stroke_width,
                        cap: ShapeLineCap::Butt,
                        join: layer.rect.stroke_join,
                        miter_limit: layer.rect.stroke_miter_limit,
                        blend_mode: BlendMode::default(),
                        opacity: 1.0,
                        dashes: layer.rect.stroke_dashes.clone(),
                        dash_offset: layer.rect.stroke_dash_offset,
                    }]
                }
                Some(_) => {
                    diagnostics.push(DISABLED_STROKE_DIAGNOSTIC);
                    Vec::new()
                }
                None => {
                    if stroke_control.is_some() {
                        diagnostics.push(MISSING_RECT_STROKE_DIAGNOSTIC);
                    }
                    Vec::new()
                }
            };
            (fills, strokes)
        }
        LayerData::Shape(layer) => materialize_array_paints(
            &layer.shape.fills,
            &layer.shape.strokes,
            fill_control.as_ref(),
            stroke_control.as_ref(),
            &mut diagnostics,
        ),
        LayerData::BooleanOperation(layer) => materialize_array_paints(
            &layer.fills,
            &layer.strokes,
            fill_control.as_ref(),
            stroke_control.as_ref(),
            &mut diagnostics,
        ),
        LayerData::Text(_) => {
            return Err(
                "Text FillEnabled/StrokeEnabled belongs to the native text document, not vector paint Opacity",
            );
        }
        LayerData::Media(_)
        | LayerData::Video(_)
        | LayerData::Image(_)
        | LayerData::Pag(_)
        | LayerData::Audio(_)
        | LayerData::Group(_)
        | LayerData::AiEdit(_)
        | LayerData::Adjustment(_) => {
            return Err("Layer kind does not own exportable Shape/Boolean/Rect paints");
        }
    };

    Ok(PaintMaterialization {
        fills,
        strokes,
        fill_control,
        stroke_control,
        diagnostics,
    })
}

fn materialize_array_paints(
    source_fills: &[ShapeFillStyle],
    source_strokes: &[ShapeStrokeStyle],
    fill_control: Option<&BoolControl>,
    stroke_control: Option<&BoolControl>,
    diagnostics: &mut Vec<&'static str>,
) -> (Vec<ShapeFillStyle>, Vec<ShapeStrokeStyle>) {
    let mut fills = source_fills.to_vec();
    if fills.is_empty() && fill_control.is_some_and(BoolControl::can_enable) {
        fills.push(default_fill());
    }

    let strokes = if let Some(control) = stroke_control {
        let mut strokes = source_strokes.to_vec();
        if strokes.is_empty() && control.can_enable() {
            strokes.push(default_stroke());
        }
        // StrokeEnabled broadcasts to every retained style, irrespective of
        // each style's static enabled bit.
        for stroke in &mut strokes {
            stroke.enabled = true;
        }
        strokes
    } else {
        let disabled = source_strokes
            .iter()
            .filter(|stroke| !stroke.enabled)
            .count();
        diagnostics.extend(std::iter::repeat_n(DISABLED_STROKE_DIAGNOSTIC, disabled));
        source_strokes
            .iter()
            .filter(|stroke| stroke.enabled)
            .cloned()
            .collect()
    };
    (fills, strokes)
}

fn bool_control(
    entries: &crate::export_document::AnimationIndex<'_>,
    id: LayerId,
    property: PropType,
) -> Result<Option<BoolControl>, &'static str> {
    let mut matching = entries.iter().filter(|entry| {
        entry
            .target
            .as_property()
            .is_some_and(|target| target.layer_id() == id && target.property_type() == property)
    });
    let Some(entry) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        return Err("Multiple paint-enable animators target one property");
    }
    if !entry.dependencies.is_empty()
        || entry.random_seed_target.is_some()
        || !entry.layer_refs.is_empty()
    {
        return Err("Dependent paint-enable animator cannot be represented natively");
    }

    match entry.animator.data() {
        AnimatorData::Constant { value } => Ok(Some(BoolControl {
            keys: vec![BoolKey {
                time_millis: 0,
                value: bool_value(value)?,
            }],
        })),
        AnimatorData::Keyframes { enabled: false, .. } => {
            let value = effective_constant(&entry.animator)
                .ok_or("Disabled paint-enable animator has no runtime-visible constant")?;
            Ok(Some(BoolControl {
                keys: vec![BoolKey {
                    time_millis: 0,
                    value: bool_value(value)?,
                }],
            }))
        }
        AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } => {
            let mut keys = Vec::with_capacity(track.keyframes().len());
            for (index, key) in track.keyframes().iter().enumerate() {
                if index > 0 && key.easing() != PropertyKeyframeEasing::Hold {
                    return Err("Boolean paint-enable transitions require incoming Hold easing");
                }
                if key.spatial_in_tangent().is_some() || key.spatial_out_tangent().is_some() {
                    return Err("Boolean paint-enable keys cannot carry spatial tangents");
                }
                keys.push(BoolKey {
                    time_millis: key.layer_time().as_millis(),
                    value: bool_value(key.value())?,
                });
            }
            Ok(Some(BoolControl { keys }))
        }
        AnimatorData::JsScript { .. } => {
            Err("JavaScript paint-enable animators are not evaluated or baked")
        }
    }
}

fn bool_value(value: &PropertyValue) -> Result<bool, &'static str> {
    match value {
        PropertyValue::Bool(value) => Ok(*value),
        _ => Err("Paint-enable animator value must be Boolean"),
    }
}

fn default_fill() -> ShapeFillStyle {
    ShapeFillStyle {
        paint: ShapePaint::Solid {
            color: [1.0, 1.0, 1.0, 1.0],
        },
        fill_rule: ShapeFillRule::default(),
        blend_mode: BlendMode::default(),
        opacity: 1.0,
    }
}

fn default_stroke() -> ShapeStrokeStyle {
    ShapeStrokeStyle {
        enabled: true,
        paint: ShapePaint::Solid {
            color: [0.0, 0.0, 0.0, 1.0],
        },
        width: NonNegativeProperty::new(1.0).expect("1.0 is finite and non-negative"),
        cap: ShapeLineCap::default(),
        join: ShapeLineJoin::default(),
        miter_limit: 4.0,
        blend_mode: BlendMode::default(),
        opacity: 1.0,
        dashes: Vec::new(),
        dash_offset: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_schema::animator::AnimationGraphEntry;
    use fx_schema::{
        PropertyTarget, TimeOffset,
        animator::{KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack},
    };
    use std::collections::BTreeMap;

    fn entry(property: PropType, animator: PropertyAnimator) -> AnimationGraphEntry {
        AnimationGraphEntry {
            target: PropertyTarget::layer(LayerId::new(7), property),
            animator,
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: BTreeMap::new(),
        }
    }

    fn bool_keys() -> PropertyAnimator {
        PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(vec![
                PropertyKeyframe::new(
                    KeyframeId::new("off"),
                    TimeOffset::from_millis(0),
                    PropertyValue::Bool(false),
                    PropertyKeyframeEasing::Hold,
                ),
                PropertyKeyframe::new(
                    KeyframeId::new("on"),
                    TimeOffset::from_millis(250),
                    PropertyValue::Bool(true),
                    PropertyKeyframeEasing::Hold,
                ),
            ])
            .expect("Hold-eased Bool keys are valid"),
        )
    }

    fn disabled_bool(value: bool) -> PropertyAnimator {
        let animator = bool_keys();
        let mut data = animator.data().clone();
        let AnimatorData::Keyframes {
            enabled,
            disabled_value,
            ..
        } = &mut data
        else {
            panic!("paint fixture is keyed")
        };
        *enabled = false;
        *disabled_value = Some(PropertyValue::Bool(value));
        PropertyAnimator::from_data(&data).unwrap()
    }

    #[test]
    fn disabled_paint_presence_controls_materialize_runtime_visible_state() {
        let off_entry = entry(PropType::FillEnabled, disabled_bool(false));
        let off = bool_control(
            &crate::export_document::AnimationIndex::new(&[off_entry]),
            LayerId::new(7),
            PropType::FillEnabled,
        )
        .unwrap()
        .expect("disabledValue false remains a materialized control");
        assert!(!off.can_enable());
        assert_eq!(off.opacity_track(42.0).keys[0].values, [0.0]);

        let authored_fill = ShapeFillStyle {
            opacity: 0.42,
            ..default_fill()
        };
        let authored_stroke = ShapeStrokeStyle {
            opacity: 0.73,
            ..default_stroke()
        };
        let mut diagnostics = Vec::new();
        let (fills, strokes) = materialize_array_paints(
            std::slice::from_ref(&authored_fill),
            std::slice::from_ref(&authored_stroke),
            Some(&off),
            None,
            &mut diagnostics,
        );
        assert_eq!(fills, [authored_fill]);
        assert_eq!(strokes, std::slice::from_ref(&authored_stroke));

        let on_entry = entry(PropType::FillEnabled, disabled_bool(true));
        let on = bool_control(
            &crate::export_document::AnimationIndex::new(&[on_entry]),
            LayerId::new(7),
            PropType::FillEnabled,
        )
        .unwrap()
        .expect("disabledValue true remains a materialized control");
        assert!(on.can_enable());
        let (fills, strokes) = materialize_array_paints(
            &[],
            std::slice::from_ref(&authored_stroke),
            Some(&on),
            None,
            &mut diagnostics,
        );
        assert_eq!(fills, [default_fill()]);
        assert_eq!(strokes, [authored_stroke]);
        assert_eq!(on.opacity_track(100.0).keys[0].values, [100.0]);
    }

    #[test]
    fn hold_and_constant_controls_preserve_discrete_values() {
        let hold = entry(PropType::FillEnabled, bool_keys());
        let control = bool_control(
            &crate::export_document::AnimationIndex::new(&[hold]),
            LayerId::new(7),
            PropType::FillEnabled,
        )
        .expect("Hold control is supported")
        .expect("control exists");
        let track = control.opacity_track(37.5);
        assert_eq!(track.keys[0].values, [0.0]);
        assert_eq!(track.keys[1].values, [37.5]);
        assert!(
            track
                .keys
                .iter()
                .all(|key| key.easing == [KeyframeEasing::Hold])
        );

        let constant = entry(
            PropType::StrokeEnabled,
            PropertyAnimator::constant(PropertyValue::Bool(true)).expect("Bool constant is valid"),
        );
        let control = bool_control(
            &crate::export_document::AnimationIndex::new(&[constant]),
            LayerId::new(7),
            PropType::StrokeEnabled,
        )
        .expect("constant is supported")
        .expect("control exists");
        assert_eq!(control.opacity_track(62.0).keys[0].values, [62.0]);
    }

    #[test]
    fn non_hold_bool_transition_is_rejected() {
        let invalid = PropertyKeyframeTrack::new(vec![
            PropertyKeyframe::new(
                KeyframeId::new("off"),
                TimeOffset::from_millis(0),
                PropertyValue::Bool(false),
                PropertyKeyframeEasing::Hold,
            ),
            PropertyKeyframe::new(
                KeyframeId::new("on"),
                TimeOffset::from_millis(250),
                PropertyValue::Bool(true),
                PropertyKeyframeEasing::Linear,
            ),
        ]);
        assert!(matches!(
            invalid,
            Err(fx_schema::animator::PropertyKeyframeError::ContinuousEasingRequiresNumericValue {
                keyframe_id,
                value_kind: "bool",
            }) if keyframe_id == "on"
        ));
    }

    #[test]
    fn empty_arrays_materialize_only_reachable_runtime_defaults() {
        let on = BoolControl {
            keys: vec![BoolKey {
                time_millis: 0,
                value: true,
            }],
        };
        let off = BoolControl {
            keys: vec![BoolKey {
                time_millis: 0,
                value: false,
            }],
        };
        let mut diagnostics = Vec::new();
        let (fills, strokes) =
            materialize_array_paints(&[], &[], Some(&on), Some(&on), &mut diagnostics);
        assert_eq!(fills, [default_fill()]);
        assert_eq!(strokes, [default_stroke()]);

        let (fills, strokes) =
            materialize_array_paints(&[], &[], Some(&off), Some(&off), &mut diagnostics);
        assert!(fills.is_empty());
        assert!(strokes.is_empty());
    }

    #[test]
    fn static_disabled_stroke_is_omitted_without_losing_fill() {
        let fill = ShapeFillStyle {
            opacity: 0.42,
            ..default_fill()
        };
        let stroke = ShapeStrokeStyle {
            enabled: false,
            opacity: 0.73,
            ..default_stroke()
        };
        let mut diagnostics = Vec::new();
        let (fills, strokes) = materialize_array_paints(
            std::slice::from_ref(&fill),
            std::slice::from_ref(&stroke),
            None,
            None,
            &mut diagnostics,
        );
        assert_eq!(fills, [fill]);
        assert!(strokes.is_empty());
        assert_eq!(diagnostics, [DISABLED_STROKE_DIAGNOSTIC]);
    }
}
