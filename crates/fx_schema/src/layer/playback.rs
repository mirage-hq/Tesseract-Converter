//! Canonical typed declaration for the windowed persisted playback format.

use crate::{Duration, Time, TimeRangeProperty, TimeRemapProperty};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use ts_rs::TS;

#[path = "playback_declaration.rs"]
mod declaration;

crate::define_layer_playback_schema! { reader: [] }
crate::define_strict_layer_playback_schema!();

const MAX_EXACT_MS: u64 = (1_u64 << 53) - 1;

impl<'de> Deserialize<'de> for LayerPlayback {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = StrictPlayback::deserialize(deserializer)?;
        let mapping = match wire.mapping {
            StrictMapping::Linear { input, output } => LayerPlaybackMapping::Linear {
                input: input.property(),
                output: output.property(),
            },
            StrictMapping::TimeRemap { property } => LayerPlaybackMapping::TimeRemap { property },
        };
        let playback = Self {
            kind: wire.kind,
            input_range: wire.input_range.property(),
            mapping,
            input_offset_ms: wire.input_offset_ms,
        };
        playback.validate().map_err(D::Error::custom)?;
        Ok(playback)
    }
}

impl LayerPlayback {
    /// Constructs a validated affine playback mapping.
    pub fn linear(
        input_range: TimeRangeProperty,
        input: TimeRangeProperty,
        output: TimeRangeProperty,
        input_offset_ms: i64,
    ) -> Result<Self, &'static str> {
        let playback = Self {
            kind: PlaybackWireType::Windowed,
            input_range,
            mapping: LayerPlaybackMapping::Linear { input, output },
            input_offset_ms,
        };
        playback.validate()?;
        Ok(playback)
    }

    /// Constructs a validated editable time-remap playback mapping.
    pub fn remapped(
        input_range: TimeRangeProperty,
        property: TimeRemapProperty,
        input_offset_ms: i64,
    ) -> Result<Self, &'static str> {
        let playback = Self {
            kind: PlaybackWireType::Windowed,
            input_range,
            mapping: LayerPlaybackMapping::TimeRemap { property },
            input_offset_ms,
        };
        playback.validate()?;
        Ok(playback)
    }

    /// Immediate-parent-local visible interval, independent of the mapping domain.
    pub fn input_range(&self) -> TimeRangeProperty {
        self.input_range
    }

    /// Authored source mapping, independent of the visible input window.
    pub fn mapping(&self) -> &LayerPlaybackMapping {
        &self.mapping
    }

    /// Authored editable curve, if this is not an affine mapping.
    pub fn time_remap(&self) -> Option<&TimeRemapProperty> {
        match &self.mapping {
            LayerPlaybackMapping::Linear { .. } => None,
            LayerPlaybackMapping::TimeRemap { property } => Some(property),
        }
    }

    /// Offset applied to parent-clock samples before evaluating the mapping.
    pub fn input_offset_ms(&self) -> i64 {
        self.input_offset_ms
    }

    /// Validates exact millisecond bounds without evaluating animated properties.
    pub fn validate(&self) -> Result<(), &'static str> {
        fn valid(range: TimeRangeProperty) -> bool {
            let start = range.start.as_millis();
            let duration = range.duration.as_millis();
            duration > 0 && start <= MAX_EXACT_MS && duration <= MAX_EXACT_MS - start
        }
        if self.input_offset_ms.unsigned_abs() > MAX_EXACT_MS {
            return Err("windowed playback offset must be an exact millisecond value");
        }
        if !valid(self.input_range) {
            return Err("windowed playback inputRange must be positive and exact");
        }
        if let LayerPlaybackMapping::Linear { input, output } = &self.mapping {
            if !valid(*input) || !valid(*output) {
                return Err("windowed playback mapping range must be positive and exact");
            }
        }
        for bound in [self.input_range.start, self.input_range.end()] {
            let shifted = i128::from(bound.as_millis()) + i128::from(self.input_offset_ms);
            let minimum = match &self.mapping {
                LayerPlaybackMapping::Linear { .. } => -i128::from(MAX_EXACT_MS),
                LayerPlaybackMapping::TimeRemap { .. } => 0,
            };
            if !(minimum..=i128::from(MAX_EXACT_MS)).contains(&shifted) {
                return Err("windowed playback offset moves the input outside exact clock bounds");
            }
            if let LayerPlaybackMapping::Linear { input, output } = &self.mapping {
                let rate = output.duration.as_millis() as f64 / input.duration.as_millis() as f64;
                let mapped = output.start.as_millis() as f64
                    + (shifted as f64 - input.start.as_millis() as f64) * rate;
                if !(0.0..=MAX_EXACT_MS as f64).contains(&mapped) {
                    return Err("windowed playback maps outside exact content clock bounds");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn playback() -> serde_json::Value {
        json!({
            "type": "windowed",
            "inputRange": {"start": 200, "duration": 300},
            "mapping": {
                "type": "linear",
                "input": {"start": 100, "duration": 1000},
                "output": {"start": 500, "duration": 2000}
            },
            "inputOffsetMs": -100
        })
    }

    #[test]
    fn rejects_inexact_offset_even_when_shifted_window_is_in_bounds() {
        let mut wire = playback();
        wire["inputRange"] = json!({"start": MAX_EXACT_MS - 10, "duration": 10});
        wire["mapping"] = json!({
            "type": "linear", "input": {"start": 0, "duration": 100},
            "output": {"start": 100, "duration": 100}
        });
        wire["inputOffsetMs"] = json!(-(MAX_EXACT_MS as i64) - 2);
        assert!(serde_json::from_value::<LayerPlayback>(wire).is_err());
    }

    #[test]
    fn preserves_independent_window_mapping_and_signed_offset() {
        let wire = playback();
        let parsed: LayerPlayback = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(parsed.input_range().start.as_millis(), 200);
        assert_eq!(parsed.input_range().duration.as_millis(), 300);
        assert_eq!(parsed.input_offset_ms(), -100);
        assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
    }

    #[test]
    fn exact_windowed_reader_rejects_malformed_and_unknown_clock_data() {
        for (path, invalid) in [
            ("/inputRange/start", json!(1.5)),
            ("/inputRange/start", json!(1.0)),
            ("/inputRange/start", json!(-1)),
            ("/inputRange/start", json!(MAX_EXACT_MS)),
            ("/inputRange/duration", json!(0)),
            ("/mapping/input/duration", json!(0)),
            ("/mapping/output/duration", json!(u64::MAX)),
            ("/inputOffsetMs", json!(-1000)),
            ("/inputOffsetMs", json!(i64::MAX)),
        ] {
            let mut wire = playback();
            *wire.pointer_mut(path).unwrap() = invalid;
            assert!(
                serde_json::from_value::<LayerPlayback>(wire).is_err(),
                "{path}"
            );
        }
        for path in ["", "/inputRange", "/mapping", "/mapping/output"] {
            let mut wire = playback();
            wire.pointer_mut(path).unwrap()["futureClock"] = json!(true);
            assert!(
                serde_json::from_value::<LayerPlayback>(wire).is_err(),
                "{path}"
            );
        }
    }
}
