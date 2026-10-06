//! Hard-edge Linear Wipes on a finite editable source or composition-space Shape plane.
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
    expression_samples::{EvaluatedProperty, ExpressionSamples, PropertyIdentity},
    properties::{self, NumericProperty, NumericValueKind},
    rifx::Chunk,
    structure::{ItemKind, Layer, ProjectItem},
};
use fx_schema::{
    FxItemId, GroupLayer, LayerData, LayerId, NonNegativeProperty, Position, PropType,
    PropertyTarget, ShapeContent, ShapePath, ShapePathCommand, Transform,
    animator::{AnimationGraphEntry, PropertyKeyframeEasing},
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
    occurrence: u32,
    controls: Vec<(&'a str, &'a [Chunk])>,
}
enum Completion<'a> {
    Native(NumericProperty),
    Evaluated(&'a EvaluatedProperty),
}
struct Wipe<'a> {
    completion: Completion<'a>,
    angle: f64,
}
struct Profile<'a> {
    wipes: Vec<Wipe<'a>>,
    anchor: Option<NumericProperty>,
}
pub(super) struct AncestorTransform<'a> {
    pub layer: &'a Layer,
    pub transform: Transform,
}
pub(super) struct Context<'a> {
    pub source: Option<&'a ProjectItem>,
    pub size: [u16; 2],
    pub depth: usize,
    pub planar: bool,
    pub composition_id: u32,
    pub composition_offset: Option<[f64; 2]>,
    pub ancestors: &'a [AncestorTransform<'a>],
    pub evaluations: &'a ExpressionSamples,
}
pub(super) struct State<'a> {
    pub next: &'a mut u64,
    pub animations: &'a mut AnimationBudget,
    pub shapes: &'a mut OutputBudget,
}
pub(super) struct Lowered {
    pub entries: Vec<AnimationGraphEntry>,
    pub notes: Vec<String>,
    pub consumed_trailing_transform: bool,
}

fn native<'a>(
    name: &'a str,
    occurrence: u32,
    run: &'a [Chunk],
) -> Result<Option<Native<'a>>, String> {
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
        _ => return Err("only Wipes and one adjacent Transform are admitted".into()),
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
        occurrence,
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
        || (value.animated && value.keyframes.len() < 2)
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
            animation::easing_for_key(&value.keyframes, index, 0, 1., &mut warnings, "Completion");
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
fn evaluated_completion<'a>(
    source: &Native<'_>,
    layer: &Layer,
    composition_id: u32,
    evaluations: &'a ExpressionSamples,
) -> Option<&'a EvaluatedProperty> {
    evaluations.lookup(
        composition_id,
        layer.record.id(),
        &PropertyIdentity::Effect {
            index: source.occurrence,
            match_name: format!("{WIPE}-0001"),
        },
    )
}

fn validate_evaluated_completion(samples: &EvaluatedProperty) -> Result<(), String> {
    if samples.values().is_empty()
        || samples.values().iter().any(|values| {
            !matches!(values.as_slice(), [value] if value.is_finite() && (0. ..=100.).contains(value))
        })
    {
        return Err("evaluated Completion requires finite scalar samples in 0..=100".into());
    }
    Ok(())
}

fn profile<'a>(
    layer: &Layer,
    composition_id: u32,
    evaluations: &'a ExpressionSamples,
) -> Result<Option<Profile<'a>>, String> {
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
    for (index, (name, run)) in runs.into_iter().enumerate() {
        let occurrence = u32::try_from(index + 1).map_err(|_| "effect occurrence overflow")?;
        if let Some(source) = native(name, occurrence, run)? {
            sources.push(source);
        }
    }
    if !sources.iter().any(|source| source.name == WIPE) {
        return Ok(None);
    }
    let leading_geometry = sources
        .first()
        .is_some_and(|source| source.name == GEOMETRY);
    let trailing_geometry =
        sources.last().is_some_and(|source| source.name == GEOMETRY) && !leading_geometry;
    let start = usize::from(leading_geometry);
    let end = sources.len() - usize::from(trailing_geometry);
    let count = end.saturating_sub(start);
    if !(1..=2).contains(&count) || sources[start..end].iter().any(|source| source.name != WIPE) {
        return Err("requires one or two Wipes with at most one adjacent Transform".into());
    }
    let mut wipes = Vec::with_capacity(count);
    for index in start..end {
        if scalar(&resolve(&sources, index, 0)?)? != 0.
            || scalar(&resolve(&sources, index, 3)?)? != 0.
        {
            return Err("requires zero feather and default dummy controls".into());
        }
        let source = &sources[index];
        let completion = read(source, 1)?;
        let completion = if completion.expression_enabled {
            match resolve(&sources, index, 1) {
                Ok(completion) => {
                    curve(&completion, 1, true)?;
                    Completion::Native(completion)
                }
                Err(alias_error) => {
                    if let Some(samples) =
                        evaluated_completion(source, layer, composition_id, evaluations)
                    {
                        validate_evaluated_completion(samples)?;
                        Completion::Evaluated(samples)
                    } else {
                        return Err(alias_error);
                    }
                }
            }
        } else {
            let completion = resolve(&sources, index, 1)?;
            curve(&completion, 1, true)?;
            Completion::Native(completion)
        };
        wipes.push(Wipe {
            completion,
            angle: scalar(&resolve(&sources, index, 2)?)?,
        });
    }
    let anchor = if trailing_geometry {
        let index = end;
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
fn initial_completion(completion: &Completion<'_>) -> f64 {
    match completion {
        Completion::Native(value) => initial(value)[0],
        Completion::Evaluated(samples) => samples.values()[0][0],
    }
}

fn minimum_completion(completion: &Completion<'_>) -> Result<f64, String> {
    match completion {
        Completion::Evaluated(samples) => samples
            .values()
            .iter()
            .map(|values| values[0])
            .reduce(f64::min)
            .ok_or_else(|| "evaluated Completion has no values".into()),
        Completion::Native(value) if !value.animated => Ok(value.values[0]),
        Completion::Native(value) => {
            let mut minimum = value.keyframes[0].values[0];
            for index in 1..value.keyframes.len() {
                let previous = value.keyframes[index - 1].values[0];
                let current = value.keyframes[index].values[0];
                minimum = minimum.min(current);
                let mut warnings = Vec::new();
                if let PropertyKeyframeEasing::CubicBezier { y1, y2, .. } =
                    animation::easing_for_key(
                        &value.keyframes,
                        index,
                        0,
                        1.0,
                        &mut warnings,
                        "Linear Wipe Completion",
                    )
                {
                    for progress in [y1, y2] {
                        minimum = minimum.min(previous + (current - previous) * progress);
                    }
                }
                if !warnings.is_empty() {
                    return Err(warnings.join("; "));
                }
            }
            minimum
                .is_finite()
                .then_some(minimum)
                .ok_or_else(|| "Completion curve has a non-finite convex hull".into())
        }
    }
}

#[derive(Clone, Copy)]
struct InversePlane {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

#[derive(Clone, Copy)]
enum EffectPlane {
    SourceLocal,
    Composition {
        inverse: InversePlane,
        offset: [f64; 2],
    },
}

impl InversePlane {
    fn from_transform(transform: &Transform) -> Result<Self, String> {
        let Position::TwoD(position) = transform.position else {
            return Err("Shape composition-plane Wipe requires a planar Transform".into());
        };
        if transform.rotation_x != 0.0
            || transform.rotation_y != 0.0
            || transform.orientation != [0.0; 3]
            || transform
                .anchor_point
                .iter()
                .chain(&position)
                .chain(&transform.scale)
                .chain([&transform.rotation, &transform.skew, &transform.skew_axis])
                .any(|value| !value.is_finite())
        {
            return Err("Shape composition-plane Wipe requires a finite 2D Transform".into());
        }
        let multiply = |left: [f64; 4], right: [f64; 4]| {
            [
                left[0] * right[0] + left[2] * right[1],
                left[1] * right[0] + left[3] * right[1],
                left[0] * right[2] + left[2] * right[3],
                left[1] * right[2] + left[3] * right[3],
            ]
        };
        let rotation = |degrees: f64| {
            let (sin, cos) = degrees.to_radians().sin_cos();
            [cos, sin, -sin, cos]
        };
        let scale = transform.scale.map(|value| value / 100.0);
        let matrix = multiply(
            rotation(transform.rotation - transform.skew_axis),
            multiply(
                [
                    1.0,
                    0.0,
                    -transform.skew.clamp(-89.9, 89.9).to_radians().tan(),
                    1.0,
                ],
                multiply(
                    rotation(transform.skew_axis),
                    [scale[0], 0.0, 0.0, scale[1]],
                ),
            ),
        );
        let [a, b, c, d] = matrix;
        Ok(Self {
            a,
            b,
            c,
            d,
            e: position[0] - a * transform.anchor_point[0] - c * transform.anchor_point[1],
            f: position[1] - b * transform.anchor_point[0] - d * transform.anchor_point[1],
        })
    }

    /// Compose `self * child`: apply `child`, then `self`.
    fn compose(self, child: Self) -> Self {
        Self {
            a: self.a * child.a + self.c * child.b,
            b: self.b * child.a + self.d * child.b,
            c: self.a * child.c + self.c * child.d,
            d: self.b * child.c + self.d * child.d,
            e: self.a * child.e + self.c * child.f + self.e,
            f: self.b * child.e + self.d * child.f + self.f,
        }
    }

    fn inverse(self) -> Result<Self, String> {
        let determinant = self.a * self.d - self.b * self.c;
        if determinant == 0.0 || !determinant.is_finite() {
            return Err("Shape composition-plane Transform chain is singular".into());
        }
        let inverse = Self {
            a: self.d / determinant,
            b: -self.b / determinant,
            c: -self.c / determinant,
            d: self.a / determinant,
            e: (self.c * self.f - self.d * self.e) / determinant,
            f: (self.b * self.e - self.a * self.f) / determinant,
        };
        [
            inverse.a, inverse.b, inverse.c, inverse.d, inverse.e, inverse.f,
        ]
        .into_iter()
        .all(f64::is_finite)
        .then_some(inverse)
        .ok_or_else(|| "Shape composition-plane inverse Transform is non-finite".into())
    }

    fn from_owner(owner: &GroupLayer, ancestors: &[AncestorTransform<'_>]) -> Result<Self, String> {
        if owner.transform.skew != 0.0 {
            return Err("Shape composition-plane Wipe requires an unskewed owner Transform".into());
        }
        let mut plane = Self::from_transform(&owner.transform)?;
        for ancestor in ancestors {
            plane = Self::from_transform(&ancestor.transform)?.compose(plane);
        }
        plane.inverse()
    }

    fn point(self, point: [f64; 2]) -> [f64; 2] {
        [
            self.a * point[0] + self.c * point[1] + self.e,
            self.b * point[0] + self.d * point[1] + self.f,
        ]
    }

    fn guide(self, mut transform: Transform, offset: [f64; 2]) -> Result<Transform, String> {
        let Position::TwoD(mut position) = transform.position else {
            return Err("Linear Wipe guide must remain planar".into());
        };
        for (value, delta) in position.iter_mut().zip(offset) {
            *value += delta;
        }
        if position.iter().any(|value| !value.is_finite()) {
            return Err("Shape composition-plane guide offset is non-finite".into());
        }
        let (sin, cos) = transform.rotation.to_radians().sin_cos();
        let matrix = [
            self.a * cos + self.c * sin,
            self.b * cos + self.d * sin,
            -self.a * sin + self.c * cos,
            -self.b * sin + self.d * cos,
        ];
        let scale_x = matrix[0].hypot(matrix[1]);
        let determinant = matrix[0] * matrix[3] - matrix[1] * matrix[2];
        if scale_x == 0.0 || !scale_x.is_finite() || !determinant.is_finite() {
            return Err("Shape composition-plane guide Transform is singular".into());
        }
        let scale_y = determinant / scale_x;
        if scale_y == 0.0 || !scale_y.is_finite() {
            return Err("Shape composition-plane guide Transform is singular".into());
        }
        let shear = (matrix[0] * matrix[2] + matrix[1] * matrix[3]) / (scale_x * scale_y);
        transform.position = Position::TwoD(self.point(position));
        transform.scale = [scale_x * 100.0, scale_y * 100.0];
        transform.rotation = matrix[1].atan2(matrix[0]).to_degrees();
        transform.skew = -shear.atan().to_degrees();
        transform.skew_axis = 0.0;
        [
            transform.scale[0],
            transform.scale[1],
            transform.rotation,
            transform.skew,
        ]
        .into_iter()
        .all(f64::is_finite)
        .then_some(transform)
        .ok_or_else(|| "Shape composition-plane guide Transform is non-finite".into())
    }
}

fn static_plane_transform(layer: &Layer, role: &str) -> Result<(), String> {
    if layer.record.flags().three_d_layer || layer.record.auto_orient() != 0 {
        return Err(format!(
            "Shape composition-plane Wipe requires a planar non-auto-oriented {role}"
        ));
    }
    let transforms = properties::read_transform(&layer.content).map_err(|e| e.to_string())?;
    if transforms
        .iter()
        .filter(|property| property.match_name != "ADBE Opacity")
        .any(|property| {
            !property
                .numeric
                .as_ref()
                .is_ok_and(|value| !value.animated && !value.expression_enabled)
        })
    {
        return Err(format!(
            "Shape composition-plane Wipe requires a static authored {role} Transform"
        ));
    }
    Ok(())
}

fn source_plane(
    layer: &Layer,
    owner: &GroupLayer,
    source: Option<&ProjectItem>,
    size: [u16; 2],
    composition_offset: Option<[f64; 2]>,
    ancestors: &[AncestorTransform<'_>],
) -> Result<EffectPlane, String> {
    if layer.record.layer_type() == 4 && source.is_none() {
        if size.contains(&0) || !layer.record.flags().collapse_transformation {
            return Err("requires a finite continuously rasterized Shape composition plane".into());
        }
        static_plane_transform(layer, "owner")?;
        let mut parent_id = layer.record.parent_id();
        for ancestor in ancestors {
            if parent_id == 0 || ancestor.layer.record.id() != parent_id {
                return Err("Shape composition-plane Wipe requires a complete parent chain".into());
            }
            static_plane_transform(ancestor.layer, "parent")?;
            parent_id = ancestor.layer.record.parent_id();
        }
        if parent_id != 0 {
            return Err("Shape composition-plane Wipe requires a complete parent chain".into());
        }
        let offset = composition_offset.unwrap_or([0.0; 2]);
        if offset.iter().any(|value| !value.is_finite()) {
            return Err("Shape composition-plane Wipe normalization offset is non-finite".into());
        }
        return Ok(EffectPlane::Composition {
            inverse: InversePlane::from_owner(owner, ancestors)?,
            offset,
        });
    }
    let Some(source) = source else {
        return Err("requires a decoded finite Solid, composition or Shape source plane".into());
    };
    let (source_size, pixel_aspect) = if let Some(Ok(solid)) = source.solid.as_ref() {
        ([solid.width, solid.height], solid.pixel_aspect)
    } else if let ItemKind::Composition(composition) = &source.kind {
        (
            [composition.width, composition.height],
            composition.pixel_aspect,
        )
    } else {
        return Err("requires a decoded finite Solid or composition source".into());
    };
    if size.contains(&0)
        || size != source_size
        || pixel_aspect.0 == 0
        || pixel_aspect.0 != pixel_aspect.1
    {
        return Err("requires a finite square-pixel source plane".into());
    }
    Ok(EffectPlane::SourceLocal)
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
fn path_extent(size: [u16; 2], angle: f64, minimum_completion: f64) -> f64 {
    let radians = angle.to_radians();
    let direction = [radians.sin().abs(), radians.cos().abs()];
    let [width, height] = size.map(f64::from);
    let travel = direction[0] * width + direction[1] * height;
    let perpendicular = (direction[1] * width + direction[0] * height) / 2.0;
    perpendicular.max(travel * (1.0 - minimum_completion / 100.0).max(0.0))
}

fn path(extent: f64) -> ShapePath {
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
) -> Result<Option<Lowered>, String> {
    let Some(profile) = profile(layer, context.composition_id, context.evaluations)? else {
        return Ok(None);
    };
    let flags = layer.record.flags();
    let plane = source_plane(
        layer,
        owner,
        context.source,
        context.size,
        context.composition_offset,
        context.ancestors,
    )?;
    if matches!(plane, EffectPlane::Composition { .. }) && profile.anchor.is_some() {
        return Err("post-wipe Transform is not mapped through the Shape composition plane".into());
    }
    if !context.planar
        || !matches!(layer.record.layer_type(), 0 | 4)
        || flags.null_layer
        || flags.three_d_layer
        || (flags.collapse_transformation && layer.record.layer_type() != 4)
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
            "requires an isolated planar finite source without masks, matte or collapse".into(),
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
        let mut notes = Vec::new();
        for (index, wipe) in profile.wipes.iter().enumerate() {
            let guide_id = LayerId::new(first + 1 + 2 * index as u64);
            let (transform, span) = guide(
                context.size,
                wipe.angle,
                initial_completion(&wipe.completion),
            );
            let transform = match plane {
                EffectPlane::SourceLocal => transform,
                EffectPlane::Composition { inverse, offset } => inverse.guide(transform, offset)?,
            };
            let extent = path_extent(
                context.size,
                wipe.angle,
                minimum_completion(&wipe.completion)?,
            );
            let target = NumericAnimationTarget::float(
                PropertyTarget::layer(guide_id, PropType::AnchorPointX),
                0,
                -span / 100.,
            );
            let (tracks, warnings, required) = match &wipe.completion {
                Completion::Native(completion) => {
                    let (tracks, warnings) = animation::numeric_entries(
                        "Linear Wipe Completion",
                        completion,
                        &[target],
                        clock,
                        state.animations,
                    );
                    (tracks, warnings, completion.animated)
                }
                Completion::Evaluated(samples) => {
                    let (tracks, warnings) = animation::evaluated_numeric_entries(
                        "Linear Wipe Completion",
                        samples,
                        &[target],
                        &[],
                        state.animations,
                    );
                    if tracks.is_empty() {
                        return Err(format!(
                            "completion animation not admitted: {}",
                            warnings.join("; ")
                        ));
                    }
                    notes.extend(warnings);
                    (tracks, Vec::new(), true)
                }
            };
            if !warnings.is_empty() || (required && tracks.is_empty()) {
                return Err(format!(
                    "completion animation not admitted: {}",
                    warnings.join("; ")
                ));
            }
            entries.extend(tracks);
            masked.layers.push(fx_schema::Layer::from_data(&LayerData::Shape(ShapeLayer{id:guide_id,parent:Some(masked.id),name:"Linear Wipe half-plane guide".into(),description:"Editable projected finite-plane half-plane; continuous-rasterized Shape planes are inverse-mapped through static owner and parent Transforms, with generated-camera correction only on unparented owners; native angled normalization and antialiasing remain approximate".into(),is_hidden:false,blend_mode:Default::default(),track_matte:None,masks:vec![],active_range:range,effects:vec![],motion_blur:false,transform,shape:ShapeContent{path:path(extent),fills:vec![],strokes:vec![],round_corners:None,offset_paths:None,trim:None,poly_star:None,ellipse:None}})).map_err(|e|e.to_string())?);
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
        Ok(Lowered {
            entries,
            notes,
            consumed_trailing_transform: profile.anchor.is_some(),
        })
    })();
    if built.is_err() {
        state.animations.rollback(checkpoint);
    }
    built.map(Some)
}
#[cfg(test)]
mod tests;
