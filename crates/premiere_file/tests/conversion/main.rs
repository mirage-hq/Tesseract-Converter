//! Public conversion API tests.
mod adjustment;
mod audio;
mod captions;
mod color_matte;
mod crop;
mod effects;
mod film_impact_pop;
mod frame_hold;
mod graphics;
mod linear_wipe;
mod mask;
mod merged;
mod multicam;
mod nested;
mod object_mask;
mod object_mask_import;
mod opacity;
mod proxy;
mod publication;
mod roundtrip;
mod source_graphic;
mod still_image;
mod subclip;
mod support;
mod test_support {
    include!("../support/mod.rs");
    #[cfg(feature = "ffmpeg-library")]
    include!("../support/media.rs");
}
mod text_shadow;
mod time_remap;
mod timing;
mod track_matte;
mod validation;
mod video_formats;
mod visibility;
