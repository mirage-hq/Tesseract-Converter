//! Baked snapshot field inventories, independent of migration or storage.

/// Bind snapshot envelope fields to either reader.
#[doc(hidden)]
#[macro_export]
macro_rules! define_snapshot_schema {
    ($emit:ident, $background_write:meta) => {
        $emit! {
            /// Root FX composition document.
            #[ts(export_to = "project_types.d.ts", rename = "FxComposition", rename_all = "camelCase", concrete(C = Composition))]
            pub struct FxCompositionDocument<C> {
                /// Additive schema revision authored by the writer.
                ///
                /// Readers do not reject a newer revision by itself. Compatibility is
                /// determined by the actual fields.
                pub version: u8,
                #[$background_write]
                pub background_color: Option<Color>,
                /// The single root composition rendered for this document.
                pub composition: C,
            }
        }
    };
}

/// Bind composition fields without duplicating their persisted shape.
#[doc(hidden)]
#[macro_export]
#[allow(clippy::crate_in_macro_def)] // Duration belongs to the reader's existing time facade.
macro_rules! define_snapshot_composition_schema {
    ($emit:ident, $defaults:meta, $motion_write:meta, $duration_reader:meta) => {
        $emit! {
            /// Root AE/PAG-style composition.
            #[ts(export_to = "project_types.d.ts")]
            pub struct Composition {
                pub id: CompositionId,
                pub name: String,
                /// AE composition-level standard motion-blur controls.
                #[$defaults]
                #[$motion_write]
                pub motion_blur: MotionBlurSettings,
                /// Composition width in pixels.
                pub width: u32,
                /// Composition height in pixels.
                pub height: u32,
                /// Composition duration. Serialized as fractional seconds (`f64`) to
                /// keep the stored-document wire format; see [`crate::time::serde_secs`].
                #[ts(type = "number")]
                #[$duration_reader]
                pub duration: crate::time::Duration,
                /// AE layer stack. The first item is index 1 and renders above later items.
                pub layers: Vec<Layer>,
            }
        }
    };
}
