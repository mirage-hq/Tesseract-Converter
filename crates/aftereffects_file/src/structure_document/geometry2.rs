//! Geometry2 on a continuously rasterized 2D Shape runs after its layer Transform;
//! on a footage still it runs on the source plane, before the layer Transform.
//! Other owner/effect-stage combinations remain explicitly unsupported.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use fx_schema::{
    Duration, GroupLayer, KeyframeId, LayerData, LayerId, PercentageProperty, Position, PropType,
    PropertyAnimator, PropertyKeyframeEasing, PropertyTarget, PropertyValue, Time, TimeOffset,
    TimeRangeProperty, Transform,
    animator::{AnimationGraphEntry, PropertyKeyframe, PropertyKeyframeTrack},
};

use crate::{
    effects::native::{self, DecodedEffect},
    properties::{self, NumericProperty},
    rifx::Chunk,
    structure::{Composition, Layer, ProjectItem},
};

use super::{
    animation::{NumericAnimationClock, NumericAnimationTarget},
    animation_budget::{AnimationBudget, committed_entry_reservation_bytes},
    control_links::{effect_instance_name, expression, finished, quoted, token, unique_run},
};

const MATCH_NAME: &str = "ADBE Geometry2";

#[derive(Clone)]
enum Point {
    Static([f64; 2]),
    Curve(BTreeMap<i64, [f64; 2]>),
}

impl Point {
    fn initial(&self) -> [f64; 2] {
        match self {
            Self::Static(point) => *point,
            // The bounded fitter always retains both interval endpoints.
            Self::Curve(keys) => *keys.first_key_value().expect("nonempty fitted point").1,
        }
    }
}

pub(super) struct Prepared {
    anchor: Point,
    position: Point,
    transform: Transform,
}

#[derive(Debug, PartialEq)]
enum PointExpression<'a> {
    Origin(&'a str),
    PositionAlias(&'a str),
}

fn parse_expression(mut text: &str) -> Option<PointExpression<'_>> {
    if text.len() > 16 * 1024 {
        return None;
    }
    if token(&mut text, "thisComp").is_some() {
        token(&mut text, ".")?;
        token(&mut text, "layer")?;
        token(&mut text, "(")?;
        let name = quoted(&mut text)?;
        token(&mut text, ")")?;
        token(&mut text, ".")?;
        token(&mut text, "toComp")?;
        token(&mut text, "(")?;
        token(&mut text, "[")?;
        token(&mut text, "0")?;
        token(&mut text, ",")?;
        token(&mut text, "0")?;
        if token(&mut text, ",").is_some() {
            token(&mut text, "0")?;
        }
        token(&mut text, "]")?;
        token(&mut text, ")")?;
        return finished(text).then_some(PointExpression::Origin(name));
    }
    token(&mut text, "effect")?;
    token(&mut text, "(")?;
    let name = quoted(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, "(")?;
    let parameter = quoted(&mut text)?;
    token(&mut text, ")")?;
    (finished(text) && matches!(parameter, "Position" | "ADBE Geometry2-0002"))
        .then_some(PointExpression::PositionAlias(name))
}

fn body(layer: &Layer) -> Result<(&[Chunk], &str), String> {
    let root = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let parade = unique_run(&root, "ADBE Effect Parade").map_err(|e| e.to_string())?;
    let groups = properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?;
    let instances = properties::runs(groups).map_err(|e| e.to_string())?;
    let [(MATCH_NAME, run)] = instances.as_slice() else {
        return Err("requires a sole Transform effect; mixed effect stages are not mapped".into());
    };
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let controls = properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?;
    validate_controls(controls)?;
    let name =
        effect_instance_name(descriptor, controls).ok_or("effect display name is malformed")?;
    Ok((controls, name))
}

fn validate_controls(controls: &[Chunk]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for (name, run) in properties::runs(controls).map_err(|e| e.to_string())? {
        if name == "ADBE Group End" {
            continue;
        }
        if !seen.insert(name) {
            return Err(format!("duplicate control {name}"));
        }
        if name == "ADBE Effect Built In Params" {
            let options = properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?;
            if properties::runs(options)
                .map_err(|e| e.to_string())?
                .iter()
                .any(|(name, _)| *name != "ADBE Group End")
            {
                return Err("nonempty effect compositing options require separate mapping".into());
            }
        }
    }
    Ok(())
}

fn parameter<'a>(
    source: &'a DecodedEffect,
    name: &str,
) -> Result<Option<&'a NumericProperty>, String> {
    source
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == name)
        .map(|parameter| parameter.numeric.as_ref().map_err(|e| e.to_string()))
        .transpose()
}

fn scalar(source: &DecodedEffect, name: &str, default: f64) -> Result<f64, String> {
    let Some(numeric) = parameter(source, name)? else {
        return Ok(default);
    };
    if numeric.animated
        || numeric.expression_enabled
        || numeric.dimensions_separated
        || !numeric.keyframes.is_empty()
    {
        return Err(format!("{name}: animated/expression scalar not mapped"));
    }
    let [value] = numeric.values.as_slice() else {
        return Err(format!("{name}: expected a scalar"));
    };
    if !value.is_finite() {
        return Err(format!("{name}: non-finite scalar"));
    }
    Ok(*value)
}

struct PointContext<'a> {
    source: &'a DecodedEffect,
    body: &'a [Chunk],
    effect_name: &'a str,
    owner: &'a Layer,
    composition: &'a Composition,
    items: &'a HashMap<u32, &'a ProjectItem>,
}

impl PointContext<'_> {
    fn read(&self, name: &str, position: Option<&Point>) -> Result<Point, String> {
        let numeric =
            parameter(self.source, name)?.ok_or_else(|| format!("{name}: point missing"))?;
        if numeric.dimensions_separated {
            return Err(format!("{name}: separated effect point not mapped"));
        }
        if numeric.expression_enabled {
            let runs = properties::runs(self.body).map_err(|e| e.to_string())?;
            let run = unique_run(&runs, name).map_err(|e| e.to_string())?;
            let leaf = properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?;
            let text = expression(leaf).map_err(|e| e.to_string())?;
            return match parse_expression(text) {
                Some(PointExpression::Origin(target)) => super::shapes::composition_origin_curve(
                    self.composition,
                    self.items,
                    self.owner,
                    target,
                )
                .map(Point::Curve),
                Some(PointExpression::PositionAlias(effect_name))
                    if self.effect_name == effect_name =>
                {
                    position
                        .cloned()
                        .ok_or_else(|| "cyclic Position alias".into())
                }
                _ => Err(format!("{name}: unsupported point expression")),
            };
        }
        if numeric.animated || !numeric.keyframes.is_empty() {
            return Err(format!("{name}: native animated point not mapped"));
        }
        let [x, y] = numeric.values.as_slice() else {
            return Err(format!("{name}: expected a two-dimensional point"));
        };
        if !x.is_finite() || !y.is_finite() {
            return Err(format!("{name}: non-finite point"));
        }
        Ok(Point::Static([*x, *y]))
    }
}

pub(super) fn prepare(
    owner: &Layer,
    composition: &Composition,
    items: &HashMap<u32, &ProjectItem>,
    converted: &GroupLayer,
    planar_ancestors: bool,
) -> Result<Option<Prepared>, String> {
    let (effects, _) = native::read_effects(
        &owner.content,
        [f64::from(composition.width), f64::from(composition.height)],
    );
    let Some(source) = effects
        .iter()
        .find(|effect| effect.match_name == MATCH_NAME)
    else {
        return Ok(None);
    };
    if !source.enabled || !owner.record.flags().effects_active {
        return Ok(None);
    }
    if owner.record.layer_type() != 4
        || owner.record.flags().three_d_layer
        || owner.record.flags().adjustment_layer
        || !planar_ancestors
    {
        return Err("post-layer mapping requires a planar Shape and planar ancestors".into());
    }
    if !converted.effects.is_empty()
        || !converted.masks.is_empty()
        || converted.track_matte.is_some()
        || owner.record.flags().preserve_transparency
    {
        return Err("mask, preserve-transparency or Layer Style combinations require separate stage mapping".into());
    }
    let (body, effect_name) = body(owner)?;
    let mut transform = static_transform(source)?;
    let points = PointContext {
        source,
        body,
        effect_name,
        owner,
        composition,
        items,
    };
    let position = points.read("ADBE Geometry2-0002", None)?;
    let anchor = points.read("ADBE Geometry2-0001", Some(&position))?;
    transform.anchor_point = anchor.initial();
    transform.position = Position::TwoD(position.initial());
    Ok(Some(Prepared {
        anchor,
        position,
        transform,
    }))
}

/// The mapped static controls as a Transform with its points at the origin.
fn static_transform(source: &DecodedEffect) -> Result<Transform, String> {
    for parameter in &source.parameters {
        if !matches!(
            parameter.match_name.as_str(),
            "ADBE Geometry2-0001"
                | "ADBE Geometry2-0002"
                | "ADBE Geometry2-0003"
                | "ADBE Geometry2-0004"
                | "ADBE Geometry2-0005"
                | "ADBE Geometry2-0006"
                | "ADBE Geometry2-0007"
                | "ADBE Geometry2-0008"
                | "ADBE Geometry2-0009"
                | "ADBE Geometry2-0010"
                | "ADBE Geometry2-0011"
                | "ADBE Geometry2-0012"
        ) {
            return Err(format!("unknown control {}", parameter.match_name));
        }
    }
    if scalar(source, "ADBE Geometry2-0005", 0.0)? != 0.0
        || scalar(source, "ADBE Geometry2-0006", 0.0)? != 0.0
        || scalar(source, "ADBE Geometry2-0010", 0.0)? != 0.0
        || scalar(source, "ADBE Geometry2-0012", 1.0)? != 1.0
    {
        return Err("nondefault skew, shutter angle or sampling not mapped".into());
    }
    let uniform = scalar(source, "ADBE Geometry2-0011", 1.0)?;
    let shutter = scalar(source, "ADBE Geometry2-0009", 1.0)?;
    if !matches!(uniform, 0.0 | 1.0) || !matches!(shutter, 0.0 | 1.0) {
        return Err("invalid Uniform Scale or composition shutter switch".into());
    }
    // Geometry2's first scale control is Height; Uniform Scale ignores Width.
    let height = scalar(source, "ADBE Geometry2-0003", 100.0)?;
    let width = scalar(source, "ADBE Geometry2-0004", 100.0)?;
    Ok(Transform {
        anchor_point: [0.0; 2],
        position: Position::TwoD([0.0; 2]),
        scale: [if uniform == 1.0 { height } else { width }, height],
        rotation: scalar(source, "ADBE Geometry2-0007", 0.0)?,
        opacity: PercentageProperty::new(scalar(source, "ADBE Geometry2-0008", 100.0)?)
            .ok_or("opacity is outside current FX range")?,
        skew: 0.0,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0; 3],
    })
}

/// Geometry2 on a footage still transforms the source image plane, before the
/// owner Transform: an editable Group between the owner and its content.
pub(super) struct StillStage {
    name: String,
    transform: Transform,
    anchor: Option<NumericProperty>,
    position: Option<NumericProperty>,
}

/// Maps native static or keyed points in source pixels with static scalar
/// controls. Point expressions and owner effects, styles or masks, whose FX
/// order would differ from the native stage order, are not mapped.
pub(super) fn prepare_still(
    owner: &Layer,
    size: [u16; 2],
    converted: &GroupLayer,
) -> Result<Option<StillStage>, String> {
    let (effects, _) = native::read_effects(&owner.content, size.map(f64::from));
    let mut instances = effects
        .iter()
        .filter(|effect| effect.match_name == MATCH_NAME);
    let Some(source) = instances.next() else {
        return Ok(None);
    };
    if instances.next().is_some() {
        return Err("several Transform effects are not mapped".into());
    }
    let flags = owner.record.flags();
    if !source.enabled || !flags.effects_active {
        return Ok(None);
    }
    if flags.three_d_layer || flags.adjustment_layer || flags.preserve_transparency {
        return Err(
            "a still-image stage requires a 2D owner without preserved transparency".into(),
        );
    }
    if !converted.effects.is_empty() || !converted.masks.is_empty() {
        return Err(
            "owner effects, Layer Styles or masks would change the native stage order".into(),
        );
    }
    if size.contains(&0) {
        return Err("source image dimensions are unavailable".into());
    }
    let (descriptor, controls) = instance(owner, source.index)?;
    validate_controls(controls)?;
    let name =
        effect_instance_name(descriptor, controls).ok_or("effect display name is malformed")?;
    let mut transform = static_transform(source)?;
    let explicit: BTreeSet<_> = properties::runs(controls)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(control, _)| control)
        .collect();
    let point = |control: &str| {
        if explicit.contains(control) {
            still_point(source, control)
        } else {
            declared_center(descriptor, control)?;
            Ok((None, size.map(|extent| f64::from(extent) / 2.0)))
        }
    };
    let (position, initial_position) = point("ADBE Geometry2-0002")?;
    let (anchor, initial_anchor) = point("ADBE Geometry2-0001")?;
    transform.anchor_point = initial_anchor;
    transform.position = Position::TwoD(initial_position);
    Ok(Some(StillStage {
        name: name.to_owned(),
        transform,
        anchor,
        position,
    }))
}

/// The plugin descriptor and explicit controls of one one-based instance.
fn instance(owner: &Layer, index: usize) -> Result<(&[Chunk], &[Chunk]), String> {
    let root = properties::root_runs(&owner.content).map_err(|e| e.to_string())?;
    let parade = unique_run(&root, "ADBE Effect Parade").map_err(|e| e.to_string())?;
    let groups = properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?;
    let instances = properties::runs(groups).map_err(|e| e.to_string())?;
    let (_, run) = index
        .checked_sub(1)
        .and_then(|index| instances.get(index))
        .ok_or("effect instance missing")?;
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let controls = properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?;
    Ok((descriptor, controls))
}

/// An explicit native point: its keys, if any, and its first value.
fn still_point(
    source: &DecodedEffect,
    control: &str,
) -> Result<(Option<NumericProperty>, [f64; 2]), String> {
    let numeric = parameter(source, control)?.ok_or_else(|| format!("{control}: point missing"))?;
    if numeric.expression_enabled || numeric.dimensions_separated {
        return Err(format!(
            "{control}: expression or separated point not mapped on a still"
        ));
    }
    let first = numeric
        .keyframes
        .first()
        .map_or(numeric.values.as_slice(), |key| key.values.as_slice());
    let &[x, y] = first else {
        return Err(format!("{control}: expected a two-dimensional point"));
    };
    if !x.is_finite() || !y.is_finite() {
        return Err(format!("{control}: non-finite point"));
    }
    let keys = (!numeric.keyframes.is_empty()).then(|| numeric.clone());
    Ok((keys, [x, y]))
}

/// Transform declares its points at 50% of the layer, in the plugin API's
/// percent words for both the value and the default. A sparse instance without
/// declarations keeps that default.
fn declared_center(descriptor: &[Chunk], control: &str) -> Result<(), String> {
    const HALF: [u8; 4] = (50_i32 << 16).to_be_bytes();
    let table = properties::unique_list(descriptor, *b"parT").map_err(|e| e.to_string())?;
    if table.is_empty() {
        return Ok(());
    }
    let rows = properties::runs(table).map_err(|e| e.to_string())?;
    let run = unique_run(&rows, control).map_err(|e| format!("{control}: {e}"))?;
    let pard = properties::data(run, *b"pard").map_err(|e| e.to_string())?;
    if pard.len() == 148
        && pard[12..16] == 6_u32.to_be_bytes()
        && [56, 60, 68, 72]
            .into_iter()
            .all(|offset| pard[offset..offset + 4] == HALF)
    {
        Ok(())
    } else {
        Err(format!(
            "{control}: nonstandard declared default not mapped"
        ))
    }
}

impl StillStage {
    /// Keyed points as stage keys on the owner clock, with their conversion
    /// notes; nothing stays charged on failure.
    pub(super) fn entries(
        &self,
        id: LayerId,
        owner: &Layer,
        budget: &mut AnimationBudget,
    ) -> Result<(Vec<AnimationGraphEntry>, Vec<String>), String> {
        let clock = NumericAnimationClock::parent_identity(owner)?;
        let checkpoint = budget.checkpoint();
        let mut entries = Vec::new();
        let mut notes = Vec::new();
        for (control, keys, properties) in [
            (
                "ADBE Geometry2-0001",
                &self.anchor,
                [PropType::AnchorPointX, PropType::AnchorPointY],
            ),
            (
                "ADBE Geometry2-0002",
                &self.position,
                [PropType::PositionX, PropType::PositionY],
            ),
        ] {
            let Some(keys) = keys else {
                continue;
            };
            let [x, y] = properties.map(|property| PropertyTarget::layer(id, property));
            let (mut converted, warnings) = super::animation::numeric_entries(
                control,
                keys,
                &[
                    NumericAnimationTarget::float(x, 0, 1.0),
                    NumericAnimationTarget::float(y, 1, 1.0),
                ],
                clock,
                budget,
            );
            if converted.is_empty() {
                budget.rollback(checkpoint);
                return Err(warnings.join("; "));
            }
            entries.append(&mut converted);
            notes.extend(warnings);
        }
        Ok((entries, notes))
    }

    /// The stage Group around the owner's source content.
    pub(super) fn wrap(
        &self,
        id: LayerId,
        owner: LayerId,
        mut content: GroupLayer,
    ) -> Result<GroupLayer, serde_json::Error> {
        let mut stage = super::group(
            id,
            self.name.clone(),
            Some(owner),
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(super::MAX_TIME_SECS)),
        );
        stage.transform = self.transform;
        stage.description = "editable Geometry2 on the still source plane before the owner Transform; native point keys; raster sampling and motion blur remain approximate".into();
        content.parent = Some(id);
        stage
            .layers
            .push(fx_schema::Layer::from_data(&LayerData::Group(content))?);
        Ok(stage)
    }
}

impl Prepared {
    pub(super) fn apply(
        &self,
        inner: &mut GroupLayer,
        id: LayerId,
        budget: &mut AnimationBudget,
    ) -> Result<Vec<AnimationGraphEntry>, String> {
        let mut entries = Vec::new();
        for (point, properties, tag) in [
            (
                &self.anchor,
                [PropType::AnchorPointX, PropType::AnchorPointY],
                "a",
            ),
            (
                &self.position,
                [PropType::PositionX, PropType::PositionY],
                "p",
            ),
        ] {
            if let Point::Curve(curve) = point {
                for (axis, property) in properties.into_iter().enumerate() {
                    let keys = curve
                        .iter()
                        .map(|(time, value)| {
                            PropertyKeyframe::new(
                                KeyframeId::new(format!("g{id}{tag}{axis}-{time}")),
                                TimeOffset::from_millis(*time),
                                PropertyValue::Float(value[axis]),
                                PropertyKeyframeEasing::Linear,
                            )
                        })
                        .collect();
                    entries.push(AnimationGraphEntry {
                        target: PropertyTarget::layer(id, property),
                        animator: PropertyAnimator::keyframes(
                            PropertyKeyframeTrack::new(keys).map_err(|e| e.to_string())?,
                        ),
                        dependencies: Vec::new(),
                        random_seed_target: None,
                        layer_refs: Default::default(),
                    });
                }
            }
        }
        let mut stage = super::group(
            id,
            inner.name.clone(),
            inner.parent,
            inner.playback.input_range(),
        );
        stage.transform = self.transform;
        stage.blend_mode = inner.blend_mode;
        stage.description = "editable post-layer Geometry2; independent point keys; raster sampling/clipping and motion blur remain approximate".into();
        // Retain the original on every validation/admission failure.
        let mut child = inner.clone();
        child.parent = Some(id);
        child.blend_mode = Default::default();
        stage.layers.push(
            fx_schema::Layer::from_data(&LayerData::Group(child)).map_err(|e| e.to_string())?,
        );
        let checkpoint = budget.checkpoint();
        for entry in &entries {
            if let Err(error) =
                committed_entry_reservation_bytes(entry).and_then(|bytes| budget.reserve(bytes))
            {
                budget.rollback(checkpoint);
                return Err(error.to_string());
            }
        }
        *inner = stage;
        Ok(entries)
    }
}

#[cfg(test)]
#[path = "geometry2/tests.rs"]
mod tests;
