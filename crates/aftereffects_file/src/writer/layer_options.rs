//! Checked native layer-envelope options shared by all fresh layer writers.

use fx_schema::LayerId;

use crate::{rifx::Chunk, schema::layer_records::LayerRecord};

use super::{
    AepWriteError, NativeMaskSpec, NativeTransform3d, Transform3dAnimations,
    source_clock::SourceClockPlan,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeMatteRef {
    pub layer: LayerId,
    pub mode: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeLayerOptions {
    pub fx_id: LayerId,
    pub parent: Option<LayerId>,
    pub matte: Option<NativeMatteRef>,
    pub enabled: bool,
    /// The fresh solid source is an adjustment gate, not drawable content.
    pub adjustment_layer: bool,
    pub motion_blur: bool,
    pub blend_mode: u8,
    /// Fresh owner-local masks, attached after content and 3D Transform authoring.
    pub masks: Vec<NativeMaskSpec>,
    /// Fresh effect instances for this occurrence, never inherited by children.
    pub effects: Vec<super::effects::NativeEffect>,
    /// Fresh native Layer Styles, separate from the Effect Parade.
    pub styles: Vec<crate::layer_styles::NativeLayerStyle>,
    /// Final source clock for a source-backed precomposition occurrence.
    /// Footage owns its clock directly and must leave this as `None`.
    pub source_clock: Option<SourceClockPlan>,
    /// Fresh native 3D Transform replacement for this emitted occurrence.
    pub transform_3d: Option<(NativeTransform3d, Transform3dAnimations)>,
}

impl NativeLayerOptions {
    /// Assigns one transform parent without conflating it with containment.
    pub(crate) fn with_parent(mut self, parent: LayerId) -> Result<Self, AepWriteError> {
        if self.fx_id == parent {
            return Err(AepWriteError::Invalid(
                "self-referencing FX transform parent",
            ));
        }
        if self.parent.replace(parent).is_some() {
            return Err(AepWriteError::Invalid("duplicate FX transform parent"));
        }
        Ok(self)
    }
}

pub(super) fn apply(
    layer: &mut Chunk,
    options: &NativeLayerOptions,
    parent_id: u32,
    matte_id: u32,
) -> Result<(), AepWriteError> {
    let Some(records) = layer.children_mut() else {
        return Err(AepWriteError::Invalid(
            "native timeline layer is not a LIST",
        ));
    };
    let Some(record) = records.iter_mut().find(|child| child.id() == *b"ldta") else {
        return Err(AepWriteError::Invalid("native timeline layer has no ldta"));
    };
    let bytes = record
        .data_payload()
        .ok_or(AepWriteError::Invalid("invalid native ldta"))?;
    let matte_type = options.matte.map_or(0, |matte| matte.mode);
    let updated = LayerRecord::decode(bytes)?
        .with_adjustment_layer(options.adjustment_layer)?
        .with_export_options(
            options.enabled,
            options.motion_blur,
            options.blend_mode,
            parent_id,
            matte_id,
            matte_type,
        )?;
    *record = Chunk::data(*b"ldta", updated.encode())?;
    Ok(())
}
