//! Shape-property ordinals differ from the native layer-record blend byte.
use super::*;
use fx_schema::BlendMode;

fn decode(value: f64) -> Option<BlendMode> {
    Some(match value {
        1.0 => BlendMode::Normal,
        3.0 => BlendMode::Darken,
        4.0 => BlendMode::Multiply,
        5.0 => BlendMode::ColorBurn,
        6.0 => BlendMode::LinearBurn,
        7.0 => BlendMode::DarkerColor,
        9.0 => BlendMode::Lighten,
        10.0 => BlendMode::Screen,
        11.0 => BlendMode::ColorDodge,
        12.0 => BlendMode::Add,
        13.0 => BlendMode::LighterColor,
        15.0 => BlendMode::Overlay,
        16.0 => BlendMode::SoftLight,
        17.0 => BlendMode::HardLight,
        18.0 => BlendMode::LinearLight,
        19.0 => BlendMode::VividLight,
        20.0 => BlendMode::PinLight,
        21.0 => BlendMode::HardMix,
        23.0 => BlendMode::Difference,
        24.0 => BlendMode::Exclusion,
        26.0 => BlendMode::Hue,
        27.0 => BlendMode::Saturation,
        28.0 => BlendMode::Color,
        29.0 => BlendMode::Luminosity,
        _ => return None,
    })
}

pub(super) fn from_run(run: &[Chunk], warnings: &mut Vec<String>) -> BlendMode {
    let leaves = match property_group(run, "shape blend") {
        Ok(leaves) => leaves,
        Err(error) => {
            warnings.push(format!(
                "shape blend group is malformed ({error}); Normal retained"
            ));
            return BlendMode::Normal;
        }
    };
    if leaves
        .iter()
        .filter(|(name, _)| *name == "ADBE Vector Blend Mode")
        .count()
        > 1
    {
        warnings.push("duplicate shape blend modes; Normal retained".into());
        return BlendMode::Normal;
    }
    let Some(value) = numeric_leaf(&leaves, "ADBE Vector Blend Mode", warnings) else {
        return BlendMode::Normal;
    };
    if value.animated || value.expression_enabled {
        warnings.push("animated/expression shape blend mode retained at its initial value".into());
    }
    if let Some(mode) = base_component(&value, 0).and_then(decode) {
        return mode;
    }
    warnings.push(format!(
        "unrecognized shape blend mode {:?}; Normal retained",
        base_component(&value, 0)
    ));
    BlendMode::Normal
}

/// Native AE ordinals, verified in libpag's AEDataTypeConverter.cpp (not PAG ordinals).
pub(super) fn composite_order(run: &[Chunk], warnings: &mut Vec<String>) -> program::PaintOrder {
    use program::PaintOrder;
    let leaves = match property_group(run, "paint Composite") {
        Ok(leaves) => leaves,
        Err(error) => {
            warnings.push(format!(
                "paint Composite group is malformed ({error}); Below Previous retained"
            ));
            return PaintOrder::BelowPrevious;
        }
    };
    let name = "ADBE Vector Composite Order";
    if leaves.iter().filter(|(key, _)| *key == name).count() > 1 {
        warnings.push("duplicate paint Composite order; Below Previous retained".into());
        return PaintOrder::BelowPrevious;
    }
    let Some(value) = numeric_leaf(&leaves, name, warnings) else {
        return PaintOrder::BelowPrevious;
    };
    if value.animated || value.expression_enabled {
        warnings
            .push("animated/expression paint Composite order retained at its initial value".into());
    }
    match base_component(&value, 0) {
        Some(1.0) => PaintOrder::BelowPrevious,
        Some(2.0) => PaintOrder::AbovePrevious,
        value => {
            warnings.push(format!(
                "unknown paint Composite order {value:?}; Below Previous retained"
            ));
            PaintOrder::BelowPrevious
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_ordinals_are_not_layer_bytes_or_rounded_unknown_values() {
        assert_eq!(decode(4.0), Some(BlendMode::Multiply));
        assert_eq!(decode(10.0), Some(BlendMode::Screen));
        assert_eq!(decode(12.0), Some(BlendMode::Add));
        assert_eq!(decode(29.0), Some(BlendMode::Luminosity));
        for unknown in [0.0, 2.0, 8.0, 14.0, 22.0, 25.0, 30.0, 4.5, f64::NAN] {
            assert_eq!(decode(unknown), None);
        }
    }

    #[test]
    fn malformed_present_blend_groups_keep_defaults_with_diagnostics() {
        let malformed = [Chunk::data(*b"tdb4", vec![0]).unwrap()];
        let mut warnings = Vec::new();
        assert_eq!(from_run(&malformed, &mut warnings), BlendMode::Normal);
        assert_eq!(
            composite_order(&malformed, &mut warnings),
            program::PaintOrder::BelowPrevious
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("shape blend group is malformed"))
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("paint Composite group is malformed"))
        );
    }
}
