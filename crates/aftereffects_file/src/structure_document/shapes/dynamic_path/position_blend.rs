//! Bounded analytical lowering for the stock Position blend used by the Intro
//! fold controllers. This recognizes one complete expression profile; it is not
//! an After Effects expression evaluator.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use crate::{
    properties::{self, NumericProperty, NumericValueKind},
    rifx::Chunk,
    structure::{Composition, Layer},
};

use super::{evaluate_numeric, seconds_to_millis, take_quoted, valid_clock};

/// A resolved `value + linear(Slider, 0, 100, a, b)` Position profile.
///
/// The parent transform rig owns dependency traversal and supplies the two
/// dependency origins in composition space. Keeping that traversal outside this
/// type lets the rig use its existing transform cache and cycle checks.
pub(super) struct BlendPosition<'a> {
    owner: &'a Layer,
    base: NumericProperty,
    controller: &'a Layer,
    slider: NumericProperty,
    slider_offset: Option<NumericProperty>,
    dependencies: [u32; 2],
}

impl BlendPosition<'_> {
    /// Layer IDs whose composition-space origins are the blend endpoints.
    pub(super) fn dependencies(&self) -> [u32; 2] {
        self.dependencies
    }

    /// Evaluates the owner-local Position after the transform rig has resolved
    /// both `toComp([0,0,0])` dependency points.
    pub(super) fn evaluate(
        &self,
        composition_time: f64,
        endpoint_origins: [[f64; 2]; 2],
    ) -> Result<[f64; 2], String> {
        if !composition_time.is_finite()
            || endpoint_origins
                .iter()
                .flatten()
                .any(|value| !value.is_finite())
        {
            return Err("Position blend received non-finite evaluation input".into());
        }
        let base = [
            evaluate_numeric(&self.base, composition_time, self.owner, 0)?,
            evaluate_numeric(&self.base, composition_time, self.owner, 1)?,
        ];
        let mut slider = evaluate_numeric(&self.slider, composition_time, self.controller, 0)?;
        if let Some(offset) = &self.slider_offset {
            slider -= evaluate_numeric(offset, composition_time, self.controller, 0)?;
        }
        if !slider.is_finite() {
            return Err("Position blend Slider produced a non-finite value".into());
        }
        // AE's five-argument linear() clamps outside the input range.
        let alpha = (slider / 100.0).clamp(0.0, 1.0);
        let result = std::array::from_fn(|component| {
            base[component]
                + endpoint_origins[0][component]
                + (endpoint_origins[1][component] - endpoint_origins[0][component]) * alpha
        });
        result
            .iter()
            .all(|value| value.is_finite())
            .then_some(result)
            .ok_or_else(|| "Position blend produced a non-finite Position".into())
    }

    /// Adds authored Position and Slider key times to the owner's sparse-fit
    /// seed set. Endpoint-transform keys are added separately by `TransformRig`.
    pub(super) fn seed_owner_times(
        &self,
        start_ms: i64,
        end_ms: i64,
        output: &mut BTreeSet<i64>,
    ) -> Result<(), String> {
        seed_curve_times(self.owner, &self.base, self.owner, start_ms, end_ms, output)?;
        seed_curve_times(
            self.controller,
            &self.slider,
            self.owner,
            start_ms,
            end_ms,
            output,
        )?;
        if let Some(offset) = &self.slider_offset {
            seed_curve_times(
                self.controller,
                offset,
                self.owner,
                start_ms,
                end_ms,
                output,
            )?;
        }
        Ok(())
    }
}

/// Resolves the exact stock combined-Position expression. `None` means that the
/// owner does not use this profile; a recognized but unsafe profile reports an
/// error rather than silently broadening expression support.
pub(super) fn resolve<'a>(
    owner: &'a Layer,
    composition: &'a Composition,
) -> Option<Result<BlendPosition<'a>, String>> {
    let body = combined_position_body(owner).ok()?;
    let expression = expression(body).ok()?;
    let parsed = parse(expression)?;
    Some(resolve_parsed(owner, composition, body, parsed))
}

struct Parsed {
    endpoints: [String; 2],
    controller: String,
    effect: String,
}

fn resolve_parsed<'a>(
    owner: &'a Layer,
    composition: &'a Composition,
    body: &[Chunk],
    parsed: Parsed,
) -> Result<BlendPosition<'a>, String> {
    if owner.record.flags().three_d_layer {
        return Err("Position blend owner is 3D".into());
    }
    if owner.record.parent_id() != 0 {
        return Err("Position blend owner must be unparented so expression output remains in composition space".into());
    }
    valid_clock(owner)?;

    let mut base = properties::read_numeric(body)
        .map_err(|error| format!("Position blend base cannot be decoded: {error}"))?;
    if !base.expression_enabled || base.dimensions_separated {
        return Err("Position blend requires one enabled combined Position expression".into());
    }
    base.expression_enabled = false;
    base.expression_present = false;
    validate_curve(owner, &base, 2, false, "Position blend base")?;

    let first = unique_layer(composition, &parsed.endpoints[0], "first Position endpoint")?;
    let second = unique_layer(
        composition,
        &parsed.endpoints[1],
        "second Position endpoint",
    )?;
    let dependencies = [first.record.id(), second.record.id()];
    if dependencies[0] == dependencies[1] {
        return Err("Position blend endpoints must resolve to two distinct layers".into());
    }
    if dependencies.contains(&owner.record.id()) {
        return Err("Position blend cannot reference its own transform".into());
    }
    for endpoint in [first, second] {
        if endpoint.record.flags().three_d_layer {
            return Err(format!(
                "Position blend endpoint {} is 3D",
                endpoint.record.id()
            ));
        }
        valid_clock(endpoint)?;
    }

    let controller = unique_layer(composition, &parsed.controller, "Slider controller")?;
    valid_clock(controller)?;
    let (slider, slider_offset) = resolve_slider(controller, &parsed.effect)?;

    Ok(BlendPosition {
        owner,
        base,
        controller,
        slider,
        slider_offset,
        dependencies,
    })
}

fn parse(text: &str) -> Option<Parsed> {
    let compact = compact(text)?;
    let mut text = compact.as_str();
    let first = assignment(&mut text, "a=", ").toComp([0,0,0]);")?;
    let second = assignment(&mut text, "b=", ").toComp([0,0,0]);")?;
    text = text.strip_prefix("s=thisComp.layer(")?;
    let (controller, rest) = take_quoted(text)?;
    text = rest.strip_prefix(").effect(")?;
    let (effect, rest) = take_quoted(text)?;
    text = rest.strip_prefix(")(")?;
    let (parameter, rest) = take_quoted(text)?;
    text = rest.strip_prefix(");")?;
    if parameter != "Slider"
        || text.strip_suffix(';').unwrap_or(text) != "value+linear(s,0,100,a,b)"
    {
        return None;
    }
    reference_name(first)?;
    reference_name(second)?;
    reference_name(controller)?;
    reference_name(effect)?;
    Some(Parsed {
        endpoints: [first.to_owned(), second.to_owned()],
        controller: controller.to_owned(),
        effect: effect.to_owned(),
    })
}

fn assignment<'a>(text: &mut &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    *text = text.strip_prefix(prefix)?.strip_prefix("thisComp.layer(")?;
    let (name, rest) = take_quoted(text)?;
    *text = rest.strip_prefix(suffix)?;
    Some(name)
}

fn compact(text: &str) -> Option<String> {
    if !text.is_ascii() {
        return None;
    }
    let mut output = String::with_capacity(text.len());
    let mut quoted = None;
    for character in text.chars() {
        if let Some(quote) = quoted {
            output.push(character);
            if character == '\\' {
                // Escaped names are deliberately outside this stock profile.
                return None;
            }
            if character == quote {
                quoted = None;
            }
        } else if matches!(character, '\'' | '"') {
            quoted = Some(character);
            output.push(character);
        } else if !character.is_whitespace() {
            output.push(character);
        }
    }
    (quoted.is_none() && !output.contains(['/', '`'])).then_some(output)
}

fn reference_name(name: &str) -> Option<()> {
    (!name.is_empty()).then_some(())
}

fn combined_position_body(layer: &Layer) -> Result<&[Chunk], String> {
    let root = properties::root_runs(&layer.content).map_err(|error| error.to_string())?;
    let transform = unique_run(&root, "ADBE Transform Group", "Transform group")?;
    let leaves = properties::runs(
        properties::unique_list(transform, *b"tdgp").map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let position = unique_run(&leaves, "ADBE Position", "combined Position")?;
    properties::unique_list(position, *b"tdbs").map_err(|error| error.to_string())
}

fn expression(body: &[Chunk]) -> Result<&str, String> {
    std::str::from_utf8(properties::data(body, *b"Utf8").map_err(|error| error.to_string())?)
        .map_err(|_| "Position blend expression is not UTF-8".into())
}

fn unique_layer<'a>(
    composition: &'a Composition,
    name: &str,
    role: &str,
) -> Result<&'a Layer, String> {
    let mut matches = composition
        .layers
        .iter()
        .filter(|candidate| candidate.name.as_ref() == name);
    let layer = matches
        .next()
        .ok_or_else(|| format!("{role} layer {name:?} is missing"))?;
    if matches.next().is_some() {
        return Err(format!("{role} layer {name:?} is ambiguous"));
    }
    Ok(layer)
}

fn resolve_slider(
    controller: &Layer,
    effect_name: &str,
) -> Result<(NumericProperty, Option<NumericProperty>), String> {
    let mut slider = slider_property(controller, effect_name)?;
    if !slider.expression_enabled {
        validate_curve(controller, &slider, 1, true, "Position blend Slider")?;
        return Ok((slider, None));
    }

    let body = slider_body(controller, effect_name)?;
    let alias = parse_value_minus_slider(expression(body)?)
        .ok_or_else(|| "Position blend Slider expression is not the exact same-layer value-minus-Slider profile".to_owned())?;
    if alias == effect_name {
        return Err("Position blend Slider alias references itself".into());
    }
    let offset = slider_property(controller, &alias)?;
    if offset.expression_enabled {
        return Err("Position blend Slider alias target has an enabled expression".into());
    }
    slider.expression_enabled = false;
    slider.expression_present = false;
    validate_curve(controller, &slider, 1, true, "Position blend Slider value")?;
    validate_curve(controller, &offset, 1, true, "Position blend Slider offset")?;
    Ok((slider, Some(offset)))
}

fn parse_value_minus_slider(text: &str) -> Option<String> {
    let compact = compact(text)?;
    let mut text = compact.as_str().strip_prefix("value-effect(")?;
    let (effect, rest) = take_quoted(text)?;
    text = rest.strip_prefix(")(")?;
    let (parameter, rest) = take_quoted(text)?;
    let rest = rest.strip_suffix(';').unwrap_or(rest);
    (parameter == "Slider" && rest == ")" && reference_name(effect).is_some())
        .then(|| effect.to_owned())
}

fn slider_property(controller: &Layer, effect_name: &str) -> Result<NumericProperty, String> {
    properties::read_numeric(slider_body(controller, effect_name)?).map_err(|error| {
        format!("Position blend Slider {effect_name:?} cannot be decoded: {error}")
    })
}

fn slider_body<'a>(controller: &'a Layer, effect_name: &str) -> Result<&'a [Chunk], String> {
    let root = properties::root_runs(&controller.content).map_err(|error| error.to_string())?;
    let parade = unique_run(&root, "ADBE Effect Parade", "Effect Parade")?;
    let effects = properties::runs(
        properties::unique_list(parade, *b"tdgp").map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut matches = effects.iter().filter_map(|(kind, run)| {
        let plugin = properties::unique_list(run, *b"sspc").ok()?;
        let body = properties::unique_list(plugin, *b"tdgp").ok()?;
        (display_name(body) == Some(effect_name)).then_some((*kind, body))
    });
    let (kind, effect) = matches
        .next()
        .ok_or_else(|| format!("Position blend Slider effect {effect_name:?} is missing"))?;
    if matches.next().is_some() || kind != "ADBE Slider Control" {
        return Err(format!(
            "Position blend Slider effect {effect_name:?} is ambiguous or has the wrong kind"
        ));
    }
    let parameters = properties::runs(effect).map_err(|error| error.to_string())?;
    let value = unique_run(
        &parameters,
        "ADBE Slider Control-0001",
        "Slider value parameter",
    )?;
    properties::unique_list(value, *b"tdbs").map_err(|error| error.to_string())
}

fn unique_run<'a>(
    runs: &[(&str, &'a [Chunk])],
    name: &str,
    role: &str,
) -> Result<&'a [Chunk], String> {
    let mut matches = runs.iter().filter(|(candidate, _)| *candidate == name);
    let (_, run) = matches
        .next()
        .ok_or_else(|| format!("{role} {name:?} is missing"))?;
    if matches.next().is_some() {
        return Err(format!("{role} {name:?} is ambiguous"));
    }
    Ok(run)
}

fn display_name(chunks: &[Chunk]) -> Option<&str> {
    let bytes = properties::data(chunks, *b"tdsn").ok()?;
    if bytes.get(..4)? != b"Utf8" {
        return None;
    }
    let length = usize::try_from(u32::from_be_bytes(bytes.get(4..8)?.try_into().ok()?)).ok()?;
    let end = 8_usize.checked_add(length)?;
    let padding = bytes.get(end..)?;
    if padding.len() > 3 || padding.iter().any(|byte| *byte != 0) {
        return None;
    }
    std::str::from_utf8(bytes.get(8..end)?).ok()
}

fn validate_curve(
    layer: &Layer,
    property: &NumericProperty,
    components: usize,
    scalar: bool,
    label: &str,
) -> Result<(), String> {
    if property.expression_enabled
        || property.dimensions_separated
        || property.value_kind == NumericValueKind::Color
        || (property.animated && property.keyframes.is_empty())
        || (!property.animated && property.values.len() < components)
        || (!property.values.is_empty() && property.values.len() < components)
    {
        return Err(format!("{label} has an unsupported numeric layout"));
    }
    if scalar && property.values.len() > 1 {
        return Err(format!("{label} is not scalar"));
    }
    if property
        .values
        .iter()
        .chain(property.keyframes.iter().flat_map(key_numbers))
        .any(|value| !value.is_finite())
    {
        return Err(format!("{label} contains a non-finite number"));
    }
    if property.keyframes.iter().any(|key| {
        key.values.len() < components
            || (scalar
                && (key.values.len() != 1
                    || !key.spatial_in.is_empty()
                    || !key.spatial_out.is_empty()))
    }) {
        return Err(format!("{label} has unsupported key dimensions"));
    }
    let (start, stretch) = valid_clock(layer)?;
    for pair in property.keyframes.windows(2) {
        let [from, to] = pair else { continue };
        if to.time_secs <= from.time_secs {
            return Err(format!("{label} keys are not strictly increasing"));
        }
        let local = from.time_secs + (to.time_secs - from.time_secs) * 0.5;
        let composition_time = start + local * stretch;
        for component in 0..components {
            evaluate_numeric(property, composition_time, layer, component)
                .map_err(|error| format!("{label} has unsupported interpolation: {error}"))?;
        }
    }
    Ok(())
}

fn key_numbers(key: &crate::properties::NumericKeyframe) -> impl Iterator<Item = &f64> {
    std::iter::once(&key.time_secs)
        .chain(&key.values)
        .chain(&key.in_speed)
        .chain(&key.in_influence)
        .chain(&key.out_speed)
        .chain(&key.out_influence)
        .chain(&key.spatial_in)
        .chain(&key.spatial_out)
}

fn seed_curve_times(
    source: &Layer,
    property: &NumericProperty,
    owner: &Layer,
    start_ms: i64,
    end_ms: i64,
    output: &mut BTreeSet<i64>,
) -> Result<(), String> {
    let (source_start, source_stretch) = valid_clock(source)?;
    let (owner_start, owner_stretch) = valid_clock(owner)?;
    for key in &property.keyframes {
        let composition_time = source_start + key.time_secs * source_stretch;
        let owner_time = (composition_time - owner_start) / owner_stretch;
        let millis = seconds_to_millis(owner_time)?;
        if (start_ms..=end_ms).contains(&millis) {
            super::insert_seed(output, millis)?;
        }
    }
    Ok(())
}
