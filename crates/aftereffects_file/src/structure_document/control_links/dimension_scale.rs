//! Bounded static lowering of composition-dimension Scale expressions.
//!
//! These complete grammars contain no time, source, or controller access.
//! Imported values are independently editable; resizing FX does not re-evaluate them.

use super::{expression, finished, finite_number, identifier, properties, token, unique_run};
use crate::{
    properties::{NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, Layer},
};

pub(super) fn lower(
    layer: &Layer,
    composition: &Composition,
) -> Option<Result<NumericProperty, PropertyError>> {
    let root = properties::root_runs(&layer.content).ok()?;
    let transform = unique_run(&root, "ADBE Transform Group").ok()?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp").ok()?).ok()?;
    let scale = properties::unique_list(unique_run(&leaves, "ADBE Scale").ok()?, *b"tdbs").ok()?;
    let profile = parse(expression(scale).ok()?)?;
    Some(
        values(profile, composition.width, composition.height).map(|values| NumericProperty {
            values: values.to_vec(),
            animated: false,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: Vec::new(),
            value_kind: NumericValueKind::Continuous,
        }),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Profile {
    Dimensions,
    Cover(f64),
    SeparateRatios {
        width_reference: f64,
        height_reference: f64,
    },
}

fn assigned_dimension<'a>(text: &mut &'a str, dimension: &str) -> Option<&'a str> {
    let variable = identifier(text)?;
    if matches!(variable, "thisComp" | "if" | "else") {
        return None;
    }
    token(text, "=")?;
    token(text, "thisComp")?;
    token(text, ".")?;
    token(text, dimension)?;
    token(text, ";")?;
    Some(variable)
}

fn variable(text: &mut &str, expected: &str) -> Option<()> {
    (identifier(text)? == expected).then_some(())
}

fn parse(mut text: &str) -> Option<Profile> {
    // Comments are ignored only at the source-proven statement boundaries.
    text = line_comments(text)?;
    if let Some(profile) = separate_ratios(text) {
        return Some(profile);
    }
    let cover_reference = if text.trim_start().starts_with("userX") {
        let width = literal_binding(&mut text, "userX")?;
        let height = literal_binding(&mut text, "userY")?;
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        text = line_comments(text)?;
        Some((width, height))
    } else {
        None
    };
    let width = assigned_dimension(&mut text, "width")?;
    let height = assigned_dimension(&mut text, "height")?;
    // Admit only the ordinary temporary bindings in the source-proven profiles;
    // assignments to AE/global names can have different native semantics.
    if !matches!((width, height), ("x", "y") | ("w", "h")) {
        return None;
    }
    if token(&mut text, "[").is_some() {
        if cover_reference.is_some() {
            return None;
        }
        variable(&mut text, width)?;
        token(&mut text, ",")?;
        variable(&mut text, height)?;
        token(&mut text, "]")?;
        return finished(text).then_some(Profile::Dimensions);
    }
    let aspect = identifier(&mut text)?;
    if cover_reference.is_some() {
        if (width, height, aspect) != ("x", "y", "ratio") {
            return None;
        }
    } else if aspect != "aspect" {
        return None;
    }
    token(&mut text, "=")?;
    let (numerator, denominator) = if let Some(reference) = cover_reference {
        variable(&mut text, "userX")?;
        token(&mut text, "/")?;
        variable(&mut text, "userY")?;
        reference
    } else {
        let numerator = finite_number(&mut text)?;
        token(&mut text, "/")?;
        (numerator, finite_number(&mut text)?)
    };
    token(&mut text, ";")?;
    token(&mut text, "if")?;
    token(&mut text, "(")?;
    variable(&mut text, width)?;
    token(&mut text, "/")?;
    variable(&mut text, height)?;
    token(&mut text, ">=")?;
    variable(&mut text, aspect)?;
    token(&mut text, ")")?;
    token(&mut text, "{")?;
    token(&mut text, "[")?;
    variable(&mut text, width)?;
    token(&mut text, ",")?;
    variable(&mut text, width)?;
    token(&mut text, "]")?;
    token(&mut text, "}")?;
    token(&mut text, "else")?;
    token(&mut text, "{")?;
    token(&mut text, "[")?;
    for axis in 0..2 {
        if axis != 0 {
            token(&mut text, ",")?;
        }
        variable(&mut text, height)?;
        token(&mut text, "*")?;
        variable(&mut text, aspect)?;
    }
    token(&mut text, "]")?;
    token(&mut text, "}")?;
    // Keep recognized-but-invalid arithmetic outside the unsupported fallback.
    finished(text).then_some(Profile::Cover(numerator / denominator))
}

fn line_comments(mut text: &str) -> Option<&str> {
    while let Some(comment) = text.trim_start().strip_prefix("//") {
        text = comment.split_once('\n')?.1;
    }
    Some(text)
}

fn literal_binding(text: &mut &str, name: &str) -> Option<f64> {
    variable(text, name)?;
    token(text, "=")?;
    let value = finite_number(text)?;
    token(text, ";")?;
    Some(value)
}

fn separate_ratios(mut text: &str) -> Option<Profile> {
    // Temporary names and the complete assignment/vector order are part of this
    // source-proven profile, not general JS bindings or arbitrary arithmetic.
    variable(&mut text, "heightScale")?;
    token(&mut text, "=")?;
    token(&mut text, "thisComp.height")?;
    token(&mut text, "/")?;
    let height_reference = finite_number(&mut text)?;
    token(&mut text, ";")?;
    variable(&mut text, "widthScale")?;
    token(&mut text, "=")?;
    token(&mut text, "thisComp.width")?;
    token(&mut text, "/")?;
    let width_reference = finite_number(&mut text)?;
    token(&mut text, ";")?;
    token(&mut text, "[")?;
    variable(&mut text, "widthScale")?;
    token(&mut text, "*")?;
    if finite_number(&mut text)? != 100.0 {
        return None;
    }
    token(&mut text, ",")?;
    variable(&mut text, "heightScale")?;
    token(&mut text, "*")?;
    if finite_number(&mut text)? != 100.0 {
        return None;
    }
    token(&mut text, "]")?;
    finished(text).then_some(Profile::SeparateRatios {
        width_reference,
        height_reference,
    })
}

fn values(profile: Profile, width: u16, height: u16) -> Result<[f64; 2], PropertyError> {
    let width = f64::from(width);
    let height = f64::from(height);
    if width == 0.0 || height == 0.0 {
        return Err(PropertyError::Layout(
            "dimension Scale requires a nonzero canvas",
        ));
    }
    let values = match profile {
        Profile::Dimensions => [width, height],
        Profile::Cover(aspect) => {
            if !aspect.is_finite() || aspect <= 0.0 {
                return Err(PropertyError::Layout(
                    "dimension Scale requires a finite positive aspect",
                ));
            }
            [width.max(height * aspect); 2]
        }
        Profile::SeparateRatios {
            width_reference,
            height_reference,
        } => {
            if [width_reference, height_reference]
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
            {
                return Err(PropertyError::Layout(
                    "dimension Scale requires finite positive reference dimensions",
                ));
            }
            [
                width / width_reference * 100.0,
                height / height_reference * 100.0,
            ]
        }
    };
    if values.iter().any(|value| !value.is_finite()) {
        return Err(PropertyError::Layout("dimension Scale overflow"));
    }
    // AE expression results are percentages; NumericProperty uses AV fractions.
    Ok(values.map(|value| value / 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WRAPPED_COVER: &str = "// created by: VIDEOLANCER.net //\r\nuserX = 1920; userY = 1080;\r\n// Width and Height of the Comp in which the Scene was created //\r\nx = thisComp.width; y = thisComp.height; ratio = userX/userY; if (x/y >= ratio){[x,x]} else {[y*ratio,y*ratio]}";

    #[test]
    fn dimension_scale_cover_wrapper_preserves_complete_static_semantics() {
        let profile = parse(WRAPPED_COVER).unwrap();
        assert_eq!(profile, Profile::Cover(1920.0 / 1080.0));
        assert_eq!(values(profile, 1920, 1080).unwrap(), [19.2, 19.2]);
        assert_eq!(values(profile, 960, 1080).unwrap(), [19.2, 19.2]);
        assert_eq!(values(profile, 1920, 540).unwrap(), [19.2, 19.2]);
        assert_eq!(values(profile, 960, 540).unwrap(), [9.6, 9.6]);
        for text in [
            WRAPPED_COVER.replace("userX = 1920", "userX = time"),
            WRAPPED_COVER.replace("userY = 1080", "userY = 0"),
            WRAPPED_COVER.replace("userX = 1920", "userX = -1920"),
            WRAPPED_COVER.replace("userX/userY", "userY/userX"),
            WRAPPED_COVER.replace("userX", "thisLayer"),
            WRAPPED_COVER.replace("x/y >= ratio", "x/y <= ratio"),
            WRAPPED_COVER.replace("[y*ratio,y*ratio]", "[y/ratio,y/ratio]"),
            WRAPPED_COVER.replace("[x,x]", "[x,y]"),
            format!("{WRAPPED_COVER};evil()"),
        ] {
            assert!(parse(&text).is_none(), "{text}");
        }
    }

    const RATIOS: &str = "heightScale = thisComp.height / 1080; widthScale = thisComp.width / 1920; [widthScale * 100, heightScale * 100]";

    #[test]
    fn dimension_scale_separate_ratios_units_and_complete_grammar() {
        let profile = parse(RATIOS).unwrap();
        assert_eq!(values(profile, 1920, 1080).unwrap(), [1.0, 1.0]);
        assert_eq!(values(profile, 960, 540).unwrap(), [0.5, 0.5]);
        assert_eq!(values(profile, 1920, 540).unwrap(), [1.0, 0.5]);
        assert_eq!(values(profile, 960, 1080).unwrap(), [0.5, 1.0]);
        assert!(values(profile, 0, 1080).is_err());
        for text in [
            RATIOS.replace(
                "widthScale * 100, heightScale * 100",
                "heightScale * 100, widthScale * 100",
            ),
            RATIOS.replace("widthScale * 100", "widthScale * 99"),
            RATIOS.replace("heightScale * 100", "heightScale * 50"),
            RATIOS.replace("thisComp.width / 1920", "thisComp.width / (1920 + time)"),
            RATIOS.replace("heightScale", "thisLayer"),
            format!("{RATIOS}; evil()"),
        ] {
            assert!(parse(&text).is_none(), "{text}");
        }
        for text in [
            RATIOS.replace("/ 1920", "/ 0"),
            RATIOS.replace("/ 1080", "/ -1"),
            RATIOS.replace("/ 1920", "/ 1e-320"),
        ] {
            assert!(values(parse(&text).unwrap(), 1920, 1080).is_err(), "{text}");
        }
    }

    const COVER: &str = "w = thisComp.width; h = thisComp.height; aspect = 1920/1080; if(w / h >= aspect){[w, w]}else{[h*aspect, h*aspect]}";

    #[test]
    fn dimension_scale_complete_grammars_and_units() {
        assert_eq!(
            parse("// resize\nx = thisComp.width;y = thisComp.height;[x, y]"),
            Some(Profile::Dimensions)
        );
        assert_eq!(
            values(Profile::Dimensions, 1920, 1080).unwrap(),
            [19.2, 10.8]
        );
        assert_eq!(parse(COVER), Some(Profile::Cover(1920.0 / 1080.0)));
        assert_eq!(
            values(parse(COVER).unwrap(), 1920, 1080).unwrap(),
            [19.2; 2]
        );
        assert_eq!(values(Profile::Cover(2.0), 100, 200).unwrap(), [4.0; 2]);
        assert_eq!(values(Profile::Cover(2.0), 400, 100).unwrap(), [4.0; 2]);
    }

    #[test]
    fn dimension_scale_rejects_changed_semantics_and_executable_suffixes() {
        for text in [
            "x=thisComp.width;y=thisComp.height;[y,x]",
            "x=thisComp.width;x=thisComp.height;[x,x]",
            "x=thisComp.width;y=thisComp.height;[x,y];evil()",
            "x=thisComp.width+time;y=thisComp.height;[x,y]",
            "thisComp=thisComp.width;y=thisComp.height;[thisComp,y]",
        ] {
            assert!(parse(text).is_none(), "{text}");
        }
        for text in [
            COVER.replace(">=", "<="),
            COVER.replace("h*aspect", "h/aspect"),
            format!("{COVER};evil()"),
        ] {
            assert!(parse(&text).is_none(), "{text}");
        }
        assert!(values(Profile::Cover(f64::INFINITY), 1920, 1080).is_err());
        assert!(values(Profile::Cover(0.0), 1920, 1080).is_err());
        assert!(values(Profile::Dimensions, 0, 1080).is_err());
    }
}
