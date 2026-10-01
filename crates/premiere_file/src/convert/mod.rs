//! Typed mappings between Premiere and Tesseract document models.

mod adjustment;
mod after_effects;
mod audio;
mod background;
mod color_matte;
mod effects;
mod fonts;
mod graphic;
mod keyframes;
mod linked_shadow_scale;
mod nested;
mod packing;
mod premiere_to_tesseract;
mod replacement;
mod script_bake;
mod still;
mod tesseract_to_premiere;
mod text;
mod text_shadow;
mod timing;
mod video_data;
#[cfg(test)]
#[path = "tests/visibility.rs"]
mod visibility;

pub(crate) use audio::embedded_sound_asset;
#[cfg(test)]
pub(crate) use background::identity_transform;
pub(crate) use background::validate_gap_coverage;
pub(crate) use replacement::active_asset_id;
pub(crate) use video_data::video_data;

pub(crate) use nested::{
    exported_audio_layers, exported_clip_videos, exported_image_layers, exported_video_layers,
};
pub(crate) use packing::apply_replacements;
pub use packing::{
    AfterEffectsPicture, PictureContainer, PictureContainerToken, PicturePackingId,
    PicturePackingRecipe, PictureReplacement, PictureSourceBoundary, SourceBoundaryToken,
};
pub(crate) use premiere_to_tesseract::sequence_document_with_progress;
#[cfg(test)]
pub(crate) use premiere_to_tesseract::{premiere_to_tesseract, sequence_document};
#[cfg(test)]
pub(crate) use script_bake::{bake_scripts, PREPARATION_CALLS};
pub(crate) use script_bake::{bake_scripts_with_progress, BakedDocument};
#[cfg(test)]
pub(crate) use tesseract_to_premiere::export_document;
#[cfg(test)]
pub(crate) use tesseract_to_premiere::lower_document;
#[cfg(test)]
pub(crate) use tesseract_to_premiere::tesseract_to_premiere;
pub(crate) use tesseract_to_premiere::{lower_document_with_progress, no_native_content};
