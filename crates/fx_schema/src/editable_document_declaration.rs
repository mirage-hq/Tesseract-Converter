//! Editable document envelope fields; storage and strictness are reader policies.

/// Provide one envelope inventory for authoring, parsing and lossless storage.
#[doc(hidden)]
#[macro_export]
#[allow(clippy::crate_in_macro_def)] // Reuse the caller's time facade without relocating policy.
macro_rules! define_editable_document_schema {
    ($emit:ident) => {
        $emit! {
            /// One self-contained, editable FX project document.
            ///
            /// This envelope persists the canonical editable
            /// [`FXComposition`] animation graph rather than only a concrete render tree.
            /// Unknown document-envelope fields are retained across typed edits and saves;
            /// `FXComposition` separately retains unknown fields inside the composition.
            #[serde(rename_all = "camelCase", bound(serialize = "C: AsRef<FXComposition>"))]
            pub struct EditableFxDocument<C> {
                #[serde(rename = "$schema")]
                schema: String,
                format_version: u8,
                dimensions: DimensionsWire,
                #[serde(with = "crate::time::serde_secs::duration")]
                duration: crate::time::Duration,
                #[serde(skip_serializing_if = "Option::is_none")]
                background_color: Option<Color>,
                #[serde(serialize_with = "serialize_composition")]
                composition: C,
                #[serde(flatten)]
                unknown_fields: BTreeMap<String, Value>,
            }
        }
    };
}

/// Dimensions use the same wire fields with reader-selected extra-field policy.
#[doc(hidden)]
#[macro_export]
macro_rules! define_document_dimensions_schema {
    ($($attrs:meta),*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        $(#[$attrs])*
        struct DimensionsWire {
            width: u32,
            height: u32,
        }
    };
}
