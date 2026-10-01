//! Canonical persisted time-remap keyframes, data and wire fields.

/// Define the shared keyframe fields with reader-specific strictness.
#[doc(hidden)]
#[macro_export]
macro_rules! define_time_remap_keyframe_schema {
    ($($strict:ident)?) => {
        /// One parent-clock to content/source-clock point on a TimeRemap curve.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $(, $strict)?)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct TimeRemapKeyframe {
            /// Stable caller-assigned keyframe identity.
            pub id: KeyframeId,
            /// Coordinate in the layer's immediate-parent clock.
            pub time: Time,
            /// Coordinate in the Group content or media source clock.
            pub value: Time,
            /// Temporal easing while arriving from the previous keyframe.
            pub easing: PropertyKeyframeEasing,
        }
    };
}

/// Bind the same authored time mapping to a runtime cache or lossless storage.
#[doc(hidden)]
#[macro_export]
macro_rules! define_time_remap_property_schema {
    ($emit:ident) => {
        $emit! {
            /// Strict persisted mapping from immediate-parent time to content/source time.
            pub struct TimeRemapProperty {
                keyframes: Vec<TimeRemapKeyframe>,
                before: TimeRemapExtrapolation,
                after: TimeRemapExtrapolation,
            }
        }
    };
}

/// Define the validated time-remap wire fields for either reader.
#[doc(hidden)]
#[macro_export]
macro_rules! define_time_remap_wire_schema {
    ($($strict:ident)?) => {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase" $(, $strict)?)]
        struct Wire {
            keyframes: Vec<TimeRemapKeyframe>,
            before: TimeRemapExtrapolation,
            after: TimeRemapExtrapolation,
        }
    };
}
