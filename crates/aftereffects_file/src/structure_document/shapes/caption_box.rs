//! A complete, guarded caption-width/reveal formula lowered without a JS runtime.
//! The cached glyph width is a diagnosed fixed source-layout approximation.

use super::*;
use crate::{
    effects::native,
    properties::{self, NumericKeyframe, NumericProperty, NumericValueKind},
};

#[cfg(test)]
#[path = "caption_box/tests.rs"]
mod tests;

#[path = "caption_box/fade.rs"]
mod fade;

const SIZE: &str = r#"
textLayer = thisComp.layer(index - 1);
textWidth = textLayer.sourceRectAtTime(time, false).width;
padding = effect("Width Padding")("Slider");
fixedHeight = effect("Height")("Slider");
slider = effect("Width Override")("Slider");
revealDur = 0.6;
startTime = inPoint;
finalWidth = textWidth + padding + slider;
w = ease(time, startTime, startTime + revealDur, 0, finalWidth);
[ w, fixedHeight ]
"#;
const ROUNDNESS: &str = r#"effect("Roundness")("Slider")"#;
const ANCHOR: &str = r#"
s = content("Rectangle 1").content("Rectangle Path 1").size;
w = s[0];
h = s[1];
[ w/-2, 0]
"#;

struct Controls {
    width: f64,
    height: f64,
    roundness: f64,
    start: f64,
    position: [f64; 2],
}

/// Recognition is tried before single-Rect paint consolidation, because these
/// paints may have independent opacity and blend mode.
pub(super) fn lower(
    collector: &mut Collector<'_>,
    program: &Program<'_>,
    name: &str,
    parent: LayerId,
    budget: &mut super::super::OutputBudget,
    layer: &Layer,
    composition: &Composition,
) -> Result<Option<Vec<FxLayer>>, serde_json::Error> {
    let [geometry] = program.geometry.as_slice() else {
        return Ok(None);
    };
    let GeometryKind::Source(source) = &geometry.kind else {
        return Ok(None);
    };
    if source.name != "ADBE Vector Shape - Rect" {
        return Ok(None);
    }
    let Ok(leaves) = property_group(source.chunks, "caption Rectangle") else {
        return Ok(None);
    };
    // Do not add a second rejection warning to unrelated static/Rigged Box Rects.
    if expression(&leaves, "ADBE Vector Rect Size")
        .and_then(canonical)
        .as_deref()
        != canonical(SIZE).as_deref()
    {
        return Ok(None);
    }
    let [root, scope] = program.scopes.as_slice() else {
        return Ok(None);
    };
    if !root.enabled
        || !scope.enabled
        || scope.parent != Some(ScopeId(0))
        || root.draws != [Draw::Group(ScopeId(1))]
        || scope.draws.len() != 2
        || program.paints.len() != 2
        || geometry.owner != ScopeId(1)
        || !geometry.modifiers.is_empty()
        || !program.warnings.is_empty()
        || program.paints.iter().any(|paint| {
            !paint.enabled
                || paint.owner != ScopeId(1)
                || paint.geometry != [super::super::program::GeometryId(0)]
        })
    {
        return Ok(None);
    }
    let controls = match resolve(layer, composition, source, scope, &leaves) {
        Ok(controls) => controls,
        Err(error) => {
            collector.warnings.push(format!("{name}: caption Rectangle expression mapping rejected ({error}); previous best-effort geometry retained"));
            return Ok(None);
        }
    };
    let mut warnings = Vec::new();
    let order = scope.paint_order(|id| {
        blend::composite_order(program.paints[id.0].operation.chunks, &mut warnings)
    });
    let mut paints = Vec::new();
    for draw in order {
        let Draw::Paint(id) = draw else {
            return Ok(None);
        };
        let paint = &program.paints[id.0].operation;
        let style = match paint.name {
            "ADBE Vector Graphic - Fill" => {
                solid_fill(paint.chunks).ok().map(|fill| (Some(fill), None))
            }
            "ADBE Vector Graphic - Stroke" => solid_stroke(paint.chunks)
                .ok()
                .map(|stroke| (None, Some(stroke))),
            _ => None,
        };
        let Some((fill, stroke)) = style else {
            return Ok(None);
        };
        if stroke
            .as_ref()
            .is_some_and(|stroke| !stroke.dashes.is_empty() || stroke.dash_offset != 0.0)
        {
            return Ok(None);
        }
        let blend_mode = blend::from_run(paint.chunks, &mut warnings);
        let Ok(paint_leaves) = property_group(paint.chunks, "caption paint") else {
            return Ok(None);
        };
        for (property, _) in &paint_leaves {
            if matches!(
                *property,
                "ADBE Vector Blend Mode"
                    | "ADBE Vector Composite Order"
                    | "ADBE Vector Fill Opacity"
                    | "ADBE Vector Stroke Opacity"
            ) && numeric_leaf(&paint_leaves, property, &mut warnings).is_some_and(|numeric| {
                numeric.animated || numeric.expression_enabled || !numeric.keyframes.is_empty()
            }) {
                return Ok(None);
            }
        }
        paints.push((paint, fill, stroke, blend_mode));
    }
    if !warnings.is_empty()
        || paints
            .iter()
            .filter(|(_, fill, _, _)| fill.is_some())
            .count()
            != 1
        || paints
            .iter()
            .filter(|(_, _, stroke, _)| stroke.is_some())
            .count()
            != 1
    {
        return Ok(None);
    }
    let fade = if !collector.includes_occurrence_pipeline {
        None
    } else {
        match fade::resolve(layer, composition) {
            Ok(fade) => Some(fade),
            Err(error) => {
                collector.warnings.push(format!(
                "{name}: caption source-opacity fade omitted ({error}); existing geometry retained"
            ));
                None
            }
        }
    };
    let animation_start = collector.animations.len();
    let animation_checkpoint = collector.animation_budget.checkpoint();
    let id_checkpoint = collector.id_checkpoint();
    let output = (|| {
        let group_id = collector.allocate()?;
        let mut group = crate::structure_document::group(
            group_id,
            name.to_owned(),
            Some(parent),
            full_active_range(),
        );
        group.transform = super::super::decode_transform(scope.transform?, &mut warnings);
        group.transform.anchor_point = [0.0, 0.0];
        group.blend_mode = scope
            .operation
            .map(|run| blend::from_run(run.chunks, &mut warnings))
            .unwrap_or_default();
        // All surrounding vector controls were validated static; the
        // recognized Anchor expression is replaced by the track below.
        let anchor = reveal(controls.start, vec![0.0], vec![-controls.width / 2.0]);
        let mandatory_start = collector.animations.len();
        collector.add_numeric(
            "caption Vector Anchor",
            &anchor,
            &[NumericAnimationTarget::float(
                PropertyTarget::layer(group_id, PropType::AnchorPointX),
                0,
                1.0,
            )],
        );
        if collector.animations.len() != mandatory_start + 1 {
            return None;
        }
        let size = reveal(
            controls.start,
            vec![0.0, controls.height],
            vec![controls.width, controls.height],
        );
        let mut layers = Vec::with_capacity(2);
        for (paint, fill, stroke, blend_mode) in paints {
            let id = collector.allocate()?;
            let color = |paint: &ShapePaint| match paint {
                ShapePaint::Solid { color } => Some(*color),
                _ => None,
            };
            let fill_color = fill
                .as_ref()
                .and_then(|fill| color(&fill.paint))
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let stroke_color = stroke.as_ref().and_then(|stroke| color(&stroke.paint));
            if fill_color
                .iter()
                .chain(stroke_color.iter().flatten())
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            {
                return None;
            }
            let opacity = fill.as_ref().map_or_else(
                || stroke.as_ref().map_or(1.0, |stroke| stroke.opacity),
                |fill| fill.opacity,
            );
            let mut transform = identity_transform();
            transform.anchor_point = [0.0, controls.height / 2.0];
            transform.position = Position::TwoD(controls.position);
            transform.opacity = PercentageProperty::new(opacity * 100.0)?;
            let mut rect = RectLayer {
                id,
                name: format!("{name}: {}", if fill.is_some() { "fill" } else { "stroke" }),
                description:
                    "Editable caption Rectangle; fixed cached source-layout width with eased reveal"
                        .into(),
                is_hidden: false,
                parent: Some(group_id),
                blend_mode,
                track_matte: None,
                masks: Vec::new(),
                active_range: full_active_range(),
                effects: Vec::new(),
                motion_blur: false,
                transform,
                rect: RectShape {
                    size: [0.0, controls.height],
                    position: [0.0, 0.0],
                    roundness: controls.roundness,
                    fill_enabled: fill.is_some(),
                    fill_color,
                    fill_paint: None,
                    fill_blend_mode: None,
                    stroke_enabled: stroke.is_some(),
                    stroke_color,
                    stroke_width: stroke
                        .as_ref()
                        .map_or(Default::default(), |stroke| stroke.width),
                    stroke_dashes: Vec::new(),
                    stroke_dash_offset: 0.0,
                    stroke_join: stroke
                        .as_ref()
                        .map_or(ShapeLineJoin::default(), |stroke| stroke.join),
                    stroke_miter_limit: stroke.as_ref().map_or(4.0, |stroke| stroke.miter_limit),
                },
            };
            let mandatory_start = collector.animations.len();
            collector.add_numeric(
                "caption Rectangle reveal",
                &size,
                &[
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
                ],
            );
            if collector.animations.len() != mandatory_start + 2 {
                return None;
            }
            let entries = [(paint.name, paint.chunks)];
            let decoration = collector.decorations(&entries, Some((layer, composition)));
            if let Some(stroke) = decoration.strokes.first() {
                rect.rect.stroke_width = stroke.width;
            }
            collector.add_scope_entries(
                &entries,
                &decoration,
                &[FxLayer::Rect(rect.clone())],
                Some((layer, composition)),
            );
            layers.push(FxLayer::Rect(rect));
        }
        if let Some(fade) = &fade {
            if group.transform.opacity.value() == 100.0
                && group.blend_mode == fx_schema::BlendMode::Normal
            {
                let mandatory_start = collector.animations.len();
                collector.add_numeric(
                    "caption Source Opacity",
                    fade,
                    &[NumericAnimationTarget::float(
                        PropertyTarget::layer(group_id, PropType::Opacity),
                        0,
                        1.0,
                    )],
                );
                if collector.animations.len() != mandatory_start + 1 {
                    return None;
                }
            } else {
                collector.warnings.push(format!("{name}: caption source-opacity fade omitted: paint group must have static 100% Normal opacity"));
            }
        }
        Some((group, layers))
    })();
    let Some((mut group, layers)) = output else {
        collector.animations.truncate(animation_start);
        collector.animation_budget.rollback(animation_checkpoint);
        collector.restore_ids(id_checkpoint);
        return Ok(None);
    };
    group.layers = match crate::structure_document::stored_layers(layers) {
        Ok(layers) => layers,
        Err(error) => {
            collector.animations.truncate(animation_start);
            collector.animation_budget.rollback(animation_checkpoint);
            collector.restore_ids(id_checkpoint);
            return Err(error);
        }
    };
    let fade_mapped = collector.animations[animation_start..]
        .iter()
        .any(|entry| entry.target == PropertyTarget::layer(group.id, PropType::Opacity));
    let output = FxLayer::Group(group);
    if !budget.reserve(&(&output, &collector.animations[animation_start..])) {
        collector.animations.truncate(animation_start);
        collector.animation_budget.rollback(animation_checkpoint);
        collector.restore_ids(id_checkpoint);
        collector
            .warnings
            .push(super::super::budget::EXHAUSTED.into());
        return Ok(Some(Vec::new()));
    }
    if fade_mapped {
        // Committed: the occurrence owner must not lower this preset again.
        collector.frame_fade_lowered = true;
        collector.warnings.push(format!("{name}: complete caption Solid Composite Source Opacity expression lowered once to an editable linear paint-Group ramp; separate fill/stroke opacity and source clock retained; other effects retain their diagnostics"));
    }
    collector.warnings.extend(warnings);
    collector.warnings.push(format!("{name}: caption sourceRectAtTime width is approximated by fixed cached source-layout glyph bounds; text/font edits and font substitution do not recompute it; complete static controls, separate fill/stroke paints and source-local eased reveal retained"));
    Ok(Some(vec![output]))
}

fn resolve(
    layer: &Layer,
    composition: &Composition,
    source: &NativeRun<'_>,
    scope: &super::super::program::Scope<'_>,
    leaves: &[(&str, &[Chunk])],
) -> Result<Controls, &'static str> {
    require_expression(leaves, "ADBE Vector Rect Size", SIZE)?;
    require_expression(leaves, "ADBE Vector Rect Roundness", ROUNDNESS)?;
    let transform = scope.transform.ok_or("caption vector Transform missing")?;
    let transform_leaves = property_group(transform, "caption Transform")
        .map_err(|_| "caption Transform malformed")?;
    require_expression(&transform_leaves, "ADBE Vector Anchor", ANCHOR)?;
    if display_name(source.chunks)? != "Rectangle Path 1"
        || display_name(
            scope
                .operation
                .ok_or("caption vector group missing")?
                .chunks,
        )? != "Rectangle 1"
    {
        return Err("caption anchor content names do not bind the same rectangle/group");
    }
    for (name, run) in &transform_leaves {
        if *name == "ADBE Vector Anchor" {
            continue;
        }
        if !matches!(
            *name,
            "ADBE Vector Position"
                | "ADBE Vector Scale"
                | "ADBE Vector Rotation"
                | "ADBE Vector Skew"
                | "ADBE Vector Skew Axis"
                | "ADBE Vector Group Opacity"
        ) {
            return Err("caption vector Transform has an unsupported control");
        }
        let body = properties::unique_list(run, *b"tdbs")
            .map_err(|_| "caption vector Transform is malformed")?;
        let numeric =
            properties::read_numeric(body).map_err(|_| "caption vector Transform is malformed")?;
        if numeric.expression_enabled
            || numeric.animated
            || !numeric.keyframes.is_empty()
            || numeric.values.iter().any(|value| !value.is_finite())
        {
            return Err("caption vector Transform is not static outside the recognized anchor");
        }
    }
    let position = initial_pair(leaves, "ADBE Vector Rect Position")
        .ok_or("caption Rectangle Position missing")?;
    if numeric_leaf(leaves, "ADBE Vector Rect Position", &mut Vec::new()).is_some_and(|numeric| {
        numeric.expression_enabled || numeric.animated || !numeric.keyframes.is_empty()
    }) || position.iter().any(|value| !value.is_finite())
    {
        return Err("caption Rectangle Position is not static");
    }
    let index = composition
        .layers
        .iter()
        .position(|candidate| std::ptr::eq(candidate, layer))
        .ok_or("caption layer identity missing")?;
    let preceding = composition
        .layers
        .get(
            index
                .checked_sub(1)
                .ok_or("caption preceding text layer missing")?,
        )
        .ok_or("caption preceding text layer missing")?;
    let width = crate::structure_document::text::cached_caption_width(preceding)?
        + slider(layer, "Width Padding")?
        + slider(layer, "Width Override")?;
    let height = slider(layer, "Height")?;
    let roundness = slider(layer, "Roundness")?;
    // This bounded formula's 0.6 seconds are composition-clock seconds.
    // Unit stretch makes the existing layer source clock an affine translation;
    // other stretch profiles need a separately proven curve-clock mapping.
    if layer.record.stretch() != Some(1.0) {
        return Err("caption reveal requires unit layer stretch");
    }
    let start = layer
        .record
        .in_point()
        .ok_or("caption source-local inPoint invalid")?;
    if ![width, height, roundness, start]
        .into_iter()
        .all(f64::is_finite)
        || width <= 0.0
        || height <= 0.0
        || roundness < 0.0
        || !(start + 0.6).is_finite()
    {
        return Err("caption geometry/reveal values invalid");
    }
    Ok(Controls {
        width,
        height,
        roundness,
        start,
        position,
    })
}

fn slider(layer: &Layer, display: &str) -> Result<f64, &'static str> {
    let roots =
        properties::root_runs(&layer.content).map_err(|_| "caption Effect Parade malformed")?;
    let mut parades = roots
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade");
    let parade = parades.next().ok_or("caption Effect Parade missing")?.1;
    if parades.next().is_some() {
        return Err("caption Effect Parade ambiguous");
    }
    let groups = properties::runs(
        properties::unique_list(parade, *b"tdgp").map_err(|_| "caption Effect Parade malformed")?,
    )
    .map_err(|_| "caption effects malformed")?;
    let mut index = None;
    for (candidate_index, (name, run)) in groups.iter().enumerate() {
        let plugin = properties::unique_list(run, *b"sspc")
            .map_err(|_| "caption effect plugin malformed")?;
        let body = properties::unique_list(plugin, *b"tdgp")
            .map_err(|_| "caption effect controls malformed")?;
        if display_name(body)? != display {
            continue;
        }
        if *name != "ADBE Slider Control" || index.replace(candidate_index + 1).is_some() {
            return Err("caption Slider display name ambiguous");
        }
        {
            let parameters =
                properties::runs(body).map_err(|_| "caption Slider parameters malformed")?;
            if parameters
                .iter()
                .filter(|(name, _)| *name == "ADBE Slider Control-0001")
                .count()
                != 1
            {
                return Err("caption Slider requires one explicit control");
            }
        }
    }
    let index = index.ok_or("caption named Slider missing")?;
    let (effects, _) = native::read_effects(&layer.content, [0.0, 0.0]);
    let effect = effects
        .iter()
        .find(|effect| effect.index == index && effect.enabled)
        .ok_or("caption Slider disabled/malformed")?;
    let mut parameters = effect
        .parameters
        .iter()
        .filter(|parameter| parameter.match_name == "ADBE Slider Control-0001");
    let numeric = parameters
        .next()
        .ok_or("caption Slider control missing")?
        .numeric
        .as_ref()
        .map_err(|_| "caption Slider control malformed")?;
    if parameters.next().is_some()
        || numeric.expression_enabled
        || numeric.animated
        || !numeric.keyframes.is_empty()
        || numeric.values.len() != 1
        || !numeric.values[0].is_finite()
    {
        return Err("caption Slider must be a unique finite static scalar");
    }
    Ok(numeric.values[0])
}

fn display_name(chunks: &[Chunk]) -> Result<&str, &'static str> {
    let chunks = if chunks.iter().any(|chunk| chunk.id() == *b"tdsn") {
        chunks
    } else {
        properties::unique_list(chunks, *b"tdgp")
            .map_err(|_| "caption display-name group missing/ambiguous")?
    };
    let bytes =
        properties::data(chunks, *b"tdsn").map_err(|_| "caption display name missing/ambiguous")?;
    if bytes.get(..4) != Some(b"Utf8") {
        return Err("caption display name encoding");
    }
    let length = u32::from_be_bytes(
        bytes
            .get(4..8)
            .ok_or("caption display name truncated")?
            .try_into()
            .map_err(|_| "caption display name truncated")?,
    );
    let end = 8_usize
        .checked_add(usize::try_from(length).map_err(|_| "caption display name length")?)
        .ok_or("caption display name length")?;
    let padding = bytes.get(end..).ok_or("caption display name truncated")?;
    if padding.len() > 3 || padding.iter().any(|byte| *byte != 0) {
        return Err("caption display name padding");
    }
    std::str::from_utf8(bytes.get(8..end).ok_or("caption display name truncated")?)
        .map_err(|_| "caption display name UTF-8")
}

fn expression<'a>(leaves: &[(&str, &'a [Chunk])], property: &str) -> Option<&'a str> {
    let mut matches = leaves.iter().filter(|(name, _)| *name == property);
    let run = matches.next()?.1;
    if matches.next().is_some() {
        return None;
    }
    let body = properties::unique_list(run, *b"tdbs").ok()?;
    let numeric = properties::read_numeric(body).ok()?;
    if !numeric.expression_enabled || numeric.animated || !numeric.keyframes.is_empty() {
        return None;
    }
    std::str::from_utf8(properties::data(body, *b"Utf8").ok()?).ok()
}

fn require_expression(
    leaves: &[(&str, &[Chunk])],
    property: &str,
    expected: &str,
) -> Result<(), &'static str> {
    if expression(leaves, property).and_then(canonical).as_deref() == canonical(expected).as_deref()
    {
        Ok(())
    } else {
        Err("caption expression is disabled, ambiguous or not the complete supported formula")
    }
}

/// Tokenize bounded source without fusing identifiers/numbers across trivia.
/// Strings keep their exact spelling, including referenced binding names.
fn canonical(source: &str) -> Option<String> {
    if source.len() > 8_192 {
        return None;
    }
    let mut output = String::new();
    let mut chars = source.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '"' => {
                output.push(character);
                loop {
                    let character = chars.next()?;
                    output.push(character);
                    if character == '\\' {
                        output.push(chars.next()?);
                    } else if character == '"' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                for character in chars.by_ref() {
                    if matches!(character, '\r' | '\n') {
                        break;
                    }
                }
                continue;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                loop {
                    let character = chars.next()?;
                    if character == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
                continue;
            }
            character if character.is_ascii_whitespace() => continue,
            character if character.is_ascii_alphabetic() || matches!(character, '_' | '$') => {
                output.push(character);
                while let Some(character) = chars.peek().copied().filter(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '$')
                }) {
                    output.push(character);
                    chars.next();
                }
            }
            character if character.is_ascii_digit() => {
                output.push(character);
                while let Some(character) = chars.peek().copied().filter(char::is_ascii_digit) {
                    output.push(character);
                    chars.next();
                }
                if chars.peek() == Some(&'.') {
                    output.push('.');
                    chars.next();
                    while let Some(character) = chars.peek().copied().filter(char::is_ascii_digit) {
                        output.push(character);
                        chars.next();
                    }
                }
            }
            character => output.push(character),
        }
        output.push('\0');
    }
    Some(output)
}

fn reveal(start: f64, from: Vec<f64>, to: Vec<f64>) -> NumericProperty {
    let dimensions = from.len();
    let key = |time_secs, values| NumericKeyframe {
        time_secs,
        values,
        in_interpolation: 2,
        out_interpolation: 2,
        in_speed: vec![0.0; dimensions],
        out_speed: vec![0.0; dimensions],
        in_influence: vec![100.0 / 3.0; dimensions],
        out_influence: vec![100.0 / 3.0; dimensions],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    };
    NumericProperty {
        values: from.clone(),
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: vec![key(start, from), key(start + 0.6, to)],
        value_kind: NumericValueKind::Continuous,
    }
}
