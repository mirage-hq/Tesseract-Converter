//! Editable mapping for AE Transform keyframes and authored Time Remap.

/// Legacy script fixture helper retained only for negative conversion tests.
#[cfg(test)]
pub(super) fn js_script(code: impl Into<String>) -> PropertyAnimator {
    PropertyAnimator::from_data(&fx_schema::animator::AnimatorData::JsScript {
        code: None,
        layer_time_js_code: Some(code.into()),
    })
    .expect("a script record containing one canonical code string is structurally valid")
}

#[cfg(test)]
pub(super) fn script_code(animator: &PropertyAnimator) -> &str {
    let fx_schema::animator::AnimatorData::JsScript {
        code: None,
        layer_time_js_code: Some(code),
    } = animator.data()
    else {
        panic!("expected generated owner-layer script, not legacy project-time code")
    };
    code
}

#[test]
fn legacy_script_fixture_wire_clock_is_owner_local() {
    let code = "return input.time.seconds;";
    let animator = js_script(code);
    assert_eq!(animator.wire_value()["layerTimeJsCode"], code);
    assert!(animator.wire_value().get("code").is_none());
    let restored: PropertyAnimator = serde_json::from_value(animator.wire_value().clone()).unwrap();
    assert_eq!(script_code(&restored), code);
}

mod expressions;
#[cfg(test)]
mod position_z_tests;
mod spatial_position;

use std::collections::HashMap;

use fx_keyframe_bake::value_curve::{ValueCurveError, ValueKey, fit_value_curve};
use fx_schema::animator::{
    AnimationGraphEntry, KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeEasing,
    PropertyKeyframeTrack,
};
use fx_schema::{
    LayerId, PropType, PropertyTarget, PropertyValue, Time, TimeOffset, TimeRangeProperty,
    TimeRemapExtrapolation, TimeRemapKeyframe, TimeRemapProperty,
};

use crate::{
    properties::{
        NumericKeyframe, NumericProperty, NumericValueKind, PropertyError, read_numeric, root_runs,
        unique_list,
    },
    rifx::Chunk,
    structure::{Composition, Layer, ProjectItem},
    structure_document::animation_budget::{
        AnimationBudget, GeneratedKeyframeIdSize, PropertyTrackEstimate, TimeRemapEstimate,
    },
};

pub(super) use expressions::{
    evaluated_numeric_entries, evaluated_transform_entries, rebased_samples, rebased_samples_with,
};

/// Clock used by the FX layer receiving an imported Transform track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AnimationTargetClock {
    /// The target has the same local clock as the native AE layer.
    #[cfg(test)]
    SourceLayerLocal,
    /// The target is an identity-clock wrapper in the immediate composition.
    ParentIdentity,
}

/// Clock mapping used by reusable numeric-property animation conversion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum NumericAnimationClock {
    SourceLocal { offset_secs: f64 },
    ParentIdentity { start: f64, stretch: f64 },
}

impl NumericAnimationClock {
    pub(super) const fn source_local() -> Self {
        Self::SourceLocal { offset_secs: 0.0 }
    }

    pub(super) const fn source_local_rebased(offset_secs: f64) -> Self {
        Self::SourceLocal { offset_secs }
    }

    pub(super) fn parent_identity(layer: &Layer) -> Result<Self, String> {
        let (Some(start), Some(stretch)) = (layer.record.start_time(), layer.record.stretch())
        else {
            return Err("invalid source-layer clock denominator".into());
        };
        if !start.is_finite() || !stretch.is_finite() || stretch == 0.0 {
            return Err(format!(
                "unsupported source-layer clock start={start} stretch={stretch}"
            ));
        }
        Ok(Self::ParentIdentity { start, stretch })
    }

    pub(super) fn seconds(self, local: f64) -> f64 {
        match self {
            Self::SourceLocal { offset_secs } => local - offset_secs,
            Self::ParentIdentity { start, stretch } => start + local * stretch,
        }
    }

    pub(super) fn reversed(self) -> bool {
        matches!(self, Self::ParentIdentity { stretch, .. } if stretch < 0.0)
    }

    fn source_seconds(self, output_secs: f64) -> Result<f64, String> {
        let source = match self {
            Self::SourceLocal { offset_secs } => output_secs + offset_secs,
            Self::ParentIdentity { start, stretch } if stretch != 0.0 => {
                (output_secs - start) / stretch
            }
            Self::ParentIdentity { .. } => {
                return Err("zero-stretch numeric animation clock".into());
            }
        };
        source
            .is_finite()
            .then_some(source)
            .ok_or_else(|| "non-finite numeric animation clock".into())
    }
}

#[derive(Clone, Debug)]
enum NumericTargetValue {
    StrokeJoin,
    OpacityColor {
        rgb: [f64; 3],
        scale: f64,
    },
    Float {
        component: usize,
        scale: f64,
    },
    Vector2 {
        components: [usize; 2],
        scale: [f64; 2],
    },
    Color {
        components: [usize; 4],
        scale: [f64; 4],
    },
}

/// Destination address and component/unit mapping for one numeric animation track.
#[derive(Clone, Debug)]
pub(super) struct NumericAnimationTarget {
    target: PropertyTarget,
    value: NumericTargetValue,
}

impl NumericAnimationTarget {
    pub(super) const fn stroke_join(target: PropertyTarget) -> Self {
        Self {
            target,
            value: NumericTargetValue::StrokeJoin,
        }
    }

    pub(super) const fn float(target: PropertyTarget, component: usize, scale: f64) -> Self {
        Self {
            target,
            value: NumericTargetValue::Float { component, scale },
        }
    }

    pub(super) const fn vector2(
        target: PropertyTarget,
        components: [usize; 2],
        scale: [f64; 2],
    ) -> Self {
        Self {
            target,
            value: NumericTargetValue::Vector2 { components, scale },
        }
    }

    pub(super) const fn color(
        target: PropertyTarget,
        components: [usize; 4],
        scale: [f64; 4],
    ) -> Self {
        Self {
            target,
            value: NumericTargetValue::Color { components, scale },
        }
    }

    /// A scalar opacity curve controls alpha while the authored RGB stays fixed.
    /// Unlike native Color keys, this uses scalar temporal speed normalization.
    pub(super) const fn opacity_color(target: PropertyTarget, rgb: [f64; 3]) -> Self {
        Self {
            target,
            value: NumericTargetValue::OpacityColor {
                rgb,
                scale: 1.0 / 255.0,
            },
        }
    }

    pub(super) const fn property_target(&self) -> &PropertyTarget {
        &self.target
    }

    /// The same destination with every component's unit scale multiplied, for
    /// expression values exposed in different units than native storage.
    pub(super) fn scaled(&self, factor: f64) -> Self {
        let mut scaled = self.clone();
        match &mut scaled.value {
            NumericTargetValue::Float { scale, .. } => *scale *= factor,
            NumericTargetValue::Vector2 { scale, .. } => {
                scale.iter_mut().for_each(|s| *s *= factor)
            }
            NumericTargetValue::Color { scale, .. } => scale.iter_mut().for_each(|s| *s *= factor),
            NumericTargetValue::OpacityColor { scale, .. } => *scale *= factor,
            NumericTargetValue::StrokeJoin => {}
        }
        scaled
    }

    fn easing_component(&self) -> (usize, f64) {
        match self.value {
            NumericTargetValue::StrokeJoin => (0, 1.0),
            NumericTargetValue::OpacityColor { scale, .. } => (0, scale),
            NumericTargetValue::Float { component, scale } => (component, scale),
            NumericTargetValue::Vector2 { components, scale } => (components[0], scale[0]),
            NumericTargetValue::Color { components, scale } => (components[0], scale[0]),
        }
    }

    fn vector_easing_components(&self) -> Option<[(usize, f64); 2]> {
        match self.value {
            NumericTargetValue::Vector2 { components, scale } => {
                Some([(components[0], scale[0]), (components[1], scale[1])])
            }
            _ => None,
        }
    }

    fn has_equal_endpoint_excursion(&self, from: &NumericKeyframe, to: &NumericKeyframe) -> bool {
        let excursion = |component, scale| equal_endpoint_excursion(from, to, component, scale);
        match self.value {
            NumericTargetValue::StrokeJoin => false,
            NumericTargetValue::OpacityColor { scale, .. } => excursion(0, scale),
            NumericTargetValue::Float { component, scale } => excursion(component, scale),
            NumericTargetValue::Vector2 { components, scale } => {
                excursion(components[0], scale[0]) || excursion(components[1], scale[1])
            }
            NumericTargetValue::Color { components, scale } => components
                .into_iter()
                .zip(scale)
                .any(|(component, scale)| excursion(component, scale)),
        }
    }

    fn validate_components(&self, values: &[f64]) -> Result<(), String> {
        let required = match self.value {
            NumericTargetValue::StrokeJoin | NumericTargetValue::OpacityColor { .. } => {
                [Some(0), None, None, None]
            }
            NumericTargetValue::Float { component, .. } => [Some(component), None, None, None],
            NumericTargetValue::Vector2 { components, .. } => {
                [Some(components[0]), Some(components[1]), None, None]
            }
            NumericTargetValue::Color { components, .. } => components.map(Some),
        };
        required
            .into_iter()
            .flatten()
            .find(|component| *component >= values.len())
            .map_or(Ok(()), |component| {
                Err(format!("lacks component {component}"))
            })
    }

    fn interpolation_error(
        &self,
        from: &[f64],
        to: &[f64],
        actual: &[f64],
        progress: f64,
    ) -> Option<f64> {
        let error = |component: usize, scale: f64| {
            let (from, to, actual) = (
                *from.get(component)?,
                *to.get(component)?,
                *actual.get(component)?,
            );
            Some(((from + (to - from) * progress - actual) * scale).abs())
        };
        match self.value {
            NumericTargetValue::StrokeJoin => Some(0.0),
            NumericTargetValue::OpacityColor { scale, .. } => error(0, scale),
            NumericTargetValue::Float { component, scale } => error(component, scale),
            NumericTargetValue::Vector2 { components, scale } => {
                Some(error(components[0], scale[0])?.max(error(components[1], scale[1])?))
            }
            NumericTargetValue::Color { components, scale } => components
                .into_iter()
                .zip(scale)
                .try_fold(0.0_f64, |maximum, (component, scale)| {
                    error(component, scale).map(|error| maximum.max(error))
                }),
        }
    }

    fn value_at(&self, key: &NumericKeyframe) -> Result<PropertyValue, String> {
        let component = |index: usize, scale: f64| {
            key.values
                .get(index)
                .copied()
                .map(|value| value * scale)
                .ok_or_else(|| format!("lacks component {index}"))
        };
        match self.value {
            NumericTargetValue::OpacityColor { rgb, scale } => Ok(PropertyValue::Color([
                rgb[0],
                rgb[1],
                rgb[2],
                component(0, scale)?,
            ])),
            NumericTargetValue::StrokeJoin => {
                let value = component(0, 1.0)?;
                let join = match value {
                    1.0 => "miter",
                    2.0 => "round",
                    3.0 => "bevel",
                    _ => return Err(format!("has unsupported Stroke Join value {value}")),
                };
                Ok(PropertyValue::String(join.into()))
            }
            NumericTargetValue::Float {
                component: index,
                scale,
            } => component(index, scale).map(PropertyValue::Float),
            NumericTargetValue::Vector2 { components, scale } => Ok(PropertyValue::Vector2([
                component(components[0], scale[0])?,
                component(components[1], scale[1])?,
            ])),
            NumericTargetValue::Color { components, scale } => Ok(PropertyValue::Color([
                component(components[0], scale[0])?,
                component(components[1], scale[1])?,
                component(components[2], scale[2])?,
                component(components[3], scale[3])?,
            ])),
        }
    }
}

/// Converts supported AE Transform tracks to editable animation graph entries.
///
/// AE keys are stored in the source layer's local clock. Callers that target a
/// transform-only wrapper must request [`AnimationTargetClock::ParentIdentity`]
/// so start/stretch are applied exactly once.
#[cfg(test)]
pub(super) fn transform_entries(
    layer: &Layer,
    composition: &Composition,
    target_id: LayerId,
    target_clock: AnimationTargetClock,
    anchor_scale: [f64; 2],
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    transform_entries_filtered(
        layer,
        composition,
        None,
        target_id,
        target_clock,
        anchor_scale,
        true,
        budget,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "Explicit native source and destination animation contexts"
)]
pub(super) fn transform_entries_with_sources(
    layer: &Layer,
    composition_id: u32,
    composition: &Composition,
    items: &HashMap<u32, &ProjectItem>,
    target_id: LayerId,
    target_clock: AnimationTargetClock,
    anchor_scale: [f64; 2],
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    transform_entries_filtered(
        layer,
        composition,
        Some((composition_id, items)),
        target_id,
        target_clock,
        anchor_scale,
        true,
        budget,
    )
}

/// Parent wrappers deliberately do not inherit opacity, so exclude it before
/// animation estimation or reservation rather than discarding a charged entry.
#[cfg(test)]
pub(super) fn transform_parent_entries(
    layer: &Layer,
    composition: &Composition,
    target_id: LayerId,
    anchor_scale: [f64; 2],
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    transform_entries_filtered(
        layer,
        composition,
        None,
        target_id,
        AnimationTargetClock::ParentIdentity,
        anchor_scale,
        false,
        budget,
    )
}

pub(super) fn transform_parent_entries_with_sources(
    layer: &Layer,
    composition_id: u32,
    composition: &Composition,
    items: &HashMap<u32, &ProjectItem>,
    target_id: LayerId,
    anchor_scale: [f64; 2],
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    transform_entries_filtered(
        layer,
        composition,
        Some((composition_id, items)),
        target_id,
        AnimationTargetClock::ParentIdentity,
        anchor_scale,
        false,
        budget,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "Explicit native source and destination animation contexts"
)]
fn transform_entries_filtered(
    layer: &Layer,
    composition: &Composition,
    sources: Option<(u32, &HashMap<u32, &ProjectItem>)>,
    target_id: LayerId,
    target_clock: AnimationTargetClock,
    anchor_scale: [f64; 2],
    include_opacity: bool,
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    let properties = sources.map_or_else(
        || super::control_links::read_layer_transform(layer, composition),
        |(composition_id, items)| {
            super::control_links::read_layer_transform_with_sources(
                layer,
                composition_id,
                composition,
                items,
            )
        },
    );
    let (properties, mut warnings) = match properties {
        Ok(result) => result,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!("Transform group: {error}; animation omitted")],
            );
        }
    };
    let mut entries = Vec::new();
    let clock = match target_clock {
        #[cfg(test)]
        AnimationTargetClock::SourceLayerLocal => NumericAnimationClock::source_local(),
        AnimationTargetClock::ParentIdentity => match NumericAnimationClock::parent_identity(layer)
        {
            Ok(clock) => clock,
            Err(error) => {
                return (
                    Vec::new(),
                    vec![format!(
                        "Transform animation: {error}; wrapper animation omitted"
                    )],
                );
            }
        },
    };
    let separated = properties
        .iter()
        .find(|property| property.match_name == "ADBE Position")
        .and_then(|property| property.numeric.as_ref().ok())
        .is_some_and(|property| property.dimensions_separated);

    for property in properties {
        let name = property.match_name.as_str();
        if (!include_opacity && name == "ADBE Opacity")
            || (name == "ADBE Position" && separated)
            || (name.starts_with("ADBE Position_") && !separated)
        {
            continue;
        }
        let numeric = match property.numeric {
            Ok(numeric) => numeric,
            Err(error) => {
                warnings.push(format!("{name}: {error}; animation omitted"));
                continue;
            }
        };
        if numeric.keyframes.is_empty() {
            continue;
        }
        if layer.record.flags().three_d_layer
            && matches!(name, "ADBE Anchor Point" | "ADBE Scale")
            && numeric.keyframes.iter().any(|key| key.values.len() >= 3)
        {
            warnings.push(format!(
                "{name}: Z component has no destination transform property; X/Y animation imported"
            ));
        }
        if name == "ADBE Orientation" {
            warnings.push("ADBE Orientation: AE interpolates authored orientations as quaternions; editable component-wise Euler tracks preserve key values but may differ between keys".into());
        }
        let (mut property_entries, property_warnings) = numeric_entries(
            name,
            &numeric,
            &targets(
                name,
                layer.record.flags().three_d_layer,
                target_id,
                anchor_scale,
            ),
            clock,
            budget,
        );
        entries.append(&mut property_entries);
        warnings.extend(property_warnings);
    }

    (entries, warnings)
}

/// Converts an authored AE Time Remap property to the existing playback schema.
///
/// Property key times use the layer-local clock; playback key times use the
/// immediate-parent clock, so the layer start/stretch affine transform is
/// applied exactly once. Source values are AE seconds.
pub(super) fn has_authored_time_remap(layer: &Layer) -> bool {
    root_runs(&layer.content).is_ok_and(|roots| {
        roots
            .into_iter()
            .any(|(name, _)| name == "ADBE Time Remapping")
    })
}

/// An authored AE Time Remap, before its lifetime gate is known.
pub(super) enum AuthoredRemap {
    /// Two or more keys on the parent clock, with AE's behavior outside them.
    Keys(TimeRemapProperty),
    /// One key: AE uses this source time over the whole layer lifetime.
    Constant(Time),
}

pub(super) fn time_remap(
    layer: &Layer,
    _composition: &Composition,
    target_id: LayerId,
    budget: &mut AnimationBudget,
) -> (Option<AuthoredRemap>, Vec<String>) {
    let numeric = match time_remap_numeric(layer) {
        Ok(Some(numeric)) => numeric,
        Ok(None) => return (None, Vec::new()),
        Err(error) => {
            return (
                None,
                vec![format!(
                    "ADBE Time Remapping: {error}; authored remap omitted"
                )],
            );
        }
    };
    // AE holds a keyed property's end values outside its keys. The separate
    // lifetime gate still makes the layer inactive outside its native span.
    let extrapolation = if !numeric.expression_enabled {
        TimeRemapExtrapolation::Hold
    } else if time_remap_property(layer)
        .ok()
        .flatten()
        .and_then(|property| super::control_links::expression(property).ok())
        .is_some_and(is_two_sided_cycle)
    {
        TimeRemapExtrapolation::Loop
    } else {
        return (None, vec!["ADBE Time Remapping: enabled AE expression is not the exact `loopIn() + loopOut() - value` cycle and requires the AE expression environment; authored remap omitted".into()]);
    };
    if numeric.keyframes.is_empty() {
        return (
            None,
            vec![
                "ADBE Time Remapping: no supported authored keys; affine layer timing retained"
                    .into(),
            ],
        );
    }
    let (Some(start), Some(stretch)) = (layer.record.start_time(), layer.record.stretch()) else {
        return (
            None,
            vec![
                "ADBE Time Remapping: invalid layer clock denominator; authored remap omitted"
                    .into(),
            ],
        );
    };
    if !start.is_finite() || !stretch.is_finite() || stretch == 0.0 {
        return (
            None,
            vec![format!(
                "ADBE Time Remapping: unsupported layer clock start={start} stretch={stretch}; authored remap omitted"
            )],
        );
    }
    if let [key] = numeric.keyframes.as_slice() {
        return match key.values.first().copied() {
            Some(value) if value.is_finite() && value >= 0.0 => (
                Some(AuthoredRemap::Constant(Time::from_secs(value))),
                vec![format!(
                    "ADBE Time Remapping: one authored key holds source time {value}s; FX requires two keys, so two equal-value keys span the layer lifetime"
                )],
            ),
            _ => (
                None,
                vec!["ADBE Time Remapping key 0: missing, negative or non-finite source time; authored remap omitted".into()],
            ),
        };
    }
    let mut warnings = Vec::new();
    let mut estimate = TimeRemapEstimate::default();
    for output_index in 0..numeric.keyframes.len() {
        let source_index = if stretch < 0.0 {
            numeric.keyframes.len() - 1 - output_index
        } else {
            output_index
        };
        let key = &numeric.keyframes[source_index];
        let parent_secs = start + key.time_secs * stretch;
        let Some(value_secs) = key.values.first().copied() else {
            warnings.push(format!("ADBE Time Remapping key {source_index}: missing scalar source time; authored remap omitted"));
            return (None, warnings);
        };
        if !parent_secs.is_finite()
            || parent_secs < 0.0
            || !value_secs.is_finite()
            || value_secs < 0.0
        {
            warnings.push(format!("ADBE Time Remapping key {source_index}: negative/non-finite parent or source time; authored remap omitted"));
            return (None, warnings);
        }
        let easing = if stretch < 0.0 {
            match reverse_easing_for_key(
                &numeric.keyframes,
                source_index,
                0,
                1.0,
                &mut warnings,
                "ADBE Time Remapping",
            ) {
                Ok(easing) => easing,
                Err(message) => {
                    warnings.push(message);
                    return (None, warnings);
                }
            }
        } else {
            easing_for_key(
                &numeric.keyframes,
                source_index,
                0,
                1.0,
                &mut warnings,
                "ADBE Time Remapping",
            )
        };
        let id = match GeneratedKeyframeIdSize::new(format_args!(
            "aep-remap-{target_id}-{output_index}"
        )) {
            Ok(id) => id,
            Err(error) => {
                warnings.push(format!(
                    "ADBE Time Remapping: {error}; affine layer timing retained"
                ));
                return (None, warnings);
            }
        };
        if let Err(error) = estimate.push_key(
            &id,
            Time::from_secs(parent_secs),
            Time::from_secs(value_secs),
            easing,
        ) {
            warnings.push(format!(
                "ADBE Time Remapping: {error}; affine layer timing retained"
            ));
            return (None, warnings);
        }
    }
    let reservation = match estimate.reservation_bytes(extrapolation, extrapolation) {
        Ok(bytes) => bytes,
        Err(error) => {
            warnings.push(format!(
                "ADBE Time Remapping: {error}; affine layer timing retained"
            ));
            return (None, warnings);
        }
    };
    let checkpoint = budget.checkpoint();
    if let Err(error) = budget.reserve(reservation) {
        warnings.push(format!(
            "ADBE Time Remapping: {error}; static content retained with affine timing"
        ));
        return (None, warnings);
    }

    // The estimate above retains only one bounded key at a time. Allocate the
    // destination vector only after the complete remap has been admitted.
    let mut keys = Vec::with_capacity(numeric.keyframes.len());
    for output_index in 0..numeric.keyframes.len() {
        let source_index = if stretch < 0.0 {
            numeric.keyframes.len() - 1 - output_index
        } else {
            output_index
        };
        let key = &numeric.keyframes[source_index];
        let parent_secs = start + key.time_secs * stretch;
        let value_secs = key.values[0];
        let mut ignored_warnings = Vec::new();
        let easing = if stretch < 0.0 {
            reverse_easing_for_key(
                &numeric.keyframes,
                source_index,
                0,
                1.0,
                &mut ignored_warnings,
                "ADBE Time Remapping",
            )
            .expect("authored remap construction repeats successful preflight")
        } else {
            easing_for_key(
                &numeric.keyframes,
                source_index,
                0,
                1.0,
                &mut ignored_warnings,
                "ADBE Time Remapping",
            )
        };
        keys.push(TimeRemapKeyframe {
            id: KeyframeId::new(format!("aep-remap-{target_id}-{output_index}")),
            time: Time::from_secs(parent_secs),
            value: Time::from_secs(value_secs),
            easing,
        });
    }
    match TimeRemapProperty::new(keys, extrapolation, extrapolation) {
        Ok(remap) => {
            if extrapolation == TimeRemapExtrapolation::Loop {
                warnings.push("ADBE Time Remapping: `loopIn() + loopOut() - value` is approximated by FX Loop on both sides; FX starts the next cycle at the final authored key, so a frame exactly at that key can show the first key's source time instead of the last; native boundary fidelity is not established".into());
            }
            (Some(AuthoredRemap::Keys(remap)), warnings)
        }
        Err(error) => {
            budget.rollback(checkpoint);
            warnings.push(format!(
                "ADBE Time Remapping: invalid authored remap: {error}; affine layer timing retained"
            ));
            (None, warnings)
        }
    }
}

/// Two equal-value keys over the layer lifetime, for one authored AE key.
pub(super) fn constant_time_remap(
    lifetime: TimeRangeProperty,
    value: Time,
    target_id: LayerId,
    budget: &mut AnimationBudget,
) -> Result<TimeRemapProperty, String> {
    let hold = TimeRemapExtrapolation::Hold;
    let times = [lifetime.start, lifetime.end()];
    let mut estimate = TimeRemapEstimate::default();
    for (index, time) in times.into_iter().enumerate() {
        let id = GeneratedKeyframeIdSize::new(format_args!("aep-remap-{target_id}-{index}"))
            .map_err(|error| error.to_string())?;
        estimate
            .push_key(&id, time, value, PropertyKeyframeEasing::Linear)
            .map_err(|error| error.to_string())?;
    }
    let reservation = estimate
        .reservation_bytes(hold, hold)
        .map_err(|error| error.to_string())?;
    budget
        .reserve(reservation)
        .map_err(|error| error.to_string())?;
    let keys = times
        .into_iter()
        .enumerate()
        .map(|(index, time)| TimeRemapKeyframe {
            id: KeyframeId::new(format!("aep-remap-{target_id}-{index}")),
            time,
            value,
            easing: PropertyKeyframeEasing::Linear,
        })
        .collect();
    TimeRemapProperty::new(keys, hold, hold).map_err(|error| error.to_string())
}

/// Exact `loopIn() + loopOut() - value`: both default calls cycle every
/// authored key, outside the keys on their own side, and `value` cancels the
/// doubled keyed value between them. Only whitespace and a final `;` vary.
fn is_two_sided_cycle(mut text: &str) -> bool {
    use super::control_links::token;
    let text = &mut text;
    let complete = ["loopIn", "(", ")", "+", "loopOut", "(", ")", "-", "value"]
        .into_iter()
        .all(|expected| token(text, expected).is_some());
    let _ = token(text, ";");
    complete && text.trim().is_empty()
}

fn time_remap_property(layer: &Layer) -> Result<Option<&[Chunk]>, PropertyError> {
    let roots = root_runs(&layer.content)?;
    let mut matches = roots
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Time Remapping");
    let Some((_, run)) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(PropertyError::Layout("duplicate Time Remap property"));
    }
    unique_list(run, *b"tdbs").map(Some)
}

fn time_remap_numeric(layer: &Layer) -> Result<Option<NumericProperty>, PropertyError> {
    time_remap_property(layer)?.map(read_numeric).transpose()
}

fn targets(
    name: &str,
    three_d: bool,
    target_id: LayerId,
    anchor_scale: [f64; 2],
) -> Vec<NumericAnimationTarget> {
    let float = |component, property, scale| {
        NumericAnimationTarget::float(PropertyTarget::layer(target_id, property), component, scale)
    };
    match name {
        "ADBE Anchor Point" => vec![
            float(0, PropType::AnchorPointX, anchor_scale[0]),
            float(1, PropType::AnchorPointY, anchor_scale[1]),
        ],
        "ADBE Position" => {
            let mut result = vec![
                float(0, PropType::PositionX, 1.0),
                float(1, PropType::PositionY, 1.0),
            ];
            if three_d {
                result.push(float(2, PropType::PositionZ, 1.0));
            }
            result
        }
        "ADBE Position_0" => vec![float(0, PropType::PositionX, 1.0)],
        "ADBE Position_1" => vec![float(0, PropType::PositionY, 1.0)],
        "ADBE Position_2" if three_d => vec![float(0, PropType::PositionZ, 1.0)],
        super::control_links::SCALE_X => vec![float(0, PropType::ScaleX, 100.0)],
        super::control_links::SCALE_Y => vec![float(0, PropType::ScaleY, 100.0)],
        "ADBE Scale" => vec![
            float(0, PropType::ScaleX, 100.0),
            float(1, PropType::ScaleY, 100.0),
        ],
        "ADBE Rotate Z" => vec![float(0, PropType::Rotation, 1.0)],
        "ADBE Rotate X" => vec![float(0, PropType::RotationX, 1.0)],
        "ADBE Rotate Y" => vec![float(0, PropType::RotationY, 1.0)],
        "ADBE Orientation" => vec![
            float(0, PropType::OrientationX, 1.0),
            float(1, PropType::OrientationY, 1.0),
            float(2, PropType::OrientationZ, 1.0),
        ],
        "ADBE Opacity" => vec![float(0, PropType::Opacity, 100.0)],
        _ => Vec::new(),
    }
}

/// Native keys as ordinary forward-clock import authors them for one scalar
/// target per component: spatial Position paths become the same refined Linear
/// keys, and each key carries the incoming easing per component (key 0 is
/// Linear). Expression evaluation reuses this so its pre-expression `value`
/// matches what the keyed import of the same property renders.
pub(crate) fn editable_native_keys(
    name: &str,
    numeric: &NumericProperty,
    layer: &Layer,
    spatial_position: bool,
    warnings: &mut Vec<String>,
) -> Result<(NumericProperty, Vec<Vec<PropertyKeyframeEasing>>), String> {
    let clock = NumericAnimationClock::parent_identity(layer)?;
    if clock.reversed() {
        return Err("reverse-stretched native keys are not admitted".into());
    }
    let prepared = if spatial_position {
        match spatial_position::prepare(numeric, clock)? {
            Some(prepared) => {
                warnings.push(format!("{name}: native spatial Position path-speed sampled into adaptive Linear keys before expression evaluation (same 0.25 source-unit refinement as keyed import); original tangents and ease controls replaced"));
                prepared
            }
            None => numeric.clone(),
        }
    } else {
        if numeric.keyframes.iter().any(|key| {
            key.spatial_in
                .iter()
                .chain(&key.spatial_out)
                .any(|value| *value != 0.0)
        }) {
            warnings.push(format!("{name}: native spatial tangents are not applied to non-Position expression input; per-component temporal easing retained"));
        }
        numeric.clone()
    };
    let keys = &prepared.keyframes;
    let dimensions = keys.first().map_or(0, |key| key.values.len());
    let easings = (0..keys.len())
        .map(|index| {
            (0..dimensions)
                .map(|component| {
                    if prepared.value_kind == NumericValueKind::Color {
                        return color_easing_for_key(keys, index, clock, warnings, name);
                    }
                    if prepared.value_kind == NumericValueKind::Continuous
                        && let Some(easing) =
                            straight_spatial_easing_for_key(keys, index, clock, warnings, name)
                    {
                        return easing;
                    }
                    Ok(easing_for_key(keys, index, component, 1.0, warnings, name))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((prepared, easings))
}

/// Maximum absolute sampled error after mapping each component into its actual
/// destination unit: percent, pixels, degrees, or normalized 0–1 color/alpha.
/// This tolerance controls the density of the replacement Linear keys.
const COUPLED_LINEAR_TOLERANCE: f64 = 0.01;

// Imported FX tracks use Vec-backed keys and have no smaller fixed format limit.
// AnimationBudget separately owns aggregate serialized-size admission.
const FX_IMPORT_MAXIMUM_KEYS: usize = usize::MAX;

fn mapped_millis(clock: NumericAnimationClock, source_secs: f64) -> Result<i64, String> {
    let millis = clock.seconds(source_secs) * 1000.0;
    if !millis.is_finite() || millis.abs() > ((1_u64 << 53) - 1) as f64 {
        return Err("numeric animation time is not an exact finite millisecond".into());
    }
    Ok(TimeOffset::from_millis_f64(millis).as_millis())
}

fn temporal_value(values: &[f64], component: usize, label: &str) -> Result<f64, String> {
    values
        .get(component)
        .or_else(|| values.first())
        .copied()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("missing or non-finite {label} for component {component}"))
}

fn cubic_coordinate(from: f64, control1: f64, control2: f64, to: f64, t: f64) -> f64 {
    let inverse = 1.0 - t;
    inverse * inverse * inverse * from
        + 3.0 * inverse * inverse * t * control1
        + 3.0 * inverse * t * t * control2
        + t * t * t * to
}

fn cubic_parameter(progress: f64, control1: f64, control2: f64) -> f64 {
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..52 {
        let parameter = (low + high) * 0.5;
        if cubic_coordinate(0.0, control1, control2, 1.0, parameter) < progress {
            low = parameter;
        } else {
            high = parameter;
        }
    }
    (low + high) * 0.5
}

fn evaluate_numeric_segment(
    from: &NumericKeyframe,
    to: &NumericKeyframe,
    source_secs: f64,
) -> Result<Vec<f64>, String> {
    if from.values.len() != to.values.len()
        || from
            .values
            .iter()
            .chain(&to.values)
            .any(|value| !value.is_finite())
    {
        return Err("numeric segment has inconsistent or non-finite values".into());
    }
    if !from.spatial_out.is_empty() || !to.spatial_in.is_empty() {
        return Err("numeric segment has spatial handles".into());
    }
    if source_secs <= from.time_secs {
        return Ok(from.values.clone());
    }
    if source_secs >= to.time_secs {
        return Ok(to.values.clone());
    }
    let duration = to.time_secs - from.time_secs;
    if !duration.is_finite() || duration <= 0.0 {
        return Err("numeric segment has a non-positive duration".into());
    }
    if from.out_interpolation == 3 {
        return Ok(from.values.clone());
    }
    if !matches!(from.out_interpolation, 1 | 2) || !matches!(to.in_interpolation, 1 | 2) {
        return Err(format!(
            "numeric segment has unsupported interpolation {}/{}",
            from.out_interpolation, to.in_interpolation
        ));
    }
    let progress = ((source_secs - from.time_secs) / duration).clamp(0.0, 1.0);
    if from.out_interpolation == 1 && to.in_interpolation == 1 {
        return Ok(from
            .values
            .iter()
            .zip(&to.values)
            .map(|(from, to)| from + (to - from) * progress)
            .collect());
    }
    from.values
        .iter()
        .zip(&to.values)
        .enumerate()
        .map(|(component, (&from_value, &to_value))| {
            let out_influence = (temporal_value(&from.out_influence, component, "out influence")?
                / 100.0)
                .clamp(0.0, 1.0);
            let in_influence = (temporal_value(&to.in_influence, component, "in influence")?
                / 100.0)
                .clamp(0.0, 1.0);
            let out_speed = temporal_value(&from.out_speed, component, "out speed")?;
            let in_speed = temporal_value(&to.in_speed, component, "in speed")?;
            let parameter = cubic_parameter(progress, out_influence, 1.0 - in_influence);
            let control1 = if from.out_interpolation == 1 {
                from_value + (to_value - from_value) * out_influence
            } else {
                from_value + out_speed * duration * out_influence
            };
            let control2 = if to.in_interpolation == 1 {
                from_value + (to_value - from_value) * (1.0 - in_influence)
            } else {
                to_value - in_speed * duration * in_influence
            };
            let value = cubic_coordinate(from_value, control1, control2, to_value, parameter);
            value.is_finite().then_some(value).ok_or_else(|| {
                format!("numeric segment produced a non-finite component {component}")
            })
        })
        .collect()
}

fn fitted_coupled_segment(
    from: &NumericKeyframe,
    to: &NumericKeyframe,
    targets: &[NumericAnimationTarget],
    clock: NumericAnimationClock,
) -> Result<Vec<NumericKeyframe>, String> {
    let from_ms = mapped_millis(clock, from.time_secs)?;
    let to_ms = mapped_millis(clock, to.time_secs)?;
    let (first_ms, last_ms) = if from_ms < to_ms {
        (from_ms, to_ms)
    } else {
        (to_ms, from_ms)
    };
    let duration_ms = u64::try_from(last_ms - first_ms)
        .map_err(|_| "numeric segment duration does not fit milliseconds".to_string())?;
    if duration_ms == 0 {
        return Err("numeric segment endpoints collide on the FX millisecond clock".into());
    }
    let evaluate = |offset_ms: u64| {
        let output_ms = first_ms
            .checked_add(i64::try_from(offset_ms).map_err(|_| "numeric time overflow".to_string())?)
            .ok_or_else(|| "numeric time overflow".to_string())?;
        if output_ms == from_ms {
            return Ok(from.values.clone());
        }
        if output_ms == to_ms {
            return Ok(to.values.clone());
        }
        let source_secs = clock.source_seconds(output_ms as f64 / 1000.0)?;
        evaluate_numeric_segment(from, to, source_secs)
    };
    let mut fitted = fit_value_curve(
        duration_ms,
        COUPLED_LINEAR_TOLERANCE,
        FX_IMPORT_MAXIMUM_KEYS,
        evaluate,
        |from, to, actual, progress| {
            targets.iter().try_fold(0.0_f64, |maximum, target| {
                target
                    .interpolation_error(from, to, actual, progress)
                    .map(|error| maximum.max(error))
            })
        },
    )
    .map_err(|error| match error {
        ValueCurveError::Evaluation(error) => error,
        ValueCurveError::KeyLimit => {
            "fitted numeric curve exceeds the FX in-memory key index".into()
        }
    })?;
    if fitted.last().is_none_or(|key| key.offset_ms != duration_ms) {
        let value = if last_ms == to_ms {
            to.values.clone()
        } else {
            from.values.clone()
        };
        fitted.push(ValueKey {
            offset_ms: duration_ms,
            value,
            linear: true,
        });
    }

    let mut keys = fitted
        .into_iter()
        .map(|key| {
            let output_ms = first_ms
                .checked_add(
                    i64::try_from(key.offset_ms)
                        .map_err(|_| "numeric time overflow".to_string())?,
                )
                .ok_or_else(|| "numeric time overflow".to_string())?;
            let time_secs = if output_ms == from_ms {
                from.time_secs
            } else if output_ms == to_ms {
                to.time_secs
            } else {
                clock.source_seconds(output_ms as f64 / 1000.0)?
            };
            let dimensions = key.value.len();
            Ok(NumericKeyframe {
                time_secs,
                values: key.value,
                in_interpolation: 1,
                out_interpolation: 1,
                in_speed: vec![0.0; dimensions],
                in_influence: vec![0.0; dimensions],
                out_speed: vec![0.0; dimensions],
                out_influence: vec![0.0; dimensions],
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    keys.sort_by(|left, right| left.time_secs.total_cmp(&right.time_secs));
    let first = keys
        .first_mut()
        .ok_or_else(|| "fitted numeric curve is empty".to_string())?;
    first.in_interpolation = from.in_interpolation;
    first.in_speed.clone_from(&from.in_speed);
    first.in_influence.clone_from(&from.in_influence);
    first.spatial_in.clone_from(&from.spatial_in);
    let last = keys
        .last_mut()
        .ok_or_else(|| "fitted numeric curve is empty".to_string())?;
    last.out_interpolation = to.out_interpolation;
    last.out_speed.clone_from(&to.out_speed);
    last.out_influence.clone_from(&to.out_influence);
    last.spatial_out.clone_from(&to.spatial_out);
    Ok(keys)
}

struct CoupledExcursionPreparation {
    numeric: NumericProperty,
    source_key_count: usize,
    recovered_segments: Vec<usize>,
    unfitted_segments: Vec<(usize, String)>,
}

impl CoupledExcursionPreparation {
    fn recovered_segment_ranges(&self) -> String {
        self.recovered_segments
            .iter()
            .map(|index| format!("{index}→{}", index + 1))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn annotate_key_indexed_diagnostic(&self, message: String) -> String {
        if self.recovered_segments.is_empty() {
            return message;
        }
        let labeled = message
            .replace("before key ", "before prepared-track key ")
            .replace("at key ", "at prepared-track key ")
            .replace(": key ", ": prepared-track key ");
        if labeled == message {
            return message;
        }
        let ranges = self.recovered_segment_ranges();
        let mut following_boundaries = self
            .recovered_segments
            .iter()
            .filter_map(|index| {
                let next_segment = index + 1;
                let boundary = index + 2;
                (boundary < self.source_key_count
                    && !self.recovered_segments.contains(&next_segment))
                .then_some(boundary)
            })
            .collect::<Vec<_>>();
        following_boundaries.sort_unstable();
        following_boundaries.dedup();
        let boundary_note = match following_boundaries.as_slice() {
            [] => String::new(),
            [boundary] => {
                format!("; original source key {boundary} is retained as the following boundary")
            }
            boundaries => format!(
                "; original source keys {} are retained as following boundaries",
                boundaries
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        format!(
            "{labeled}; prepared-track key indices are used because original source keys {ranges} were sampled{boundary_note}"
        )
    }

    fn success_warnings(&self, name: &str) -> Vec<String> {
        let mut warnings = Vec::new();
        if !self.recovered_segments.is_empty() {
            let segments = self.recovered_segment_ranges();
            warnings.push(format!("{name}: equal-endpoint Bezier source keys {segments} sampled on the receiving FX integer-millisecond clock and reduced to coupled editable Linear keys within {COUPLED_LINEAR_TOLERANCE} actual destination units; exact source boundary endpoints retained, native temporal handles and sibling easing in each sampled segment replaced, key density depends on the curve and tolerance, fractional-millisecond fidelity unverified"));
        }
        warnings.extend(self.unfitted_segments.iter().map(|(index, error)| {
            format!("{name}: equal-endpoint Bezier source keys {index}→{} could not be sampled ({error}); native keys retained with supported siblings, but the equal-endpoint excursion lost to the existing Linear easing fallback", index + 1)
        }));
        warnings
    }
}

fn validate_target_components(
    name: &str,
    numeric: &NumericProperty,
    targets: &[NumericAnimationTarget],
) -> Result<(), String> {
    for (source_index, key) in numeric.keyframes.iter().enumerate() {
        for target in targets {
            target.validate_components(&key.values).map_err(|error| {
                format!(
                    "{name}: source key {source_index} {error}; {:?} animation omitted",
                    target.target
                )
            })?;
        }
    }
    Ok(())
}

fn prepare_equal_endpoint_excursions(
    numeric: &NumericProperty,
    targets: &[NumericAnimationTarget],
    clock: NumericAnimationClock,
) -> Result<Option<CoupledExcursionPreparation>, String> {
    if numeric.value_kind != NumericValueKind::Continuous || numeric.keyframes.len() < 2 {
        return Ok(None);
    }
    let excursion_segments = numeric
        .keyframes
        .windows(2)
        .map(|pair| {
            targets
                .iter()
                .any(|target| target.has_equal_endpoint_excursion(&pair[0], &pair[1]))
        })
        .collect::<Vec<_>>();
    if !excursion_segments.iter().any(|excursion| *excursion) {
        return Ok(None);
    }

    let mut prepared = numeric.clone();
    prepared.keyframes = vec![numeric.keyframes[0].clone()];
    let mut recovered_segments = Vec::new();
    let mut unfitted_segments = Vec::new();
    for (index, pair) in numeric.keyframes.windows(2).enumerate() {
        if !excursion_segments[index] {
            prepared.keyframes.push(pair[1].clone());
            continue;
        }
        let mut fitted = match fitted_coupled_segment(&pair[0], &pair[1], targets, clock) {
            Ok(fitted) => fitted,
            Err(error) => {
                prepared.keyframes.push(pair[1].clone());
                unfitted_segments.push((index, error));
                continue;
            }
        };
        let incoming = prepared
            .keyframes
            .pop()
            .ok_or_else(|| "coupled numeric preparation lost its boundary key".to_string())?;
        let first = fitted
            .first_mut()
            .ok_or_else(|| "fitted numeric segment is empty".to_string())?;
        first.in_interpolation = incoming.in_interpolation;
        first.in_speed = incoming.in_speed;
        first.in_influence = incoming.in_influence;
        first.spatial_in = incoming.spatial_in;
        prepared.keyframes.extend(fitted);
        recovered_segments.push(index);
    }
    Ok(Some(CoupledExcursionPreparation {
        numeric: prepared,
        source_key_count: numeric.keyframes.len(),
        recovered_segments,
        unfitted_segments,
    }))
}

/// Converts one decoded numeric property into reusable editable graph entries.
pub(super) fn numeric_entries(
    name: &str,
    numeric: &NumericProperty,
    targets: &[NumericAnimationTarget],
    clock: NumericAnimationClock,
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    if numeric.expression_enabled {
        return (
            Vec::new(),
            vec![format!(
                "{name}: enabled AE expression requires the AE expression environment; animation omitted"
            )],
        );
    }
    if numeric.keyframes.is_empty() || targets.is_empty() {
        return (Vec::new(), Vec::new());
    }
    if let Err(error) = validate_target_components(name, numeric, targets) {
        return (Vec::new(), vec![error]);
    }
    let mut warnings = Vec::new();
    let spatial_prepared;
    let numeric = if targets.iter().all(|target| {
        target.target.as_property().is_some_and(|property| {
            matches!(
                property.property_type(),
                PropType::PositionX | PropType::PositionY | PropType::PositionZ
            )
        })
    }) {
        match spatial_position::prepare(numeric, clock) {
            Ok(Some(value)) => {
                spatial_prepared = value;
                warnings.push(format!("{name}: native spatial Position shared path-speed converted to adaptive editable Linear keys (0.25 source-unit quarter-point sampled tolerance on the receiving FX millisecond clock; adjacent milliseconds have no interior sample); original tangents and ease controls replaced, unsampled and fractional-millisecond fidelity unverified"));
                &spatial_prepared
            }
            Ok(None) => numeric,
            Err(error) => {
                return (
                    Vec::new(),
                    vec![format!(
                        "{name}: {error}; coupled Position animation omitted"
                    )],
                );
            }
        }
    } else {
        numeric
    };
    let coupled_preparation = match prepare_equal_endpoint_excursions(numeric, targets, clock) {
        Ok(prepared) => prepared,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!(
                    "{name}: {error}; coupled numeric animation omitted"
                )],
            );
        }
    };
    let numeric = coupled_preparation
        .as_ref()
        .map_or(numeric, |prepared| &prepared.numeric);
    if numeric.expression_present {
        warnings.push(format!(
            "{name}: disabled AE expression retained in source; native keyframes imported"
        ));
    }
    let mut estimates = Vec::with_capacity(targets.len());
    let mut preflight_warnings = Vec::new();
    let annotate_keyed_diagnostic = |message| match &coupled_preparation {
        Some(prepared) => prepared.annotate_key_indexed_diagnostic(message),
        None => message,
    };
    for (target_index, target) in targets.iter().enumerate() {
        if let Err(message) = validate_vector_easing(name, numeric, target, clock) {
            warnings.push(annotate_keyed_diagnostic(message));
            return (Vec::new(), warnings);
        }
        match estimate_target(
            name,
            numeric,
            target,
            target_index,
            clock,
            &mut preflight_warnings,
        ) {
            Ok(estimate) => estimates.push(estimate),
            Err(message) => {
                warnings.push(annotate_keyed_diagnostic(message));
                return (Vec::new(), warnings);
            }
        }
    }
    let reservations = estimates
        .iter()
        .zip(targets)
        .map(|(estimate, target)| estimate.entry_reservation_bytes(&target.target))
        .collect::<Result<Vec<_>, _>>();
    let reservations = match reservations {
        Ok(reservations) => reservations,
        Err(error) => {
            warnings.push(format!(
                "{name}: {error}; coupled animation target set omitted"
            ));
            return (Vec::new(), warnings);
        }
    };
    let checkpoint = budget.checkpoint();
    if let Err(error) = budget.reserve_all(reservations) {
        warnings.push(format!(
            "{name}: {error}; coupled animation target set omitted and static values retained"
        ));
        return (Vec::new(), warnings);
    }

    // Preflight holds one bounded prospective key at a time. Destination key
    // vectors are allocated only after the complete coupled target set fits.
    let mut entries = Vec::with_capacity(targets.len());
    for (target_index, target) in targets.iter().enumerate() {
        let mut ignored_warnings = Vec::new();
        match entry_for_target(
            name,
            numeric,
            target,
            target_index,
            clock,
            &mut ignored_warnings,
        ) {
            Ok(entry) => entries.push(entry),
            Err(message) => {
                budget.rollback(checkpoint);
                warnings.push(annotate_keyed_diagnostic(message));
                return (Vec::new(), warnings);
            }
        }
    }
    if let Some(prepared) = &coupled_preparation {
        preflight_warnings
            .retain(|warning| !warning.contains("equal-endpoint Bezier excursion before key"));
        for warning in &mut preflight_warnings {
            *warning = prepared.annotate_key_indexed_diagnostic(std::mem::take(warning));
        }
        preflight_warnings.extend(prepared.success_warnings(name));
    }
    warnings.extend(preflight_warnings);
    (entries, warnings)
}

/// Converts a one-target source-local numeric track through an O(1)-key
/// projection. This is used by Audio Levels so admission happens before any
/// full destination track or cloned `NumericProperty` exists.
pub(super) fn projected_numeric_entries(
    name: &str,
    numeric: &NumericProperty,
    target: NumericAnimationTarget,
    project: impl Fn(&NumericKeyframe) -> Result<NumericKeyframe, String>,
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    if numeric.expression_enabled || numeric.keyframes.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut warnings = Vec::new();
    let mut estimate = PropertyTrackEstimate::default();
    for index in 0..numeric.keyframes.len() {
        if let Err(error) =
            GeneratedKeyframeIdSize::new(format_args!("aep-{}-{name}-0-{index}", target.target))
        {
            warnings.push(format!("{name}: {error}; animation omitted"));
            return (Vec::new(), warnings);
        }
        let key = match projected_key(name, numeric, &target, index, &project, &mut warnings) {
            Ok(key) => key,
            Err(error) => {
                warnings.push(error);
                return (Vec::new(), warnings);
            }
        };
        if let Err(error) = estimate.push_prospective(&key) {
            warnings.push(format!("{name}: {error}; animation omitted"));
            return (Vec::new(), warnings);
        }
    }
    let reservation = match estimate.entry_reservation_bytes(&target.target) {
        Ok(reservation) => reservation,
        Err(error) => {
            warnings.push(format!("{name}: {error}; animation omitted"));
            return (Vec::new(), warnings);
        }
    };
    let checkpoint = budget.checkpoint();
    if let Err(error) = budget.reserve(reservation) {
        warnings.push(format!(
            "{name}: {error}; animation omitted and static value retained"
        ));
        return (Vec::new(), warnings);
    }

    let mut keys = Vec::with_capacity(numeric.keyframes.len());
    for index in 0..numeric.keyframes.len() {
        let mut ignored_warnings = Vec::new();
        match projected_key(
            name,
            numeric,
            &target,
            index,
            &project,
            &mut ignored_warnings,
        ) {
            Ok(key) => keys.push(key),
            Err(error) => {
                budget.rollback(checkpoint);
                warnings.push(error);
                return (Vec::new(), warnings);
            }
        }
    }
    let track = match PropertyKeyframeTrack::new(keys) {
        Ok(track) => track,
        Err(error) => {
            budget.rollback(checkpoint);
            warnings.push(format!(
                "{name}: invalid projected track: {error}; animation omitted"
            ));
            return (Vec::new(), warnings);
        }
    };
    (
        vec![AnimationGraphEntry {
            target: target.target,
            animator: PropertyAnimator::keyframes(track),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        }],
        warnings,
    )
}

fn projected_key(
    name: &str,
    numeric: &NumericProperty,
    target: &NumericAnimationTarget,
    index: usize,
    project: &impl Fn(&NumericKeyframe) -> Result<NumericKeyframe, String>,
    warnings: &mut Vec<String>,
) -> Result<PropertyKeyframe, String> {
    let current = project(&numeric.keyframes[index])?;
    let (component, multiplier) = target.easing_component();
    let easing = if index == 0 {
        PropertyKeyframeEasing::Linear
    } else {
        let previous = project(&numeric.keyframes[index - 1])?;
        easing_for_key(
            &[previous, current.clone()],
            1,
            component,
            multiplier,
            warnings,
            name,
        )
    };
    let value = target
        .value_at(&current)
        .map_err(|error| format!("{name}: key {index} {error}; animation omitted"))?;
    Ok(PropertyKeyframe::new(
        KeyframeId::new(format!("aep-{}-{name}-0-{index}", target.target)),
        TimeOffset::from_millis_f64(current.time_secs * 1000.0),
        value,
        easing,
    ))
}

fn validate_vector_easing(
    name: &str,
    numeric: &NumericProperty,
    target: &NumericAnimationTarget,
    clock: NumericAnimationClock,
) -> Result<(), String> {
    if target.vector_easing_components().is_none() && numeric.value_kind != NumericValueKind::Color
    {
        return Ok(());
    }
    for output_index in 0..numeric.keyframes.len() {
        let source_index = if clock.reversed() {
            numeric.keyframes.len() - 1 - output_index
        } else {
            output_index
        };
        easing_for_target_key(
            name,
            numeric,
            target,
            source_index,
            clock,
            false,
            &mut Vec::new(),
        )?;
    }
    Ok(())
}

/// Treat only floating-point noise from normalizing the same native ease as equal.
/// Eight unit-scale ULPs cover the bounded arithmetic in `easing_for_key` while
/// remaining far below a meaningful control-handle difference.
fn easing_equivalent(left: &PropertyKeyframeEasing, right: &PropertyKeyframeEasing) -> bool {
    const NORMALIZATION_ULPS: f64 = 8.0;

    let finite_component_equivalent = |left: f64, right: f64| {
        left.is_finite()
            && right.is_finite()
            && (left == right
                || (left - right).abs()
                    <= NORMALIZATION_ULPS * f64::EPSILON * left.abs().max(right.abs()).max(1.0))
    };
    match (left, right) {
        (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Linear)
        | (PropertyKeyframeEasing::Hold, PropertyKeyframeEasing::Hold) => true,
        (
            PropertyKeyframeEasing::CubicBezier {
                x1: left_x1,
                y1: left_y1,
                x2: left_x2,
                y2: left_y2,
            },
            PropertyKeyframeEasing::CubicBezier {
                x1: right_x1,
                y1: right_y1,
                x2: right_x2,
                y2: right_y2,
            },
        ) => [
            (*left_x1, *right_x1),
            (*left_y1, *right_y1),
            (*left_x2, *right_x2),
            (*left_y2, *right_y2),
        ]
        .into_iter()
        .all(|(left, right)| finite_component_equivalent(left, right)),
        _ => false,
    }
}

/// Choose the one easing a Vector2 key can store from axes that actually move
/// on its incoming output-clock segment. Both moving axes remain strict.
fn easing_for_target_key(
    name: &str,
    numeric: &NumericProperty,
    target: &NumericAnimationTarget,
    source_index: usize,
    clock: NumericAnimationClock,
    discrete: bool,
    warnings: &mut Vec<String>,
) -> Result<PropertyKeyframeEasing, String> {
    let keys = &numeric.keyframes;
    if numeric.value_kind == NumericValueKind::Color && !discrete {
        return color_easing_for_key(keys, source_index, clock, warnings, name);
    }
    if numeric.value_kind == NumericValueKind::Continuous
        && !discrete
        && let Some(easing) =
            straight_spatial_easing_for_key(keys, source_index, clock, warnings, name)
    {
        return easing;
    }
    let Some(components) = target.vector_easing_components() else {
        let (component, multiplier) = target.easing_component();
        return effective_easing_for_key(
            keys,
            source_index,
            EffectiveEasing {
                component,
                multiplier,
                clock,
                discrete,
            },
            warnings,
            name,
        );
    };
    let segment = if clock.reversed() {
        (source_index + 1 < keys.len()).then_some((source_index + 1, source_index))
    } else {
        source_index
            .checked_sub(1)
            .map(|previous| (previous, source_index))
    };
    let mut active = [false; 2];
    if let Some((from_index, to_index)) = segment {
        for (axis, (component, multiplier)) in components.into_iter().enumerate() {
            let value = |key_index: usize| {
                keys[key_index]
                    .values
                    .get(component)
                    .copied()
                    .map(|value| value * multiplier)
                    .ok_or_else(|| format!("{name}: key {key_index} lacks component {component}"))
            };
            active[axis] = value(from_index)? != value(to_index)?;
        }
    }
    let selected = match active {
        [false, true] => 1,
        _ => 0,
    };
    let (component, multiplier) = components[selected];
    let easing = effective_easing_for_key(
        keys,
        source_index,
        EffectiveEasing {
            component,
            multiplier,
            clock,
            discrete,
        },
        warnings,
        name,
    )?;
    if active == [true, true] {
        let (other_component, other_multiplier) = components[1];
        let other = effective_easing_for_key(
            keys,
            source_index,
            EffectiveEasing {
                component: other_component,
                multiplier: other_multiplier,
                clock,
                discrete,
            },
            warnings,
            name,
        )?;
        if !easing_equivalent(&easing, &other) {
            return Err(format!(
                "{name}: component-specific temporal eases before key {source_index} cannot be represented by one Vector2 easing; coupled animation target set omitted"
            ));
        }
    }
    Ok(easing)
}

/// AE spatial keys use a shared speed magnitude along the source vector.
/// Only straight two/three dimensional segments with zero tangents are
/// admitted here; ordinary scalar and curved spatial paths keep their mapping.
fn straight_spatial_easing_for_key(
    keys: &[NumericKeyframe],
    source_index: usize,
    clock: NumericAnimationClock,
    warnings: &mut Vec<String>,
    name: &str,
) -> Option<Result<PropertyKeyframeEasing, String>> {
    let index = if clock.reversed() {
        source_index.checked_add(1)?
    } else {
        source_index
    };
    let previous = keys.get(index.checked_sub(1)?)?;
    let current = keys.get(index)?;
    let dimensions = previous.values.len();
    if !(2..=3).contains(&dimensions)
        || current.values.len() != dimensions
        || [
            previous.spatial_in.as_slice(),
            previous.spatial_out.as_slice(),
            current.spatial_in.as_slice(),
            current.spatial_out.as_slice(),
        ]
        .iter()
        .any(|v| v.len() != dimensions || v.iter().any(|v| *v != 0.))
        || [
            previous.out_speed.as_slice(),
            previous.out_influence.as_slice(),
            current.in_speed.as_slice(),
            current.in_influence.as_slice(),
        ]
        .iter()
        .any(|v| v.len() != 1 || !v[0].is_finite())
        || previous.out_speed[0] < 0.
        || current.in_speed[0] < 0.
        || previous
            .values
            .iter()
            .chain(&current.values)
            .any(|v| !v.is_finite())
    {
        return None;
    }
    let distance = previous
        .values
        .iter()
        .zip(&current.values)
        .fold(0_f64, |distance, (from, to)| distance.hypot(to - from));
    if distance == 0. || !distance.is_finite() {
        return None;
    }
    // Preserve the native coordinate magnitudes for the upstream rounding
    // guard. Normalizing a noise-only vector to [0, distance] would turn its
    // shared speed into an enormous, artificial progress handle.
    if previous
        .values
        .iter()
        .zip(&current.values)
        .all(|(from, to)| spatial_displacement_is_rounding(previous, current, *from, *to))
    {
        return None;
    }
    if clock.reversed() && previous.out_interpolation == 3 {
        return Some(Err(format!(
            "{name}: reverse-stretched Hold spatial segment cannot be represented; animation omitted"
        )));
    }
    let mut from = previous.clone();
    let mut to = current.clone();
    from.values = vec![0.];
    to.values = vec![distance];
    let easing = easing_for_key(&[from, to], 1, 0, 1., warnings, name);
    Some(Ok(if clock.reversed() {
        match easing {
            PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
                PropertyKeyframeEasing::CubicBezier {
                    x1: 1. - x2,
                    y1: 1. - y2,
                    x2: 1. - x1,
                    y2: 1. - y1,
                }
            }
            other => other,
        }
    } else {
        easing
    }))
}

/// Native color keys store one temporal speed in 0..255 RGB-vector units.
/// That speed describes progress along the color vector, including decreasing
/// channels; it is independent of the destination channel and affine mapping.
fn color_easing_for_key(
    keys: &[NumericKeyframe],
    source_index: usize,
    clock: NumericAnimationClock,
    warnings: &mut Vec<String>,
    name: &str,
) -> Result<PropertyKeyframeEasing, String> {
    let index = if clock.reversed() {
        if source_index + 1 >= keys.len() {
            return Ok(PropertyKeyframeEasing::Linear);
        }
        if keys[source_index].out_interpolation == 3 {
            return Err(format!(
                "{name}: reverse-stretched Hold color segment cannot be represented; animation omitted"
            ));
        }
        source_index + 1
    } else {
        source_index
    };
    if index == 0 {
        return Ok(PropertyKeyframeEasing::Linear);
    }
    let previous = &keys[index - 1];
    let current = &keys[index];
    let easing = if previous.out_interpolation == 3
        || (previous.out_interpolation == 1 && current.in_interpolation == 1)
        || !matches!(previous.out_interpolation, 1 | 2)
        || !matches!(current.in_interpolation, 1 | 2)
    {
        // Hold and linear interpolation need no color-speed normalization.
        easing_for_key(keys, index, 0, 1., warnings, name)
    } else {
        let ([r0, g0, b0, a0], [r1, g1, b1, a1]) =
            (previous.values.as_slice(), current.values.as_slice())
        else {
            return Err(format!(
                "{name}: incomplete native RGBA color keys; animation omitted"
            ));
        };
        if !previous
            .values
            .iter()
            .chain(&current.values)
            .all(|v| v.is_finite())
        {
            return Err(format!(
                "{name}: nonfinite native color values; animation omitted"
            ));
        }
        if a0 != a1 {
            return Err(format!(
                "{name}: alpha-varying native color speed has no validated vector normalization; animation omitted"
            ));
        }
        if [
            previous.out_speed.as_slice(),
            previous.out_influence.as_slice(),
            current.in_speed.as_slice(),
            current.in_influence.as_slice(),
        ]
        .iter()
        .any(|v| v.len() != 1)
        {
            return Err(format!(
                "{name}: requires one shared native color temporal ease; animation omitted"
            ));
        }
        let distance = 255. * (r1 - r0).hypot(g1 - g0).hypot(b1 - b0);
        if !distance.is_finite() {
            return Err(format!(
                "{name}: nonfinite RGB vector distance; animation omitted"
            ));
        }
        // Reuse interpolation and influence handling on a positive progress
        // segment in the same native units as the shared speed. The two cloned
        // keys are bounded (four source components), including during preflight.
        let mut from = previous.clone();
        let mut to = current.clone();
        from.values = vec![0.];
        to.values = vec![distance];
        easing_for_key(&[from, to], 1, 0, 1., warnings, name)
    };
    Ok(if clock.reversed() {
        match easing {
            PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
                PropertyKeyframeEasing::CubicBezier {
                    x1: 1. - x2,
                    y1: 1. - y2,
                    x2: 1. - x1,
                    y2: 1. - y1,
                }
            }
            other => other,
        }
    } else {
        easing
    })
}

fn validate_target(
    name: &str,
    numeric: &NumericProperty,
    target: &NumericAnimationTarget,
    clock: NumericAnimationClock,
) -> Result<bool, String> {
    let discrete = matches!(target.value, NumericTargetValue::StrokeJoin);
    if !discrete {
        return Ok(false);
    }
    if numeric.keyframes.len() > 1 && clock.reversed() {
        return Err(format!(
            "{name}: reverse-stretched discrete transitions cannot be represented; animation omitted"
        ));
    }
    if numeric
        .keyframes
        .windows(2)
        .any(|pair| pair[0].out_interpolation != 3)
    {
        return Err(format!(
            "{name}: non-Hold discrete transition cannot be represented; animation omitted"
        ));
    }
    if numeric.keyframes.iter().any(|key| {
        key.spatial_in
            .iter()
            .chain(&key.spatial_out)
            .any(|value| *value != 0.0)
    }) {
        return Err(format!(
            "{name}: spatial tangents cannot be represented on Stroke Join; animation omitted"
        ));
    }
    Ok(true)
}

struct ProspectiveKeyContext {
    target_index: usize,
    output_index: usize,
    clock: NumericAnimationClock,
    discrete: bool,
}

fn prospective_key(
    name: &str,
    numeric: &NumericProperty,
    target: &NumericAnimationTarget,
    context: ProspectiveKeyContext,
    warnings: &mut Vec<String>,
) -> Result<PropertyKeyframe, String> {
    let ProspectiveKeyContext {
        target_index,
        output_index,
        clock,
        discrete,
    } = context;
    let source_index = if clock.reversed() {
        numeric.keyframes.len() - 1 - output_index
    } else {
        output_index
    };
    let key = &numeric.keyframes[source_index];
    let value = target.value_at(key).map_err(|error| {
        format!(
            "{name}: key {source_index} {error}; {:?} animation omitted",
            target.target
        )
    })?;
    let easing = easing_for_target_key(
        name,
        numeric,
        target,
        source_index,
        clock,
        discrete,
        warnings,
    )?;
    let mut output = PropertyKeyframe::new(
        KeyframeId::new(format!(
            "aep-{}-{name}-{target_index}-{output_index}",
            target.target
        )),
        TimeOffset::from_millis_f64(clock.seconds(key.time_secs) * 1000.0),
        value,
        easing,
    );
    if matches!(target.value, NumericTargetValue::Float { .. })
        && (!key.spatial_in.is_empty() || !key.spatial_out.is_empty())
    {
        let supports_spatial = target.target.as_property().is_some_and(|property| {
            matches!(
                property.property_type(),
                PropType::PositionX | PropType::PositionY
            )
        });
        if !supports_spatial
            && key
                .spatial_in
                .iter()
                .chain(&key.spatial_out)
                .any(|value| *value != 0.0)
        {
            return Err(format!(
                "{name}: nonzero spatial tangents cannot be represented on {}; animation omitted",
                target.target
            ));
        }
        let (spatial_in, spatial_out) = if clock.reversed() {
            (&key.spatial_out, &key.spatial_in)
        } else {
            (&key.spatial_in, &key.spatial_out)
        };
        // AE's zero-handle segments interpolate traveled distance, whereas
        // present FX zero handles apply cubic geometry to the temporal progress
        // a second time. Curved Position tracks are normalized by prepare;
        // omit redundant geometry here rather than changing their temporal ease.
        // Preserve both handles if either side is nonzero.
        if supports_spatial
            && spatial_in
                .iter()
                .chain(spatial_out)
                .any(|value| *value != 0.0)
        {
            let (component, _) = target.easing_component();
            output = output.with_spatial_tangents(
                spatial_in.get(component).copied(),
                spatial_out.get(component).copied(),
            );
        }
    }
    Ok(output)
}

fn estimate_target(
    name: &str,
    numeric: &NumericProperty,
    target: &NumericAnimationTarget,
    target_index: usize,
    clock: NumericAnimationClock,
    warnings: &mut Vec<String>,
) -> Result<PropertyTrackEstimate, String> {
    let discrete = validate_target(name, numeric, target, clock)?;
    let mut estimate = PropertyTrackEstimate::default();
    for output_index in 0..numeric.keyframes.len() {
        GeneratedKeyframeIdSize::new(format_args!(
            "aep-{}-{name}-{target_index}-{output_index}",
            target.target
        ))
        .map_err(|error| format!("{name}: {error}; animation omitted"))?;
        let key = prospective_key(
            name,
            numeric,
            target,
            ProspectiveKeyContext {
                target_index,
                output_index,
                clock,
                discrete,
            },
            warnings,
        )?;
        estimate
            .push_prospective(&key)
            .map_err(|error| format!("{name}: {error}; animation omitted"))?;
    }
    Ok(estimate)
}

fn entry_for_target(
    name: &str,
    numeric: &NumericProperty,
    target: &NumericAnimationTarget,
    target_index: usize,
    clock: NumericAnimationClock,
    warnings: &mut Vec<String>,
) -> Result<AnimationGraphEntry, String> {
    let discrete = validate_target(name, numeric, target, clock)?;
    let mut keys = Vec::with_capacity(numeric.keyframes.len());
    for output_index in 0..numeric.keyframes.len() {
        keys.push(prospective_key(
            name,
            numeric,
            target,
            ProspectiveKeyContext {
                target_index,
                output_index,
                clock,
                discrete,
            },
            warnings,
        )?);
    }
    let track = PropertyKeyframeTrack::new(keys).map_err(|error| {
        format!("{name}: invalid {target:?} keyframe track: {error}; animation omitted")
    })?;
    Ok(AnimationGraphEntry {
        target: target.target.clone(),
        animator: PropertyAnimator::keyframes(track),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    })
}

struct EffectiveEasing {
    component: usize,
    multiplier: f64,
    clock: NumericAnimationClock,
    discrete: bool,
}

fn effective_easing_for_key(
    keys: &[NumericKeyframe],
    source_index: usize,
    effective: EffectiveEasing,
    warnings: &mut Vec<String>,
    name: &str,
) -> Result<PropertyKeyframeEasing, String> {
    let EffectiveEasing {
        component,
        multiplier,
        clock,
        discrete,
    } = effective;
    if discrete {
        // FX attaches Hold to the incoming segment, including string keys.
        Ok(PropertyKeyframeEasing::Hold)
    } else if clock.reversed() {
        reverse_easing_for_key(keys, source_index, component, multiplier, warnings, name)
    } else {
        Ok(easing_for_key(
            keys,
            source_index,
            component,
            multiplier,
            warnings,
            name,
        ))
    }
}

fn reverse_easing_for_key(
    keys: &[NumericKeyframe],
    source_index: usize,
    component: usize,
    multiplier: f64,
    warnings: &mut Vec<String>,
    name: &str,
) -> Result<PropertyKeyframeEasing, String> {
    if source_index + 1 >= keys.len() {
        return Ok(PropertyKeyframeEasing::Linear);
    }
    if keys[source_index].out_interpolation == 3 {
        return Err(format!(
            "{name}: reverse-stretched Hold segment at key {source_index} cannot be represented by the destination keyframe model; animation omitted"
        ));
    }
    let easing = easing_for_key(
        keys,
        source_index + 1,
        component,
        multiplier,
        warnings,
        name,
    );
    Ok(match easing {
        PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            PropertyKeyframeEasing::CubicBezier {
                x1: 1.0 - x2,
                y1: 1.0 - y2,
                x2: 1.0 - x1,
                y2: 1.0 - y1,
            }
        }
        other => other,
    })
}

fn equal_endpoint_excursion(
    previous: &NumericKeyframe,
    current: &NumericKeyframe,
    component: usize,
    multiplier: f64,
) -> bool {
    // Spatial speeds have distance-space semantics and their own conversion.
    if !previous.spatial_out.is_empty()
        || !current.spatial_in.is_empty()
        || previous.out_interpolation == 3
        || current.time_secs <= previous.time_secs
        || multiplier == 0.0
        || previous
            .values
            .get(component)
            .zip(current.values.get(component))
            .is_none_or(|(from, to)| from != to)
    {
        return false;
    }
    let has_handle = |interpolation, speeds: &[f64], influences: &[f64]| {
        interpolation == 2
            && speeds
                .get(component)
                .or_else(|| speeds.first())
                .is_some_and(|speed| *speed != 0.0)
            && influences
                .get(component)
                .or_else(|| influences.first())
                .is_some_and(|influence| *influence > 0.0)
    };
    has_handle(
        previous.out_interpolation,
        &previous.out_speed,
        &previous.out_influence,
    ) || has_handle(
        current.in_interpolation,
        &current.in_speed,
        &current.in_influence,
    )
}

pub(super) fn easing_for_key(
    keys: &[NumericKeyframe],
    index: usize,
    component: usize,
    multiplier: f64,
    warnings: &mut Vec<String>,
    name: &str,
) -> PropertyKeyframeEasing {
    if index == 0 {
        return PropertyKeyframeEasing::Linear;
    }
    let previous = &keys[index - 1];
    let current = &keys[index];
    if previous.out_interpolation == 3 {
        return PropertyKeyframeEasing::Hold;
    }
    if previous.out_interpolation == 1 && current.in_interpolation == 1 {
        return PropertyKeyframeEasing::Linear;
    }
    if !matches!(previous.out_interpolation, 1 | 2) || !matches!(current.in_interpolation, 1 | 2) {
        warnings.push(format!(
            "{name}: unknown interpolation pair {}/{} before key {index}; linear easing used",
            previous.out_interpolation, current.in_interpolation
        ));
        return PropertyKeyframeEasing::Linear;
    }
    let Some((&out_influence, &in_influence, &out_speed, &in_speed, &from, &to)) = previous
        .out_influence
        .get(component)
        .or_else(|| previous.out_influence.first())
        .zip(
            current
                .in_influence
                .get(component)
                .or_else(|| current.in_influence.first()),
        )
        .zip(
            previous
                .out_speed
                .get(component)
                .or_else(|| previous.out_speed.first()),
        )
        .zip(
            current
                .in_speed
                .get(component)
                .or_else(|| current.in_speed.first()),
        )
        .zip(previous.values.get(component))
        .zip(current.values.get(component))
        .map(|(((((a, b), c), d), e), f)| (a, b, c, d, e, f))
    else {
        warnings.push(format!(
            "{name}: incomplete temporal ease before key {index}; linear easing used"
        ));
        return PropertyKeyframeEasing::Linear;
    };
    let duration = current.time_secs - previous.time_secs;
    let delta = (to - from) * multiplier;
    if equal_endpoint_excursion(previous, current, component, multiplier) {
        warnings.push(format!(
            "{name}: equal-endpoint Bezier excursion before key {index} cannot be represented by normalized FX easing; linear fallback omits the excursion"
        ));
    }
    if duration <= 0.0
        || delta.abs() <= f64::EPSILON
        || spatial_displacement_is_rounding(previous, current, from, to)
    {
        return PropertyKeyframeEasing::Linear;
    }
    let x1 = (out_influence / 100.0).clamp(0.0, 1.0);
    let x2 = 1.0 - (in_influence / 100.0).clamp(0.0, 1.0);
    // A linear side is a diagonal handle, not a reason to discard the
    // opposite side's authored Bezier ease.
    let y1 = if previous.out_interpolation == 1 {
        x1
    } else {
        out_speed * multiplier * duration / delta * x1
    };
    let y2 = if current.in_interpolation == 1 {
        x2
    } else {
        1.0 - in_speed * multiplier * duration / delta * (1.0 - x2)
    };
    if [x1, y1, x2, y2].into_iter().all(f64::is_finite) {
        PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 }
    } else {
        warnings.push(format!(
            "{name}: non-finite temporal ease before key {index}; linear easing used"
        ));
        PropertyKeyframeEasing::Linear
    }
}

/// Whether a native spatial segment moves this axis only by rounding.
///
/// Spatial keys store one speed along the whole motion path, and
/// [`easing_for_key`] divides it by this axis's displacement. When the
/// endpoints differ only by rounding in AE's own arithmetic, that quotient is
/// noise and its unbounded progress would drive the FX spatial curve orders of
/// magnitude away, so the segment is treated like exactly equal endpoints.
/// Value-space keys are unaffected: their speed belongs to this value and
/// still moves it away between nearly equal endpoints.
fn spatial_displacement_is_rounding(
    previous: &NumericKeyframe,
    current: &NumericKeyframe,
    from: f64,
    to: f64,
) -> bool {
    /// Relative rounding bound, in `f64::EPSILON`s of the larger endpoint
    /// magnitude: far below any authorable displacement.
    const ENDPOINT_NOISE_ULPS: f64 = 8.0;

    let spatial = !previous.spatial_out.is_empty() || !current.spatial_in.is_empty();
    spatial && (to - from).abs() <= ENDPOINT_NOISE_ULPS * f64::EPSILON * from.abs().max(to.abs())
}

#[cfg(test)]
mod tests {
    use fx_schema::{AnimationGraph, LayerId, PropType, PropertyTarget, PropertyValue};

    use super::{
        AnimationGraphEntry, AnimationTargetClock, EffectiveEasing, NumericAnimationClock,
        NumericAnimationTarget, NumericKeyframe, PropertyKeyframeEasing, effective_easing_for_key,
        reverse_easing_for_key,
    };
    use crate::structure_document::animation_budget::AnimationBudget;
    use crate::{
        properties::{NumericProperty, NumericValueKind, read_transform},
        structure::{Composition, ItemKind, Layer, StructuralProject, read_project},
    };

    fn numeric_entries(
        name: &str,
        numeric: &NumericProperty,
        targets: &[NumericAnimationTarget],
        clock: NumericAnimationClock,
    ) -> (Vec<AnimationGraphEntry>, Vec<String>) {
        super::numeric_entries(
            name,
            numeric,
            targets,
            clock,
            &mut AnimationBudget::default(),
        )
    }

    fn transform_entries(
        layer: &Layer,
        composition: &Composition,
        target_id: LayerId,
        target_clock: AnimationTargetClock,
    ) -> (Vec<AnimationGraphEntry>, Vec<String>) {
        super::transform_entries(
            layer,
            composition,
            target_id,
            target_clock,
            [1.0; 2],
            &mut AnimationBudget::default(),
        )
    }

    fn numeric_key(time_secs: f64, value: f64) -> NumericKeyframe {
        NumericKeyframe {
            time_secs,
            values: vec![value],
            in_interpolation: 2,
            out_interpolation: 2,
            in_speed: vec![1.0],
            in_influence: vec![25.0],
            out_speed: vec![1.0],
            out_influence: vec![25.0],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        }
    }

    fn sampled_components(entry: &AnimationGraphEntry, time_ms: i64) -> Vec<f64> {
        let keys = entry.animator.keyframe_track().unwrap().keyframes();
        let upper = keys.partition_point(|key| key.layer_time().as_millis() <= time_ms);
        let from = &keys[upper.saturating_sub(1)];
        let values = |key: &fx_schema::animator::PropertyKeyframe| match key.value() {
            PropertyValue::Float(value) => vec![*value],
            PropertyValue::Vector2(value) => value.to_vec(),
            value => panic!("unexpected sampled value {value:?}"),
        };
        let Some(to) = keys.get(upper) else {
            return values(from);
        };
        assert_eq!(to.easing(), PropertyKeyframeEasing::Linear);
        let duration = to.layer_time().as_millis() - from.layer_time().as_millis();
        let progress = (time_ms - from.layer_time().as_millis()) as f64 / duration as f64;
        values(from)
            .into_iter()
            .zip(values(to))
            .map(|(from, to)| from + (to - from) * progress)
            .collect()
    }

    #[test]
    fn equal_endpoint_bezier_excursion_becomes_editable_linear_motion() {
        let target = NumericAnimationTarget::float(
            PropertyTarget::layer(LayerId::new(17), PropType::Rotation),
            0,
            1.0,
        );
        let mut numeric = rect_size_numeric(0.0);
        numeric.values = vec![10.0];
        numeric.keyframes = vec![numeric_key(0.0, 10.0), numeric_key(1.0, 10.0)];
        numeric.keyframes[0].out_influence = vec![100.0 / 3.0];
        numeric.keyframes[1].in_influence = vec![100.0 / 3.0];
        numeric.keyframes[0].out_speed = vec![120.0];
        numeric.keyframes[1].in_speed = vec![-120.0];
        for clock in [
            NumericAnimationClock::source_local(),
            NumericAnimationClock::ParentIdentity {
                start: 1.0,
                stretch: -1.0,
            },
        ] {
            let (entries, warnings) = numeric_entries(
                "Review Rotation",
                &numeric,
                std::slice::from_ref(&target),
                clock,
            );
            assert_eq!(entries.len(), 1, "{clock:?}: {warnings:?}");
            let keys = entries[0].animator.keyframe_track().unwrap().keyframes();
            assert!(keys.len() > 2, "the native excursion needs an interior key");
            assert!((sampled_components(&entries[0], 500)[0] - 40.0).abs() <= 0.01);
            assert_eq!(sampled_components(&entries[0], 0), [10.0]);
            assert_eq!(sampled_components(&entries[0], 1000), [10.0]);
            assert!(warnings.iter().any(|warning| {
                warning.contains("Review Rotation")
                    && warning.contains("equal-endpoint Bezier")
                    && warning.contains("editable Linear keys")
            }));
            assert!(warnings.iter().all(|warning| !warning.contains("omitted")));
            AnimationGraph::from_entries(entries).unwrap();
        }
        let mut sibling = numeric.clone();
        sibling.keyframes[1].values = vec![20.0];
        let (entries, warnings) = numeric_entries(
            "Review sibling",
            &sibling,
            std::slice::from_ref(&target),
            NumericAnimationClock::source_local(),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0]
                .animator
                .keyframe_track()
                .unwrap()
                .keyframes()
                .len(),
            2,
            "independent representable curves keep their native keys"
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        for interpolation in [1, 3] {
            let mut constant = numeric.clone();
            constant.keyframes[0].out_interpolation = interpolation;
            constant.keyframes[1].in_interpolation = interpolation;
            let (entries, warnings) = numeric_entries(
                "Review constant",
                &constant,
                std::slice::from_ref(&target),
                NumericAnimationClock::source_local(),
            );
            assert_eq!(entries.len(), 1);
            assert!(warnings.is_empty(), "{warnings:?}");
        }
        let mut denied = AnimationBudget::with_limit(0);
        let (entries, warnings) = super::numeric_entries(
            "Review denied excursion",
            &numeric,
            std::slice::from_ref(&target),
            NumericAnimationClock::source_local(),
            &mut denied,
        );
        assert!(entries.is_empty());
        assert!(warnings.iter().all(|warning| {
            !warning.contains("sampled") && !warning.contains("editable Linear keys")
        }));

        numeric.keyframes[0].out_speed = vec![0.0];
        numeric.keyframes[1].in_speed = vec![0.0];
        let (entries, warnings) = numeric_entries(
            "Review constant",
            &numeric,
            &[target],
            NumericAnimationClock::source_local(),
        );
        assert_eq!(entries.len(), 1);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn one_millisecond_excursion_keeps_splice_boundary_and_neighbor_easing() {
        let mut first = numeric_key(0.0, 10.0);
        first.out_speed = vec![40_000.0];
        first.out_influence = vec![75.0];
        let mut shared = numeric_key(0.0005, 10.0);
        shared.in_speed = vec![-10_000.0];
        shared.in_influence = vec![10.0];
        shared.out_speed = vec![70.0];
        shared.out_influence = vec![75.0];
        let mut last = numeric_key(0.5, 60.0);
        last.in_speed = vec![10.0];
        last.in_influence = vec![10.0];
        let numeric = NumericProperty {
            values: vec![10.0],
            animated: true,
            expression_present: false,
            expression_enabled: false,
            dimensions_separated: false,
            value_kind: NumericValueKind::Continuous,
            keyframes: vec![first, shared, last],
        };
        let mut native_neighbor = numeric.clone();
        native_neighbor.keyframes[0].out_interpolation = 1;
        native_neighbor.keyframes[1].in_interpolation = 1;
        let target = NumericAnimationTarget::float(
            PropertyTarget::layer(LayerId::new(18), PropType::Rotation),
            0,
            1.0,
        );
        for (clock, expected_times, neighbor_end_ms) in [
            (
                NumericAnimationClock::ParentIdentity {
                    start: 1.0,
                    stretch: 2.0,
                },
                vec![1000, 1001, 2000],
                2000,
            ),
            (
                NumericAnimationClock::ParentIdentity {
                    start: 2.0,
                    stretch: -2.0,
                },
                vec![1000, 1999, 2000],
                1999,
            ),
        ] {
            let (entries, warnings) = numeric_entries(
                "Review splice",
                &numeric,
                std::slice::from_ref(&target),
                clock,
            );
            let (native_entries, native_warnings) = numeric_entries(
                "Review native neighbor",
                &native_neighbor,
                std::slice::from_ref(&target),
                clock,
            );
            assert_eq!(entries.len(), 1, "{clock:?}: {warnings:?}");
            assert_eq!(native_entries.len(), 1, "{clock:?}: {native_warnings:?}");
            assert!(native_warnings.is_empty(), "{native_warnings:?}");
            let keys = entries[0].animator.keyframe_track().unwrap().keyframes();
            let native_keys = native_entries[0]
                .animator
                .keyframe_track()
                .unwrap()
                .keyframes();
            let times = |keys: &[fx_schema::animator::PropertyKeyframe]| {
                keys.iter()
                    .map(|key| key.layer_time().as_millis())
                    .collect::<Vec<_>>()
            };
            assert_eq!(times(keys), expected_times, "{clock:?}");
            assert_eq!(times(keys), times(native_keys), "{clock:?}");
            let neighbor_key = keys
                .iter()
                .find(|key| key.layer_time().as_millis() == neighbor_end_ms)
                .unwrap();
            let native_neighbor_key = native_keys
                .iter()
                .find(|key| key.layer_time().as_millis() == neighbor_end_ms)
                .unwrap();
            assert_eq!(
                neighbor_key.easing(),
                native_neighbor_key.easing(),
                "the moving neighbor keeps its native easing for {clock:?}"
            );
            assert!(matches!(
                neighbor_key.easing(),
                PropertyKeyframeEasing::CubicBezier { .. }
            ));
            assert!(warnings.iter().any(|warning| {
                warning.contains("source keys 0→1") && warning.contains("editable Linear keys")
            }));
        }
    }

    #[test]
    fn asymmetric_parent_identity_excursion_matches_independent_polynomial_samples() {
        let mut first = numeric_key(0.0, 10.0);
        first.out_speed = vec![40.0];
        first.out_influence = vec![75.0];
        let mut last = numeric_key(0.5, 10.0);
        last.in_speed = vec![-10.0];
        last.in_influence = vec![10.0];
        let numeric = NumericProperty {
            values: vec![10.0],
            animated: true,
            expression_present: false,
            expression_enabled: false,
            dimensions_separated: false,
            value_kind: NumericValueKind::Continuous,
            keyframes: vec![first, last],
        };
        let target = NumericAnimationTarget::float(
            PropertyTarget::layer(LayerId::new(22), PropType::Rotation),
            0,
            1.0,
        );
        // Independently solving B(0, 0.75, 0.9, 1; t) = progress, then
        // evaluating B(10, 25, 10.5, 10; t), gives these constants. This does
        // not call the production segment evaluator or clock inversion.
        let cases: [(NumericAnimationClock, [(i64, f64); 2]); 2] = [
            (
                NumericAnimationClock::ParentIdentity {
                    start: 1.0,
                    stretch: 2.0,
                },
                [(1250, 14.269580975459584), (1500, 16.61254599596015)],
            ),
            (
                NumericAnimationClock::ParentIdentity {
                    start: 2.0,
                    stretch: -2.0,
                },
                [(1250, 15.7318178504171), (1500, 16.61254599596015)],
            ),
        ];
        for (clock, samples) in cases {
            let (entries, warnings) = numeric_entries(
                "Review asymmetric polynomial",
                &numeric,
                std::slice::from_ref(&target),
                clock,
            );
            assert_eq!(entries.len(), 1, "{clock:?}: {warnings:?}");
            for (output_ms, expected) in samples {
                let actual = sampled_components(&entries[0], output_ms)[0];
                let epsilon = 8.0 * f64::EPSILON * expected.abs().max(1.0);
                assert!(
                    (actual - expected).abs() <= super::COUPLED_LINEAR_TOLERANCE + epsilon,
                    "{clock:?} at {output_ms}ms: expected {expected}, got {actual}"
                );
            }
        }
    }

    #[test]
    fn unfittable_excursion_keeps_native_siblings_and_reports_source_segment() {
        let mut numeric = source_shaped_coupled_numeric(
            0.0,
            1.0,
            [10.0, 20.0],
            [30.0, 20.0],
            [[40.0, 60.0], [5.0, -60.0]],
        );
        numeric.keyframes[1].in_interpolation = 99;
        let id = LayerId::new(19);
        let targets = [
            NumericAnimationTarget::float(PropertyTarget::layer(id, PropType::ScaleX), 0, 1.0),
            NumericAnimationTarget::float(PropertyTarget::layer(id, PropType::ScaleY), 1, 1.0),
        ];
        let (entries, warnings) = numeric_entries(
            "Review local fallback",
            &numeric,
            &targets,
            NumericAnimationClock::source_local(),
        );
        assert_eq!(entries.len(), 2, "{warnings:?}");
        assert_eq!(sampled_components(&entries[0], 0), [10.0]);
        assert_eq!(sampled_components(&entries[0], 1000), [30.0]);
        assert_eq!(sampled_components(&entries[1], 0), [20.0]);
        assert_eq!(sampled_components(&entries[1], 1000), [20.0]);
        assert!(warnings.iter().any(|warning| {
            warning.contains("source keys 0→1")
                && warning.contains("native keys retained")
                && warning.contains("excursion lost")
        }));
        assert!(warnings.iter().all(|warning| {
            !warning.contains("coupled numeric animation omitted")
                && !warning.contains("editable Linear keys")
        }));
    }

    #[test]
    fn out_of_range_influence_uses_clamped_excursion_recovery() {
        let target = NumericAnimationTarget::float(
            PropertyTarget::layer(LayerId::new(20), PropType::Rotation),
            0,
            1.0,
        );
        let mut numeric = rect_size_numeric(0.0);
        numeric.values = vec![10.0];
        numeric.keyframes = vec![numeric_key(0.0, 10.0), numeric_key(1.0, 10.0)];
        numeric.keyframes[0].out_speed = vec![120.0];
        numeric.keyframes[0].out_influence = vec![125.0];
        numeric.keyframes[1].in_speed = vec![-120.0];
        numeric.keyframes[1].in_influence = vec![-25.0];
        let (entries, warnings) = numeric_entries(
            "Review clamped influence",
            &numeric,
            &[target],
            NumericAnimationClock::source_local(),
        );
        assert_eq!(entries.len(), 1, "{warnings:?}");
        assert_ne!(sampled_components(&entries[0], 500), [10.0]);
        assert!(warnings.iter().any(|warning| {
            warning.contains("source keys 0→1") && warning.contains("editable Linear keys")
        }));
        assert!(warnings.iter().all(|warning| !warning.contains("omitted")));
    }

    #[test]
    fn invalid_target_component_is_rejected_before_excursion_sampling() {
        let valid = NumericAnimationTarget::float(
            PropertyTarget::layer(LayerId::new(21), PropType::Rotation),
            0,
            1.0,
        );
        let invalid = NumericAnimationTarget::float(
            PropertyTarget::layer(LayerId::new(21), PropType::ScaleX),
            1,
            1.0,
        );
        let mut numeric = rect_size_numeric(0.0);
        numeric.values = vec![10.0];
        numeric.keyframes = vec![numeric_key(0.0, 10.0), numeric_key(1.0, 10.0)];
        numeric.keyframes[0].out_speed = vec![120.0];
        numeric.keyframes[1].in_speed = vec![-120.0];
        let (entries, warnings) = numeric_entries(
            "Review invalid target",
            &numeric,
            &[valid, invalid],
            NumericAnimationClock::source_local(),
        );
        assert!(entries.is_empty());
        assert!(warnings.iter().any(|warning| {
            warning.contains("source key 0") && warning.contains("lacks component 1")
        }));
        assert!(warnings.iter().all(|warning| {
            !warning.contains("sampled") && !warning.contains("editable Linear keys")
        }));
    }

    fn source_shaped_coupled_numeric(
        start: f64,
        duration: f64,
        from: [f64; 2],
        to: [f64; 2],
        speeds: [[f64; 2]; 2],
    ) -> NumericProperty {
        let key = |time_secs: f64,
                   first: bool,
                   values: [f64; 2],
                   in_speed: [f64; 2],
                   out_speed: [f64; 2]| {
            NumericKeyframe {
                time_secs,
                values: values.into(),
                in_interpolation: if first { 1 } else { 2 },
                out_interpolation: if first { 2 } else { 1 },
                in_speed: in_speed.into(),
                in_influence: [100.0 / 3.0; 2].into(),
                out_speed: out_speed.into(),
                out_influence: [100.0 / 3.0; 2].into(),
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            }
        };
        NumericProperty {
            values: Vec::new(),
            animated: true,
            expression_present: false,
            expression_enabled: false,
            dimensions_separated: false,
            value_kind: NumericValueKind::Continuous,
            keyframes: vec![
                key(start, true, from, [0.0; 2], speeds[0]),
                key(start + duration, false, to, speeds[1], [0.0; 2]),
            ],
        }
    }

    #[test]
    fn transform_matte_scale_keeps_moving_x_and_equal_endpoint_y_excursion() {
        let numeric = source_shaped_coupled_numeric(
            0.9342676009342676,
            0.6006006006006006,
            [1.0, 2.0],
            [2.0, 2.0],
            [[2.5, 1.0], [2.5, -1.0]],
        );
        let id = LayerId::new(619);
        let targets = [
            NumericAnimationTarget::float(PropertyTarget::layer(id, PropType::ScaleX), 0, 100.0),
            NumericAnimationTarget::float(PropertyTarget::layer(id, PropType::ScaleY), 1, 100.0),
        ];
        let (entries, warnings) = numeric_entries(
            "ADBE Scale matte copy",
            &numeric,
            &targets,
            NumericAnimationClock::ParentIdentity {
                start: 1.001001001001001,
                stretch: 1.0,
            },
        );
        assert_eq!(entries.len(), 2, "{warnings:?}");
        assert_eq!(sampled_components(&entries[0], 1935), [100.0]);
        assert_eq!(sampled_components(&entries[0], 2536), [200.0]);
        assert_eq!(sampled_components(&entries[1], 1935), [200.0]);
        assert_eq!(sampled_components(&entries[1], 2536), [200.0]);
        let x = sampled_components(&entries[0], 2235)[0];
        let y = sampled_components(&entries[1], 2235)[0];
        let source_secs = NumericAnimationClock::ParentIdentity {
            start: 1.001001001001001,
            stretch: 1.0,
        }
        .source_seconds(2.235)
        .unwrap();
        let expected = super::evaluate_numeric_segment(
            &numeric.keyframes[0],
            &numeric.keyframes[1],
            source_secs,
        )
        .unwrap();
        assert!((x - expected[0] * 100.0).abs() <= 0.01);
        assert!((y - expected[1] * 100.0).abs() <= 0.01);
        assert!(y > 200.0, "equal endpoints hid Y={y}");
        for entry in &entries {
            let keys = entry.animator.keyframe_track().unwrap().keyframes();
            assert_eq!(keys.first().unwrap().layer_time().as_millis(), 1935);
            assert_eq!(keys.last().unwrap().layer_time().as_millis(), 2536);
        }
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("editable Linear keys"))
        );
    }

    #[test]
    fn rectangle_size_and_derived_anchors_keep_one_coupled_excursion_curve() {
        let numeric = source_shaped_coupled_numeric(
            0.0,
            1.2,
            [480.0, 1080.0],
            [1920.0, 1080.0],
            [[1800.0, 120.0], [1800.0, -120.0]],
        );
        let (entries, warnings) = numeric_entries(
            "ADBE Vector Rect Size matte copy",
            &numeric,
            &coupled_rect_size_targets(),
            NumericAnimationClock::source_local_rebased(0.4),
        );
        assert_eq!(entries.len(), 3, "{warnings:?}");
        let size = sampled_components(&entries[0], 200);
        let anchor_x = sampled_components(&entries[1], 200)[0];
        let anchor_y = sampled_components(&entries[2], 200)[0];
        assert!(480.0 < size[0] && size[0] < 1920.0);
        assert!(size[1] > 1080.0, "equal endpoints hid Y={}", size[1]);
        assert!((anchor_x - size[0] * 0.5).abs() <= 0.01);
        assert!((anchor_y - size[1] * 0.5).abs() <= 0.01);
        let times = |entry: &AnimationGraphEntry| {
            entry
                .animator
                .keyframe_track()
                .unwrap()
                .keyframes()
                .iter()
                .map(|key| key.layer_time().as_millis())
                .collect::<Vec<_>>()
        };
        assert_eq!(times(&entries[0]), times(&entries[1]));
        assert_eq!(times(&entries[0]), times(&entries[2]));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("editable Linear keys"))
        );
        AnimationGraph::from_entries(entries).unwrap();
    }

    #[test]
    fn prepared_rect_diagnostic_identifies_original_source_key_after_excursion() {
        let key = |time_secs, values: [f64; 2], in_speed: [f64; 2], out_speed: [f64; 2]| {
            NumericKeyframe {
                time_secs,
                values: values.into(),
                in_interpolation: 2,
                out_interpolation: 2,
                in_speed: in_speed.into(),
                in_influence: [33.0; 2].into(),
                out_speed: out_speed.into(),
                out_influence: [33.0; 2].into(),
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            }
        };
        let numeric = NumericProperty {
            values: vec![100.0, 100.0],
            animated: true,
            expression_present: false,
            expression_enabled: false,
            dimensions_separated: false,
            value_kind: NumericValueKind::Continuous,
            keyframes: vec![
                key(0.0, [100.0, 100.0], [0.0; 2], [100.0, 60.0]),
                key(1.0, [200.0, 100.0], [100.0, -60.0], [25.0, 75.0]),
                key(2.0, [300.0, 200.0], [25.0, 75.0], [0.0; 2]),
            ],
        };
        let (entries, warnings) = numeric_entries(
            "Review prepared Rect Size",
            &numeric,
            &coupled_rect_size_targets(),
            NumericAnimationClock::source_local(),
        );
        assert!(entries.is_empty());
        let warning = warnings
            .iter()
            .find(|warning| warning.contains("component-specific temporal eases"))
            .unwrap();
        assert!(warning.contains("before prepared-track key"), "{warning}");
        assert!(
            warning.contains("original source keys 0→1 were sampled"),
            "{warning}"
        );
        assert!(
            warning.contains("original source key 2 is retained as the following boundary"),
            "{warning}"
        );
        assert!(!warning.contains("before source key"), "{warning}");
    }

    fn rect_size_numeric(second_component_speed: f64) -> NumericProperty {
        let key = |time_secs, values| NumericKeyframe {
            time_secs,
            values,
            in_interpolation: 2,
            out_interpolation: 2,
            in_speed: vec![25.0, second_component_speed],
            in_influence: vec![33.0, 33.0],
            out_speed: vec![25.0, second_component_speed],
            out_influence: vec![33.0, 33.0],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        };
        NumericProperty {
            values: vec![100.0, 100.0],
            animated: true,
            expression_present: false,
            expression_enabled: false,
            dimensions_separated: false,
            value_kind: NumericValueKind::Continuous,
            keyframes: vec![key(0.0, vec![100.0, 100.0]), key(1.0, vec![200.0, 200.0])],
        }
    }

    fn dark_masker_rect_size_numeric() -> NumericProperty {
        let key = |time_secs,
                   values: [f64; 2],
                   in_speed: [f64; 2],
                   in_influence: [f64; 2],
                   out_speed: [f64; 2],
                   out_influence: [f64; 2]| NumericKeyframe {
            time_secs,
            values: values.into(),
            in_interpolation: if time_secs == 0.0 { 1 } else { 2 },
            out_interpolation: 2,
            in_speed: in_speed.into(),
            in_influence: in_influence.into(),
            out_speed: out_speed.into(),
            out_influence: out_influence.into(),
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        };
        NumericProperty {
            values: Vec::new(),
            animated: true,
            expression_present: false,
            expression_enabled: false,
            dimensions_separated: false,
            value_kind: NumericValueKind::Continuous,
            keyframes: vec![
                key(
                    0.0,
                    [1600.0, 1600.0000000000007],
                    [0.0, 0.0],
                    [0.0, 0.0],
                    [-39215.07637717509, -39215.07637717509],
                    [1.8461538461538463, 1.8461538461538463],
                ),
                key(
                    0.5416666666666666,
                    [900.0000000000003, 900.0],
                    [-108.56288244282553, -108.56288244282554],
                    [99.7007608731394, 99.70076087313939],
                    [-108.56288244282553, -108.56288244282554],
                    [100.0, 100.0],
                ),
                key(
                    1.5,
                    [300.00000000000034, 300.0],
                    [-18000.0848943537, -18000.0848943537],
                    [1.0434782608695652, 1.0434782608695652],
                    [0.0, 0.0],
                    [16.666666666999998, 16.666666666999998],
                ),
            ],
        }
    }

    fn coupled_rect_size_targets() -> [NumericAnimationTarget; 3] {
        let id = LayerId::new(42);
        [
            NumericAnimationTarget::vector2(
                PropertyTarget::layer(id, PropType::RectSize),
                [0, 1],
                [1.0, 1.0],
            ),
            NumericAnimationTarget::float(
                PropertyTarget::layer(id, PropType::AnchorPointX),
                0,
                0.5,
            ),
            NumericAnimationTarget::float(
                PropertyTarget::layer(id, PropType::AnchorPointY),
                1,
                0.5,
            ),
        ]
    }

    #[test]
    fn tiny_budget_rejects_coupled_rect_targets_atomically_and_exact_limit_keeps_all() {
        let numeric = rect_size_numeric(25.0);
        let targets = coupled_rect_size_targets();
        let mut generous = AnimationBudget::default();
        let (entries, warnings) = super::numeric_entries(
            "ADBE Vector Rect Size",
            &numeric,
            &targets,
            NumericAnimationClock::source_local(),
            &mut generous,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(entries.len(), 3);
        let required = generous.used();

        let mut too_small = AnimationBudget::with_limit(required - 1);
        let (entries, warnings) = super::numeric_entries(
            "ADBE Vector Rect Size",
            &numeric,
            &targets,
            NumericAnimationClock::source_local(),
            &mut too_small,
        );
        assert!(entries.is_empty());
        assert_eq!(too_small.remaining(), required - 1);
        assert!(warnings.iter().any(|warning| warning.contains("coupled")));

        let mut exact = AnimationBudget::with_limit(required);
        let (entries, warnings) = super::numeric_entries(
            "ADBE Vector Rect Size",
            &numeric,
            &targets,
            NumericAnimationClock::source_local(),
            &mut exact,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(entries.len(), 3);
        assert_eq!(exact.remaining(), 0);
    }

    #[test]
    fn native_paired_square_eases_keep_coupled_dark_masker_rect_targets() {
        let numeric = dark_masker_rect_size_numeric();
        for clock in [
            NumericAnimationClock::source_local(),
            NumericAnimationClock::ParentIdentity {
                start: 1.0,
                stretch: -1.0,
            },
        ] {
            let (entries, warnings) = numeric_entries(
                "ADBE Vector Rect Size",
                &numeric,
                &coupled_rect_size_targets(),
                clock,
            );

            assert!(warnings.is_empty(), "{clock:?}: {warnings:?}");
            assert_eq!(
                entries.len(),
                3,
                "Size and both anchors must be retained for {clock:?}"
            );
            AnimationGraph::from_entries(entries).unwrap();
        }
    }

    #[test]
    fn review_import_incompatible_vector_eases_omit_coupled_rect_targets() {
        let numeric = rect_size_numeric(75.0);
        for clock in [
            NumericAnimationClock::source_local(),
            NumericAnimationClock::ParentIdentity {
                start: 1.0,
                stretch: -1.0,
            },
        ] {
            let (entries, warnings) = numeric_entries(
                "ADBE Vector Rect Size",
                &numeric,
                &coupled_rect_size_targets(),
                clock,
            );

            assert!(
                entries.is_empty(),
                "Size and both derived anchors must be omitted atomically for {clock:?}: {entries:?}"
            );
            assert!(warnings.iter().any(|warning| {
                warning.contains("component")
                    && warning.contains("eas")
                    && warning.contains("omitted")
            }));
        }
    }

    #[test]
    fn review_import_matching_vector_eases_keep_coupled_rect_targets() {
        let numeric = rect_size_numeric(25.0);
        let (entries, warnings) = numeric_entries(
            "ADBE Vector Rect Size",
            &numeric,
            &coupled_rect_size_targets(),
            NumericAnimationClock::source_local(),
        );

        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(entries.len(), 3, "Size and both anchors must be retained");
        AnimationGraph::from_entries(entries).unwrap();
    }

    #[test]
    fn easing_equivalence_is_tight_finite_and_variant_preserving() {
        let cubic = |y1| PropertyKeyframeEasing::CubicBezier {
            x1: 0.25,
            y1,
            x2: 0.75,
            y2: 1.0,
        };
        let at_bound = cubic(0.5 + 8.0 * f64::EPSILON);
        let beyond_bound = cubic(0.5 + 9.0 * f64::EPSILON);

        assert!(super::easing_equivalent(&cubic(0.5), &at_bound));
        assert!(!super::easing_equivalent(&cubic(0.5), &beyond_bound));
        assert!(!super::easing_equivalent(
            &PropertyKeyframeEasing::Linear,
            &cubic(0.5)
        ));
        assert!(!super::easing_equivalent(
            &cubic(f64::NAN),
            &cubic(f64::NAN)
        ));
        assert!(!super::easing_equivalent(
            &cubic(f64::INFINITY),
            &cubic(f64::INFINITY)
        ));
    }

    fn assert_single_axis_vector_easing(active_axis: usize) {
        let mut numeric = rect_size_numeric(80.0);
        numeric.keyframes[0].values = vec![10.0, 20.0];
        numeric.keyframes[1].values = if active_axis == 0 {
            vec![110.0, 20.0]
        } else {
            vec![10.0, 220.0]
        };
        // A nonzero temporal speed on the unchanged axis is a genuine
        // equal-endpoint excursion. The coupled recovery must retain it, while
        // the static copy below still isolates the active axis' native easing.
        let inactive_axis = 1 - active_axis;
        let mut excursion = numeric.clone();
        for key in &mut numeric.keyframes {
            key.in_speed[inactive_axis] = 0.0;
            key.out_speed[inactive_axis] = 0.0;
        }
        for key in &mut excursion.keyframes {
            key.in_speed[inactive_axis] = 80.0;
            key.out_speed[inactive_axis] = 80.0;
        }
        let target = NumericAnimationTarget::vector2(
            PropertyTarget::layer(LayerId::new(42), PropType::RectSize),
            [0, 1],
            [1.0, 1.0],
        );
        for clock in [
            NumericAnimationClock::source_local(),
            NumericAnimationClock::ParentIdentity {
                start: 1.0,
                stretch: -1.0,
            },
        ] {
            let (entries, warnings) = numeric_entries(
                "ADBE Vector Rect Size",
                &excursion,
                std::slice::from_ref(&target),
                clock,
            );
            assert_eq!(entries.len(), 1, "{warnings:?}");
            let quarter = sampled_components(&entries[0], 250);
            assert!(
                (quarter[inactive_axis] - numeric.keyframes[0].values[inactive_axis]).abs() > 7.0,
                "the inactive endpoint axis must retain a meaningful native excursion"
            );
            assert!(
                warnings
                    .iter()
                    .any(|warning| warning.contains("editable Linear keys")),
                "{warnings:?}"
            );
            let (entries, warnings) = numeric_entries(
                "ADBE Vector Rect Size",
                &numeric,
                std::slice::from_ref(&target),
                clock,
            );
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(entries.len(), 1);
            let keys = entries[0].animator.keyframe_track().unwrap().keyframes();
            let source_index = if clock.reversed() { 0 } else { 1 };
            let mut expected_warnings = Vec::new();
            let expected = effective_easing_for_key(
                &numeric.keyframes,
                source_index,
                EffectiveEasing {
                    component: active_axis,
                    multiplier: 1.0,
                    clock,
                    discrete: false,
                },
                &mut expected_warnings,
                "ADBE Vector Rect Size",
            )
            .unwrap();
            assert!(expected_warnings.is_empty(), "{expected_warnings:?}");
            assert_eq!(
                keys[1].easing(),
                expected,
                "the emitted {:?} easing must come from active axis {active_axis}",
                clock
            );
            assert!(matches!(
                keys[1].easing(),
                PropertyKeyframeEasing::CubicBezier { .. }
            ));
        }
    }

    #[test]
    fn vector2_x_only_motion_uses_x_easing_forward_and_reversed() {
        assert_single_axis_vector_easing(0);
    }

    #[test]
    fn vector2_y_only_motion_uses_y_easing_forward_and_reversed() {
        assert_single_axis_vector_easing(1);
    }

    #[test]
    fn stroke_join_keys_are_discrete_and_reject_unrepresentable_tracks() {
        use crate::properties::{NumericProperty, NumericValueKind};

        let numeric = NumericProperty {
            values: vec![1.0],
            animated: true,
            expression_present: false,
            expression_enabled: false,
            dimensions_separated: false,
            value_kind: NumericValueKind::Integer,
            keyframes: [(-1.0, 1.0), (0.5, 2.0), (2.0, 3.0)]
                .into_iter()
                .map(|(time, value)| {
                    let mut key = numeric_key(time, value);
                    key.out_interpolation = 3;
                    key
                })
                .collect(),
        };
        let target = NumericAnimationTarget::stroke_join(PropertyTarget::layer(
            LayerId::new(42),
            PropType::StrokeJoin,
        ));
        let convert = |value: &NumericProperty, clock| {
            numeric_entries(
                "ADBE Vector Stroke Line Join",
                value,
                std::slice::from_ref(&target),
                clock,
            )
        };
        let (entries, warnings) = convert(&numeric, NumericAnimationClock::source_local());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(entries.len(), 1);
        let keys = entries[0].animator.keyframe_track().unwrap().keyframes();
        for ((key, time), join) in keys
            .iter()
            .zip([-1000, 500, 2000])
            .zip(["miter", "round", "bevel"])
        {
            assert_eq!(key.layer_time().as_millis(), time);
            assert_eq!(key.value(), &PropertyValue::String(join.into()));
            assert_eq!(key.easing(), PropertyKeyframeEasing::Hold);
        }
        AnimationGraph::from_entries(entries).unwrap();

        let mut static_join = numeric.clone();
        static_join.keyframes.clear();
        static_join.animated = false;
        let (entries, warnings) = convert(&static_join, NumericAnimationClock::source_local());
        assert!(entries.is_empty());
        assert!(warnings.is_empty());
        let mut spatial_join = numeric.clone();
        spatial_join.keyframes[0].spatial_out = vec![1.0];
        let (entries, warnings) = convert(&spatial_join, NumericAnimationClock::source_local());
        assert!(entries.is_empty());
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("spatial tangents"))
        );

        for invalid in [0.0, 1.5, 4.0, f64::NAN, f64::INFINITY] {
            let mut invalid_numeric = numeric.clone();
            invalid_numeric.keyframes[1].values[0] = invalid;
            let (entries, warnings) =
                convert(&invalid_numeric, NumericAnimationClock::source_local());
            assert!(entries.is_empty());
            assert!(
                warnings
                    .iter()
                    .any(|warning| warning.contains("Stroke Join value"))
            );
        }
        let mut non_hold = numeric.clone();
        non_hold.keyframes[0].out_interpolation = 1;
        let (entries, warnings) = convert(&non_hold, NumericAnimationClock::source_local());
        assert!(entries.is_empty());
        assert!(warnings.iter().any(|warning| warning.contains("non-Hold")));
        let (entries, warnings) = convert(
            &numeric,
            NumericAnimationClock::ParentIdentity {
                start: 3.0,
                stretch: -1.0,
            },
        );
        assert!(entries.is_empty());
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("reverse-stretched"))
        );
        let mut expression = numeric;
        expression.expression_enabled = true;
        let (entries, warnings) = convert(&expression, NumericAnimationClock::source_local());
        assert!(entries.is_empty());
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("enabled AE expression"))
        );
    }

    #[test]
    fn nonposition_spatial_curves_are_diagnosed_without_invalid_fx_metadata() {
        use fx_schema::{LayerId, PropType, PropertyTarget};

        let project = read_project(include_bytes!(
            "../../tests/fixtures/properties/property_2D_position.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("pinned composition")
        };
        let properties = read_transform(&comp.layers[0].content).unwrap();
        let mut numeric = properties
            .iter()
            .find(|p| p.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .clone();
        numeric.keyframes[0].spatial_out[0] = 10.0;
        numeric.keyframes[1].spatial_in[0] = -10.0;
        for property in [PropType::AnchorPointX, PropType::PositionX] {
            let target = NumericAnimationTarget::float(
                PropertyTarget::layer(LayerId::new(1), property),
                0,
                1.0,
            );
            let (entries, warnings) = numeric_entries(
                "spatial curve",
                &numeric,
                &[target],
                NumericAnimationClock::source_local(),
            );
            if property == PropType::AnchorPointX {
                assert!(entries.is_empty());
                assert!(
                    warnings
                        .iter()
                        .any(|warning| warning.contains("nonzero spatial tangents"))
                );
            } else {
                assert_eq!(entries.len(), 1);
                let track = entries[0].animator.keyframe_track().unwrap();
                assert!(!track.has_spatial_tangents());
                assert!(
                    warnings
                        .iter()
                        .any(|warning| warning.contains("shared path-speed"))
                );
            }
        }
    }

    #[test]
    fn mixed_linear_bezier_sides_keep_the_authored_ease() {
        for (out_kind, in_kind) in [(1, 2), (2, 1)] {
            let mut keys = [numeric_key(0.0, 0.0), numeric_key(2.0, 100.0)];
            keys[0].out_interpolation = out_kind;
            keys[1].in_interpolation = in_kind;
            keys[0].out_speed[0] = 0.0;
            keys[1].in_speed[0] = 0.0;
            let mut warnings = Vec::new();
            let easing = super::easing_for_key(&keys, 1, 0, 1.0, &mut warnings, "Opacity");
            let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = easing else {
                panic!("mixed interpolation was lost: {warnings:?}");
            };
            assert!(warnings.is_empty());
            assert_eq!(y1, if out_kind == 1 { x1 } else { 0.0 });
            assert_eq!(y2, if in_kind == 1 { x2 } else { 1.0 });
            let reversed =
                reverse_easing_for_key(&keys, 0, 0, 1.0, &mut warnings, "Opacity").unwrap();
            assert_eq!(
                reversed,
                PropertyKeyframeEasing::CubicBezier {
                    x1: 1.0 - x2,
                    y1: 1.0 - y2,
                    x2: 1.0 - x1,
                    y2: 1.0 - y1,
                }
            );
        }
    }

    #[test]
    fn native_straight_position_does_not_ease_geometry_twice() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/pr4442_native/sources/hierarchy_animated_bounds_precomp.aep"
        ))
        .unwrap();
        let ItemKind::Composition(composition) = &project.item(16).unwrap().kind else {
            panic!("pinned composition")
        };
        let layer = composition
            .layers
            .iter()
            .find(|layer| layer.record.id() == 29)
            .unwrap();
        let numeric = read_transform(&layer.content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .unwrap();
        assert_eq!(numeric.keyframes.len(), 2);
        assert_ne!(numeric.keyframes[0].values, numeric.keyframes[1].values);
        for key in &numeric.keyframes {
            assert_eq!(key.spatial_in, vec![0.; 3]);
            assert_eq!(key.spatial_out, vec![0.; 3]);
        }
        for clock in [
            NumericAnimationClock::source_local(),
            NumericAnimationClock::ParentIdentity {
                start: 3.,
                stretch: -1.,
            },
        ] {
            let targets = [PropType::PositionX, PropType::PositionY]
                .into_iter()
                .enumerate()
                .map(|(axis, property)| {
                    NumericAnimationTarget::float(
                        PropertyTarget::layer(LayerId::new(29), property),
                        axis,
                        1.,
                    )
                })
                .collect::<Vec<_>>();
            let (entries, warnings) = numeric_entries("ADBE Position", &numeric, &targets, clock);
            assert_eq!(entries.len(), 2, "{warnings:?}");
            for (axis, entry) in entries.iter().enumerate() {
                let track = entry.animator.keyframe_track().unwrap();
                assert_eq!(track.keyframes().len(), 2);
                // AE zero-handle geometry is straight traveled distance. In FX,
                // present zero tangents apply smoothstep to temporal progress,
                // changing quarter/three-quarter positions despite equal ends.
                assert!(!track.has_spatial_tangents(), "axis {axis}, {clock:?}");
                let source_index = if clock.reversed() { 0 } else { 1 };
                let expected = super::straight_spatial_easing_for_key(
                    &numeric.keyframes,
                    source_index,
                    clock,
                    &mut Vec::new(),
                    "ADBE Position",
                )
                .unwrap()
                .unwrap();
                assert_eq!(track.keyframes()[1].easing(), expected);
            }
        }
    }

    #[test]
    fn noise_equal_endpoints_stay_put_only_under_a_spatial_path_speed() {
        // Supplemental: the AE-authored spatial Bezier keys of pr4442
        // hierarchy_animated_bounds_precomp.aep layer 29 (zero tangents, 60%
        // influences) with rewritten Y values and arriving speed.
        let project = read_project(include_bytes!(
            "../../tests/fixtures/pr4442_native/sources/hierarchy_animated_bounds_precomp.aep"
        ))
        .unwrap();
        let ItemKind::Composition(composition) = &project.item(16).unwrap().kind else {
            panic!("pinned composition")
        };
        let layer = composition
            .layers
            .iter()
            .find(|layer| layer.record.id() == 29)
            .unwrap();
        let mut spatial = read_transform(&layer.content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .unwrap()
            .keyframes;
        assert!(!spatial[0].spatial_out.is_empty() && spatial[1].in_speed.len() == 1);
        spatial[0].values[1] = 350.000_000_000_000_1;
        spatial[1].values[1] = 350.0;
        spatial[1].in_speed[0] = 6.25;
        let mut warnings = Vec::new();
        let mut arriving = |keys: &[NumericKeyframe], multiplier| {
            super::easing_for_key(keys, 1, 1, multiplier, &mut warnings, "ADBE Position")
        };

        // The target scale cannot make noise move, including zero or flipped.
        for multiplier in [1.0, -1.0, 0.0, 100.0] {
            assert_eq!(
                arriving(&spatial, multiplier),
                PropertyKeyframeEasing::Linear,
                "scale {multiplier}"
            );
        }
        let mut real = spatial.clone();
        real[1].values[1] = 360.0;
        let eased = arriving(&real, 1.0);
        assert!(matches!(eased, PropertyKeyframeEasing::CubicBezier { .. }));
        assert_eq!(arriving(&real, -1.0), eased);

        // Value-space keys with the same numbers genuinely move away and back:
        // AE's arriving handle stays speed x duration x influence below the
        // end value, which the unbounded normalized handle keeps.
        let mut value_space = spatial.clone();
        for key in &mut value_space {
            key.spatial_in.clear();
            key.spatial_out.clear();
        }
        let PropertyKeyframeEasing::CubicBezier { x2, y2, .. } = arriving(&value_space, 1.0) else {
            panic!("value-space excursion erased")
        };
        let delta = value_space[1].values[1] - value_space[0].values[1];
        let duration = value_space[1].time_secs - value_space[0].time_secs;
        assert!(((1.0 - y2) * delta - 6.25 * duration * (1.0 - x2)).abs() < 1e-9);

        assert_eq!(
            reverse_easing_for_key(&spatial, 0, 1, 1.0, &mut warnings, "ADBE Position").unwrap(),
            PropertyKeyframeEasing::Linear
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn reverse_clock_reverses_temporal_bezier_and_rejects_hold() {
        let mut warnings = Vec::new();
        let keys = [numeric_key(0.0, 0.0), numeric_key(1.0, 1.0)];
        assert!(matches!(
            reverse_easing_for_key(&keys, 0, 0, 1.0, &mut warnings, "Position").unwrap(),
            PropertyKeyframeEasing::CubicBezier { .. }
        ));

        let mut hold = keys;
        hold[0].out_interpolation = 3;
        assert!(
            reverse_easing_for_key(&hold, 0, 0, 1.0, &mut warnings, "Position")
                .unwrap_err()
                .contains("cannot be represented")
        );
    }

    fn composition_layer_with_animated_transform<'a>(
        project: &'a StructuralProject,
        property_name: &str,
    ) -> (&'a Composition, &'a Layer) {
        project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) => composition.layers.iter().find_map(|layer| {
                    read_transform(&layer.content)
                        .ok()
                        .is_some_and(|properties| {
                            properties.into_iter().any(|property| {
                                property.match_name == property_name
                                    && property
                                        .numeric
                                        .is_ok_and(|numeric| !numeric.keyframes.is_empty())
                            })
                        })
                        .then_some((composition.as_ref(), layer))
                }),
                _ => None,
            })
            .expect("pinned property fixture must contain the requested animated transform")
    }

    #[test]
    fn parent_opacity_is_skipped_before_budget_admission_for_later_parent_motion() {
        let opacity_project = read_project(include_bytes!(
            "../../tests/fixtures/properties/property_1D_opacity.aep"
        ))
        .unwrap();
        let (opacity_comp, opacity_layer) =
            composition_layer_with_animated_transform(&opacity_project, "ADBE Opacity");
        let mut opacity_probe = AnimationBudget::default();
        let (opacity_entries, opacity_warnings) = super::transform_entries(
            opacity_layer,
            opacity_comp,
            LayerId::new(90),
            AnimationTargetClock::ParentIdentity,
            [1.0; 2],
            &mut opacity_probe,
        );
        assert!(opacity_warnings.is_empty(), "{opacity_warnings:?}");
        assert!(opacity_entries.iter().any(|entry| {
            entry.target == PropertyTarget::layer(LayerId::new(90), PropType::Opacity)
        }));
        assert!(opacity_probe.used() > 0);

        let rotation_project = read_project(include_bytes!(
            "../../tests/fixtures/properties/property_rotation.aep"
        ))
        .unwrap();
        let (rotation_comp, rotation_layer) =
            composition_layer_with_animated_transform(&rotation_project, "ADBE Rotate Z");
        let mut rotation_probe = AnimationBudget::default();
        let (rotation_entries, rotation_warnings) = super::transform_parent_entries(
            rotation_layer,
            rotation_comp,
            LayerId::new(91),
            [1.0; 2],
            &mut rotation_probe,
        );
        assert!(rotation_warnings.is_empty(), "{rotation_warnings:?}");
        assert_eq!(rotation_entries.len(), 1);
        let rotation_bytes = rotation_probe.used();

        let mut exact = AnimationBudget::with_limit(rotation_bytes);
        let (discarded_opacity, opacity_warnings) = super::transform_parent_entries(
            opacity_layer,
            opacity_comp,
            LayerId::new(90),
            [1.0; 2],
            &mut exact,
        );
        assert!(discarded_opacity.is_empty());
        assert!(opacity_warnings.is_empty(), "{opacity_warnings:?}");
        assert_eq!(exact.remaining(), rotation_bytes);
        assert_eq!(exact.denials(), 0);

        let (rotation_entries, rotation_warnings) = super::transform_parent_entries(
            rotation_layer,
            rotation_comp,
            LayerId::new(91),
            [1.0; 2],
            &mut exact,
        );
        assert!(rotation_warnings.is_empty(), "{rotation_warnings:?}");
        assert_eq!(rotation_entries.len(), 1);
        assert_eq!(
            rotation_entries[0].target,
            PropertyTarget::layer(LayerId::new(91), PropType::Rotation)
        );
        assert_eq!(exact.remaining(), 0);
    }

    #[test]
    fn native_transform_tracks_form_a_valid_editable_graph() {
        for bytes in [
            include_bytes!("../../tests/fixtures/properties/property_2D_position.aep").as_slice(),
            include_bytes!("../../tests/fixtures/properties/property_rotation.aep").as_slice(),
            include_bytes!("../../tests/fixtures/properties/property_scale.aep").as_slice(),
            include_bytes!("../../tests/fixtures/properties/property_1D_opacity.aep").as_slice(),
        ] {
            let project = read_project(bytes).expect("pinned native AEP must parse");
            let mut tested = false;
            for item in &project.items {
                let ItemKind::Composition(composition) = &item.kind else {
                    continue;
                };
                for layer in &composition.layers {
                    let (entries, warnings) = transform_entries(
                        layer,
                        composition,
                        LayerId::new(9001),
                        AnimationTargetClock::SourceLayerLocal,
                    );
                    if entries.is_empty() {
                        continue;
                    }
                    assert!(
                        warnings.iter().all(|warning| !warning.contains("omitted")),
                        "native animation unexpectedly omitted: {warnings:?}"
                    );
                    AnimationGraph::from_entries(entries)
                        .expect("native tracks must satisfy graph invariants");
                    tested = true;
                }
            }
            assert!(tested, "fixture must contain a mapped native animation");
        }
    }

    #[test]
    fn source_local_rebase_preserves_signed_keys_values_easing_and_ids() {
        let numeric = NumericProperty {
            values: vec![0.0],
            animated: true,
            expression_present: false,
            expression_enabled: false,
            dimensions_separated: false,
            value_kind: NumericValueKind::Continuous,
            keyframes: vec![numeric_key(0.0, 0.0), numeric_key(2.0, 100.0)],
        };
        let target = NumericAnimationTarget::float(
            PropertyTarget::layer(LayerId::new(42), PropType::Opacity),
            0,
            1.0,
        );
        let convert = |clock| {
            numeric_entries(
                "Source Text Opacity",
                &numeric,
                std::slice::from_ref(&target),
                clock,
            )
            .0
        };
        let original = convert(NumericAnimationClock::source_local());
        let rebased = convert(NumericAnimationClock::source_local_rebased(1.0));
        let original_keys = original[0].animator.keyframe_track().unwrap().keyframes();
        let rebased_keys = rebased[0].animator.keyframe_track().unwrap().keyframes();

        assert_eq!(
            rebased_keys
                .iter()
                .map(|key| key.layer_time().as_millis())
                .collect::<Vec<_>>(),
            vec![-1_000, 1_000]
        );
        for (original, rebased) in original_keys.iter().zip(rebased_keys) {
            assert_eq!(rebased.id(), original.id());
            assert_eq!(rebased.value(), original.value());
            assert_eq!(rebased.easing(), original.easing());
        }
    }

    fn native_color_vector_numeric() -> NumericProperty {
        let parsed = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/properties/native-color-vector-ease.rifx"),
            |_| false,
        )
        .unwrap();
        crate::properties::read_numeric(parsed.chunks()[0].children().unwrap()).unwrap()
    }

    fn color_target() -> NumericAnimationTarget {
        NumericAnimationTarget::color(
            PropertyTarget::effect_param(fx_schema::EffectId::new(1), "color"),
            [0, 1, 2, 3],
            [1.; 4],
        )
    }

    #[test]
    fn native_color_speed_uses_rgb_vector_distance_in_both_directions() {
        let numeric = native_color_vector_numeric();
        assert_eq!(numeric.value_kind, NumericValueKind::Color);
        assert_eq!(numeric.keyframes[0].values, [0.75, 0.75, 0.75, 1.]);
        for clock in [
            NumericAnimationClock::source_local(),
            NumericAnimationClock::ParentIdentity {
                start: 2.,
                stretch: -1.,
            },
        ] {
            let (entries, warnings) =
                numeric_entries("native Tint", &numeric, &[color_target()], clock);
            assert!(warnings.is_empty(), "{warnings:?}");
            let fx_schema::animator::AnimatorData::Keyframes { track, .. } =
                entries[0].animator.data()
            else {
                panic!()
            };
            for key in &track.keyframes()[1..] {
                let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = key.easing() else {
                    panic!()
                };
                let expected = [0.88, 0.14, 0.12, 0.86];
                for (actual, expected) in [x1, y1, x2, y2].into_iter().zip(expected) {
                    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
                }
            }
        }
    }

    #[test]
    fn color_vector_ease_is_independent_of_axis_sign_and_target_scaling() {
        let mut numeric = native_color_vector_numeric();
        numeric.keyframes.truncate(2);
        numeric.keyframes[0].values = vec![0.1, 0.2, 0.3, 0.8];
        numeric.keyframes[1].values = vec![0.4, 0.2, 0.7, 0.8];
        // The RGB distance is 0.5 normalized units, or 127.5 native units.
        numeric.keyframes[0].out_speed = vec![127.5 * 0.5];
        numeric.keyframes[1].in_speed = vec![127.5 * 0.5];
        for target in [
            color_target(),
            NumericAnimationTarget::float(
                PropertyTarget::effect_param(fx_schema::EffectId::new(2), "red"),
                0,
                -255.,
            ),
        ] {
            let (entries, warnings) = numeric_entries(
                "changed color",
                &numeric,
                &[target],
                NumericAnimationClock::source_local(),
            );
            assert!(warnings.is_empty(), "{warnings:?}");
            let fx_schema::animator::AnimatorData::Keyframes { track, .. } =
                entries[0].animator.data()
            else {
                panic!()
            };
            let PropertyKeyframeEasing::CubicBezier { y1, y2, .. } = track.keyframes()[1].easing()
            else {
                panic!()
            };
            assert!((y1 - 0.5 * (2. / 3.) * 0.88).abs() < 1e-12);
            assert!((y2 - (1. - 0.5 * (2. / 3.) * 0.88)).abs() < 1e-12);
        }
        numeric.keyframes[1].values[3] = 0.5;
        let (entries, warnings) = numeric_entries(
            "changing alpha",
            &numeric,
            &[color_target()],
            NumericAnimationClock::source_local(),
        );
        assert!(entries.is_empty());
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("alpha-varying native color speed")),
            "{warnings:?}"
        );
    }

    #[test]
    fn native_keys_map_to_parent_identity_clock_and_typed_targets() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/properties/property_1D_opacity.aep"
        ))
        .unwrap();
        let numeric = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) => composition.layers.first(),
                _ => None,
            })
            .and_then(|layer| read_transform(&layer.content).ok())
            .and_then(|properties| {
                properties
                    .into_iter()
                    .find(|property| property.match_name == "ADBE Opacity")
            })
            .unwrap()
            .numeric
            .unwrap();
        let targets = [NumericAnimationTarget::float(
            PropertyTarget::layer(LayerId::new(77), PropType::Opacity),
            0,
            100.0,
        )];
        let (entries, warnings) = numeric_entries(
            "ADBE Opacity",
            &numeric,
            &targets,
            NumericAnimationClock::ParentIdentity {
                start: 2.0,
                stretch: 3.0,
            },
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entries[0].animator.data()
        else {
            panic!("native numeric property must produce keyframes")
        };
        assert_eq!(track.keyframes()[0].layer_time().as_millis(), 2_000);
        assert_eq!(track.keyframes()[1].layer_time().as_millis(), 17_000);

        let vector = NumericAnimationTarget::vector2(
            PropertyTarget::effect_param(fx_schema::EffectId::new(1), "point"),
            [0, 1],
            [1.0, 1.0],
        );
        let color = NumericAnimationTarget::color(
            PropertyTarget::effect_param(fx_schema::EffectId::new(1), "color"),
            [0, 1, 2, 3],
            [1.0; 4],
        );
        let key = NumericKeyframe {
            values: vec![0.1, 0.2, 0.3, 0.4],
            ..numeric_key(0.0, 0.1)
        };
        assert_eq!(
            vector.value_at(&key).unwrap(),
            PropertyValue::Vector2([0.1, 0.2])
        );
        assert_eq!(
            color.value_at(&key).unwrap(),
            PropertyValue::Color([0.1, 0.2, 0.3, 0.4])
        );
    }
    #[test]
    fn shared_straight_spatial_easing_uses_distance_for_generic_axes_and_leaves_curves_unchanged() {
        let key = |time, values: Vec<f64>| NumericKeyframe {
            values,
            spatial_in: vec![0.; 3],
            spatial_out: vec![0.; 3],
            ..numeric_key(time, 0.)
        };
        for destination in [vec![0., -10., 0.], vec![-6., 0., -8.], vec![6., 0., 8.]] {
            let keys = vec![key(0., vec![0.; 3]), key(2., destination)];
            let forward = super::straight_spatial_easing_for_key(
                &keys,
                1,
                NumericAnimationClock::source_local(),
                &mut Vec::new(),
                "generic spatial",
            )
            .unwrap()
            .unwrap();
            let expected = super::easing_for_key(
                &[numeric_key(0., 0.), numeric_key(2., 10.)],
                1,
                0,
                1.,
                &mut Vec::new(),
                "distance",
            );
            assert_eq!(forward, expected);
            let mut curved = keys.clone();
            curved[0].spatial_out[0] = 1.;
            assert!(
                super::straight_spatial_easing_for_key(
                    &curved,
                    1,
                    NumericAnimationClock::source_local(),
                    &mut Vec::new(),
                    "curved"
                )
                .is_none()
            );
            let mut axes = keys.clone();
            axes[0].out_speed = vec![1., 1., 1.];
            assert!(
                super::straight_spatial_easing_for_key(
                    &axes,
                    1,
                    NumericAnimationClock::source_local(),
                    &mut Vec::new(),
                    "per-axis"
                )
                .is_none()
            );
        }
    }
}
