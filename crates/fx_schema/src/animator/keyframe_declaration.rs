//! Shared keyframe wire field and variant ownership.

/// Declare the checked keyframe track's persisted envelope.
#[doc(hidden)]
#[macro_export]
macro_rules! define_keyframe_track_reader_schema {
    ($emit:ident, $($strict:ident)?) => {
        $emit! {
            #[serde(rename_all = "camelCase" $(, $strict)?)]
            struct Wire {
                keyframes: Vec<PropertyKeyframe>,
            }
        }
    };
}

/// Define the keyframe schema with optional strict product-only reader attributes.
#[doc(hidden)]
#[macro_export]
macro_rules! define_property_keyframe_schema {
    ($($strict:ident)?) => {
        /// Stable caller-assigned identity of one property keyframe.
        ///
        /// Callers should use UUIDs. FX readers treat the value as an opaque identifier so
        /// it can remain stable while a keyframe moves or changes value.
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
        #[serde(transparent)]
        #[ts(type = "string")]
        pub struct KeyframeId(String);

        /// Easing applied while arriving at one keyframe from the previous keyframe.
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
        #[serde(tag = "type", rename_all = "camelCase" $(, $strict)?)]
        #[ts(export_to = "project_types.d.ts")]
        pub enum PropertyKeyframeEasing {
            /// Keep the previous value until arriving at this keyframe.
            Hold,
            /// Interpolate with unmodified normalized progress.
            Linear,
            /// Interpolate using a Web-compatible cubic Bézier timing function.
            CubicBezier {
                /// First control point x coordinate, in `0..=1`.
                x1: f64,
                /// First control point y coordinate; overshoot is allowed.
                y1: f64,
                /// Second control point x coordinate, in `0..=1`.
                x2: f64,
                /// Second control point y coordinate; overshoot is allowed.
                y2: f64,
            },
        }

        /// One value on a property keyframe track.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $(, $strict)?)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct PropertyKeyframe {
            #[ts(type = "string")]
            id: KeyframeId,
            layer_time: TimeOffset,
            value: PropertyValue,
            /// Applies from the previous keyframe to this one; ignored while first.
            easing: PropertyKeyframeEasing,
            /// This axis's offset from the key to the incoming spatial control point.
            ///
            /// Only valid for `PositionX` and `PositionY`. The paired axis supplies the
            /// other component of the two-dimensional tangent.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            spatial_in_tangent: Option<f64>,
            /// This axis's offset from the key to the outgoing spatial control point.
            ///
            /// Only valid for `PositionX` and `PositionY`. The paired axis supplies the
            /// other component of the two-dimensional tangent.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            spatial_out_tangent: Option<f64>,
        }

        /// Validated, strictly time-sorted property keyframes.
        #[derive(Debug, Clone, PartialEq, Serialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct PropertyKeyframeTrack {
            keyframes: Vec<PropertyKeyframe>,
        }


    };
}
