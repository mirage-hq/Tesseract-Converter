//! Typed mappings between Premiere and Tesseract document models.

mod adjustment;
mod adjustment_geometry;
mod adjustment_wipe;
mod after_effects;
mod audio;
mod audio_groups;
mod background;
mod channel_levels;
mod color_matte;
mod corner_path;
mod cross_dissolve;
mod effect_mask;
mod effects;
mod fonts;
mod graphic;
mod invert_alpha;
mod keyframes;
mod linked_audio;
mod linked_shadow_scale;
mod mask_animation;
mod nested;
mod packing;
mod playback_segments;
mod premiere_to_tesseract;
mod replacement;
mod script_bake;
mod still;
mod tesseract_to_premiere;
mod text;
mod text_shadow;
mod time_remap;
mod timed_images;
mod timing;
mod video_data;
mod video_fitting;
#[cfg(test)]
#[path = "tests/visibility.rs"]
mod visibility;

pub(crate) use audio::embedded_sound_asset;
#[cfg(test)]
pub(crate) use background::identity_transform;
#[cfg(test)]
pub(crate) use effects::LINKED_SOURCE_EDITING_REASON;
pub(crate) use replacement::active_asset_id;
pub(crate) use video_data::video_data;

pub(crate) use nested::{
    exported_audio_layers, exported_clip_videos, exported_image_layers, exported_video_layers,
};
pub(crate) use packing::{apply_empty_root_picture, apply_replacements};
pub use packing::{
    AfterEffectsPicture, PictureContainer, PictureContainerToken, PicturePackingId,
    PicturePackingRecipe, PictureReplacement, PictureSourceBoundary, SourceBoundaryToken,
};
pub(crate) use premiere_to_tesseract::{omit_pop_emulation, sequence_document_with_progress};
#[cfg(test)]
pub(crate) use premiere_to_tesseract::{premiere_to_tesseract, sequence_document};
#[cfg(test)]
pub(crate) use script_bake::bake_scripts;
#[cfg(all(test, feature = "ffmpeg-library"))]
pub(crate) use script_bake::PREPARATION_CALLS;
pub(crate) use script_bake::{bake_scripts_with_progress, BakedDocument};
#[cfg(test)]
pub(crate) use tesseract_to_premiere::export_document;
#[cfg(test)]
pub(crate) use tesseract_to_premiere::lower_document;
#[cfg(test)]
pub(crate) use tesseract_to_premiere::tesseract_to_premiere;
pub(crate) use tesseract_to_premiere::{lower_document_with_progress, no_native_content};
