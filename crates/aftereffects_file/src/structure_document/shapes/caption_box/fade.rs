//! The complete caption Source Opacity expression, not a general effect evaluator.
use super::*;
use crate::structure_document::control_links::{effect_instance_name, unique_run};
use std::collections::HashSet;

const CONTROLLER: &str = "ADBE CM FadeInOutFrames";
const COMPOSITE: &str = "ADBE Solid Composite";
const FORMULA: &str = r#"
var fadeInDuration = framesToTime(effect("Fade In+Out - frames")("Fade In Duration (frames)"));
var fadeOutDuration = framesToTime(effect("Fade In+Out - frames")("Fade Out Duration (frames)"));
var fadeInOpacity = fadeInDuration ? linear(time, inPoint, inPoint + fadeInDuration, 0, value) : value;
var fadeOutOpacity = fadeOutDuration ? linear(time, outPoint - fadeOutDuration, outPoint, value, 0) : value;
fadeInOpacity + fadeOutOpacity - value;
"#;
const BLENDS: &[u8] = b"Normal|(-|Add|Multiply|Screen|Overlay|Soft Light|Hard Light|(-|Color Dodge|Color Burn|(-|Darken|Lighten|Difference|Exclusion|(-|Hue|Saturation|Color|Luminosity";

pub(super) fn resolve(
    layer: &Layer,
    composition: &Composition,
) -> Result<NumericProperty, &'static str> {
    let flags = layer.record.flags();
    if !flags.enabled
        || !flags.effects_active
        || flags.three_d_layer
        || flags.adjustment_layer
        || flags.preserve_transparency
        || layer.record.track_matte_type() != 0
        || layer.record.stretch() != Some(1.0)
    {
        return Err("requires enabled unit-clock 2D caption without matte");
    }
    let roots = properties::root_runs(&layer.content).map_err(|_| "malformed layer properties")?;
    for (name, run) in &roots {
        match *name {
            "ADBE Mask Parade" => {
                let masks = properties::runs(
                    properties::unique_list(run, *b"tdgp").map_err(|_| "malformed masks")?,
                )
                .map_err(|_| "malformed masks")?;
                if masks.iter().any(|(name, _)| *name != "ADBE Group End") {
                    return Err("native masks change the fade placement");
                }
            }
            "ADBE Layer Styles" => empty_styles(run)?,
            "ADBE Layer Style Group" => return Err("unsupported layer style group"),
            "ADBE Time Remapping" => {
                scalar(run, false, Some(0.0))?;
            }
            _ => {}
        }
    }
    let parade = unique_run(&roots, "ADBE Effect Parade").map_err(|_| "ambiguous Effect Parade")?;
    let effects = properties::runs(
        properties::unique_list(parade, *b"tdgp").map_err(|_| "malformed effects")?,
    )
    .map_err(|_| "malformed effects")?;
    let expected = [
        "ADBE Slider Control",
        "ADBE Slider Control",
        "ADBE Slider Control",
        "ADBE Slider Control",
        CONTROLLER,
        COMPOSITE,
    ];
    if effects.len() != expected.len()
        || effects
            .iter()
            .zip(expected)
            .any(|((name, _), expected)| *name != expected)
    {
        return Err("requires four Sliders, fade controller and final Solid Composite only");
    }
    let mut fade_in = None;
    for (index, (name, run)) in effects.iter().enumerate() {
        let descriptor =
            properties::unique_list(run, *b"sspc").map_err(|_| "ambiguous plugin descriptor")?;
        if !properties::group_enabled(descriptor).map_err(|_| "malformed effect enable state")? {
            return Err("required caption effect disabled");
        }
        let body =
            properties::unique_list(descriptor, *b"tdgp").map_err(|_| "malformed controls")?;
        let controls = properties::runs(body).map_err(|_| "malformed controls")?;
        validate_controls(&controls, name, layer.record.id())?;
        if index < 4 {
            let display = effect_instance_name(descriptor, body).ok_or("malformed Slider name")?;
            if display != ["Width Padding", "Width Override", "Height", "Roundness"][index] {
                return Err("Slider binding renamed or reordered");
            }
            scalar(
                unique_run(&controls, "ADBE Slider Control-0001")
                    .map_err(|_| "ambiguous Slider")?,
                false,
                None,
            )?;
        } else if *name == CONTROLLER {
            if effect_instance_name(descriptor, body) != Some("Fade In+Out - frames") {
                return Err("fade controller name does not bind expression");
            }
            declarations(descriptor, CONTROLLER)?;
            let value = scalar(
                unique_run(&controls, "ADBE CM FadeInOutFrames-0001")
                    .map_err(|_| "fade-in control missing or ambiguous")?,
                false,
                None,
            )?;
            if value <= 0.0 || value.fract() != 0.0 {
                return Err("fade-in frames must be positive integral");
            }
            scalar(
                unique_run(&controls, "ADBE CM FadeInOutFrames-0002")
                    .map_err(|_| "fade-out control missing or ambiguous")?,
                false,
                Some(0.0),
            )?;
            fade_in = Some(value);
        } else {
            declarations(descriptor, COMPOSITE)?;
            require_expression(&controls, "ADBE Solid Composite-0001", FORMULA)?;
            scalar(
                unique_run(&controls, "ADBE Solid Composite-0001")
                    .map_err(|_| "ambiguous Source Opacity")?,
                true,
                Some(100.0),
            )?;
            scalar(
                unique_run(&controls, "ADBE Solid Composite-0003")
                    .map_err(|_| "explicit background Opacity required")?,
                false,
                Some(0.0),
            )?;
            if controls
                .iter()
                .any(|(name, _)| *name == "ADBE Solid Composite-0004")
            {
                scalar(
                    unique_run(&controls, "ADBE Solid Composite-0004")
                        .map_err(|_| "ambiguous blending")?,
                    false,
                    Some(1.0),
                )?;
            }
            if let Some((_, color)) = controls
                .iter()
                .find(|(name, _)| *name == "ADBE Solid Composite-0002")
            {
                let numeric = properties::read_numeric(
                    properties::unique_list(color, *b"tdbs").map_err(|_| "malformed Color")?,
                )
                .map_err(|_| "malformed Color")?;
                if numeric.animated
                    || numeric.expression_present
                    || numeric.expression_enabled
                    || !numeric.keyframes.is_empty()
                    || numeric.values.iter().any(|v| !v.is_finite())
                {
                    return Err("background Color must be static");
                }
            }
        }
    }
    curve(
        layer.record.in_point().ok_or("invalid inPoint")?,
        layer.record.out_point().ok_or("invalid outPoint")?,
        fade_in.ok_or("fade controller missing")?,
        composition.frame_rate,
    )
}

fn empty_styles(run: &[Chunk]) -> Result<(), &'static str> {
    let styles =
        properties::runs(properties::unique_list(run, *b"tdgp").map_err(|_| "malformed styles")?)
            .map_err(|_| "malformed styles")?;
    let mut seen = HashSet::new();
    for (name, style) in styles {
        if !seen.insert(name) {
            return Err("ambiguous layer styles");
        }
        let leaves = properties::runs(
            properties::unique_list(style, *b"tdgp").map_err(|_| "malformed style scaffold")?,
        )
        .map_err(|_| "malformed style scaffold")?;
        if name == "ADBE Blend Options Group" {
            let [("ADBE Adv Blend Group", advanced)] = leaves.as_slice() else {
                return Err("nonempty style blend options");
            };
            if !properties::runs(
                properties::unique_list(advanced, *b"tdgp")
                    .map_err(|_| "malformed advanced blend")?,
            )
            .map_err(|_| "malformed advanced blend")?
            .is_empty()
            {
                return Err("nonempty style blend options");
            }
        } else if !matches!(
            name,
            "dropShadow/enabled"
                | "innerShadow/enabled"
                | "outerGlow/enabled"
                | "innerGlow/enabled"
                | "bevelEmboss/enabled"
                | "chromeFX/enabled"
                | "solidFill/enabled"
                | "gradientFill/enabled"
                | "patternFill/enabled"
                | "frameFX/enabled"
        ) || properties::group_enabled(style)
            .map_err(|_| "malformed style enable state")?
            || !leaves.is_empty()
        {
            return Err("layer styles change the fade placement");
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
        } else if *name == "ADBE Group End" {
            continue;
        } else {
            let suffix = name
                .strip_prefix(effect)
                .and_then(|s| s.strip_prefix('-'))
                .ok_or("unknown effect control")?;
            let allowed = match effect {
                "ADBE Slider Control" => matches!(suffix, "0000" | "0001"),
                CONTROLLER => matches!(suffix, "0000" | "0001" | "0002"),
                COMPOSITE => matches!(suffix, "0000" | "0001" | "0002" | "0003" | "0004"),
                _ => false,
            };
            if !allowed {
                return Err("unknown effect control");
            }
            if suffix == "0000" {
                scalar_inner(run, false, Some(0.0), Some(owner))?;
            }
        }
    }
    Ok(())
}
fn scalar(
    run: &[Chunk],
    expression_allowed: bool,
    expected: Option<f64>,
) -> Result<f64, &'static str> {
    scalar_inner(run, expression_allowed, expected, None)
}
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
            return Err("private or keyed scalar payload is outside caption fade profile");
        }
    }
    if let Some(owner) = input_owner {
        let id = properties::data(leaf, *b"tdpi").map_err(|_| "input source binding required")?;
        let stage = properties::data(leaf, *b"tdps").map_err(|_| "input source stage required")?;
        if id != owner.to_be_bytes() || stage != 0_i32.to_be_bytes() {
            return Err("input must bind this layer's source");
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
        return Err("scalar is outside caption fade profile");
    }
    Ok(value)
}

/// Sparse instances use the native standard profile proved by the existing source;
/// any supplied local declarations must agree completely, including popup ordering.
fn declarations(descriptor: &[Chunk], effect: &str) -> Result<(), &'static str> {
    let table = properties::unique_list(descriptor, *b"parT")
        .map_err(|_| "ambiguous parameter declarations")?;
    if table.is_empty() {
        return Ok(());
    }
    let rows = properties::runs(table).map_err(|_| "malformed parameter declarations")?;
    let expected: &[(&str, u32, u32, &str)] = if effect == CONTROLLER {
        &[
            ("0000", 0, 0, ""),
            ("0001", 10, 0x40180000, "Fade In Duration (frames)"),
            ("0002", 10, 0, "Fade Out Duration (frames)"),
            ("built-in", 9, 0, ""),
        ]
    } else {
        &[
            ("0000", 0, 0, ""),
            ("0001", 2, 100 << 16, "Source Opacity"),
            ("0002", 5, u32::MAX, "Color"),
            ("0003", 2, 0, "Opacity"),
            ("0004", 7, 1, "Blending Mode"),
            ("built-in", 9, 0, ""),
        ]
    };
    if rows.len() != expected.len()
        || properties::data(table, *b"parn").map_err(|_| "parameter count missing or ambiguous")?
            != u32::try_from(expected.len())
                .map_err(|_| "parameter count overflow")?
                .to_be_bytes()
    {
        return Err("incomplete parameter declarations");
    }
    for (suffix, kind, value, label) in expected {
        let name = if *suffix == "built-in" {
            "ADBE Effect Built In Params".to_owned()
        } else {
            format!("{effect}-{suffix}")
        };
        let run = unique_run(&rows, &name).map_err(|_| "ambiguous declaration")?;
        let data = properties::data(run, *b"pard").map_err(|_| "malformed declaration")?;
        if data.len() != 148
            || data[12..16] != kind.to_be_bytes()
            || data[56..60] != value.to_be_bytes()
        {
            return Err("conflicting parameter declaration");
        }
        if *kind == 10 && data[60..64] != [0; 4] {
            return Err("conflicting frame-control declaration");
        }
        if !label.is_empty() && data[16..48].split(|v| *v == 0).next() != Some(label.as_bytes()) {
            return Err("parameter label no longer binds expression/profile");
        }
        if *suffix == "0004" && effect == COMPOSITE {
            let menu = properties::data(run, *b"pdnm").map_err(|_| "blending menu missing")?;
            if menu.len() != BLENDS.len() + 9
                || menu[..4] != *b"Utf8"
                || menu[4..8] != (BLENDS.len() as u32).to_be_bytes()
                || menu[8..menu.len() - 1] != *BLENDS
                || menu[menu.len() - 1] != 0
            {
                return Err("blending menu is outside Normal profile");
            }
        }
    }
    Ok(())
}
fn curve(start: f64, out: f64, frames: f64, fps: f64) -> Result<NumericProperty, &'static str> {
    let end = start + frames / fps;
    if ![start, out, frames, fps, end].iter().all(|v| v.is_finite())
        || frames <= 0.0
        || frames.fract() != 0.0
        || fps <= 0.0
        || end <= start
        || end > out
    {
        return Err("fade interval must fit finite authored source lifetime");
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
        keyframes: vec![key(start, 0.0), key(end, 100.0)],
        value_kind: NumericValueKind::Continuous,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_six_frame_linear_curve_uses_source_clock_and_fractional_fps() {
        let p = curve(
            11.600333667000333,
            37.02575909242576,
            6.0,
            29.970001220703125,
        )
        .unwrap();
        assert_eq!(p.keyframes.len(), 2);
        assert_eq!((p.keyframes[0].time_secs * 1000.0).round(), 11600.0);
        assert_eq!((p.keyframes[1].time_secs * 1000.0).round(), 11801.0);
        assert_eq!(p.keyframes[0].values, [0.0]);
        assert_eq!(p.keyframes[1].values, [100.0]);
        for key in p.keyframes {
            assert_eq!(key.in_interpolation, 1);
            assert_eq!(key.out_interpolation, 1);
        }
        for (out, frames, fps) in [
            (11.7, 6.0, 29.97),
            (40.0, 0.0, 30.0),
            (40.0, 6.5, 30.0),
            (40.0, 6.0, 0.0),
            (40.0, 6.0, f64::NAN),
        ] {
            assert!(curve(11.6, out, frames, fps).is_err());
        }
    }
    #[test]
    fn complete_source_opacity_formula_does_not_admit_modified_bindings() {
        assert_eq!(
            canonical(FORMULA),
            canonical(&FORMULA.replace('\n', "\r\n"))
        );
        for changed in [
            FORMULA.replace("linear(", "ease("),
            FORMULA.replace("inPoint +", "inPoint -"),
            FORMULA.replace("Fade In+Out - frames", "Other"),
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
