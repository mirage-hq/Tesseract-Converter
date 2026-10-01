//! Canonical inventories for serialized layer projections.

/// Layer variants emitted by the split wire writer.
#[doc(hidden)]
#[macro_export]
macro_rules! define_wire_layer_ref_schema {
    ($emit:ident) => {
        $emit! {
            enum WireLayerRef<'a> {
                Text(&'a TextLayer),
                Video(&'a VideoLayer),
                Image(&'a ImageLayer),
                Pag(&'a PagLayer),
                Rect(&'a RectLayer),
                Shape(&'a ShapeLayer),
                Audio(&'a AudioLayer),
                Group(&'a GroupLayer),
                AiEdit(&'a AiEditLayer),
                BooleanOperation(&'a BooleanOperationLayer),
                Adjustment(&'a AdjustmentLayer),
            }
        }
    };
}

/// Legacy media discriminant used by the compatibility writer.
#[doc(hidden)]
#[macro_export]
macro_rules! define_legacy_wire_layer_ref_schema {
    ($emit:ident, $media:ident) => {
        $emit! { enum LegacyWireLayerRef<'a> { Media($media<'a>), } }
    };
}
