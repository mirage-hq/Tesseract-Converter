//! Typed static values for bounded Film Impact control profiles.
use crate::error::{ensure, unsupported, Result};

/// Each control declares whether its value affects the admitted picture.
#[derive(Debug, Clone, Copy)]
pub(in crate::format::reader) enum Rule {
    Number(f64),
    Boolean(bool),
    Point([f64; 2]),
    Colour(u64),
    /// UI expansion state only. Structure and static-key encoding still validate.
    Inert,
    /// Version stamps are metadata, not a rendering capability boundary.
    Version,
    /// An authored numeric control; the caller checks supported combinations.
    Range {
        min: f64,
        max: f64,
    },
    /// No opaque Curve Graph payload is admitted.
    EmptyCurveGraph,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::format::reader) enum Value {
    Number(f64),
    Boolean(bool),
    Point([f64; 2]),
    Colour(u64),
}

fn number(text: &str) -> Result<f64> {
    let value = text
        .parse::<f64>()
        .map_err(|_| unsupported("invalid Film Impact number"))?;
    ensure!(value.is_finite(), "nonfinite Film Impact number");
    Ok(value)
}

impl Rule {
    pub(super) fn value(self, text: &str) -> Result<Value> {
        let value = match self {
            Self::Boolean(_) => match text {
                "true" => Value::Boolean(true),
                "false" => Value::Boolean(false),
                _ => return Err(unsupported("invalid Film Impact boolean")),
            },
            Self::Point(_) => {
                let (x, y) = text
                    .split_once(':')
                    .ok_or_else(|| unsupported("invalid Film Impact point"))?;
                Value::Point([number(x)?, number(y)?])
            }
            Self::Colour(_) => Value::Colour(
                text.parse::<u64>()
                    .map_err(|_| unsupported("invalid Film Impact packed colour"))?,
            ),
            Self::Inert => match text {
                "true" => Value::Boolean(true),
                "false" => Value::Boolean(false),
                _ => Value::Number(number(text)?),
            },
            Self::Number(_) | Self::Range { .. } | Self::Version => Value::Number(number(text)?),
            Self::EmptyCurveGraph => {
                return Err(unsupported("unexpected Film Impact Curve Graph value"))
            }
        };
        let accepted = match (self, value) {
            (Self::Number(expected), Value::Number(actual)) => actual == expected,
            (Self::Boolean(expected), Value::Boolean(actual)) => actual == expected,
            (Self::Point(expected), Value::Point(actual)) => actual == expected,
            (Self::Colour(expected), Value::Colour(actual)) => actual == expected,
            (Self::Range { min, max }, Value::Number(actual)) => (min..=max).contains(&actual),
            (Self::Version, Value::Number(actual)) => actual.fract() == 0.0,
            (Self::Inert, Value::Number(_) | Value::Boolean(_)) => true,
            _ => false,
        };
        ensure!(accepted, "unsupported Film Impact static control value");
        Ok(value)
    }

    pub(super) fn default_value(self) -> Option<Value> {
        match self {
            Self::Number(value) => Some(Value::Number(value)),
            Self::Boolean(value) => Some(Value::Boolean(value)),
            Self::Point(value) => Some(Value::Point(value)),
            Self::Colour(value) => Some(Value::Colour(value)),
            Self::Inert | Self::Version | Self::Range { .. } | Self::EmptyCurveGraph => None,
        }
    }

    pub(super) fn key(self, text: &str, _point: bool) -> Result<Value> {
        let value = text
            .split(',')
            .nth(1)
            .ok_or_else(|| unsupported("missing Film Impact authored control value"))?;
        self.value(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn film_impact_version_does_not_gate_usable_controls_by_patch_layout() {
        for version in [
            "260299",
            "260300",
            "260399",
            "260400",
            "270100",
            "2.60399e5",
        ] {
            assert!(Rule::Version.value(version).is_ok(), "{version}");
        }
        for version in ["260301.5", "inf"] {
            assert!(Rule::Version.value(version).is_err(), "{version}");
        }
    }

    #[test]
    fn typed_rules_keep_static_and_precision_boundaries() {
        assert_eq!(
            Rule::Number(6.0)
                .key("-91445760000000000,6e0,0.,0,0,0,0,0", false)
                .unwrap(),
            Value::Number(6.0)
        );
        assert!(Rule::Number(6.0).key("0,6,0,0,0,0,0,0", false).is_ok());
        assert!(Rule::Number(6.0)
            .key("-91445760000000000,6,0,0,0,0,0,1", false)
            .is_ok());
        assert!(Rule::Number(6.0).value("NaN").is_err());
        assert!(Rule::Inert.value("true").is_ok());
        let colour = 18374966859414961920;
        assert!(Rule::Colour(colour).value("18374966859414961920").is_ok());
        assert!(Rule::Colour(colour).value("18374966859414961921").is_err());
        assert!(Rule::Point([0.5, 0.5]).value("0.5:0.5").is_ok());
        assert!(Rule::Point([0.5, 0.5]).value("0.5:NaN").is_err());
        assert!(Rule::Range {
            min: 0.0,
            max: 100.0
        }
        .value("100")
        .is_ok());
        assert!(Rule::Range {
            min: 0.0,
            max: 100.0
        }
        .value("101")
        .is_err());
    }
}
