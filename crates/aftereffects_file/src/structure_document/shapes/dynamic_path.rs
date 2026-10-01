//! Bounded lowering for the stock Layer-Control `createPath` rigs used by the
//! Intro source. This is deliberately not an expression evaluator: a complete
//! expression must match one of the structural profiles below.

mod point_curve;
mod position_blend;

pub(in crate::structure_document) use point_curve::composition_origin_curve;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use fx_schema::{
    KeyframeId, PropertyAnimator, PropertyKeyframeEasing, PropertyTarget, PropertyValue, ShapePath,
    ShapePathCommand, TimeOffset,
    animator::{AnimationGraphEntry, PropertyKeyframe, PropertyKeyframeTrack},
};

use super::path;
use crate::{
    properties::{self, NumericKeyframe, NumericProperty, PropertyError},
    rifx::Chunk,
    structure::{Composition, ItemKind, Layer, ProjectItem},
    structure_document::{
        animation::NumericAnimationClock,
        animation_budget::{AnimationBudget, committed_entry_reservation_bytes},
        control_links, solid_anchor_scale, source_anchor_dimensions,
    },
};

const MAX_PARENT_DEPTH: usize = 16;
const MAX_DURATION_MS: i64 = 60_000;
const MAX_EVALUATIONS: usize = 200_000;
const FIT_TOLERANCE_PIXELS: f64 = 0.01;

/// Result of a recognized dynamic-Path expression. `None` means the expression
/// is absent or outside these exact profiles, so ordinary native Path handling
/// remains authoritative.
type LoweredPath = Result<(Vec<AnimationGraphEntry>, Vec<String>), String>;

pub(super) fn entries<'items>(
    owner: &Layer,
    composition: &Composition,
    source_items: &'items HashMap<u32, &'items ProjectItem>,
    run: &[Chunk],
    target: PropertyTarget,
    clock: NumericAnimationClock,
    budget: &mut AnimationBudget,
) -> Option<LoweredPath> {
    let expression = path_expression(run).ok().flatten()?;
    let parsed = parse_expression(expression)?;
    let checkpoint = budget.checkpoint();
    let result = lower_parsed(
        owner,
        composition,
        source_items,
        run,
        target,
        clock,
        budget,
        parsed,
        None,
    );
    if result.is_err() {
        budget.rollback(checkpoint);
    }
    Some(result)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DynamicExpression {
    CreatePath {
        controls: Vec<String>,
    },
    CopySibling {
        layer: String,
        contents: Vec<String>,
    },
    CopyComposition {
        composition: String,
        layer: String,
        contents: Vec<String>,
    },
}

#[allow(
    clippy::too_many_arguments,
    reason = "Source lookup, destination clock, and shared budgets stay explicit"
)]
fn lower_parsed<'items>(
    owner: &Layer,
    composition: &Composition,
    source_items: &'items HashMap<u32, &'items ProjectItem>,
    run: &[Chunk],
    target: PropertyTarget,
    clock: NumericAnimationClock,
    budget: &mut AnimationBudget,
    expression: DynamicExpression,
    fit_interval_ms: Option<(i64, i64)>,
) -> Result<(Vec<AnimationGraphEntry>, Vec<String>), String> {
    match expression {
        DynamicExpression::CreatePath { controls } => lower_create_path(
            owner,
            composition,
            source_items,
            run,
            target,
            clock,
            budget,
            &controls,
            fit_interval_ms,
        ),
        DynamicExpression::CopySibling { layer, contents } => lower_copy(
            owner,
            composition,
            source_items,
            target,
            clock,
            budget,
            &layer,
            &contents,
        ),
        DynamicExpression::CopyComposition {
            composition: source_composition,
            layer,
            contents,
        } => {
            let source = unique_composition_by_name(source_items, &source_composition)?;
            let (entries, mut warnings) = lower_copy(
                owner,
                source,
                source_items,
                target,
                clock,
                budget,
                &layer,
                &contents,
            )?;
            warnings.push(format!(
                "direct Path from composition {source_composition:?} was copied as independent editable data; live cross-composition linkage is not retained"
            ));
            Ok((entries, warnings))
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Source lookup, destination clock, and shared budgets stay explicit"
)]
fn lower_create_path<'items>(
    owner: &Layer,
    composition: &Composition,
    source_items: &'items HashMap<u32, &'items ProjectItem>,
    run: &[Chunk],
    target: PropertyTarget,
    clock: NumericAnimationClock,
    budget: &mut AnimationBudget,
    controls: &[String],
    fit_interval_ms: Option<(i64, i64)>,
) -> Result<(Vec<AnimationGraphEntry>, Vec<String>), String> {
    if !matches!(controls.len(), 2 | 4) {
        return Err("dynamic Path requires exactly two or four Layer Controls".into());
    }
    let mut ids = Vec::with_capacity(controls.len());
    for control in controls {
        let id = layer_control_id(owner, control)
            .map_err(|error| format!("Layer Control {control:?}: {error}"))?;
        ids.push(id);
    }

    let (initial_path, _) = path::decode_first(run, [1.0, 1.0])
        .map_err(|error| format!("dynamic Path initial geometry cannot be decoded: {error}"))?;
    let initial_geometry = PathGeometry::new(initial_path)?;
    if initial_geometry.points.len() != controls.len() {
        return Err(format!(
            "dynamic Path has {} Layer Controls but {} stored vertices",
            controls.len(),
            initial_geometry.points.len()
        ));
    }

    let mut rig = TransformRig::new(composition, source_items);
    rig.add(owner.record.id())?;
    for id in &ids {
        rig.add(*id)?;
    }
    let (start_ms, end_ms) = fit_interval_ms.map_or_else(|| owner_interval_ms(owner), Ok)?;
    // Fit only the interval consumed by the destination owner. Direct sibling
    // copies pass their rebased destination interval; fitting invisible driver
    // history can exceed the track budget without preserving rendered semantics.
    let mut seed_times = BTreeSet::new();
    rig.seed_owner_times(owner, start_ms, end_ms, &mut seed_times)?;
    seed_times.extend([start_ms, end_ms]);
    if seed_times.len() > MAX_EVALUATIONS {
        return Err("dynamic Path source keys exceed the analytical evaluation bound".into());
    }
    let mut evaluations = 0usize;
    let evaluate = |time_ms: i64, evaluations: &mut usize| -> Result<Vec<[f64; 2]>, String> {
        *evaluations = evaluations
            .checked_add(1)
            .ok_or_else(|| "dynamic Path evaluation count overflow".to_owned())?;
        if *evaluations > MAX_EVALUATIONS {
            return Err("dynamic Path exceeded its analytical evaluation bound".into());
        }
        let owner_local = time_ms as f64 / 1000.0;
        let composition_time = owner_local_to_comp(owner, owner_local)?;
        let owner_to_comp = rig.matrix(owner.record.id(), composition_time)?;
        let comp_to_owner = owner_to_comp
            .inverse()
            .ok_or_else(|| "dynamic Path owner transform is singular".to_owned())?;
        ids.iter()
            .enumerate()
            .map(|(index, id)| {
                // The stock expression explicitly skips self references, leaving
                // that vertex at its authored value (e.g. Noise-Matte).
                if *id == owner.record.id() {
                    return Ok(initial_geometry.points[index]);
                }
                let source = rig
                    .nodes
                    .get(id)
                    .ok_or_else(|| format!("dynamic Path source layer {id} is missing"))?;
                let anchor = source.anchor(composition_time)?;
                let point = rig.matrix(*id, composition_time)?.apply(anchor);
                let point = comp_to_owner.apply(point);
                point
                    .iter()
                    .all(|value| value.is_finite())
                    .then_some(point)
                    .ok_or_else(|| "dynamic Path produced a non-finite point".into())
            })
            .collect()
    };

    let mut fitted: BTreeMap<i64, Vec<[f64; 2]>> = BTreeMap::new();
    for time in &seed_times {
        let mut points = evaluate(*time, &mut evaluations)?;
        compact_fitted_coordinates(&mut points);
        fitted.insert(*time, points);
    }
    let owner_id = target
        .layer_id()
        .ok_or_else(|| "dynamic Path target is not a layer property".to_owned())?;
    loop {
        let times: Vec<_> = fitted.keys().copied().collect();
        let mut splits = Vec::new();
        for pair in times.windows(2) {
            let [from_time, to_time] = [pair[0], pair[1]];
            if to_time - from_time <= 1 {
                continue;
            }
            let from = fitted
                .get(&from_time)
                .ok_or_else(|| "dynamic Path fit start vanished".to_owned())?;
            let to = fitted
                .get(&to_time)
                .ok_or_else(|| "dynamic Path fit end vanished".to_owned())?;
            let candidates = [
                from_time + (to_time - from_time) / 4,
                from_time + (to_time - from_time) / 2,
                from_time + (to_time - from_time) * 3 / 4,
            ];
            let mut split = None;
            for candidate in candidates {
                if candidate <= from_time || candidate >= to_time {
                    continue;
                }
                let actual = evaluate(candidate, &mut evaluations)?;
                let progress = (candidate - from_time) as f64 / (to_time - from_time) as f64;
                if point_error(&actual, &interpolate_points(from, to, progress))
                    > FIT_TOLERANCE_PIXELS
                {
                    split = Some((candidate, actual));
                    break;
                }
            }
            if let Some(split) = split {
                splits.push(split);
            }
        }
        if splits.is_empty() {
            splits = validation_splits(&fitted, &evaluate, &mut evaluations)?;
        }
        if splits.is_empty() {
            break;
        }
        for (_, points) in &mut splits {
            compact_fitted_coordinates(points);
        }
        fitted.extend(splits);
    }

    // Duration and evaluation bounds limit analytical work; the canonical
    // constructor remains the final structural contract authority.
    let track = build_path_track(owner_id, fitted, clock, &initial_geometry)?;
    let entry = AnimationGraphEntry {
        target,
        animator: PropertyAnimator::keyframes(track),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    reserve_entry(budget, &entry)?;
    let warnings = vec![format!(
        "Path expression lowered from {} Layer Controls to {} sparse editable Path keys; toComp(anchorPoint)/fromCompToSurface used the bounded 2D parent chains and generated coordinates were compacted to four decimal places where displacement is at most 0.0001, and the resulting 1ms analytical fit was checked against unrounded controls within {FIT_TOLERANCE_PIXELS} owner-local units before output clock quantization (not a rendered pixel guarantee); live controller linkage is not retained",
        controls.len(),
        match entry.animator.data() {
            fx_schema::animator::AnimatorData::Keyframes { track, .. } => track.keyframes().len(),
            _ => 0,
        }
    )];
    Ok((vec![entry], warnings))
}

#[allow(
    clippy::too_many_arguments,
    reason = "Source lookup, destination clock, and shared budgets stay explicit"
)]
fn lower_copy<'items>(
    owner: &Layer,
    composition: &Composition,
    source_items: &'items HashMap<u32, &'items ProjectItem>,
    target: PropertyTarget,
    clock: NumericAnimationClock,
    budget: &mut AnimationBudget,
    source_name: &str,
    contents: &[String],
) -> Result<(Vec<AnimationGraphEntry>, Vec<String>), String> {
    if clock != NumericAnimationClock::source_local() {
        return Err("sibling Path copies require a source-local destination clock".into());
    }
    let source = unique_layer_by_name(composition, source_name)?;
    let source_id = source.record.id();
    if source_id == owner.record.id() {
        return Err(format!(
            "cyclic direct sibling Path copy through layer {source_id}"
        ));
    }
    let (owner_start, owner_stretch) = valid_clock(owner)?;
    let (source_start, source_stretch) = valid_clock(source)?;
    if (owner_stretch - source_stretch).abs() > f64::EPSILON || owner_stretch <= 0.0 {
        return Err("direct sibling Path copy requires equal positive layer stretch".into());
    }
    let runs = path_runs(source)?;
    let [source_run] = runs.as_slice() else {
        return Err("direct sibling Path copy requires exactly one source Path".into());
    };
    let selected = selected_path(source, contents)?;
    if selected.as_ptr() != source_run.as_ptr() {
        return Err("sibling Path selector does not identify the sole native Path".into());
    }
    let source_expression = path_expression(source_run).map_err(|error| error.to_string())?;
    let parsed = if let Some(text) = source_expression {
        Some(
            parse_expression(text)
                .ok_or_else(|| "copied source has an unsupported Path expression".to_owned())?,
        )
    } else {
        generated_path_controls(composition, source_name, contents)
            .map(|controls| DynamicExpression::CreatePath { controls })
    };
    let rebased =
        NumericAnimationClock::source_local_rebased((owner_start - source_start) / owner_stretch);
    if let Some(parsed) = parsed {
        if matches!(
            parsed,
            DynamicExpression::CopySibling { .. } | DynamicExpression::CopyComposition { .. }
        ) {
            return Err("nested direct Path copies are unsupported".into());
        }
        let (destination_start_ms, destination_end_ms) = owner_interval_ms(owner)?;
        let offset_ms = seconds_to_millis((owner_start - source_start) / owner_stretch)?;
        let source_start_ms = destination_start_ms
            .checked_add(offset_ms)
            .ok_or_else(|| "direct Path source interval start overflow".to_owned())?;
        let source_end_ms = destination_end_ms
            .checked_add(offset_ms)
            .ok_or_else(|| "direct Path source interval end overflow".to_owned())?;
        let result = lower_parsed(
            source,
            composition,
            source_items,
            source_run,
            target,
            rebased,
            budget,
            parsed,
            Some((source_start_ms, source_end_ms)),
        );
        let (entries, mut warnings) = result?;
        warnings.push(format!(
            "direct Path from {source_name:?} was copied as independent editable keys; live Path linkage is not retained"
        ));
        return Ok((entries, warnings));
    }

    let (entries, warnings) =
        path::entries(source_run, [1.0, 1.0], target.clone(), rebased, budget);
    if !warnings.is_empty() {
        return Err(warnings.join("; "));
    }
    if !entries.is_empty() {
        return Ok((
            entries,
            vec![format!(
                "direct sibling Path animation from {source_name:?} was copied with native keys/easing; live Path linkage is not retained"
            )],
        ));
    }

    let (initial, _) = path::decode_first(source_run, [1.0, 1.0])
        .map_err(|error| format!("copied sibling Path cannot be decoded: {error}"))?;
    let entry = AnimationGraphEntry {
        target,
        animator: PropertyAnimator::constant(PropertyValue::Path(initial))
            .map_err(|error| error.to_string())?,
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    reserve_entry(budget, &entry)?;
    Ok((
        vec![entry],
        vec![format!(
            "direct sibling Path from {source_name:?} was copied as independent editable geometry; live Path linkage is not retained"
        )],
    ))
}

fn path_keyframe(
    owner_id: fx_schema::LayerId,
    owner_ms: i64,
    points: &[[f64; 2]],
    clock: NumericAnimationClock,
    initial_geometry: &PathGeometry,
) -> Result<PropertyKeyframe, String> {
    let output_seconds = clock.seconds(owner_ms as f64 / 1000.0);
    let output_ms = seconds_to_millis(output_seconds)?;
    let mut path = initial_geometry.deform(points)?;
    // Close already draws the final straight segment. Keep curved closing
    // segments, but avoid serializing the identical start vertex twice per key.
    if initial_geometry.closed
        && let Some(closing_index) = path.commands.len().checked_sub(2)
        && matches!(path.commands.get(closing_index), Some(ShapePathCommand::LineTo { x, y, .. })
            if *x == points[0][0] && *y == points[0][1])
    {
        path.commands.remove(closing_index);
    }
    Ok(PropertyKeyframe::new(
        // IDs are opaque; retain owner/time uniqueness without repeating a long
        // label in every generated key and every independent matte copy.
        KeyframeId::new(format!("p{owner_id}-{owner_ms}")),
        TimeOffset::from_millis(output_ms),
        PropertyValue::Path(path),
        PropertyKeyframeEasing::Linear,
    ))
}

fn build_path_track(
    owner_id: fx_schema::LayerId,
    fitted: BTreeMap<i64, Vec<[f64; 2]>>,
    clock: NumericAnimationClock,
    initial_geometry: &PathGeometry,
) -> Result<PropertyKeyframeTrack, String> {
    let mut keys = Vec::with_capacity(fitted.len());
    for (owner_ms, points) in fitted {
        keys.push(path_keyframe(
            owner_id,
            owner_ms,
            &points,
            clock,
            initial_geometry,
        )?);
    }
    if clock.reversed() {
        keys.reverse();
    }
    PropertyKeyframeTrack::new(keys).map_err(|error| error.to_string())
}

fn reserve_entry(budget: &mut AnimationBudget, entry: &AnimationGraphEntry) -> Result<(), String> {
    let bytes = committed_entry_reservation_bytes(entry).map_err(|error| error.to_string())?;
    budget.reserve(bytes).map_err(|error| error.to_string())
}

fn path_expression(run: &[Chunk]) -> Result<Option<&str>, PropertyError> {
    let property = if run.iter().any(|chunk| chunk.list_kind() == Some(*b"om-s")) {
        run
    } else {
        properties::runs(properties::unique_list(run, *b"tdgp")?)?
            .into_iter()
            .find(|(name, _)| *name == "ADBE Vector Shape")
            .map(|(_, property)| property)
            .ok_or(PropertyError::Layout("missing vector path property"))?
    };
    let value = properties::unique_list(property, *b"om-s")?;
    let body = properties::unique_list(value, *b"tdbs")?;
    let metadata = properties::read_path_metadata(body)?;
    if !metadata.expression_enabled {
        return Ok(None);
    }
    let bytes = properties::data(body, *b"Utf8")?;
    std::str::from_utf8(bytes)
        .map(Some)
        .map_err(|_| PropertyError::Layout("invalid Path expression text"))
}

fn parse_expression(text: &str) -> Option<DynamicExpression> {
    let compact = compact_expression(text)?;
    if let Some(stock) = parse_stock_create_path(&compact) {
        return Some(stock);
    }
    parse_copy(&compact)
}

fn compact_expression(text: &str) -> Option<String> {
    if !text.is_ascii() {
        return None;
    }
    let mut output = String::with_capacity(text.len());
    let mut quoted = None;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if let Some(quote) = quoted {
            output.push(character);
            if character == '\\' {
                output.push(chars.next()?);
            } else if character == quote {
                quoted = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quoted = Some(character);
            output.push(character);
        } else if character == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for next in chars.by_ref() {
                if matches!(next, '\n' | '\r') {
                    break;
                }
            }
        } else if !character.is_whitespace() {
            output.push(character);
        }
    }
    (quoted.is_none() && !output.contains("/*")).then_some(output)
}

fn parse_stock_create_path(text: &str) -> Option<DynamicExpression> {
    let rest = text.strip_prefix("varnullLayerNames=[")?;
    let end = closing_bracket(rest)?;
    let controls = split_top_level(&rest[..end], ',')?
        .into_iter()
        .map(|value| {
            take_quoted(value)
                .filter(|(_, tail)| tail.is_empty())
                .map(|(value, _)| value.to_owned())
        })
        .collect::<Option<Vec<_>>>()?;
    if !matches!(controls.len(), 2 | 4) {
        return None;
    }
    let suffix = &rest[end + 1..];
    const STOCK_SUFFIX: &str = r#";varorigPath=thisProperty;varorigPoints=origPath.points();varorigInTang=origPath.inTangents();varorigOutTang=origPath.outTangents();vargetNullLayers=[];vargetNullLayersIn=[];vargetNullLayersOut=[];for(vari=0;i<nullLayerNames.length;i++){try{getNullLayers.push(effect(nullLayerNames[i])("ADBE Layer Control-0001"));}catch(err){getNullLayers.push(null);}}for(vari=0;i<getNullLayers.length;i++){if(getNullLayers[i]!=null&&getNullLayers[i].index!=thisLayer.index){origPoints[i]=fromCompToSurface(getNullLayers[i].toComp(getNullLayers[i].anchorPoint));}}createPath(origPoints,origInTang,origOutTang,origPath.isClosed());"#;
    (suffix == STOCK_SUFFIX
        || suffix == STOCK_SUFFIX.replace("vargetNullLayersIn=[];vargetNullLayersOut=[];", ""))
    .then_some(DynamicExpression::CreatePath { controls })
}

fn generated_path_controls(
    composition: &Composition,
    source_name: &str,
    contents: &[String],
) -> Option<Vec<String>> {
    let path_name = contents.last()?;
    let mut controls = Vec::new();
    for index in 0..4 {
        let name = format!("{source_name}: {path_name} [1.1.{index}]");
        let count = composition
            .layers
            .iter()
            .filter(|layer| layer.name.as_ref() == name)
            .count();
        if count == 0 {
            break;
        }
        if count != 1 {
            return None;
        }
        controls.push(name);
    }
    matches!(controls.len(), 2 | 4).then_some(controls)
}

fn parse_copy(text: &str) -> Option<DynamicExpression> {
    let mut input = text.strip_suffix(';').unwrap_or(text);
    let composition = if let Some(rest) = input.strip_prefix("thisComp.layer(") {
        input = rest;
        None
    } else {
        let rest = input.strip_prefix("comp(")?;
        let (composition, rest) = take_quoted(rest)?;
        input = rest.strip_prefix(").layer(")?;
        Some(composition.to_owned())
    };
    let (layer, rest) = take_quoted(input)?;
    let mut rest = rest.strip_prefix(')')?;
    let mut contents = Vec::new();
    while let Some(next) = rest.strip_prefix(".content(") {
        let (name, next) = take_quoted(next)?;
        rest = next.strip_prefix(')')?;
        contents.push(name.to_owned());
        if contents.len() > MAX_PARENT_DEPTH {
            return None;
        }
    }
    if contents.is_empty() || !matches!(rest, ".path" | ".path.value") {
        return None;
    }
    if let Some(composition) = composition {
        Some(DynamicExpression::CopyComposition {
            composition,
            layer: layer.to_owned(),
            contents,
        })
    } else {
        Some(DynamicExpression::CopySibling {
            layer: layer.to_owned(),
            contents,
        })
    }
}

fn split_top_level(text: &str, separator: char) -> Option<Vec<&str>> {
    if text.is_empty() {
        return Some(Vec::new());
    }
    let mut result = Vec::new();
    let mut start = 0usize;
    let mut depths = [0i32; 2];
    let mut quoted = None;
    let bytes = text.as_bytes();
    for (index, character) in text.char_indices() {
        if let Some(quote) = quoted {
            if character as u8 == quote && bytes.get(index.wrapping_sub(1)) != Some(&b'\\') {
                quoted = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quoted = Some(character as u8),
            '(' => depths[0] += 1,
            ')' => depths[0] -= 1,
            '[' => depths[1] += 1,
            ']' => depths[1] -= 1,
            _ if character == separator && depths == [0, 0] => {
                result.push(&text[start..index]);
                start = index + character.len_utf8();
            }
            _ => (),
        }
        if depths.iter().any(|depth| *depth < 0) {
            return None;
        }
    }
    if quoted.is_some() || depths != [0, 0] {
        return None;
    }
    result.push(&text[start..]);
    Some(result)
}

fn closing_bracket(text: &str) -> Option<usize> {
    let mut depth = 1i32;
    let mut quoted = None;
    let bytes = text.as_bytes();
    for (index, character) in text.char_indices() {
        if let Some(quote) = quoted {
            if character as u8 == quote && bytes.get(index.wrapping_sub(1)) != Some(&b'\\') {
                quoted = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quoted = Some(character as u8),
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => (),
        }
    }
    None
}

fn take_quoted(text: &str) -> Option<(&str, &str)> {
    let quote = text
        .chars()
        .next()
        .filter(|character| matches!(character, '\'' | '"'))?;
    let body = text.get(1..)?;
    let end = body.find(quote)?;
    let value = &body[..end];
    if value.contains(['\\', '\n', '\r']) {
        return None;
    }
    Some((value, &body[end + 1..]))
}

fn layer_control_id(owner: &Layer, display: &str) -> Result<u32, String> {
    if !owner.record.flags().effects_active {
        return Err("owner Effects are disabled".into());
    }
    let roots = properties::root_runs(&owner.content).map_err(|error| error.to_string())?;
    let (_, parade) = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Effect Parade")
        .ok_or_else(|| "owner has no Effect Parade".to_owned())?;
    let groups = properties::unique_list(parade, *b"tdgp").map_err(|error| error.to_string())?;
    let runs = properties::runs(groups).map_err(|error| error.to_string())?;
    let mut matches = runs.iter().filter_map(|(kind, run)| {
        let plugin = properties::unique_list(run, *b"sspc").ok()?;
        let body = properties::unique_list(plugin, *b"tdgp").ok()?;
        (*kind == "ADBE Layer Control" && display_name(body) == Some(display)).then_some(body)
    });
    let body = matches
        .next()
        .ok_or_else(|| "named Layer Control is missing".to_owned())?;
    if matches.next().is_some() {
        return Err("named Layer Control is ambiguous".into());
    }
    let parameters = properties::runs(body).map_err(|error| error.to_string())?;
    let candidates: Vec<_> = parameters
        .iter()
        .filter(|(name, _)| *name == "ADBE Layer Control-0001")
        .collect();
    let [(_, parameter)] = candidates.as_slice() else {
        return Err("Layer Control value is missing or ambiguous".into());
    };
    let body = properties::unique_list(parameter, *b"tdbs").map_err(|error| error.to_string())?;
    let numeric = properties::read_numeric(body).map_err(|error| error.to_string())?;
    if numeric.animated || numeric.expression_enabled || !numeric.keyframes.is_empty() {
        return Err("animated or expression-driven Layer Control selection is unsupported".into());
    }
    let references: Vec<_> = body.iter().filter(|chunk| chunk.id() == *b"tdpi").collect();
    let [reference] = references.as_slice() else {
        return Err("Layer Control reference metadata is missing or ambiguous".into());
    };
    let bytes: [u8; 4] = reference
        .data_payload()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| "Layer Control reference metadata is malformed".to_owned())?;
    let id = u32::from_be_bytes(bytes);
    (id != 0)
        .then_some(id)
        .ok_or_else(|| "Layer Control does not select a concrete layer".into())
}

fn display_name(chunks: &[Chunk]) -> Option<&str> {
    let bytes = properties::data(chunks, *b"tdsn").ok()?;
    if bytes.get(..4)? != b"Utf8" {
        return None;
    }
    let length = usize::try_from(u32::from_be_bytes(bytes.get(4..8)?.try_into().ok()?)).ok()?;
    let end = 8usize.checked_add(length)?;
    let padding = bytes.get(end..)?;
    if padding.len() > 3 || padding.iter().any(|byte| *byte != 0) {
        return None;
    }
    std::str::from_utf8(bytes.get(8..end)?).ok()
}

fn path_runs(layer: &Layer) -> Result<Vec<&[Chunk]>, String> {
    fn collect<'a>(chunks: &'a [Chunk], output: &mut Vec<&'a [Chunk]>) {
        if let Ok(runs) = properties::runs(chunks) {
            for (name, run) in runs {
                if name == "ADBE Vector Shape" {
                    output.push(run);
                }
            }
        }
        for chunk in chunks {
            if let Some(children) = chunk.children() {
                collect(children, output);
            }
        }
    }
    let mut output = Vec::new();
    collect(&layer.content, &mut output);
    output.sort_by_key(|run| run.as_ptr() as usize);
    output.dedup_by_key(|run| run.as_ptr() as usize);
    Ok(output)
}

fn selected_path<'a>(layer: &'a Layer, selectors: &[String]) -> Result<&'a [Chunk], String> {
    let root = properties::root_runs(&layer.content).map_err(|error| error.to_string())?;
    let root = root
        .iter()
        .find(|(name, _)| *name == "ADBE Root Vectors Group")
        .ok_or("copied layer has no vector contents")?
        .1;
    let mut contents = super::property_group(root, "copied vector contents")?;
    for (index, selector) in selectors.iter().enumerate() {
        let final_selector = index + 1 == selectors.len();
        let expected_type = if final_selector {
            "ADBE Vector Shape - Group"
        } else {
            "ADBE Vector Group"
        };
        let mut matches = contents.iter().filter_map(|(kind, run)| {
            if *kind != expected_type {
                return None;
            }
            let body = properties::unique_list(run, *b"tdgp").ok()?;
            (display_name(body) == Some(selector.as_str())).then_some(body)
        });
        let selected = matches
            .next()
            .ok_or_else(|| format!("sibling Path content selector {selector:?} is missing"))?;
        if matches.next().is_some() {
            return Err(format!(
                "sibling Path content selector {selector:?} is ambiguous"
            ));
        }
        let leaves = properties::runs(selected).map_err(|error| error.to_string())?;
        if final_selector {
            let mut paths = leaves
                .iter()
                .filter(|(kind, _)| *kind == "ADBE Vector Shape");
            let path = paths.next().ok_or("selected content has no Path")?.1;
            if paths.next().is_some() {
                return Err("selected content has ambiguous Paths".into());
            }
            return Ok(path);
        }
        let children = leaves
            .iter()
            .find(|(kind, _)| *kind == "ADBE Vectors Group")
            .ok_or("selected vector group has no contents")?
            .1;
        contents = super::property_group(children, "copied vector group")?;
    }
    Err("sibling Path has no content selectors".into())
}

fn unique_layer_by_name<'a>(composition: &'a Composition, name: &str) -> Result<&'a Layer, String> {
    let mut matches = composition
        .layers
        .iter()
        .filter(|layer| layer.name.as_ref() == name);
    let layer = matches
        .next()
        .ok_or_else(|| format!("direct Path layer {name:?} is missing"))?;
    if matches.next().is_some() {
        return Err(format!("direct Path layer {name:?} is ambiguous"));
    }
    Ok(layer)
}

fn unique_composition_by_name<'items>(
    source_items: &'items HashMap<u32, &'items ProjectItem>,
    name: &str,
) -> Result<&'items Composition, String> {
    let mut matches = source_items.values().filter_map(|item| {
        if item.name == name
            && let ItemKind::Composition(composition) = &item.kind
        {
            return Some(composition.as_ref());
        }
        None
    });
    let composition = matches
        .next()
        .ok_or_else(|| format!("direct Path composition {name:?} is missing"))?;
    if matches.next().is_some() {
        return Err(format!("direct Path composition {name:?} is ambiguous"));
    }
    Ok(composition)
}

struct PathGeometry {
    initial: ShapePath,
    points: Vec<[f64; 2]>,
    closed: bool,
}

impl PathGeometry {
    fn new(initial: ShapePath) -> Result<Self, String> {
        let closed = matches!(initial.commands.last(), Some(ShapePathCommand::Close));
        let Some(ShapePathCommand::MoveTo { x, y, .. }) = initial.commands.first() else {
            return Err("dynamic Path stored outline does not begin with MoveTo".into());
        };
        let segment_end = initial.commands.len() - usize::from(closed);
        let segments = initial
            .commands
            .get(1..segment_end)
            .ok_or_else(|| "dynamic Path stored outline has no segments".to_owned())?;
        if segments.is_empty()
            || segments.iter().any(|command| {
                !matches!(
                    command,
                    ShapePathCommand::LineTo { .. } | ShapePathCommand::CubicTo { .. }
                )
            })
        {
            return Err("dynamic Path stored outline is not one finite contour".into());
        }
        let vertex_count = segments.len() + usize::from(!closed);
        let mut points = Vec::with_capacity(vertex_count);
        points.push([*x, *y]);
        for command in segments.iter().take(vertex_count - 1) {
            let (x, y) = command
                .endpoint()
                .ok_or_else(|| "dynamic Path segment has no endpoint".to_owned())?;
            points.push([x, y]);
        }
        if closed {
            let (x, y) = segments
                .last()
                .and_then(ShapePathCommand::endpoint)
                .ok_or_else(|| "dynamic Path closing segment has no endpoint".to_owned())?;
            if (x - points[0][0]).abs() > 1e-6 || (y - points[0][1]).abs() > 1e-6 {
                return Err(
                    "dynamic Path closing segment does not return to its first vertex".into(),
                );
            }
        }
        Ok(Self {
            initial,
            points,
            closed,
        })
    }

    fn deform(&self, points: &[[f64; 2]]) -> Result<ShapePath, String> {
        if points.len() != self.points.len() {
            return Err("dynamic Path fitted vertex count changed".into());
        }
        let mut result = self.initial.clone();
        let ShapePathCommand::MoveTo { x, y, .. } = &mut result.commands[0] else {
            return Err("dynamic Path stored MoveTo vanished".into());
        };
        *x = points[0][0];
        *y = points[0][1];
        let segment_count = self.points.len() - usize::from(!self.closed);
        for segment in 0..segment_count {
            let from = segment;
            let to = (segment + 1) % self.points.len();
            let from_delta = [
                points[from][0] - self.points[from][0],
                points[from][1] - self.points[from][1],
            ];
            let to_delta = [
                points[to][0] - self.points[to][0],
                points[to][1] - self.points[to][1],
            ];
            match &mut result.commands[segment + 1] {
                ShapePathCommand::LineTo { x, y, .. } => {
                    *x = points[to][0];
                    *y = points[to][1];
                }
                ShapePathCommand::CubicTo {
                    c1x,
                    c1y,
                    c2x,
                    c2y,
                    x,
                    y,
                    ..
                } => {
                    *c1x += from_delta[0];
                    *c1y += from_delta[1];
                    *c2x += to_delta[0];
                    *c2y += to_delta[1];
                    *x = points[to][0];
                    *y = points[to][1];
                }
                _ => return Err("dynamic Path contour topology changed".into()),
            }
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug)]
struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

impl Affine {
    fn layer(anchor: [f64; 2], position: [f64; 2], scale: [f64; 2], degrees: f64) -> Self {
        let radians = degrees.to_radians();
        let (sin, cos) = radians.sin_cos();
        let a = cos * scale[0];
        let b = sin * scale[0];
        let c = -sin * scale[1];
        let d = cos * scale[1];
        Self {
            a,
            b,
            c,
            d,
            tx: position[0] - a * anchor[0] - c * anchor[1],
            ty: position[1] - b * anchor[0] - d * anchor[1],
        }
    }

    fn then(self, parent: Self) -> Self {
        Self {
            a: parent.a * self.a + parent.c * self.b,
            b: parent.b * self.a + parent.d * self.b,
            c: parent.a * self.c + parent.c * self.d,
            d: parent.b * self.c + parent.d * self.d,
            tx: parent.a * self.tx + parent.c * self.ty + parent.tx,
            ty: parent.b * self.tx + parent.d * self.ty + parent.ty,
        }
    }

    fn apply(self, point: [f64; 2]) -> [f64; 2] {
        [
            self.a * point[0] + self.c * point[1] + self.tx,
            self.b * point[0] + self.d * point[1] + self.ty,
        ]
    }

    fn inverse(self) -> Option<Self> {
        let determinant = self.a * self.d - self.b * self.c;
        if !determinant.is_finite() || determinant.abs() <= f64::EPSILON {
            return None;
        }
        let a = self.d / determinant;
        let b = -self.b / determinant;
        let c = -self.c / determinant;
        let d = self.a / determinant;
        Some(Self {
            a,
            b,
            c,
            d,
            tx: -(a * self.tx + c * self.ty),
            ty: -(b * self.tx + d * self.ty),
        })
    }
}

struct TransformNode<'a> {
    layer: &'a Layer,
    properties: Vec<crate::properties::TransformProperty>,
    default_position: [f64; 2],
    default_anchor: Option<[f64; 2]>,
    anchor_scale: [f64; 2],
    position_blend: Option<position_blend::BlendPosition<'a>>,
}

impl TransformNode<'_> {
    fn anchor(&self, composition_time: f64) -> Result<[f64; 2], String> {
        if let Some(property) = self.optional("ADBE Anchor Point")? {
            return Ok([
                evaluate_numeric(property, composition_time, self.layer, 0)? * self.anchor_scale[0],
                evaluate_numeric(property, composition_time, self.layer, 1)? * self.anchor_scale[1],
            ]);
        }
        if let Some(default_anchor) = self.default_anchor {
            return Ok(default_anchor);
        }
        let flags = self.layer.record.flags();
        if flags.null_layer
            || matches!(self.layer.record.layer_type(), 3 | 4)
            || self.layer.record.source_id() == 0
        {
            return Ok([0.0, 0.0]);
        }
        Err(format!(
            "dynamic Path layer {} (type {}, source {}) lacks Anchor Point and physical source dimensions",
            self.layer.record.id(),
            self.layer.record.layer_type(),
            self.layer.record.source_id()
        ))
    }

    fn matrix(
        &self,
        composition_time: f64,
        position_override: Option<[f64; 2]>,
    ) -> Result<Affine, String> {
        let anchor = self.anchor(composition_time)?;
        let position = if let Some(position) = position_override {
            position
        } else if self
            .optional("ADBE Position")?
            .is_some_and(|property| property.dimensions_separated)
        {
            [
                self.scalar("ADBE Position_0", composition_time, 1.0)?,
                self.scalar("ADBE Position_1", composition_time, 1.0)?,
            ]
        } else if self.optional("ADBE Position")?.is_some() {
            self.vector("ADBE Position", composition_time, 1.0)?
        } else {
            // Sparse unseparated Position defaults to the composition center;
            // dormant separated leaves must not override that native default.
            self.default_position
        };
        let mut scale = if self.optional("ADBE Scale")?.is_some() {
            self.vector("ADBE Scale", composition_time, 1.0)?
        } else {
            [1.0, 1.0]
        };
        if let Some(property) = self.optional(control_links::SCALE_X)? {
            scale[0] = evaluate_numeric(property, composition_time, self.layer, 0)?;
        }
        if let Some(property) = self.optional(control_links::SCALE_Y)? {
            scale[1] = evaluate_numeric(property, composition_time, self.layer, 0)?;
        }
        let rotation = if self.optional("ADBE Rotate Z")?.is_some() {
            self.scalar("ADBE Rotate Z", composition_time, 1.0)?
        } else {
            0.0
        };
        if [
            anchor[0],
            anchor[1],
            position[0],
            position[1],
            scale[0],
            scale[1],
            rotation,
        ]
        .into_iter()
        .any(|value| !value.is_finite())
        {
            return Err(format!(
                "layer {} has a non-finite 2D transform",
                self.layer.record.id()
            ));
        }
        Ok(Affine::layer(anchor, position, scale, rotation))
    }

    fn property(&self, name: &str) -> Result<&NumericProperty, String> {
        self.optional(name)?
            .ok_or_else(|| format!("layer {} lacks {name}", self.layer.record.id()))
    }

    fn optional(&self, name: &str) -> Result<Option<&NumericProperty>, String> {
        self.properties
            .iter()
            .find(|property| property.match_name == name)
            .map(|property| property.numeric.as_ref().map_err(|error| error.to_string()))
            .transpose()
    }

    fn scalar(&self, name: &str, composition_time: f64, unit: f64) -> Result<f64, String> {
        Ok(evaluate_numeric(self.property(name)?, composition_time, self.layer, 0)? * unit)
    }

    fn vector(&self, name: &str, composition_time: f64, unit: f64) -> Result<[f64; 2], String> {
        let property = self.property(name)?;
        Ok([
            evaluate_numeric(property, composition_time, self.layer, 0)? * unit,
            evaluate_numeric(property, composition_time, self.layer, 1)? * unit,
        ])
    }
}

struct TransformRig<'composition, 'items> {
    composition: &'composition Composition,
    source_items: &'items HashMap<u32, &'items ProjectItem>,
    nodes: BTreeMap<u32, TransformNode<'composition>>,
}

impl<'composition, 'items> TransformRig<'composition, 'items> {
    fn new(
        composition: &'composition Composition,
        source_items: &'items HashMap<u32, &'items ProjectItem>,
    ) -> Self {
        Self {
            composition,
            source_items,
            nodes: BTreeMap::new(),
        }
    }

    fn add(&mut self, id: u32) -> Result<(), String> {
        self.add_inner(id, &mut Vec::new())
    }

    fn add_inner(&mut self, id: u32, visiting: &mut Vec<u32>) -> Result<(), String> {
        if self.nodes.contains_key(&id) {
            return Ok(());
        }
        if visiting.len() >= MAX_PARENT_DEPTH {
            return Err("dynamic Path transform-parent chain is too deep".into());
        }
        if visiting.contains(&id) {
            return Err(format!("dynamic Path transform-parent cycle at layer {id}"));
        }
        let mut matches = self
            .composition
            .layers
            .iter()
            .filter(|layer| layer.record.id() == id);
        let layer = matches
            .next()
            .ok_or_else(|| format!("dynamic Path layer {id} is missing"))?;
        if matches.next().is_some() {
            return Err(format!("dynamic Path layer {id} is ambiguous"));
        }
        let flags = layer.record.flags();
        if flags.auto_orient_along_path {
            return Err(format!(
                "dynamic Path layer {id} uses unsupported auto-orient along path"
            ));
        }
        if flags.three_d_layer {
            return Err(format!("dynamic Path layer {id} is 3D"));
        }
        let (properties, warnings) = control_links::read_layer_transform(layer, self.composition)
            .map_err(|error| format!("layer {id} Transform: {error}"))?;
        for property in &properties {
            let Ok(numeric) = &property.numeric else {
                continue;
            };
            // Separated Position retains dormant combined spatial keys in AEP.
            // The affine model consumes only the independent scalar axes.
            if property.match_name == "ADBE Position" && numeric.dimensions_separated {
                continue;
            }
            for (key_index, key) in numeric.keyframes.iter().enumerate() {
                if has_nonzero_spatial_tangent(key) {
                    return Err(format!(
                        "layer {id} Transform {} key {key_index} at {}s has unsupported nonzero spatial tangents",
                        property.match_name, key.time_secs
                    ));
                }
            }
        }
        let source = self.source_items.get(&layer.record.source_id()).copied();
        let anchor_dimensions = source_anchor_dimensions(source, layer);
        let anchor_scale = solid_anchor_scale(source, anchor_dimensions);
        let flags = layer.record.flags();
        let default_anchor = (!flags.null_layer
            && !matches!(layer.record.layer_type(), 3 | 4)
            && !anchor_dimensions.contains(&0))
        .then(|| {
            [
                f64::from(anchor_dimensions[0]) / 2.0,
                f64::from(anchor_dimensions[1]) / 2.0,
            ]
        });
        let position_blend = position_blend::resolve(layer, self.composition).transpose()?;
        if properties.iter().any(|property| {
            !(property.match_name == "ADBE Position" && position_blend.is_some())
                && property
                    .numeric
                    .as_ref()
                    .is_ok_and(|numeric| numeric.expression_enabled)
        }) {
            return Err(format!("layer {id} has an unresolved Transform expression"));
        }
        if warnings
            .iter()
            .any(|warning| warning.contains("not lowered"))
        {
            return Err(format!(
                "layer {id} Transform is not mathematically resolved: {}",
                warnings.join("; ")
            ));
        }
        visiting.push(id);
        let parent = layer.record.parent_id();
        if parent != 0 {
            self.add_inner(parent, visiting)?;
        }
        if let Some(blend) = &position_blend {
            for dependency in blend.dependencies() {
                self.add_inner(dependency, visiting)?;
            }
        }
        visiting.pop();
        self.nodes.insert(
            id,
            TransformNode {
                layer,
                properties,
                default_position: [
                    f64::from(self.composition.width) / 2.0,
                    f64::from(self.composition.height) / 2.0,
                ],
                default_anchor,
                anchor_scale,
                position_blend,
            },
        );
        Ok(())
    }

    fn matrix(&self, id: u32, composition_time: f64) -> Result<Affine, String> {
        self.matrix_inner(id, composition_time, 0, &mut BTreeMap::new())
    }

    fn matrix_inner(
        &self,
        id: u32,
        composition_time: f64,
        depth: usize,
        cache: &mut BTreeMap<u32, Affine>,
    ) -> Result<Affine, String> {
        if let Some(matrix) = cache.get(&id) {
            return Ok(*matrix);
        }
        if depth >= MAX_PARENT_DEPTH {
            return Err("dynamic Path transform-parent chain is too deep".into());
        }
        let node = self
            .nodes
            .get(&id)
            .ok_or_else(|| format!("dynamic Path transform layer {id} is missing"))?;
        let position = if let Some(blend) = &node.position_blend {
            let [first, second] = blend.dependencies();
            Some(
                blend.evaluate(
                    composition_time,
                    [
                        self.matrix_inner(first, composition_time, depth + 1, cache)?
                            .apply([0.0, 0.0]),
                        self.matrix_inner(second, composition_time, depth + 1, cache)?
                            .apply([0.0, 0.0]),
                    ],
                )?,
            )
        } else {
            None
        };
        let local = node.matrix(composition_time, position)?;
        let parent = node.layer.record.parent_id();
        let matrix = if parent == 0 {
            local
        } else {
            local.then(self.matrix_inner(parent, composition_time, depth + 1, cache)?)
        };
        cache.insert(id, matrix);
        Ok(matrix)
    }

    fn seed_owner_times(
        &self,
        owner: &Layer,
        start_ms: i64,
        end_ms: i64,
        output: &mut BTreeSet<i64>,
    ) -> Result<(), String> {
        let (owner_start, owner_stretch) = valid_clock(owner)?;
        for node in self.nodes.values() {
            let (start, stretch) = valid_clock(node.layer)?;
            if let Some(blend) = &node.position_blend {
                let mut local_times = BTreeSet::new();
                blend.seed_owner_times(i64::MIN, i64::MAX, &mut local_times)?;
                for local in local_times {
                    let millis = seconds_to_millis(
                        (start + local as f64 / 1000.0 * stretch - owner_start) / owner_stretch,
                    )?;
                    if (start_ms..=end_ms).contains(&millis) {
                        insert_seed(output, millis)?;
                    }
                }
            }
            for key in node
                .properties
                .iter()
                .filter_map(|property| property.numeric.as_ref().ok())
                .flat_map(|numeric| &numeric.keyframes)
            {
                let composition_time = start + key.time_secs * stretch;
                let owner_time = (composition_time - owner_start) / owner_stretch;
                let millis = seconds_to_millis(owner_time)?;
                if (start_ms..=end_ms).contains(&millis) {
                    insert_seed(output, millis)?;
                }
            }
        }
        Ok(())
    }
}

fn insert_seed(output: &mut BTreeSet<i64>, time: i64) -> Result<(), String> {
    output.insert(time);
    if output.len() > MAX_EVALUATIONS {
        return Err("dynamic Path source keys exceed the analytical evaluation bound".into());
    }
    Ok(())
}

fn evaluate_numeric(
    property: &NumericProperty,
    composition_time: f64,
    layer: &Layer,
    component: usize,
) -> Result<f64, String> {
    if property.expression_enabled {
        return Err("unresolved Transform expression".into());
    }
    if property.keyframes.is_empty() {
        if property.animated {
            return Err("animated Transform property has no supported keys".into());
        }
        return property
            .values
            .get(component)
            .or_else(|| property.values.first())
            .copied()
            .filter(|value| value.is_finite())
            .ok_or_else(|| "Transform property lacks a finite component".into());
    }
    let (start, stretch) = valid_clock(layer)?;
    let local = (composition_time - start) / stretch;
    let keys = &property.keyframes;
    if local <= keys[0].time_secs {
        return key_component(&keys[0], component);
    }
    let index = keys.partition_point(|key| key.time_secs <= local) - 1;
    if index + 1 == keys.len() {
        return key_component(&keys[index], component);
    }
    let from = &keys[index];
    let to = &keys[index + 1];
    let duration = to.time_secs - from.time_secs;
    if !duration.is_finite() || duration <= 0.0 {
        return Err("Transform keys are not strictly increasing".into());
    }
    let linear = (local - from.time_secs) / duration;
    let spatial = !from.spatial_in.is_empty()
        || !from.spatial_out.is_empty()
        || !to.spatial_in.is_empty()
        || !to.spatial_out.is_empty();
    if spatial && (has_nonzero_spatial_tangent(from) || has_nonzero_spatial_tangent(to)) {
        return Err(format!(
            "layer {} Transform component {component} has unsupported nonzero spatial tangents between keys at {}s and {}s",
            layer.record.id(),
            from.time_secs,
            to.time_secs
        ));
    }
    if !spatial && key_component(from, component)? == key_component(to, component)? {
        return equal_endpoint_value(from, to, component, duration, linear);
    }
    let progress = native_progress(from, to, component, duration, linear)?;
    let from_value = key_component(from, component)?;
    let value = from_value + (key_component(to, component)? - from_value) * progress;
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| "Transform interpolation produced a non-finite value".into())
}

fn key_component(key: &NumericKeyframe, component: usize) -> Result<f64, String> {
    key.values
        .get(component)
        .or_else(|| key.values.first())
        .copied()
        .filter(|value| value.is_finite())
        .ok_or_else(|| "Transform key lacks a finite component".into())
}

fn has_nonzero_spatial_tangent(key: &NumericKeyframe) -> bool {
    key.spatial_in
        .iter()
        .chain(&key.spatial_out)
        .any(|value| *value != 0.0)
}

// Equal scalar endpoints can still overshoot: normalized progress would divide
// by zero and erase the excursion. Evaluate their absolute-value handles instead.
fn equal_endpoint_value(
    from: &NumericKeyframe,
    to: &NumericKeyframe,
    component: usize,
    duration: f64,
    progress: f64,
) -> Result<f64, String> {
    let value = key_component(from, component)?;
    if from.out_interpolation == 3 || (from.out_interpolation == 1 && to.in_interpolation == 1) {
        return Ok(value);
    }
    if !matches!(from.out_interpolation, 1 | 2) || !matches!(to.in_interpolation, 1 | 2) {
        return Err("unknown Transform temporal interpolation".into());
    }
    let temporal = |values: &[f64]| {
        values
            .get(component)
            .or_else(|| values.first())
            .copied()
            .filter(|value| value.is_finite())
            .ok_or_else(|| "incomplete scalar temporal metadata".to_owned())
    };
    let x1 = temporal(&from.out_influence)? / 100.0;
    let x2 = 1.0 - temporal(&to.in_influence)? / 100.0;
    if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
        return Err("scalar temporal influence is outside 0..100".into());
    }
    let first = if from.out_interpolation == 1 {
        value
    } else {
        value + temporal(&from.out_speed)? * duration * x1
    };
    let second = if to.in_interpolation == 1 {
        value
    } else {
        value - temporal(&to.in_speed)? * duration * (1.0 - x2)
    };
    let result = cubic(
        cubic_bezier_parameter(progress, x1, x2),
        value,
        first,
        second,
        value,
    );
    result
        .is_finite()
        .then_some(result)
        .ok_or_else(|| "non-finite scalar temporal interpolation".into())
}

fn native_progress(
    from: &NumericKeyframe,
    to: &NumericKeyframe,
    component: usize,
    duration: f64,
    progress: f64,
) -> Result<f64, String> {
    if from.out_interpolation == 3 {
        return Ok(0.0);
    }
    if from.out_interpolation == 1 && to.in_interpolation == 1 {
        return Ok(progress);
    }
    if !matches!(from.out_interpolation, 1 | 2) || !matches!(to.in_interpolation, 1 | 2) {
        return Err("unknown Transform temporal interpolation".into());
    }
    let spatial = !from.spatial_in.is_empty()
        || !from.spatial_out.is_empty()
        || !to.spatial_in.is_empty()
        || !to.spatial_out.is_empty();
    let temporal = |values: &[f64]| {
        if spatial {
            values.first().copied()
        } else {
            values.get(component).or_else(|| values.first()).copied()
        }
    };
    let out_influence = temporal(&from.out_influence)
        .ok_or_else(|| "incomplete Transform outgoing influence".to_owned())?;
    let in_influence = temporal(&to.in_influence)
        .ok_or_else(|| "incomplete Transform incoming influence".to_owned())?;
    let out_speed = temporal(&from.out_speed)
        .ok_or_else(|| "incomplete Transform outgoing speed".to_owned())?;
    let in_speed =
        temporal(&to.in_speed).ok_or_else(|| "incomplete Transform incoming speed".to_owned())?;
    let delta = key_component(to, component)? - key_component(from, component)?;
    let distance = if spatial {
        to.values
            .iter()
            .zip(&from.values)
            .map(|(to, from)| (to - from).powi(2))
            .sum::<f64>()
            .sqrt()
    } else {
        delta.abs()
    };
    if distance <= f64::EPSILON {
        if out_speed.abs() > f64::EPSILON || in_speed.abs() > f64::EPSILON {
            return Err("equal Transform endpoints have nonzero temporal speed".into());
        }
        return Ok(progress);
    }
    let x1 = (out_influence / 100.0).clamp(0.0, 1.0);
    let x2 = 1.0 - (in_influence / 100.0).clamp(0.0, 1.0);
    let normalization = if spatial { distance } else { delta };
    let y1 = if from.out_interpolation == 1 {
        x1
    } else {
        out_speed * duration / normalization * x1
    };
    let y2 = if to.in_interpolation == 1 {
        x2
    } else {
        1.0 - in_speed * duration / normalization * (1.0 - x2)
    };
    if [x1, y1, x2, y2].into_iter().any(|value| !value.is_finite()) {
        return Err("non-finite Transform temporal ease".into());
    }
    Ok(cubic_bezier_progress(progress, x1, y1, x2, y2))
}

fn cubic_bezier_progress(progress: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    cubic(cubic_bezier_parameter(progress, x1, x2), 0.0, y1, y2, 1.0)
}

fn cubic_bezier_parameter(progress: f64, x1: f64, x2: f64) -> f64 {
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..24 {
        let middle = (low + high) * 0.5;
        if cubic(middle, 0.0, x1, x2, 1.0) < progress {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) * 0.5
}

fn cubic(t: f64, p0: f64, p1: f64, p2: f64, p3: f64) -> f64 {
    let one_minus = 1.0 - t;
    one_minus.powi(3) * p0
        + 3.0 * one_minus.powi(2) * t * p1
        + 3.0 * one_minus * t.powi(2) * p2
        + t.powi(3) * p3
}

fn owner_interval_ms(owner: &Layer) -> Result<(i64, i64), String> {
    let start = owner
        .record
        .in_point()
        .filter(|value| value.is_finite())
        .ok_or_else(|| "dynamic Path owner has an invalid in-point".to_owned())?;
    let end = owner
        .record
        .out_point()
        .filter(|value| value.is_finite())
        .ok_or_else(|| "dynamic Path owner has an invalid out-point".to_owned())?;
    let start_ms = (start * 1000.0).ceil() as i64;
    let end_ms = (end * 1000.0).floor() as i64;
    if end_ms <= start_ms || end_ms - start_ms > MAX_DURATION_MS {
        return Err(format!(
            "dynamic Path owner interval must be positive and at most {} seconds",
            MAX_DURATION_MS / 1000
        ));
    }
    Ok((start_ms, end_ms))
}

fn valid_clock(layer: &Layer) -> Result<(f64, f64), String> {
    let start = layer
        .record
        .start_time()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("layer {} has an invalid start time", layer.record.id()))?;
    let stretch = layer
        .record
        .stretch()
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| format!("layer {} has unsupported stretch", layer.record.id()))?;
    Ok((start, stretch))
}

fn owner_local_to_comp(owner: &Layer, local: f64) -> Result<f64, String> {
    let (start, stretch) = valid_clock(owner)?;
    let result = start + local * stretch;
    result
        .is_finite()
        .then_some(result)
        .ok_or_else(|| "dynamic Path owner clock produced non-finite time".into())
}

fn seconds_to_millis(seconds: f64) -> Result<i64, String> {
    let millis = (seconds * 1000.0).round();
    if !millis.is_finite() || millis.abs() > i64::MAX as f64 {
        return Err("dynamic Path time is outside the FX clock".into());
    }
    Ok(millis as i64)
}

fn interpolate_points(from: &[[f64; 2]], to: &[[f64; 2]], progress: f64) -> Vec<[f64; 2]> {
    from.iter()
        .zip(to)
        .map(|(from, to)| {
            [
                from[0] + (to[0] - from[0]) * progress,
                from[1] + (to[1] - from[1]) * progress,
            ]
        })
        .collect()
}

fn point_error(left: &[[f64; 2]], right: &[[f64; 2]]) -> f64 {
    if left.len() != right.len() {
        return f64::INFINITY;
    }
    left.iter()
        .zip(right)
        .flat_map(|(left, right)| [(left[0] - right[0]).abs(), (left[1] - right[1]).abs()])
        .fold(0.0, f64::max)
}

// Generated samples need not retain floating-point arithmetic noise in JSON.
// Round before fitting/byte accounting, never after validation: the unchanged
// analytical evaluator still checks the emitted endpoints on the full 1ms grid.
// The explicit displacement guard also protects very large finite coordinates.
fn compact_fitted_coordinates(points: &mut [[f64; 2]]) {
    for coordinate in points.iter_mut().flatten() {
        let rounded = (*coordinate * 10_000.0).round() / 10_000.0;
        if rounded.is_finite() && (*coordinate - rounded).abs() <= 0.0001 {
            *coordinate = rounded;
        }
    }
}

type PathValidationSample = (i64, Vec<[f64; 2]>);

fn validation_splits(
    fitted: &BTreeMap<i64, Vec<[f64; 2]>>,
    evaluate: &impl Fn(i64, &mut usize) -> Result<Vec<[f64; 2]>, String>,
    evaluations: &mut usize,
) -> Result<Vec<PathValidationSample>, String> {
    let times: Vec<_> = fitted.keys().copied().collect();
    let mut splits = Vec::new();
    for pair in times.windows(2) {
        let [from_time, to_time] = [pair[0], pair[1]];
        let from = fitted
            .get(&from_time)
            .ok_or_else(|| "dynamic Path validation start vanished".to_owned())?;
        let to = fitted
            .get(&to_time)
            .ok_or_else(|| "dynamic Path validation end vanished".to_owned())?;
        let mut worst: Option<(f64, i64, Vec<[f64; 2]>)> = None;
        for time in from_time + 1..to_time {
            let progress = (time - from_time) as f64 / (to_time - from_time) as f64;
            let expected = interpolate_points(from, to, progress);
            let actual = evaluate(time, evaluations)?;
            let error = point_error(&actual, &expected);
            if error > worst.as_ref().map_or(FIT_TOLERANCE_PIXELS, |entry| entry.0) {
                worst = Some((error, time, actual));
            }
        }
        if let Some((_, time, actual)) = worst {
            splits.push((time, actual));
        }
    }
    Ok(splits)
}
