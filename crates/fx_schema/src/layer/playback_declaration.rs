//! Shared wire declaration for a finite playback window and authored mapping.

/// Defines the same typed persisted window in the product reader and public schema.
/// Clock validation and legacy migration stay with the respective readers.
#[doc(hidden)]
#[macro_export]
macro_rules! define_layer_playback_schema {
    (reader: [$($derive:ident),*]) => {
        /// Mapping from a parent-clock sample to a layer-content sample.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS $(, $derive)*)]
        #[serde(tag = "type", rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum LayerPlaybackMapping {
            /// Affine stretch from the authored input span to the output span.
            Linear {
                input: TimeRangeProperty,
                output: TimeRangeProperty,
            },
            /// Editable curve with its original extrapolation and loop phase.
            TimeRemap { property: TimeRemapProperty },
        }

        /// Finite playback interval in immediate-parent time, with a separate mapping.
        #[derive(Debug, Clone, PartialEq, Serialize, TS $(, $derive)*)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct LayerPlayback {
            #[serde(rename = "type")]
            #[ts(type = "\"windowed\"")]
            kind: PlaybackWireType,
            input_range: TimeRangeProperty,
            mapping: LayerPlaybackMapping,
            input_offset_ms: i64,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        enum PlaybackWireType {
            Windowed,
        }
    };
}

/// Exact-integer reader projection of the canonical playback fields.
/// General time readers accept historical rounded numbers; windowed clocks do not.
#[doc(hidden)]
#[macro_export]
macro_rules! define_strict_layer_playback_schema {
    () => {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct StrictRange {
            start: u64,
            duration: u64,
        }

        impl StrictRange {
            fn property(self) -> TimeRangeProperty {
                TimeRangeProperty::new(
                    Time::from_millis(self.start),
                    Duration::from_millis(self.duration),
                )
            }
        }

        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
        enum StrictMapping {
            Linear {
                input: StrictRange,
                output: StrictRange,
            },
            TimeRemap {
                property: TimeRemapProperty,
            },
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct StrictPlayback {
            #[serde(rename = "type")]
            kind: PlaybackWireType,
            input_range: StrictRange,
            mapping: StrictMapping,
            input_offset_ms: i64,
        }
    };
}
