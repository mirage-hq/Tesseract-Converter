//! One declaration of persisted color-transform payloads for both reader policies.

/// Retain supported and opaque input transforms with the caller's strictness.
#[doc(hidden)]
#[macro_export]
macro_rules! define_persisted_input_transform_schema {
    () => {
        #[derive(Debug, Clone, PartialEq)]
        enum PersistedInputTransformValue {
            Supported(InputTransform),
            Unsupported(serde_json::Value),
        }

        /// A persisted Input Transform that can retain a future semantic payload.
        ///
        /// This product reader wraps the strict local projection of the shared
        /// declaration, rather than migrating future payloads in the public reader.
        #[derive(Debug, Clone, PartialEq)]
        pub struct PersistedInputTransform(PersistedInputTransformValue);
    };
}

/// Define strict input and transform payloads with the caller's known-field policy.
#[doc(hidden)]
#[macro_export]
macro_rules! define_color_transform_schema {
    ($($strict:meta)?) => {
        /// Canonical transform representations supported by semantic version 1.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(tag = "type", rename_all = "camelCase" $(, $strict)?)]
        #[ts(export_to = "project_types.d.ts")]
        pub enum ColorTransform {
            /// A three-dimensional color lookup table.
            Lut3d {
                /// Captions asset containing a Phase-0 `.cube` file.
                #[serde(rename = "assetId")]
                asset_id: AssetId,
                /// Encoding expected at the LUT input.
                #[serde(rename = "inputEncoding")]
                #[ts(type = "string")]
                input_encoding: ColorEncodingId,
                /// Encoding produced by the LUT.
                #[serde(rename = "outputEncoding")]
                #[ts(type = "string")]
                output_encoding: ColorEncodingId,
                /// Phase-0 interpolation contract.
                interpolation: ColorLutInterpolation,
            },
        }

        /// Strict canonical Input Transform payload.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $(, $strict)?)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct InputTransform {
            /// Version fixing ordering, encoding, math, alpha, and fallback semantics.
            #[ts(type = "number")]
            pub semantic_version: ColorTransformSemanticVersion,
            /// Transform representation evaluated before ordinary layer effects.
            pub transform: ColorTransform,
        }
    };
}
