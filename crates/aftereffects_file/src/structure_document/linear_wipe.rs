//! Hard-edge Linear Wipes on a finite editable Solid source plane.
//! Angled projected-canvas normalization and raster edges are approximations.
use std::collections::HashSet;

use super::{
    MAX_GROUP_DEPTH,
    animation::{self, NumericAnimationClock, NumericAnimationTarget},
    animation_budget::AnimationBudget,
    control_links::{expression, finished, quoted, token},
    group, reserve_ids,
    shapes::OutputBudget,
    stored_layers,
};
use crate::{
    properties::{self, NumericProperty, NumericValueKind},
    rifx::Chunk,
    structure::{Layer, ProjectItem},
};
use fx_schema::{
    FxItemId, GroupLayer, LayerData, LayerId, NonNegativeProperty, Position, PropType,
    PropertyTarget, ShapeContent, ShapePath, ShapePathCommand, Transform,
    animator::AnimationGraphEntry,
    layer::{MaskMode, PathMask, ShapeLayer},
};
const WIPE: &str = "ADBE Linear Wipe";
const GEOMETRY: &str = "ADBE Geometry2";
const WIPE_DEFAULTS: &[(u32, u32, u32)] = &[(0, 0, 0), (1, 2, 0), (2, 3, 90 << 16), (3, 2, 0)];
const GEOMETRY_DEFAULTS: &[(u32, u32, u32)] = &[
    (0, 0, 0),
    (1, 6, 50 << 16),
    (2, 6, 50 << 16),
    (11, 4, 1),
    (3, 2, 100 << 16),
    (4, 2, 100 << 16),
    (5, 2, 0),
    (6, 3, 0),
    (7, 3, 0),
    (8, 2, 100 << 16),
    (9, 4, 1),
    (10, 2, 0),
    (12, 7, 1),
];
struct Native<'a> {
    name: &'a str,
    display: &'a str,
    controls: Vec<(&'a str, &'a [Chunk])>,
}
struct Wipe {
    completion: NumericProperty,
    angle: f64,
}
struct Profile {
    wipes: Vec<Wipe>,
    anchor: Option<NumericProperty>,
}
pub(super) struct Context<'a> {
    pub source: Option<&'a ProjectItem>,
    pub size: [u16; 2],
    pub depth: usize,
    pub planar: bool,
}
pub(super) struct State<'a> {
    pub next: &'a mut u64,
    pub animations: &'a mut AnimationBudget,
    pub shapes: &'a mut OutputBudget,
}

fn native<'a>(name: &'a str, run: &'a [Chunk]) -> Result<Option<Native<'a>>, String> {
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let mut warnings = Vec::new();
    let enabled = properties::group_enabled_or_warn(descriptor, name, &mut warnings);
    if !warnings.is_empty() {
        return Err(warnings.join("; "));
    }
    if !enabled {
        return Ok(None);
    }
    let defaults = match name {
        WIPE => WIPE_DEFAULTS,
        GEOMETRY => GEOMETRY_DEFAULTS,
        _ => return Err("only Wipes followed by an optional Transform are admitted".into()),
    };
    let table = properties::unique_list(descriptor, *b"parT").map_err(|e| e.to_string())?;
    if !table.is_empty() {
        let rows = properties::runs(table).map_err(|e| e.to_string())?;
        if rows.len() != defaults.len() + 1 {
            return Err("incomplete plugin declarations".into());
        }
        let mut seen = HashSet::new();
        for (parameter, run) in rows {
            if !seen.insert(parameter) {
                return Err("duplicate plugin declaration".into());
            }
            let (kind, value) = if parameter == "ADBE Effect Built In Params" {
                (9, 0)
            } else {
                let (_, kind, value) = defaults
                    .iter()
                    .find(|(n, _, _)| parameter == format!("{name}-{n:04}"))
                    .ok_or("unknown plugin declaration")?;
                (*kind, *value)
            };
            let bytes = properties::data(run, *b"pard").map_err(|e| e.to_string())?;
            if bytes.len() != 148
                || bytes[12..16] != kind.to_be_bytes()
                || if kind == 7 {
                    bytes[60..62] != 2_u16.to_be_bytes()
                        || bytes[62..64] != (value as u16).to_be_bytes()
                } else {
                    bytes[56..60] != value.to_be_bytes()
                }
            {
                return Err("native plugin ABI/default conflict".into());
            }
            if kind == 6
                && (bytes[60..64] != value.to_be_bytes()
                    || bytes[64..68] != [0; 4]
                    || bytes[68..72] != value.to_be_bytes()
                    || bytes[72..76] != value.to_be_bytes())
            {
                return Err("unknown normalized Point default".into());
            }
        }
    }
    let body = properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?;
    let controls = properties::runs(body).map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for (parameter, run) in &controls {
        if !seen.insert(*parameter) {
            return Err("duplicate plugin control".into());
        }
        if *parameter == "ADBE Group End" {
            continue;
        }
        if *parameter == "ADBE Effect Built In Params" {
            if properties::runs(properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
                .iter()
                .any(|(n, _)| *n != "ADBE Group End")
            {
                return Err("effect compositing options are unsupported".into());
            }
        } else if !defaults
            .iter()
            .any(|(n, _, _)| *parameter == format!("{name}-{n:04}"))
        {
            return Err("unknown plugin control".into());
        }
    }
    let display = super::control_links::effect_name(descriptor, body)
        .ok_or("malformed effect display name")?;
    Ok(Some(Native {
        name,
        display,
        controls,
    }))
}
fn constant(values: Vec<f64>) -> NumericProperty {
    NumericProperty {
        values,
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: vec![],
        value_kind: NumericValueKind::Continuous,
    }
}
fn read(source: &Native<'_>, number: u32) -> Result<NumericProperty, String> {
    let name = format!("{}-{number:04}", source.name);
    let Some((_, run)) = source.controls.iter().find(|(n, _)| *n == name) else {
        let (_, kind, value) = match source.name {
            WIPE => WIPE_DEFAULTS,
            _ => GEOMETRY_DEFAULTS,
        }
        .iter()
        .find(|(n, _, _)| *n == number)
        .ok_or("missing native default")?;
        return Ok(constant(if *kind == 6 {
            vec![0.5, 0.5]
        } else {
            vec![if matches!(*kind, 2 | 3) {
                f64::from(*value) / 65536.
            } else {
                f64::from(*value)
            }]
        }));
    };
    let leaf = properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?;
    if source.name == GEOMETRY && matches!(number, 1 | 2) {
        let metadata = properties::data(leaf, *b"tdb4").map_err(|e| e.to_string())?;
        if metadata.len() != 124 || metadata[59] != 4 {
            return Err("requires an explicit normalized PF_Point".into());
        }
        properties::read_effect_point(leaf).map_err(|e| e.to_string())
    } else {
        properties::read_numeric(leaf).map_err(|e| e.to_string())
    }
}
fn alias(mut text: &str, number: u32) -> Option<(&str, bool)> {
    if text.len() > 16 * 1024 {
        return None;
    }
    token(&mut text, "effect")?;
    token(&mut text, "(")?;
    let effect = quoted(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, "(")?;
    let parameter = quoted(&mut text)?;
    token(&mut text, ")")?;
    if parameter != format!("{WIPE}-{number:04}") {
        return None;
    }
    let opposed = number == 2 && token(&mut text, "+").is_some();
    if opposed {
        token(&mut text, "180")?;
    }
    finished(text).then_some((effect, opposed))
}
fn resolve(sources: &[Native<'_>], index: usize, number: u32) -> Result<NumericProperty, String> {
    let source = &sources[index];
    let mut value = read(source, number)?;
    if !value.expression_enabled {
        return Ok(value);
    }
    if source.name != WIPE || value.animated || !value.keyframes.is_empty() {
        return Err("unsupported control expression".into());
    }
    let name = format!("{WIPE}-{number:04}");
    let (_, run) = source
        .controls
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or("missing alias control")?;
    let text = expression(properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let (referenced, opposed) =
        alias(text, number).ok_or("expression is outside direct same-property alias grammar")?;
    let mut matches = sources
        .iter()
        .enumerate()
        .filter(|(_, s)| s.display == referenced);
    let (from, other) = matches.next().ok_or("alias source missing")?;
    if matches.next().is_some() || from >= index || other.name != WIPE {
        return Err("ambiguous, forward or cyclic alias".into());
    }
    value = resolve(sources, from, number)?;
    if opposed {
        for v in value
            .values
            .iter_mut()
            .chain(value.keyframes.iter_mut().flat_map(|k| k.values.iter_mut()))
        {
            *v += 180.;
        }
    }
    value.expression_enabled = false;
    value.expression_present = false;
    Ok(value)
}
fn curve(value: &NumericProperty, dimensions: usize, completion: bool) -> Result<(), String> {
    let valid = |values: &[f64]| {
        values.len() == dimensions
            && values
                .iter()
                .all(|v| v.is_finite() && (!completion || (0. ..=100.).contains(v)))
    };
    if value.expression_enabled
        || value.expression_present
        || value.dimensions_separated
        || (!value.animated && (!value.keyframes.is_empty() || !valid(&value.values)))
        || (value.animated && (value.keyframes.len() < 2 || value.keyframes.len() > 64))
        || value.keyframes.iter().any(|k| {
            !valid(&k.values)
                || !k.time_secs.is_finite()
                || !matches!(k.in_interpolation, 1..=3)
                || !matches!(k.out_interpolation, 1..=3)
                || k.in_speed
                    .iter()
                    .chain(&k.out_speed)
                    .any(|v| !v.is_finite())
                || k.in_influence
                    .iter()
                    .chain(&k.out_influence)
                    .any(|v| !v.is_finite() || !(0. ..=100.).contains(v))
                || k.spatial_in.iter().chain(&k.spatial_out).any(|v| *v != 0.)
        })
        || value
            .keyframes
            .windows(2)
            .any(|p| p[0].time_secs >= p[1].time_secs)
    {
        return Err(
            "requires finite static or bounded native keys without curved spatial tangents".into(),
        );
    }
    if dimensions == 2
        && value.animated
        && (0..2)
            .filter(|&axis| {
                value
                    .keyframes
                    .iter()
                    .any(|k| k.values[axis] != value.keyframes[0].values[axis])
            })
            .count()
            > 1
    {
        return Err("only axis-aligned normalized Anchor curves are admitted".into());
    }
    if completion {
        for index in 1..value.keyframes.len() {
            let mut warnings = Vec::new();
            if let fx_schema::PropertyKeyframeEasing::CubicBezier { y1, y2, .. } =
                animation::easing_for_key(
                    &value.keyframes,
                    index,
                    0,
                    1.,
                    &mut warnings,
                    "Completion",
                )
                && (!(0. ..=1.).contains(&y1) || !(0. ..=1.).contains(&y2))
            {
                return Err("completion easing can leave its bounded source interval".into());
            }
            if !warnings.is_empty() {
                return Err(warnings.join("; "));
            }
        }
    }
    Ok(())
}
fn scalar(value: &NumericProperty) -> Result<f64, String> {
    curve(value, 1, false)?;
    if value.animated {
        return Err("angle, feather and Transform scalar controls must be static".into());
    }
    Ok(value.values[0])
}
fn profile(layer: &Layer) -> Result<Option<Profile>, String> {
    if !layer.record.flags().effects_active {
        return Ok(None);
    }
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let mut parades = roots.iter().filter(|(n, _)| *n == "ADBE Effect Parade");
    let Some((_, parade)) = parades.next() else {
        return Ok(None);
    };
    if parades.next().is_some() {
        return Err("duplicate Effect Parade".into());
    }
    let runs =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if !runs.iter().any(|(n, _)| *n == WIPE) {
        return Ok(None);
    }
    let mut sources = Vec::new();
    for (name, run) in runs {
        if let Some(source) = native(name, run)? {
            sources.push(source);
        }
    }
    if !sources.iter().any(|source| source.name == WIPE) {
        return Ok(None);
    }
    let geometry = sources.last().is_some_and(|s| s.name == GEOMETRY);
    let count = sources.len() - usize::from(geometry);
    if !(1..=2).contains(&count) || sources[..count].iter().any(|s| s.name != WIPE) {
        return Err("requires one or two Wipes followed by at most one Transform".into());
    }
    let mut wipes = Vec::with_capacity(count);
    for index in 0..count {
        if scalar(&resolve(&sources, index, 0)?)? != 0.
            || scalar(&resolve(&sources, index, 3)?)? != 0.
        {
            return Err("requires zero feather and default dummy controls".into());
        }
        let completion = resolve(&sources, index, 1)?;
        curve(&completion, 1, true)?;
        wipes.push(Wipe {
            completion,
            angle: scalar(&resolve(&sources, index, 2)?)?,
        });
    }
    let anchor = if geometry {
        let index = count;
        for (n, expected) in [
            (0, 0.),
            (3, 100.),
            (4, 100.),
            (5, 0.),
            (6, 0.),
            (7, 0.),
            (8, 100.),
            (9, 1.),
            (10, 0.),
            (11, 1.),
            (12, 1.),
        ] {
            if scalar(&resolve(&sources, index, n)?)? != expected {
                return Err("post-wipe Transform requires default scalar controls".into());
            }
        }
        let position = resolve(&sources, index, 2)?;
        curve(&position, 2, false)?;
        if position.animated || position.values != [0.5, 0.5] {
            return Err("post-wipe Transform requires default centered Position".into());
        }
        let anchor = resolve(&sources, index, 1)?;
        curve(&anchor, 2, false)?;
        Some(anchor)
    } else {
        None
    };
    Ok(Some(Profile { wipes, anchor }))
}
fn initial(value: &NumericProperty) -> &[f64] {
    value
        .keyframes
        .first()
        .map_or(value.values.as_slice(), |k| k.values.as_slice())
}
fn guide(size: [u16; 2], angle: f64, completion: f64) -> (Transform, f64) {
    let angle = angle.rem_euclid(360.);
    let d = match angle {
        0. => [0., 1.],
        90. => [1., 0.],
        180. => [0., -1.],
        270. => [-1., 0.],
        _ => [angle.to_radians().sin(), angle.to_radians().cos()],
    };
    let [width, height] = size.map(f64::from);
    let span = d[0].abs() * width + d[1].abs() * height;
    (
        Transform {
            anchor_point: [-completion * span / 100., 0.],
            position: Position::xy(
                width / 2. - d[0] * span / 2.,
                height / 2. - d[1] * span / 2.,
            ),
            rotation: 90. - angle,
            scale: [100., 100.],
            skew: 0.,
            skew_axis: 0.,
            rotation_x: 0.,
            rotation_y: 0.,
            orientation: [0.; 3],
            opacity: fx_schema::PercentageProperty::new(100.).expect("100 is valid opacity"),
        },
        span,
    )
}
fn path(size: [u16; 2]) -> ShapePath {
    let extent = 2. * f64::from(size[0]).hypot(f64::from(size[1])) + 1.;
    let line = |x, y| ShapePathCommand::LineTo {
        x,
        y,
        mirror: None,
        corner_radius: None,
    };
    ShapePath {
        commands: vec![
            ShapePathCommand::MoveTo {
                x: 0.,
                y: -extent,
                mirror: None,
                corner_radius: None,
            },
            line(extent, -extent),
            line(extent, extent),
            line(0., extent),
            ShapePathCommand::Close,
        ],
    }
}
pub(super) fn apply(
    layer: &Layer,
    owner: &mut GroupLayer,
    context: Context<'_>,
    state: State<'_>,
) -> Result<Option<Vec<AnimationGraphEntry>>, String> {
    let Some(profile) = profile(layer)? else {
        return Ok(None);
    };
    let flags = layer.record.flags();
    let Some(Ok(solid)) = context.source.and_then(|s| s.solid.as_ref()) else {
        return Err("requires a decoded raster Solid source".into());
    };
    if !context.planar
        || context.size.contains(&0)
        || context.size != [solid.width, solid.height]
        || solid.pixel_aspect.0 == 0
        || solid.pixel_aspect.0 != solid.pixel_aspect.1
        || layer.record.layer_type() != 0
        || flags.null_layer
        || flags.three_d_layer
        || flags.collapse_transformation
        || flags.adjustment_layer
        || flags.preserve_transparency
        || flags.motion_blur
        || layer.record.track_matte_type() != 0
        || !owner.masks.is_empty()
        || owner.track_matte.is_some()
        || owner.layers.len() != 1
        || owner.playback != super::identity_playback(owner.playback.input_range())
    {
        return Err(
            "requires an isolated planar square-pixel Solid without masks, matte or collapse"
                .into(),
        );
    }
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    if roots.iter().any(|(n, _)| *n == "ADBE Mask Parade") {
        return Err("native masks cannot commute across the Wipes".into());
    }
    let additional = 1 + usize::from(profile.anchor.is_some());
    let mut pending = vec![(&*owner, context.depth)];
    while let Some((group, depth)) = pending.pop() {
        if depth + additional >= MAX_GROUP_DEPTH {
            return Err("Wipe helper depth exceeds allowance".into());
        }
        for child in &group.layers {
            if let LayerData::Group(group) = child.data() {
                pending.push((group, depth + 1));
            }
        }
    }
    let clock = NumericAnimationClock::parent_identity(layer)?;
    let checkpoint = state.animations.checkpoint();
    let built = (|| {
        let mut cursor = *state.next;
        let first = reserve_ids(
            &mut cursor,
            1 + 2 * profile.wipes.len() as u64 + u64::from(profile.anchor.is_some()),
        )
        .ok_or("Wipe helper identity allocation exhausted")?;
        let range = owner.playback.input_range();
        let mut masked = group(
            LayerId::new(first),
            "Linear Wipe mask stage".into(),
            Some(owner.id),
            range,
        );
        let mut original = owner.layers[0].data().clone();
        let LayerData::Group(content) = &mut original else {
            return Err("requires editable source content Group".into());
        };
        content.parent = Some(masked.id);
        masked.layers = stored_layers(vec![original]).map_err(|e| e.to_string())?;
        let mut entries = Vec::new();
        for (index, wipe) in profile.wipes.iter().enumerate() {
            let guide_id = LayerId::new(first + 1 + 2 * index as u64);
            let (transform, span) = guide(context.size, wipe.angle, initial(&wipe.completion)[0]);
            let target = NumericAnimationTarget::float(
                PropertyTarget::layer(guide_id, PropType::AnchorPointX),
                0,
                -span / 100.,
            );
            let (tracks, warnings) = animation::numeric_entries(
                "Linear Wipe Completion",
                &wipe.completion,
                &[target],
                clock,
                state.animations,
            );
            if !warnings.is_empty() || (wipe.completion.animated && tracks.is_empty()) {
                return Err(format!(
                    "completion animation not admitted: {}",
                    warnings.join("; ")
                ));
            }
            entries.extend(tracks);
            masked.layers.push(fx_schema::Layer::from_data(&LayerData::Shape(ShapeLayer{id:guide_id,parent:Some(masked.id),name:"Linear Wipe half-plane guide".into(),description:"Editable projected source-canvas half-plane; native angled normalization and antialiasing remain approximate".into(),is_hidden:false,blend_mode:Default::default(),track_matte:None,masks:vec![],active_range:range,effects:vec![],motion_blur:false,transform,shape:ShapeContent{path:path(context.size),fills:vec![],strokes:vec![],round_corners:None,offset_paths:None,trim:None,poly_star:None,ellipse:None}})).map_err(|e|e.to_string())?);
            masked.masks.push(PathMask {
                id: FxItemId::new(first + 2 + 2 * index as u64),
                mode: if index == 0 {
                    MaskMode::Add
                } else {
                    MaskMode::Intersect
                },
                inverted: false,
                layer: Some(guide_id),
                legacy_path: None,
                feather: [0., 0.],
                expansion: 0.,
                opacity: NonNegativeProperty::new(1.).expect("1 is nonnegative"),
            });
        }
        let mut output = masked;
        if let Some(anchor) = &profile.anchor {
            let id = LayerId::new(first + 1 + 2 * profile.wipes.len() as u64);
            let mut geometry = group(
                id,
                "Post-wipe Anchor Transform".into(),
                Some(owner.id),
                range,
            );
            let size = context.size.map(f64::from);
            let initial = initial(anchor);
            if anchor
                .values
                .iter()
                .chain(anchor.keyframes.iter().flat_map(|k| k.values.iter()))
                .any(|v| v.abs() * size[0].max(size[1]) > 1_000_000.)
            {
                return Err("Anchor exceeds the bounded geometry plane".into());
            }
            geometry.transform.anchor_point = [initial[0] * size[0], initial[1] * size[1]];
            geometry.transform.position = Position::xy(size[0] / 2., size[1] / 2.);
            let targets = [
                NumericAnimationTarget::float(
                    PropertyTarget::layer(id, PropType::AnchorPointX),
                    0,
                    size[0],
                ),
                NumericAnimationTarget::float(
                    PropertyTarget::layer(id, PropType::AnchorPointY),
                    1,
                    size[1],
                ),
            ];
            let (tracks, warnings) = animation::numeric_entries(
                "Post-wipe Geometry2 Anchor",
                anchor,
                &targets,
                clock,
                state.animations,
            );
            if !warnings.is_empty() || (anchor.animated && tracks.is_empty()) {
                return Err(format!(
                    "Anchor animation not admitted: {}",
                    warnings.join("; ")
                ));
            }
            entries.extend(tracks);
            output.parent = Some(id);
            geometry.layers =
                stored_layers(vec![LayerData::Group(output)]).map_err(|e| e.to_string())?;
            output = geometry;
        }
        let mut candidate = owner.clone();
        candidate.layers =
            stored_layers(vec![LayerData::Group(output)]).map_err(|e| e.to_string())?;
        fx_schema::Layer::from_data(&LayerData::Group(candidate.clone()))
            .map_err(|e| e.to_string())?;
        let shape_checkpoint = state.shapes.checkpoint();
        if !state.shapes.reserve(&candidate) {
            state.shapes.restore(shape_checkpoint);
            return Err("Wipe helper output serialization allowance exceeded".into());
        }
        *owner = candidate;
        *state.next = cursor;
        Ok(entries)
    })();
    if built.is_err() {
        state.animations.rollback(checkpoint);
    }
    built.map(Some)
}
#[cfg(test)]
mod tests;
