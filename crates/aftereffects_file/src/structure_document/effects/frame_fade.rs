//! The `Fade In+Out - frames` preset: a pseudo-effect controller whose frame
//! counts drive a Solid Composite Source Opacity expression. Its fade-in-only
//! profile lowers to editable owner Opacity keys; no expression is evaluated.
//! The Composite multiplies the image of the effects before it, so FX owner
//! opacity is equivalent only while no enabled effect after it renders.

use std::collections::HashSet;

use crate::{
    properties::{self, NumericKeyframe, NumericProperty, NumericValueKind},
    rifx::Chunk,
    structure::Layer,
    structure_document::control_links::{effect_instance_name, unique_run},
};

const CONTROLLER: &str = "ADBE CM FadeInOutFrames";
const COMPOSITE: &str = "ADBE Solid Composite";
/// The expression binds the controller and its controls by these names.
const CONTROLLER_NAME: &str = "Fade In+Out - frames";
const FADE_IN: &str = "Fade In Duration (frames)";
const FADE_OUT: &str = "Fade Out Duration (frames)";
const FORMULA: &str = r#"
var fadeInDuration = framesToTime(effect("Fade In+Out - frames")("Fade In Duration (frames)"));
var fadeOutDuration = framesToTime(effect("Fade In+Out - frames")("Fade Out Duration (frames)"));
var fadeInOpacity = fadeInDuration ? linear(time, inPoint, inPoint + fadeInDuration, 0, value) : value;
var fadeOutOpacity = fadeOutDuration ? linear(time, outPoint - fadeOutDuration, outPoint, value, 0) : value;
fadeInOpacity + fadeOutOpacity - value;
"#;
/// Solid Composite's native Blending Mode menu; value 1 is Normal.
const BLENDS: &[u8] = b"Normal|(-|Add|Multiply|Screen|Overlay|Soft Light|Hard Light|(-|Color Dodge|Color Burn|(-|Darken|Lighten|Difference|Exclusion|(-|Hue|Saturation|Color|Luminosity";

/// One recognized preset: Source Opacity rises linearly from 0 at the layer
/// inPoint to its full value over the fade-in, with a transparent background.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::structure_document) struct FrameFade {
    /// One-based Effect Parade indices of the controller and the Composite.
    effects: [usize; 2],
    /// Native fade-in length in composition frames.
    frames: f64,
}

impl FrameFade {
    /// Whether the lowering replaces the native effect at this one-based index.
    pub(in crate::structure_document) fn consumes(&self, index: usize) -> bool {
        self.effects.contains(&index)
    }

    /// Owner Opacity keys on the layer clock: 0 at inPoint, then the static
    /// owner opacity after `framesToTime(frames)` composition seconds.
    pub(in crate::structure_document) fn opacity(
        &self,
        layer: &Layer,
        frame_rate: f64,
        owner_opacity: f64,
    ) -> Result<NumericProperty, &'static str> {
        let (Some(input), Some(stretch)) = (layer.record.in_point(), layer.record.stretch()) else {
            return Err("invalid layer clock");
        };
        let end = input + self.frames / frame_rate / stretch;
        if ![input, end, owner_opacity]
            .iter()
            .all(|value| value.is_finite())
            || frame_rate <= 0.0
            || stretch <= 0.0
            || end <= input
        {
            return Err("fade-in interval is not a finite positive layer-clock span");
        }
        let key = |time_secs, value| NumericKeyframe {
            time_secs,
            values: vec![value],
            in_interpolation: 1,
            out_interpolation: 1,
            in_speed: vec![0.0],
            out_speed: vec![0.0],
            in_influence: vec![100.0 / 3.0],
            out_influence: vec![100.0 / 3.0],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        };
        Ok(NumericProperty {
            values: vec![0.0],
            animated: true,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: vec![key(input, 0.0), key(end, owner_opacity)],
            value_kind: NumericValueKind::Continuous,
        })
    }
}

/// Finds the preset in a layer's Effect Parade. `Ok(None)` means the layer
/// has no active controller; an error names why a present preset is not lowered.
pub(in crate::structure_document) fn recognize(
    layer: &Layer,
) -> Result<Option<FrameFade>, &'static str> {
    let flags = layer.record.flags();
    // The shared decoder reports malformed root names and omits every effect.
    let Ok(roots) = properties::root_runs(&layer.content) else {
        return Ok(None);
    };
    let parades: Vec<_> = roots
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade")
        .map(|(_, run)| parade_effects(run))
        .collect();
    let has_controller = |effects: &Option<Vec<(&str, &[Chunk])>>| {
        effects
            .iter()
            .flatten()
            .any(|(name, _)| *name == CONTROLLER)
    };
    let effects = match parades.as_slice() {
        [Some(effects)] if effects.iter().any(|(name, _)| *name == CONTROLLER) => effects,
        // Duplicate roots are ambiguous; the shared decoder omits their effects.
        [_, _, ..] if parades.iter().any(has_controller) => {
            return Err("duplicate Effect Parade roots");
        }
        _ => return Ok(None),
    };
    if !flags.effects_active {
        return Ok(None);
    }
    if flags.three_d_layer || flags.adjustment_layer || flags.preserve_transparency {
        return Err("requires a 2D non-adjustment layer without preserved transparency");
    }
    if !layer
        .record
        .stretch()
        .is_some_and(|stretch| stretch.is_finite() && stretch > 0.0)
    {
        return Err("requires a forward layer clock");
    }
    owner_opacity_is_static(layer)?;
    let mut controller = None;
    let mut composite = None;
    for (index, (name, run)) in effects.iter().enumerate() {
        let descriptor =
            properties::unique_list(run, *b"sspc").map_err(|_| "ambiguous plugin descriptor")?;
        let instance = Instance {
            index: index + 1,
            descriptor,
            body: properties::unique_list(descriptor, *b"tdgp")
                .map_err(|_| "malformed controls")?,
            enabled: properties::group_enabled(descriptor)
                .map_err(|_| "malformed effect enable state")?,
        };
        if effect_instance_name(descriptor, instance.body) == Some(CONTROLLER_NAME) {
            if *name != CONTROLLER || controller.is_some() {
                return Err("the controller name does not bind one controller");
            }
            controller = Some(instance);
        } else if *name == CONTROLLER {
            return Err("a renamed controller does not bind the expression");
        }
        if *name == COMPOSITE {
            if composite.is_some() {
                return Err("several Solid Composite effects");
            }
            composite = Some(instance);
        } else if composite.is_some()
            && instance.enabled
            && !matches!(*name, CONTROLLER | "ADBE Geometry2")
        {
            return Err("an effect after the Solid Composite renders the faded image");
        }
    }
    let (Some(controller), Some(composite)) = (controller, composite) else {
        return Err("requires one controller and one Solid Composite");
    };
    if !controller.enabled || !composite.enabled {
        return Err("the controller or the Solid Composite is disabled");
    }
    let frames = controller_frames(controller.descriptor, controller.body, layer.record.id())?;
    composite_profile(composite.descriptor, composite.body, layer.record.id())?;
    Ok(Some(FrameFade {
        effects: [controller.index, composite.index],
        frames,
    }))
}

/// The named instances of one parade; `None` when the shared decoder rejects it.
fn parade_effects(parade: &[Chunk]) -> Option<Vec<(&str, &[Chunk])>> {
    properties::runs(properties::unique_list(parade, *b"tdgp").ok()?).ok()
}

/// One native Effect Parade instance.
#[derive(Clone, Copy)]
struct Instance<'a> {
    /// One-based position, as in the shared effect decoder.
    index: usize,
    descriptor: &'a [Chunk],
    body: &'a [Chunk],
    enabled: bool,
}

/// Native owner opacity keys or expressions cannot multiply a second track.
fn owner_opacity_is_static(layer: &Layer) -> Result<(), &'static str> {
    let transform =
        properties::read_transform(&layer.content).map_err(|_| "malformed Transform")?;
    let opacity = transform
        .iter()
        .find(|property| property.match_name == "ADBE Opacity");
    match opacity.map(|property| property.numeric.as_ref()) {
        None => Ok(()),
        Some(Ok(numeric))
            if !numeric.animated && !numeric.expression_enabled && numeric.keyframes.is_empty() =>
        {
            Ok(())
        }
        Some(_) => Err("owner Opacity is animated or expression-driven"),
    }
}

fn controller_frames(
    descriptor: &[Chunk],
    body: &[Chunk],
    owner: u32,
) -> Result<f64, &'static str> {
    let controls = properties::runs(body).map_err(|_| "malformed controls")?;
    validate_controls(&controls, CONTROLLER, owner)?;
    declarations(descriptor, CONTROLLER)?;
    let frames = scalar(
        unique_run(&controls, "ADBE CM FadeInOutFrames-0001").map_err(|_| "fade-in missing")?,
        false,
        None,
    )?;
    if frames <= 0.0 {
        return Err("fade-in frames must be positive");
    }
    scalar(
        unique_run(&controls, "ADBE CM FadeInOutFrames-0002").map_err(|_| "fade-out missing")?,
        false,
        Some(0.0),
    )
    .map_err(|_| "only a zero fade-out is mapped")?;
    Ok(frames)
}

fn composite_profile(descriptor: &[Chunk], body: &[Chunk], owner: u32) -> Result<(), &'static str> {
    let controls = properties::runs(body).map_err(|_| "malformed controls")?;
    validate_controls(&controls, COMPOSITE, owner)?;
    let explicit_blend = controls
        .iter()
        .any(|(name, _)| *name == "ADBE Solid Composite-0004");
    declarations(descriptor, COMPOSITE)?;
    require_formula(&controls)?;
    scalar(
        unique_run(&controls, "ADBE Solid Composite-0001").map_err(|_| "Source Opacity missing")?,
        true,
        Some(100.0),
    )?;
    scalar(
        unique_run(&controls, "ADBE Solid Composite-0003")
            .map_err(|_| "explicit background Opacity required")?,
        false,
        Some(0.0),
    )
    .map_err(|_| "only a transparent background is mapped")?;
    if explicit_blend {
        scalar(
            unique_run(&controls, "ADBE Solid Composite-0004").map_err(|_| "ambiguous blending")?,
            false,
            Some(1.0),
        )
        .map_err(|_| "only Normal blending is mapped")?;
    } else if declared_default(descriptor, "ADBE Solid Composite-0004")?
        .is_some_and(|value| value != 1)
    {
        return Err("only Normal blending is mapped");
    }
    if let Some((_, color)) = controls
        .iter()
        .find(|(name, _)| *name == "ADBE Solid Composite-0002")
    {
        let numeric = properties::read_numeric(
            properties::unique_list(color, *b"tdbs").map_err(|_| "malformed Color")?,
        )
        .map_err(|_| "malformed Color")?;
        if numeric.animated || numeric.expression_enabled || !numeric.keyframes.is_empty() {
            return Err("background Color must be static");
        }
    }
    Ok(())
}

fn validate_controls(
    controls: &[(&str, &[Chunk])],
    effect: &str,
    owner: u32,
) -> Result<(), &'static str> {
    let mut seen = HashSet::new();
    for (name, run) in controls {
        if *name == "ADBE Group End" {
            continue;
        }
        if !seen.insert(*name) {
            return Err("duplicate effect control");
        }
        if *name == "ADBE Effect Built In Params" {
            let options = properties::runs(
                properties::unique_list(run, *b"tdgp")
                    .map_err(|_| "malformed compositing options")?,
            )
            .map_err(|_| "malformed compositing options")?;
            if options.iter().any(|(name, _)| *name != "ADBE Group End") {
                return Err("nonempty effect compositing options");
            }
            continue;
        }
        let suffix = name
            .strip_prefix(effect)
            .and_then(|suffix| suffix.strip_prefix('-'))
            .ok_or("unknown effect control")?;
        let known = match effect {
            CONTROLLER => matches!(suffix, "0000" | "0001" | "0002"),
            _ => matches!(suffix, "0000" | "0001" | "0002" | "0003" | "0004"),
        };
        if !known {
            return Err("unknown effect control");
        }
        if suffix == "0000" {
            scalar_inner(run, false, Some(0.0), Some(owner))?;
        }
    }
    Ok(())
}

fn require_formula(controls: &[(&str, &[Chunk])]) -> Result<(), &'static str> {
    let run = unique_run(controls, "ADBE Solid Composite-0001").map_err(|_| "Source Opacity")?;
    let leaf = properties::unique_list(run, *b"tdbs").map_err(|_| "Source Opacity")?;
    let numeric = properties::read_numeric(leaf).map_err(|_| "Source Opacity")?;
    let source = properties::data(leaf, *b"Utf8")
        .ok()
        .and_then(|bytes| std::str::from_utf8(bytes).ok());
    if numeric.expression_enabled
        && source.and_then(canonical).is_some()
        && source.and_then(canonical) == canonical(FORMULA)
    {
        Ok(())
    } else {
        Err("Source Opacity is not the enabled complete preset expression")
    }
}

fn scalar(
    run: &[Chunk],
    expression_allowed: bool,
    expected: Option<f64>,
) -> Result<f64, &'static str> {
    scalar_inner(run, expression_allowed, expected, None)
}

/// A finite unkeyed scalar with an explicit value and only public payloads.
fn scalar_inner(
    run: &[Chunk],
    expression_allowed: bool,
    expected: Option<f64>,
    input_owner: Option<u32>,
) -> Result<f64, &'static str> {
    let leaf = properties::unique_list(run, *b"tdbs").map_err(|_| "ambiguous scalar property")?;
    let mut seen = HashSet::new();
    for chunk in leaf {
        let tag = chunk.id();
        let input_ref = input_owner.is_some() && (tag == *b"tdpi" || tag == *b"tdps");
        if !seen.insert(tag)
            || (!input_ref
                && ![
                    *b"tdsb", *b"tdsn", *b"tdb4", *b"cdat", *b"Utf8", *b"tdum", *b"tduM",
                ]
                .contains(&tag))
        {
            return Err("private or keyed scalar payload");
        }
    }
    if let Some(owner) = input_owner {
        let id = properties::data(leaf, *b"tdpi").map_err(|_| "input source binding required")?;
        let stage = properties::data(leaf, *b"tdps").map_err(|_| "input source stage required")?;
        if id != owner.to_be_bytes() || stage != 0_i32.to_be_bytes() {
            return Err("effect input must be this layer's source");
        }
    }
    properties::data(leaf, *b"cdat").map_err(|_| "explicit scalar value required")?;
    let numeric = properties::read_numeric(leaf).map_err(|_| "malformed scalar property")?;
    if numeric.animated
        || !numeric.keyframes.is_empty()
        || (!expression_allowed && (numeric.expression_present || numeric.expression_enabled))
        || numeric.values.len() != 1
        || !numeric.values[0].is_finite()
    {
        return Err("requires a finite unkeyed scalar");
    }
    let value = numeric.values[0];
    if expected.is_some_and(|expected| value != expected) {
        return Err("scalar is outside the mapped profile");
    }
    Ok(value)
}

/// A local `parT` must declare the preset's controls under the names the
/// expression binds. Sparse instances without one use the standard profile.
/// Declared defaults of explicit controls do not affect the result.
fn declarations(descriptor: &[Chunk], effect: &str) -> Result<(), &'static str> {
    let rows = declaration_rows(descriptor)?;
    if rows.is_empty() {
        return Ok(());
    }
    let expected: &[(&str, u32, &str)] = if effect == CONTROLLER {
        &[
            ("ADBE CM FadeInOutFrames-0000", 0, ""),
            ("ADBE CM FadeInOutFrames-0001", 10, FADE_IN),
            ("ADBE CM FadeInOutFrames-0002", 10, FADE_OUT),
            ("ADBE Effect Built In Params", 9, ""),
        ]
    } else {
        &[
            ("ADBE Solid Composite-0000", 0, ""),
            ("ADBE Solid Composite-0001", 2, "Source Opacity"),
            ("ADBE Solid Composite-0002", 5, "Color"),
            ("ADBE Solid Composite-0003", 2, "Opacity"),
            ("ADBE Solid Composite-0004", 7, "Blending Mode"),
            ("ADBE Effect Built In Params", 9, ""),
        ]
    };
    if rows.len() != expected.len() {
        return Err("incomplete parameter declarations");
    }
    for (name, kind, label) in expected {
        let run = unique_run(&rows, name).map_err(|_| "ambiguous declaration")?;
        let data = properties::data(run, *b"pard").map_err(|_| "malformed declaration")?;
        if data.len() != 148 || data[12..16] != kind.to_be_bytes() {
            return Err("conflicting parameter declaration");
        }
        if !label.is_empty()
            && data[16..48].split(|byte| *byte == 0).next() != Some(label.as_bytes())
        {
            return Err("parameter label no longer binds the expression");
        }
        if *name == "ADBE Solid Composite-0004" {
            let menu = properties::data(run, *b"pdnm").map_err(|_| "blending menu missing")?;
            if menu.len() != BLENDS.len() + 9
                || menu[..4] != *b"Utf8"
                || menu[4..8]
                    != u32::try_from(BLENDS.len())
                        .unwrap_or(u32::MAX)
                        .to_be_bytes()
                || menu[8..menu.len() - 1] != *BLENDS
                || menu[menu.len() - 1] != 0
            {
                return Err("blending menu is outside the Normal profile");
            }
        }
    }
    Ok(())
}

/// Local `parT` rows. Only a sparse instance, whose table is empty, has none;
/// a present table that declares nothing is incomplete.
fn declaration_rows(descriptor: &[Chunk]) -> Result<Vec<(&str, &[Chunk])>, &'static str> {
    let table = properties::unique_list(descriptor, *b"parT")
        .map_err(|_| "ambiguous parameter declarations")?;
    if table.is_empty() {
        return Ok(Vec::new());
    }
    let rows = properties::runs(table).map_err(|_| "malformed parameter declarations")?;
    let count = properties::data(table, *b"parn").map_err(|_| "parameter count missing")?;
    if rows.is_empty()
        || u32::try_from(rows.len()).ok().map(u32::to_be_bytes) != count.try_into().ok()
    {
        return Err("incomplete parameter declarations");
    }
    Ok(rows)
}

/// The declared integer default of one popup control, when a `parT` exists.
fn declared_default(descriptor: &[Chunk], name: &str) -> Result<Option<u32>, &'static str> {
    let rows = declaration_rows(descriptor)?;
    if rows.is_empty() {
        return Ok(None);
    }
    let run = unique_run(&rows, name).map_err(|_| "ambiguous declaration")?;
    let data = properties::data(run, *b"pard").map_err(|_| "malformed declaration")?;
    let value = data.get(56..60).ok_or("malformed declaration")?;
    Ok(Some(u32::from_be_bytes(
        value.try_into().map_err(|_| "malformed declaration")?,
    )))
}

/// Tokenizes bounded source without fusing identifiers or numbers across
/// whitespace or comments. Strings keep their exact spelling.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_source_opacity_formula_does_not_admit_modified_bindings() {
        assert_eq!(
            canonical(FORMULA),
            canonical(&FORMULA.replace('\n', "\r\n"))
        );
        for changed in [
            FORMULA.replace("linear(", "ease("),
            FORMULA.replace("inPoint +", "inPoint -"),
            FORMULA.replace(CONTROLLER_NAME, "Other"),
            FORMULA.replace(
                "fadeInOpacity + fadeOutOpacity - value",
                "fadeInOpacity * fadeOutOpacity",
            ),
            format!("{FORMULA}value;"),
        ] {
            assert_ne!(canonical(FORMULA), canonical(&changed));
        }
    }
}
