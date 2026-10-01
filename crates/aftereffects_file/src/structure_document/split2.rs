//! Bounded flat-profile CC Split 2 as independently editable masked sibling copies.
//! Height-percent displacement and finite-segment extension are approximations.

use super::{
    Converter, LayerContext, LayerPurpose, Limitation,
    animation::{self, NumericAnimationClock, NumericAnimationTarget},
    group,
    posterize_time::{animations_closed, closed_visual, collect_layers, reparent},
};
use crate::{
    document::DocumentError,
    effects::native::{self, DecodedEffect},
    properties::{self, NumericKeyframe, NumericProperty, NumericValueKind},
    rifx::Chunk,
    structure::{Layer, ProjectItem},
};
use fx_schema::{
    Duration, FxItemId, GroupLayer, LayerData as FxLayer, LayerId, NonNegativeProperty, Position,
    PropertyTarget, ShapeContent, ShapePath, ShapePathCommand, Time, TimeRangeProperty, Transform,
    layer::{MaskMode, PathMask, ShapeLayer},
};
use std::collections::HashSet;

const MATCH_NAME: &str = "CC Split 2";
struct Profile {
    line_y: f64,
    amounts: [NumericProperty; 2],
    start: u64,
    end: u64,
}

fn scalar(effect: &DecodedEffect, name: &str) -> Result<NumericProperty, String> {
    let mut parameters = effect.parameters.iter().filter(|p| p.match_name == name);
    let value = parameters.next().ok_or_else(|| format!("missing {name}"))?;
    if parameters.next().is_some() {
        return Err(format!("duplicate {name}"));
    }
    value.numeric.clone().map_err(|e| e.to_string())
}
fn validate_amount(value: &NumericProperty) -> Result<(), String> {
    let bounded = |v: &[f64]| v.len() == 1 && v[0].is_finite() && (0.0..=250.0).contains(&v[0]);
    if value.expression_present || value.expression_enabled || value.dimensions_separated {
        return Err("Split amount expressions/separated values are unsupported".into());
    }
    if !value.animated {
        if bounded(&value.values) && value.keyframes.is_empty() { return Ok(()); }
    } else if value.keyframes.len() >= 2 && value.keyframes.len() <= 64
        && value.keyframes.first().is_some_and(|k| k.values == [0.0])
        && value.keyframes.last().is_some_and(|k| k.values.len() == 1 && k.values[0] > 0.0)
        && value.keyframes.iter().all(|k| bounded(&k.values) && k.time_secs.is_finite()
            && matches!(k.in_interpolation, 1..=3) && matches!(k.out_interpolation, 1..=3)
            && k.in_speed.iter().chain(&k.out_speed).all(|s| s.is_finite() && *s >= 0.0)
            && k.spatial_in.is_empty() && k.spatial_out.is_empty())
        && value.keyframes.windows(2).all(|ks| {
            let dt = ks[1].time_secs - ks[0].time_secs;
            let delta = ks[1].values[0] - ks[0].values[0];
            let bounded_handle = |speed: &[f64], influence: &[f64]| matches!((speed, influence), ([s], [i]) if i.is_finite() && (0.0..=100.0).contains(i) && s * dt * i / 100.0 <= delta);
            dt > 0.0 && delta >= 0.0
                && (ks[0].out_interpolation != 2 || bounded_handle(&ks[0].out_speed, &ks[0].out_influence))
                && (ks[1].in_interpolation != 2 || bounded_handle(&ks[1].in_speed, &ks[1].in_influence))
        }) {
        return Ok(());
    }
    Err(
        "requires finite nonnegative static amounts or a monotone zero-to-positive scalar schedule"
            .into(),
    )
}
fn profile_bytes(layer: &Layer, effect: &DecodedEffect) -> Result<(), String> {
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let parade = super::control_links::unique_run(&roots, "ADBE Effect Parade")
        .map_err(|e| e.to_string())?;
    let effects =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let (name, run) = effects
        .get(effect.index.checked_sub(1).ok_or("invalid effect index")?)
        .ok_or("effect missing")?;
    if *name != MATCH_NAME {
        return Err("effect occurrence mismatch".into());
    }
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let body = properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?;
    let controls = properties::runs(body).map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for (name, run) in controls {
        if name == "ADBE Group End" {
            continue;
        }
        if !seen.insert(name) {
            return Err(format!("duplicate {name}"));
        }
        match name {
            "CC Split 2-0000" | "CC Split 2-0001" | "CC Split 2-0002" | "CC Split 2-0003"
            | "CC Split 2-0004" => {}
            "ADBE Effect Built In Params" => {
                let options = properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?;
                if properties::runs(options)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .any(|(n, _)| *n != "ADBE Group End")
                {
                    return Err("effect compositing options are unsupported".into());
                }
            }
            _ => return Err(format!("unknown Split control {name}")),
        }
    }
    fn payloads<'a>(chunks: &'a [Chunk], found: &mut Vec<&'a [u8]>) {
        for chunk in chunks {
            if chunk.id() == *b"sdat"
                && let Some(bytes) = chunk.data_payload()
            {
                found.push(bytes);
            }
            if let Some(children) = chunk.children() {
                payloads(children, found);
            }
        }
    }
    let mut payload = Vec::new();
    payloads(descriptor, &mut payload);
    let [bytes] = payload.as_slice() else {
        return Err("requires one flat profile payload".into());
    };
    validate_flat_profile(bytes)
}
fn validate_flat_profile(bytes: &[u8]) -> Result<(), String> {
    // Observed Cycore framing: prefix0, 256 little-endian float samples, then
    // interpolation flags0/1/1 and an opaque native allocation word. The latter
    // is deliberately not a source hash/identity admission condition.
    if bytes.len() != 4 + 256 * 4 + 16
        || bytes[..4] != 0_u32.to_le_bytes()
        || bytes[1028..1040]
            != [0_u32, 1, 1]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
    {
        return Err("unknown custom-profile framing or interpolation flags".into());
    }
    if !bytes[4..1028]
        .chunks_exact(4)
        .all(|b| f32::from_le_bytes(b.try_into().expect("four-byte sample")) == 1.0)
    {
        return Err("only finite flat unit profiles are approximated".into());
    }
    Ok(())
}
fn prepare(
    layer: &Layer,
    context: &LayerContext<'_>,
    source: Option<&ProjectItem>,
) -> Result<Option<Profile>, String> {
    let flags = layer.record.flags();
    let (effects, warnings) = native::read_effects(
        &layer.content,
        [
            f64::from(context.comp.width),
            f64::from(context.comp.height),
        ],
    );
    let active: Vec<_> = effects.iter().filter(|e| e.enabled).collect();
    if !flags.enabled || !flags.effects_active || !active.iter().any(|e| e.match_name == MATCH_NAME)
    {
        return Ok(None);
    }
    if active.len() != 1 || active[0].match_name != MATCH_NAME || !warnings.is_empty() {
        return Err(
            "requires one valid enabled Split effect; remaining pixel effects cannot be reordered"
                .into(),
        );
    }
    if !flags.adjustment_layer
        || flags.three_d_layer
        || flags.preserve_transparency
        || layer.record.parent_id() != 0
        || layer.record.track_matte_type() != 0
        || layer.record.blend_mode() != 2
        || context.comp.pixel_aspect.0 != context.comp.pixel_aspect.1
        || context.comp.width == 0
        || context.comp.height == 0
    {
        return Err(
            "requires unparented planar square-pixel Normal Adjustment without matte".into(),
        );
    }
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    for (name, run) in roots {
        if name == "ADBE Mask Parade"
            && !properties::runs(properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
                .is_empty()
        {
            return Err("authored Adjustment masks are unsupported".into());
        }
        if name == "ADBE Time Remapping" {
            let leaf = properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?;
            let remap = properties::read_numeric(leaf).map_err(|e| e.to_string())?;
            if remap.animated
                || remap.expression_present
                || remap.expression_enabled
                || !remap.keyframes.is_empty()
                || remap.values != [0.0]
            {
                return Err("authored Adjustment remap is unsupported".into());
            }
        }
    }
    let size = super::source_dimensions(source, layer);
    if size != [context.comp.width, context.comp.height]
        || !source.and_then(|s| s.solid.as_ref()).is_some_and(|s| {
            s.as_ref()
                .is_ok_and(|s| s.pixel_aspect.0 == s.pixel_aspect.1 && s.pixel_aspect.0 != 0)
        })
    {
        return Err(
            "requires a known composition-sized square-pixel Adjustment solid source".into(),
        );
    }
    let relative_anchor = properties::read_static_source_relative_anchor(&layer.content)
        .map_err(|e| e.to_string())?;
    let center = [f64::from(size[0]) / 2.0, f64::from(size[1]) / 2.0];
    let transform = properties::read_transform(&layer.content).map_err(|e| e.to_string())?;
    if transform
        .iter()
        .find(|p| p.match_name == "ADBE Position")
        .and_then(|p| p.numeric.as_ref().ok())
        .is_some_and(|p| p.dimensions_separated)
    {
        return Err("requires static unseparated identity Adjustment Transform".into());
    }
    for p in transform {
        if p.match_name.starts_with("ADBE Position_") {
            // Native layers retain dormant followers even when Position is not separated.
            continue;
        }
        let n = p.numeric.as_ref().map_err(|e| e.to_string())?;
        if n.animated
            || n.expression_present
            || n.expression_enabled
            || n.dimensions_separated
            || !n.keyframes.is_empty()
        {
            return Err("requires static unseparated identity Adjustment Transform".into());
        }
        let expected: &[f64] = match p.match_name.as_str() {
            "ADBE Anchor Point" if relative_anchor.is_some() => &[0.5, 0.5],
            "ADBE Anchor Point" | "ADBE Position" => &center,
            "ADBE Scale" => &[1.0, 1.0],
            "ADBE Opacity" => &[1.0],
            "ADBE Rotate X" | "ADBE Rotate Y" | "ADBE Rotate Z" => &[0.0],
            "ADBE Orientation" => &[0.0; 3],
            _ => return Err("separated Adjustment Transform followers are unsupported".into()),
        };
        if n.values != expected {
            return Err("requires centered identity Adjustment Transform and full opacity".into());
        }
    }
    let effect = active[0];
    profile_bytes(layer, effect)?;
    let a = scalar(effect, "CC Split 2-0001")?;
    let b = scalar(effect, "CC Split 2-0002")?;
    let point = |p: &NumericProperty| {
        !p.animated
            && !p.dimensions_separated
            && !p.expression_enabled
            && !p.expression_present
            && p.keyframes.is_empty()
            && p.values.len() == 2
            && p.values
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
    };
    if !point(&a)
        || !point(&b)
        || a.values[1] != b.values[1]
        || a.values[0] >= b.values[0]
        || a.values[1] <= 0.0
        || a.values[1] >= f64::from(context.comp.height)
    {
        return Err("requires static horizontal left-to-right segment within source height".into());
    }
    let amounts = [
        scalar(effect, "CC Split 2-0003")?,
        scalar(effect, "CC Split 2-0004")?,
    ];
    for amount in &amounts {
        validate_amount(amount)?;
    }
    if amounts[0] != amounts[1] {
        return Err(
            "requires identical side schedules; native asymmetric side assignment is unproven"
                .into(),
        );
    }
    if amounts.iter().all(|a| !a.animated && a.values == [0.0]) {
        return Ok(None);
    }
    let clock = NumericAnimationClock::parent_identity(layer)?;
    if clock.reversed() {
        return Err("reverse Adjustment clock is unsupported".into());
    }
    let in_point = layer.record.in_point().ok_or("invalid in-point")?;
    let out_point = layer.record.out_point().ok_or("invalid out-point")?;
    let mut start = clock.seconds(in_point).max(0.0);
    let end = clock.seconds(out_point).min(context.comp.duration_secs);
    if amounts.iter().all(|a| a.animated) {
        let first = amounts[0]
            .keyframes
            .first()
            .expect("validated keys")
            .time_secs;
        if amounts[1]
            .keyframes
            .first()
            .expect("validated keys")
            .time_secs
            != first
        {
            return Err("animated side amounts require the same zero support boundary".into());
        }
        start = start.max(clock.seconds(first) + 0.001);
    } else if amounts.iter().any(|a| a.animated) {
        return Err("mixed static/animated side schedules are unsupported".into());
    }
    if !start.is_finite()
        || !end.is_finite()
        || start >= end
        || start < 0.0
        || end > super::MAX_TIME_SECS
    {
        return Err("invalid Split support interval".into());
    }
    Ok(Some(Profile {
        line_y: a.values[1],
        amounts,
        start: (start * 1000.0).ceil() as u64,
        end: (end * 1000.0).ceil() as u64,
    }))
}

fn gate(start: u64, end: u64, wet: bool) -> NumericProperty {
    let value = |active: bool| if active == wet { 100.0 } else { 0.0 };
    let mut points = vec![(0, value(start == 0))];
    if start > 0 {
        points.push((start, value(true)));
    }
    points.push((end, value(false)));
    NumericProperty {
        values: vec![points[0].1],
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        value_kind: NumericValueKind::Continuous,
        keyframes: points
            .into_iter()
            .map(|(time, value)| NumericKeyframe {
                time_secs: time as f64 / 1000.0,
                values: vec![value],
                in_interpolation: 3,
                out_interpolation: 3,
                in_speed: vec![0.0],
                out_speed: vec![0.0],
                in_influence: vec![0.0],
                out_influence: vec![0.0],
                spatial_in: vec![],
                spatial_out: vec![],
            })
            .collect(),
    }
}
fn half_plane(width: u16, height: u16, y: f64, lower: bool) -> ShapePath {
    let extent = f64::from(width.max(height)) * 4.0 + 1.0;
    let (top, bottom) = if lower {
        (y, y + extent)
    } else {
        (y - extent, y)
    };
    let line = |x, y| ShapePathCommand::LineTo {
        x,
        y,
        mirror: None,
        corner_radius: None,
    };
    ShapePath {
        commands: vec![
            ShapePathCommand::MoveTo {
                x: -extent,
                y: top,
                mirror: None,
                corner_radius: None,
            },
            line(extent, top),
            line(extent, bottom),
            line(-extent, bottom),
            ShapePathCommand::Close,
        ],
    }
}
impl Converter<'_> {
    pub(super) fn apply_split2(
        &mut self,
        context: &LayerContext<'_>,
        sources: &[usize],
        layers: &mut Vec<FxLayer>,
    ) -> Result<(), DocumentError> {
        // Native occurrence-to-sibling correspondence is needed for independent
        // reimports. A preceding stack handler can already have consumed it.
        if sources.len() != layers.len() {
            return Ok(());
        }
        let mut candidates = Vec::new();
        for (index, &native_index) in sources.iter().enumerate() {
            let layer = &context.comp.layers[native_index];
            match prepare(
                layer,
                context,
                self.items.get(&layer.record.source_id()).copied(),
            ) {
                Ok(Some(profile)) => candidates.push((index, profile)),
                Ok(None) => {}
                Err(error) => self.warn(
                    Limitation::Properties,
                    Some(context.comp_id),
                    Some(layer.record.id()),
                    format!("CC Split 2 omitted: {error}; original siblings retained"),
                ),
            }
        }
        if candidates.len() > 1 {
            self.warn(Limitation::Properties, Some(context.comp_id), None, "CC Split 2 omitted: multiple stack-dependent Split owners exceed the bounded single-stage profile".into());
            return Ok(());
        }
        let Some((index, profile)) = candidates.pop() else {
            return Ok(());
        };
        let owner = &context.comp.layers[sources[index]];
        let result = self.split_stack(context, sources, layers, index, &profile);
        match result {
            Ok(replacement) => {
                layers.splice(index + 1.., replacement);
                self.warn(Limitation::Properties, Some(context.comp_id), Some(owner.record.id()), "CC Split 2: flat horizontal profile approximated with independent editable half-plane copies and source-authored scalar Position keys; displacement uses source height times amount/100. Native finite-segment endpoints, custom-profile flags/allocation word, raster kernel and exact Cycore displacement remain unverified. Above siblings and after-Split Adjustment styles are retained; copies are independently editable".into());
            }
            Err(error) => self.warn(
                Limitation::Properties,
                Some(context.comp_id),
                Some(owner.record.id()),
                format!("CC Split 2 omitted: {error}; original siblings retained"),
            ),
        }
        Ok(())
    }
    fn split_stack(
        &mut self,
        context: &LayerContext<'_>,
        sources: &[usize],
        layers: &[FxLayer],
        index: usize,
        profile: &Profile,
    ) -> Result<Vec<FxLayer>, String> {
        let FxLayer::Adjustment(adjustment) = &layers[index] else {
            return Err("Split owner is not a direct Adjustment".into());
        };
        if adjustment.is_hidden
            || adjustment.transform.opacity.value() != 100.0
            || !adjustment.masks.is_empty()
            || adjustment.track_matte.is_some()
            || index + 1 == layers.len()
            || self
                .animations
                .iter()
                .any(|e| e.target.layer_id() == Some(adjustment.id))
        {
            return Err("masked, animated-opacity or empty Adjustment scope".into());
        }
        let below = &layers[index + 1..];
        let mut ids = HashSet::new();
        for layer in below {
            collect_layers(layer, &mut ids, context.depth + 3)?;
        }
        let mut above_ids = HashSet::new();
        for layer in &layers[..=index] {
            collect_layers(layer, &mut above_ids, context.depth + 1)?;
        }
        if !below
            .iter()
            .all(|l| closed_visual(l, &ids, context.parent, true))
            || !layers[..=index]
                .iter()
                .all(|l| closed_visual(l, &above_ids, context.parent, false))
        {
            return Err(
                "stack has physical media or crossing matte/effect/layer references".into(),
            );
        }
        let native_ids: HashSet<_> = sources[index + 1..]
            .iter()
            .map(|&i| context.comp.layers[i].record.id())
            .collect();
        if context.comp.layers.iter().any(|l| {
            l.record.parent_id() != 0
                && native_ids.contains(&l.record.id()) != native_ids.contains(&l.record.parent_id())
        }) {
            return Err("native parenting crosses the Split boundary".into());
        }
        let first = ids
            .iter()
            .copied()
            .map(u64::from)
            .min()
            .ok_or("empty stack")?;
        if !animations_closed(&self.animations, &(first..self.next_id)) {
            return Err("stack contains dependency/script/seed references".into());
        }
        let id_checkpoint = self.next_id;
        let budget_checkpoint = self.animation_budget.checkpoint();
        let animation_start = self.animations.len();
        let diagnostics_start = self.diagnostics.len();
        let shape_checkpoint = self.shape_budget.checkpoint();
        let assets_start = self.assets.len();
        let visited = self.visited_compositions.clone();
        let stack = self.stack.clone();
        let overrides = self.overrides.clone();
        #[cfg(test)]
        let inline_bytes = self.committed_inline_remap_bytes;
        let denied = self.animation_budget.denials();
        let built = (|| {
            let range =
                TimeRangeProperty::new(Time::ZERO, Duration::from_secs(context.comp.duration_secs));
            let mut replacement = Vec::new();
            let mut original = group(
                self.allocate_id().map_err(|e| e.to_string())?,
                "CC Split unchanged support".into(),
                Some(context.parent),
                range,
            );
            self.split_gate(&mut original, profile, false)?;
            let mut original_layers = below.to_vec();
            for layer in &mut original_layers {
                reparent(layer, original.id)?;
            }
            original.layers = super::stored_layers(original_layers).map_err(|e| e.to_string())?;
            replacement.push(FxLayer::Group(original));
            for side in 0..2 {
                let mut branch = group(
                    self.allocate_id().map_err(|e| e.to_string())?,
                    "CC Split editable half".into(),
                    Some(context.parent),
                    range,
                );
                let lower = side == 1;
                let factor =
                    f64::from(context.comp.height) / 100.0 * if lower { 1.0 } else { -1.0 };
                let amount = &profile.amounts[side];
                let initial = amount
                    .keyframes
                    .first()
                    .map_or(amount.values.as_slice(), |k| k.values.as_slice());
                branch.transform.position = Position::TwoD([0.0, initial[0] * factor]);
                self.split_gate(&mut branch, profile, true)?;
                let clock =
                    NumericAnimationClock::parent_identity(&context.comp.layers[sources[index]])?;
                let target = NumericAnimationTarget::float(
                    PropertyTarget::layer(branch.id, fx_schema::PropType::PositionY),
                    0,
                    factor,
                );
                let (entries, warnings) = animation::numeric_entries(
                    "CC Split amount",
                    amount,
                    &[target],
                    clock,
                    &mut self.animation_budget,
                );
                if !warnings.is_empty() || (amount.animated && entries.is_empty()) {
                    return Err(format!(
                        "amount animation was not admitted atomically: {}",
                        warnings.join("; ")
                    ));
                }
                self.animations.extend(entries);
                let sample = LayerContext {
                    parent: branch.id,
                    depth: context.depth + 2,
                    ..*context
                };
                let copy_start = self.next_id;
                let mut copies = Vec::new();
                let mut guides = Vec::new();
                for &source_index in &sources[index + 1..] {
                    let source = &context.comp.layers[source_index];
                    if source.record.flags().adjustment_layer {
                        let (adjustment, mut extra) = self
                            .adjustment_layer(&sample, source)
                            .map_err(|e| e.to_string())?;
                        copies.push(FxLayer::Adjustment(adjustment));
                        guides.append(&mut extra);
                    } else {
                        copies.push(FxLayer::Group(
                            self.layer(&sample, source, LayerPurpose::Ordinary)
                                .map_err(|e| e.to_string())?,
                        ));
                    }
                }
                self.apply_mattes(&sample, &sources[index + 1..], &mut copies)
                    .map_err(|e| e.to_string())?;
                self.apply_set_mattes(&sample, &sources[index + 1..], &mut copies)
                    .map_err(|e| e.to_string())?;
                self.apply_preserve_transparency(&sample, &sources[index + 1..], &mut copies)
                    .map_err(|e| e.to_string())?;
                let mut fresh = HashSet::new();
                for layer in &copies {
                    collect_layers(layer, &mut fresh, context.depth + 3)?;
                }
                if !guides.is_empty()
                    || copies.len() != below.len()
                    || !copies
                        .iter()
                        .all(|l| closed_visual(l, &fresh, branch.id, true))
                    || !animations_closed(&self.animations, &(copy_start..self.next_id))
                {
                    return Err("regenerated stack is not independent closed visual content".into());
                }
                branch.layers = super::stored_layers(copies).map_err(|e| e.to_string())?;
                let raw =
                    super::reserve_ids(&mut self.next_id, 2).ok_or("mask identity overflow")?;
                let guide_id = LayerId::new(raw + 1);
                let guide = ShapeLayer {
                    id: guide_id, parent: Some(branch.id), name: "CC Split half-plane guide".into(),
                    description: "Editable hard horizontal half-plane; finite segment endpoint behavior approximate".into(),
                    is_hidden: false, blend_mode: Default::default(), track_matte: None, masks: vec![], active_range: range,
                    effects: vec![], motion_blur: false, transform: Transform { position: Position::TwoD([0.0, 0.0]), opacity: fx_schema::PercentageProperty::new(100.0).expect("100 is valid"), ..branch.transform },
                    shape: ShapeContent { path: half_plane(context.comp.width, context.comp.height, profile.line_y, lower), fills: vec![], strokes: vec![], round_corners: None, offset_paths: None, trim: None, poly_star: None, ellipse: None },
                };
                if !self.shape_budget.reserve(&guide) {
                    return Err(
                        "generated Split guide exceeds shape serialization allowance".into(),
                    );
                }
                branch.layers.push(
                    fx_schema::Layer::from_data(&FxLayer::Shape(guide))
                        .map_err(|e| e.to_string())?,
                );
                let mask = PathMask {
                    id: FxItemId::new(raw),
                    mode: MaskMode::Add,
                    inverted: false,
                    layer: Some(guide_id),
                    legacy_path: None,
                    feather: [0.0, 0.0],
                    expansion: 0.0,
                    opacity: NonNegativeProperty::new(1.0).expect("1 is nonnegative"),
                };
                if !self.shape_budget.reserve(&mask) {
                    return Err("generated Split mask exceeds shape serialization allowance".into());
                }
                branch.masks.push(mask);
                replacement.push(FxLayer::Group(branch));
            }
            if self.animation_budget.denials() != denied
                || self.diagnostics[diagnostics_start..]
                    .iter()
                    .any(|n| matches!(n.limitation, Limitation::Cycle | Limitation::ExpansionLimit))
            {
                return Err("Split copies exceeded import allowance".into());
            }
            Ok(replacement)
        })();
        if built.is_err() {
            self.next_id = id_checkpoint;
            self.animation_budget.rollback(budget_checkpoint);
            self.animations.truncate(animation_start);
            self.diagnostics.truncate(diagnostics_start);
            self.shape_budget.restore(shape_checkpoint);
            self.assets.truncate(assets_start);
            self.visited_compositions = visited;
            self.stack = stack;
            self.overrides = overrides;
            #[cfg(test)]
            {
                self.committed_inline_remap_bytes = inline_bytes;
            }
        }
        built
    }
    fn split_gate(
        &mut self,
        group: &mut GroupLayer,
        profile: &Profile,
        wet: bool,
    ) -> Result<(), String> {
        let schedule = gate(profile.start, profile.end, wet);
        group.transform.opacity =
            fx_schema::PercentageProperty::new(schedule.values[0]).ok_or("invalid gate opacity")?;
        let target = NumericAnimationTarget::float(
            PropertyTarget::layer(group.id, fx_schema::PropType::Opacity),
            0,
            1.0,
        );
        let (entries, warnings) = animation::numeric_entries(
            "CC Split support gate",
            &schedule,
            &[target],
            NumericAnimationClock::source_local(),
            &mut self.animation_budget,
        );
        if !warnings.is_empty() || entries.is_empty() {
            return Err(format!(
                "support gate not admitted: {}",
                warnings.join("; ")
            ));
        }
        self.animations.extend(entries);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
