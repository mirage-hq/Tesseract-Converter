//! Canonical tagged animator input schema. Both readers retain their own migrations.

/// Known editable animator fields retained by the lossless public reader.
/// Runtime `PropertyAnimator` instead stores evaluated/migrated state; both
/// projections read the tagged schema declared by `define_wire_property_animator_schema`.
#[doc(hidden)]
#[macro_export]
macro_rules! define_stored_animator_data_schema {
    () => {
        /// Known animator fields, without execution or legacy-clock interpretation.
        #[derive(Debug, Clone, PartialEq, Serialize)]
        #[serde(tag = "type", rename_all = "camelCase")]
        pub enum AnimatorData {
            Constant {
                value: PropertyValue,
            },
            JsScript {
                #[serde(skip_serializing_if = "Option::is_none")]
                code: Option<String>,
                #[serde(rename = "layerTimeJsCode", skip_serializing_if = "Option::is_none")]
                layer_time_js_code: Option<String>,
            },
            Keyframes {
                #[serde(flatten)]
                track: PropertyKeyframeTrack,
                enabled: bool,
                #[serde(rename = "disabledValue", skip_serializing_if = "Option::is_none")]
                disabled_value: Option<PropertyValue>,
            },
        }
    };
}

/// Declare the product's evaluated animator state using canonical variant names.
/// Script migration and keyframe validation remain private runtime behavior;
/// the tagged persisted field contract is `define_wire_property_animator_schema`.
#[doc(hidden)]
#[macro_export]
macro_rules! define_runtime_property_animator_schema {
    () => {
        /// Evaluated animator state attached to one animated property.
        #[derive(Debug, Clone, PartialEq, TS)]
        #[ts(as = "wire::WirePropertyAnimator", export_to = "project_types.d.ts")]
        pub enum PropertyAnimator {
            /// Returns the same value for every frame.
            Constant {
                /// The typed value returned for every frame.
                value: PropertyValue,
            },
            /// A script authored against the owning layer's local clock.
            JsScript {
                /// JavaScript function body evaluated with time zero at the owning
                /// layer's `activeRange.start`.
                code: String,
            },
            /// Native editable values on the owning layer's local clock.
            Keyframes {
                /// Canonical, strictly time-sorted authoring values.
                track: PropertyKeyframeTrack,
                /// Whether the authored keys currently override the static property.
                #[ts(skip)]
                enabled: bool,
                /// Static value rendered while the retained keyframes are disabled.
                #[ts(skip)]
                disabled_value: Option<PropertyValue>,
            },
        }
    };
}

/// Define the borrowed write projection of the persisted animator variants.
/// Runtime ownership and migration remain in the product serializer.
#[doc(hidden)]
#[macro_export]
macro_rules! define_wire_property_animator_ref_schema {
    () => {
        #[derive(Serialize)]
        #[serde(tag = "type", rename_all = "camelCase")]
        enum WirePropertyAnimatorRef<'a> {
            Constant {
                value: &'a PropertyValue,
            },
            JsScript {
                #[serde(rename = "layerTimeJsCode")]
                layer_time_js_code: &'a str,
            },
            Keyframes {
                enabled: bool,
                keyframes: &'a [PropertyKeyframe],
                #[serde(rename = "disabledValue", skip_serializing_if = "Option::is_none")]
                disabled_value: Option<&'a PropertyValue>,
            },
        }
    };
}

/// Emit the persisted animator variant and field declaration for either reader.
#[doc(hidden)]
#[macro_export]
macro_rules! define_wire_property_animator_schema {
    () => {
        // Cross-variant poison fields are intentionally never read: their presence
        // fails deserialization before a value can be constructed.
        #[allow(dead_code)]
        #[derive(Deserialize, TS)]
        #[serde(tag = "type", rename_all = "camelCase")]
        #[ts(rename = "PropertyAnimator", export_to = "project_types.d.ts")]
        pub(super) enum WirePropertyAnimator {
            Constant {
                value: PropertyValue,
                #[serde(default, deserialize_with = "reject_other_animator_variant_field")]
                #[ts(skip)]
                code: (),
                #[serde(
                    default,
                    rename = "layerTimeJsCode",
                    deserialize_with = "reject_other_animator_variant_field"
                )]
                #[ts(skip)]
                layer_time_js_code: (),
                #[serde(default, deserialize_with = "reject_other_animator_variant_field")]
                #[ts(skip)]
                enabled: (),
                #[serde(default, deserialize_with = "reject_other_animator_variant_field")]
                #[ts(skip)]
                keyframes: (),
                #[serde(
                    default,
                    rename = "disabledValue",
                    deserialize_with = "reject_other_animator_variant_field"
                )]
                #[ts(skip)]
                disabled_value: (),
            },
            JsScript {
                #[serde(default)]
                #[ts(skip)]
                code: Option<String>,
                /// JavaScript function body. The runtime calls it with one `input` object.
                /// `input.time.seconds` and `input.time.milliseconds` are the owner layer's local time.
                /// The clock is 0 at the layer's active range start. `input.time` is an object, not a
                /// number. `input.deps[i].value` holds each declared dependency in canonical sorted order.
                /// Return the target's value: a finite number for scalar properties, `[x, y]` for vector
                /// properties, `[r, g, b, a]` in 0 to 1 for color properties, a string for `textContent`.
                #[serde(default, rename = "layerTimeJsCode")]
                #[ts(optional)]
                layer_time_js_code: Option<String>,
                #[serde(default, deserialize_with = "reject_other_animator_variant_field")]
                #[ts(skip)]
                value: (),
                #[serde(default, deserialize_with = "reject_other_animator_variant_field")]
                #[ts(skip)]
                enabled: (),
                #[serde(default, deserialize_with = "reject_other_animator_variant_field")]
                #[ts(skip)]
                keyframes: (),
                #[serde(
                    default,
                    rename = "disabledValue",
                    deserialize_with = "reject_other_animator_variant_field"
                )]
                #[ts(skip)]
                disabled_value: (),
            },
            Keyframes {
                /// When true, evaluate the keyframes and omit disabledValue or set it to null.
                /// When false, render the required non-null disabledValue instead.
                enabled: bool,
                keyframes: Vec<PropertyKeyframe>,
                /// Static value rendered while disabled. Required and non-null when enabled
                /// is false; must be omitted or null when enabled is true.
                #[serde(default, rename = "disabledValue")]
                #[ts(optional, rename = "disabledValue")]
                disabled_value: Option<PropertyValue>,
                #[serde(default, deserialize_with = "reject_other_animator_variant_field")]
                #[ts(skip)]
                value: (),
                #[serde(default, deserialize_with = "reject_other_animator_variant_field")]
                #[ts(skip)]
                code: (),
                #[serde(
                    default,
                    rename = "layerTimeJsCode",
                    deserialize_with = "reject_other_animator_variant_field"
                )]
                #[ts(skip)]
                layer_time_js_code: (),
            },
        }
    };
}
