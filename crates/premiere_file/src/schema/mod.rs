//! Shared typed data for the supported Premiere project subset.

pub(crate) mod adjustment;
pub(crate) mod after_effects;
pub use after_effects::PrAfterEffectsComposition;

mod audio;
pub(crate) mod caption;
mod color;
pub(crate) mod color_matte;
mod crop;
mod effects;
mod mask;
mod motion;
pub(crate) mod native;
mod nested;
mod opacity;
pub(crate) mod records;
pub(crate) mod spatial;
mod still;
pub(crate) mod text;
pub(crate) mod text_shadow;
mod timeline;
mod timing;
pub(crate) mod video_codec;

pub(crate) use audio::{
    AudioChannels, CustomFadeShape, PrAudioFade, PrAudioOccurrence, PrAudioSourceChannel,
    PrAudioStream, PrFadeCurve, PrVolumeKeys, PrVolumeLayout, RUNTIME_PITCH_RATES,
};
pub(crate) use color::{ColorSpace, HdrProfile, ToneMapSettings};
pub(crate) use color_matte::PrColorMatte;
pub(crate) use crop::{PrStaticCrop, CROP_PARAMS, CROP_PARAMS_26_5, CROP_PARAM_COUNT};
pub(crate) use effects::{
    alpha_glow_size, lens_curvature, scalar_range, turn_corners, turns, PrPosterize, ALPHA_GLOW,
    ALPHA_GLOW_APPROXIMATION, ALPHA_GLOW_BRIGHTNESS, ALPHA_GLOW_END, ALPHA_GLOW_FADE,
    ALPHA_GLOW_SIZE, ALPHA_GLOW_START, ALPHA_GLOW_USE_END, LENS_APPROXIMATION, LENS_CURVATURE,
    LENS_DISTORTION, POSTERIZE, POSTERIZE_LEVEL,
};
pub(crate) use effects::{
    chain_render_order, is_coverage_effect, normalize_legacy_luma, validate_legacy_luma,
    EffectParamBinding, EffectParamSpec, EffectSpec, PrBrightnessContrast, PrColour,
    PrColourKeyframe, PrCornerPin, PrDirectionalBlur, PrEffect, PrEffectParamAnimation,
    PrEffectParamKeys, PrEffectParams, PrFilmImpactBlur, PrFilmImpactDirectionalBlur, PrFindEdges,
    PrGaussianBlur, PrInvert, PrLevelChannel, PrLevels, PrMosaic, PrRamp, PrTint, PrTransform,
    ADJUSTMENT_GEOMETRY2, BLACK_WHITE, BLUR_DIMENSIONS_HORIZONTAL_AND_VERTICAL,
    BRIGHTNESS_CONTRAST, BRIGHTNESS_CONTRAST_BRIGHTNESS, BRIGHTNESS_CONTRAST_CONTRAST, CORNER_PIN,
    DIRECTIONAL_BLUR, DIRECTIONAL_BLUR_DIRECTION, DIRECTIONAL_BLUR_LENGTH, FILM_IMPACT_BLUR,
    FILM_IMPACT_BLUR_26_2, FILM_IMPACT_BLUR_26_2_DEFAULTS, FILM_IMPACT_BLUR_AMOUNT,
    FILM_IMPACT_BLUR_ANGLE, FILM_IMPACT_BLUR_CHROMATIC, FILM_IMPACT_BLUR_CONTROLS,
    FILM_IMPACT_BLUR_DEFAULTS, FILM_IMPACT_BLUR_EDGE, FILM_IMPACT_BLUR_THICKNESS,
    FILM_IMPACT_BLUR_UNIFORM, FILM_IMPACT_DIRECTIONAL_BLUR, FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT,
    FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE, FILM_IMPACT_DIRECTIONAL_BLUR_DEFAULTS, FIND_EDGES,
    FIND_EDGES_BLEND, FIND_EDGES_INVERT, GAUSSIAN_BLUR, GAUSSIAN_BLUR_BLURRINESS,
    GAUSSIAN_BLUR_DIMENSIONS, GAUSSIAN_BLUR_MAX_BLURRINESS, GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS,
    INVERT, INVERT_BLEND, INVERT_CHANNEL, INVERT_CHANNEL_RGB, LEGACY_LUMA_CUTOFF, LEGACY_LUMA_KEY,
    LEGACY_LUMA_KEY_MAPPING_REASON, LEGACY_LUMA_KEY_MATCH_NAME, LEGACY_LUMA_THRESHOLD, LEVELS,
    LEVELS_NEUTRAL, LUMETRI_EXPOSURE, LUMETRI_SATURATION, LUMETRI_TEMPERATURE, LUMETRI_TINT,
    LUMETRI_VIGNETTE_AMOUNT, LUMETRI_VIGNETTE_FEATHER, LUMETRI_VIGNETTE_MIDPOINT,
    MASK_EFFECT_ORDER_REASON, MOSAIC, MOSAIC_HORIZONTAL_BLOCKS, MOSAIC_SHARP_COLORS,
    MOSAIC_VERTICAL_BLOCKS, OFFSET, OFFSET_CENTER, PREMIERE_NATIVE_FILTER_VERSIONS,
    PREMIERE_NATIVE_PARAMETER_ID, RAMP, RAMP_BLEND, RAMP_END, RAMP_END_COLOR, RAMP_SCATTER,
    RAMP_SHAPE, RAMP_SHAPE_LINEAR, RAMP_START, RAMP_START_COLOR, TINT, TINT_AMOUNT,
    TINT_MAP_BLACK_TO, TINT_MAP_WHITE_TO, TRACK_MATTE_KEY, TRACK_MATTE_KEY_COMPOSITE,
    TRACK_MATTE_KEY_MATTE, TRACK_MATTE_KEY_REVERSE, TRANSFORM, TRANSFORM_ANCHOR_POINT,
    TRANSFORM_COMPOSITION_SHUTTER_ANGLE, TRANSFORM_OPACITY, TRANSFORM_POSITION, TRANSFORM_ROTATION,
    TRANSFORM_SAMPLING, TRANSFORM_SAMPLING_BICUBIC, TRANSFORM_SAMPLING_BILINEAR,
    TRANSFORM_SCALE_HEIGHT, TRANSFORM_SCALE_WIDTH, TRANSFORM_SHUTTER_ANGLE, TRANSFORM_SKEW,
    TRANSFORM_SKEW_AXIS, TRANSFORM_UNIFORM_SCALE,
};
pub(crate) use effects::{PrReplicate, REPLICATE, REPLICATE_COUNT};
pub(crate) use effects::{PrSharpen, SHARPEN, SHARPEN_AMOUNT};
pub(crate) use effects::{
    MODERN_NOISE, NOISE, NOISE_AMOUNT, NOISE_APPROXIMATION, NOISE_CLIPPING, NOISE_COLOR,
};
pub(crate) use effects::{POSTERIZE_TIME, POSTERIZE_TIME_FRAME_RATE};
pub(crate) use mask::mask_numeric_easing;
pub(crate) use mask::{
    at_default, decode_mask_tracker, encode_mask_path, is_saved_tracker_state, mask_match_name,
    MaskControl, MaskForm, MaskParamRole, MaskTrackerTransform, PrMask, PrMaskPathKey, RasterMask,
    MASK_FORM_V7, MASK_MATCH_NAME_26_5, MASK_PATH_RECORD_VERSION, MASK_PRIVATE_DATA,
    MASK_TYPE_26_5, OBJECT_MASK_TYPE,
};
#[cfg(test)]
pub(crate) use mask::{decode_mask_path, MASK_FEATHER_APPROXIMATION};
pub(crate) use motion::{MotionParamSpec, MOTION_PARAMS, MOTION_PARAMS_26_5, MOTION_PARAM_COUNT};
pub(crate) use nested::{
    nested_transform_canvas_reason, nested_transform_import_canvas_reason, PrNestOccurrence,
    MAX_NEST_DEPTH,
};
pub(crate) use opacity::{PrBlendMode, OPACITY_PARAMS, OPACITY_PARAMS_26_5, OPACITY_PARAM_COUNT};
pub(crate) use records::VIDEO_MEDIA;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
pub(crate) use still::{
    OpenExrChannels, PrMediaKind, OPENEXR_ALPHA_TYPE, OPENEXR_CODEC_TYPE, STILL_CODEC_TYPE,
    STILL_INTRINSIC_TICKS, STILL_STRAIGHT_ALPHA_TYPE,
};
pub(crate) use text::PrText;
pub(crate) use timeline::{
    check_track_matte, linear_tail_within_media, source_span_matches, track_matte_provider,
};

pub(crate) use timing::{seconds, SourceFrameRate, VideoOrientation, TICKS, TICKS_PER_MILLISECOND};
pub use timing::{FrameRate, NativeFrameRate};
pub(crate) use video_codec::VideoCodec;

/// One supported Premiere project in structured form.
#[derive(Debug)]
pub struct PrProjectFile {
    pub(crate) sequences: Vec<PrSequence>,
    pub(crate) media: BTreeMap<MediaId, PrMedia>,
}

impl PrProjectFile {
    pub(crate) fn from_sequences(
        sequences: Vec<PrSequence>,
        media: BTreeMap<MediaId, PrMedia>,
    ) -> Self {
        Self { sequences, media }
    }

    /// Checks the completed model, after native XML has been decoded and references resolved.
    /// Native-only fields that are not retained in this model must be checked by the reader.
    pub(crate) fn validate(&self) -> crate::format::Result<()> {
        self.validate_media_references()?;
        let mut failures = Vec::new();
        for sequence in &self.sequences {
            if let Err(error) = sequence.validate_timeline(&self.media) {
                failures.push(format!(
                    "sequence {} ({:?}): {error}",
                    sequence.id.as_deref().unwrap_or("<no GUID>"),
                    sequence.name
                ));
            }
        }
        crate::format::ensure_valid!(failures.is_empty(), "{}", failures.join("\n"));
        Ok(())
    }

    pub(crate) fn validate_media_references(&self) -> crate::format::Result<()> {
        let mut referenced = BTreeSet::new();
        for sequence in &self.sequences {
            for id in sequence.media_in_order() {
                crate::format::ensure_valid!(
                    self.media.contains_key(id),
                    "sequence {:?}: unknown media {id}",
                    sequence.name
                );
                referenced.insert(id);
            }
        }
        for id in self.media.keys() {
            crate::format::ensure_valid!(referenced.contains(id), "unreferenced media {id}");
        }
        Ok(())
    }

    /// Returns facts for the source referenced by an occurrence.
    pub fn media(&self, occurrence: &PrVideoOccurrence) -> Option<&PrMedia> {
        self.media.get(&occurrence.media)
    }

    pub(crate) fn single_sequence(&self) -> Option<&PrSequence> {
        if self.sequences.len() == 1 {
            self.sequences.first()
        } else {
            None
        }
    }

    pub(crate) fn into_parts(self) -> (Vec<PrSequence>, BTreeMap<MediaId, PrMedia>) {
        (self.sequences, self.media)
    }

    /// Returns the supported sequences in this project.
    pub fn sequences(&self) -> impl ExactSizeIterator<Item = &PrSequence> {
        self.sequences.iter()
    }
}

/// One supported Premiere sequence.
#[derive(Debug, Clone)]
pub struct PrSequence {
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) top_level: Option<bool>,
    pub(crate) video_tracks: Vec<PrVideoTrack>,
    /// Sound placements. The reader orders them by start; native track membership is not retained.
    pub(crate) audio: Vec<PrAudioOccurrence>,
    pub(crate) frame_rate: FrameRate,
    /// Saved sequence grid when only the editable sampling rate is approximated.
    /// Native occurrence boundaries and media/audio clocks remain unchanged.
    pub(crate) native_frame_ticks: Option<i64>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The last native video item end on input, including omitted items, or the
    /// document end on output. `end_ticks` also includes the exact converted audio
    /// end; a longer document tail snaps to the sequence frame grid.
    pub(crate) timeline_end_ticks: i64,
}

impl PrSequence {
    /// Returns the native sequence identifier when the project was loaded from Premiere.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Returns the sequence name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns whether this is a root sequence when project topology is known.
    pub fn is_top_level(&self) -> Option<bool> {
        self.top_level
    }

    /// Returns video tracks from bottom to top, with items in timeline order.
    pub fn video_tracks(
        &self,
    ) -> impl DoubleEndedIterator<Item = &[PrVideoItem]> + ExactSizeIterator {
        self.video_tracks.iter().map(|track| track.items.as_slice())
    }

    /// Returns media and graphic items in track-major order, from the bottom track upward.
    pub(crate) fn video_items(&self) -> impl DoubleEndedIterator<Item = &PrVideoItem> {
        self.video_tracks.iter().flat_map(|track| &track.items)
    }

    /// Returns media occurrences in track-major order, from the bottom track upward.
    pub fn video_occurrences(&self) -> impl DoubleEndedIterator<Item = &PrVideoOccurrence> {
        self.video_items().filter_map(PrVideoItem::media)
    }

    pub(crate) fn media_in_order(&self) -> Vec<&MediaId> {
        let mut seen = BTreeSet::new();
        self.video_occurrences()
            .flat_map(|clip| {
                std::iter::once(&clip.media).chain(clip.opacity_mask.as_ref().and_then(|mask| {
                    match &mask.raster {
                        Some(RasterMask::Prepared(media)) => Some(media),
                        _ => None,
                    }
                }))
            })
            .chain(self.audio.iter().map(|clip| &clip.media))
            .chain(self.nested_media())
            .filter(|id| seen.insert(*id))
            .collect()
    }

    /// Returns the sequence dimensions.
    pub fn dimensions(&self) -> [u32; 2] {
        [self.width, self.height]
    }
}

/// One video lane in bottom-to-top sequence order.
#[derive(Debug, Clone)]
pub(crate) struct PrVideoTrack {
    pub(crate) items: Vec<PrVideoItem>,
    /// Nested-sequence placements on this lane, in timeline order.
    pub(crate) nests: Vec<PrNestOccurrence>,
    pub(crate) transitions: Vec<PrVideoTransition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrVideoTransitionKind {
    CrossDissolve,
    /// Measured default profile; its import uses a declared smoothstep approximation.
    FilmImpactDissolve,
    /// Measured static profile; import uses editable sampled geometry.
    FilmImpactPop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrVideoTransition {
    pub(crate) id: String,
    pub(crate) kind: PrVideoTransitionKind,
    pub(crate) start_ticks: i64,
    pub(crate) cut_ticks: i64,
    pub(crate) end_ticks: i64,
    pub(crate) outgoing_clip: Option<String>,
    pub(crate) incoming_clip: Option<String>,
}

#[cfg(test)]
impl PrVideoTrack {
    /// A track that holds only media occurrences.
    pub(crate) fn media(occurrences: impl IntoIterator<Item = PrVideoOccurrence>) -> Self {
        Self {
            items: occurrences.into_iter().map(PrVideoItem::Media).collect(),
            nests: Vec::new(),
            transitions: Vec::new(),
        }
    }

    /// The media occurrence at `index`.
    pub(crate) fn clip(&self, index: usize) -> &PrVideoOccurrence {
        self.items[index].media().expect("test item is media")
    }

    pub(crate) fn clip_mut(&mut self, index: usize) -> &mut PrVideoOccurrence {
        match &mut self.items[index] {
            PrVideoItem::Media(clip) => clip,
            PrVideoItem::Graphic(_) | PrVideoItem::Capsule(_) => panic!("test item is media"),
        }
    }
}

/// One non-overlapping item on a video track.
#[derive(Debug, Clone)]
pub enum PrVideoItem {
    /// A placement of shared source media.
    Media(PrVideoOccurrence),
    /// A Type-tool graphic that holds editable text.
    Graphic(PrGraphic),
    /// Saved native Capsule content, imported as a full editable FX picture.
    Capsule(PrCapsule),
}

/// Capsule placement metadata and its qualified instance-only native source.
/// This is converter-internal state, never persisted as a replay payload.
#[derive(Debug, Clone)]
pub struct PrCapsule {
    pub(crate) placement: PrGraphic,
    pub(crate) source_out_ticks: i64,
    pub(crate) motion_animations: Vec<PrPropertyAnimation>,
    pub(crate) source: std::sync::Arc<crate::capsule::picture::PictureSource>,
}

impl PrCapsule {
    pub(crate) fn validate(&self) -> crate::error::Result<()> {
        crate::error::ensure!(
            self.placement.start_ticks >= 0
                && self.placement.end_ticks > self.placement.start_ticks
                && self.placement.in_ticks >= 0
                && self.source_out_ticks > self.placement.in_ticks,
            "invalid Capsule timeline/source ranges"
        );
        self.placement.clip_motion.validate()?;
        if let Some(mask) = &self.placement.opacity_mask {
            mask.validate()?;
        }
        Ok(())
    }
}

impl PrVideoItem {
    /// Returns the native occurrence identifier when loaded from Premiere.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Media(clip) => clip.id(),
            Self::Graphic(graphic) => graphic.id(),
            Self::Capsule(capsule) => capsule.placement.id(),
        }
    }

    /// Returns the timeline interval in Premiere ticks.
    pub fn timeline_ticks(&self) -> std::ops::Range<i64> {
        match self {
            Self::Media(clip) => clip.timeline_ticks(),
            Self::Graphic(graphic) => graphic.timeline_ticks(),
            Self::Capsule(capsule) => capsule.placement.timeline_ticks(),
        }
    }

    pub(crate) fn media(&self) -> Option<&PrVideoOccurrence> {
        match self {
            Self::Media(clip) => Some(clip),
            Self::Graphic(_) | Self::Capsule(_) => None,
        }
    }

    pub(crate) fn graphic(&self) -> Option<&PrGraphic> {
        match self {
            Self::Graphic(graphic) => Some(graphic),
            Self::Media(_) | Self::Capsule(_) => None,
        }
    }

    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        let (this, other) = (self.timeline_ticks(), other.timeline_ticks());
        this.start < other.end && other.start < this.end
    }
}

/// One Type-tool graphic placement. Its text and shape objects stay
/// editable; the synthetic generator media behind it carries no source facts.
#[derive(Debug, Clone)]
pub struct PrGraphic {
    pub(crate) id: Option<String>,
    pub(crate) start_ticks: i64,
    pub(crate) end_ticks: i64,
    /// Generator time at the placement start (`InPoint`). Graphic keys use the
    /// generator clock, so a key's layer time is its time minus this.
    pub(crate) in_ticks: i64,
    /// Keyed Vector Motion, which moves the whole graphic. A static Vector
    /// Motion is composed into a graphic's one object instead, unless that
    /// would take the object outside its native ranges or move it before a
    /// retained Ramp; a graphic with several objects keeps it here.
    pub(crate) vector_motion: Option<text::PrVectorMotion>,
    /// The clip's own static Motion (`AE.ADBE Motion`), which moves the whole
    /// graphic after its Vector Motion. Only a Source Graphic placement, whose
    /// objects its master clip holds, reads one other than the default; other
    /// graphics keep the default. Export keeps the default, the only Motion
    /// that the graphic writer writes.
    pub(crate) clip_motion: PrStaticTransform,
    /// The clip's own Opacity (`AE.ADBE Opacity`), over the whole graphic: 100
    /// under `DefaultOpacity` `true`. A chain with neither a `DefaultOpacity`
    /// nor an Opacity component also reads as 100; that reading is inferred,
    /// the shared video reader's fallback, not seen in the Adobe evidence.
    pub(crate) opacity: f64,
    pub(crate) blend_mode: PrBlendMode,
    /// Keys on the clip Opacity, on the generator clock like the text keys.
    /// The clip's Motion has no keys, so no other property is keyed.
    pub(crate) animations: Vec<PrPropertyAnimation>,
    /// The one mask on the clip Opacity (`SubComponents`), its outline
    /// in unit fractions of the sequence frame. Premiere applies it to the
    /// whole graphic after the Vector Motion (fixture
    /// `feature_graphic_masks_d_26_5`, probe d1).
    pub(crate) opacity_mask: Option<PrMask>,
    /// The graphic's objects, in the order that its component chain lists
    /// them.
    pub(crate) objects: Vec<text::PrGraphicObject>,
    /// Converter-local loss retained until mask/matte admission; never an FX field.
    pub(crate) effect_loss: Option<PrGraphicEffectLoss>,
    /// Effective picture output, flattened like `PrVideoOccurrence::enabled`.
    pub(crate) enabled: bool,
}

/// Native Ramp coverage still unproved on a graphic. An editable replacement
/// may retain its controls, but cannot certify native alpha or matte luma.
#[derive(Debug, Clone)]
pub(crate) struct PrGraphicEffectLoss {
    pub(crate) ramp_component: String,
    /// Leading, representable Ramps, in native chain order. Other Ramp parts
    /// remain diagnosed losses; this transport does not clear coverage admission.
    pub(crate) mapped_ramps: Vec<PrEffect>,
}

impl PrGraphic {
    /// Returns the native occurrence identifier when loaded from Premiere.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Returns the timeline interval in Premiere ticks.
    pub fn timeline_ticks(&self) -> std::ops::Range<i64> {
        self.start_ticks..self.end_ticks
    }

    /// Leaf objects in native chain order, including SubGroup members.
    fn object_leaves(&self) -> Vec<&text::PrGraphicObject> {
        fn visit<'a>(
            objects: &'a [text::PrGraphicObject],
            out: &mut Vec<&'a text::PrGraphicObject>,
        ) {
            for object in objects {
                match object {
                    text::PrGraphicObject::Group(group) => visit(&group.objects, out),
                    object => out.push(object),
                }
            }
        }
        let mut leaves = Vec::new();
        visit(&self.objects, &mut leaves);
        leaves
    }

    /// The text objects, in chain order.
    pub(crate) fn texts(&self) -> impl Iterator<Item = &PrText> {
        self.object_leaves()
            .into_iter()
            .filter_map(|object| match object {
                text::PrGraphicObject::Text(text) => Some(text),
                text::PrGraphicObject::TextLines(_)
                | text::PrGraphicObject::Shape(_)
                | text::PrGraphicObject::Group(_) => None,
            })
    }

    /// Every editable line's font/style document, including a mixed-style block.
    pub(crate) fn text_documents(&self) -> impl Iterator<Item = (&str, &text::PrTextDocument)> {
        self.object_leaves().into_iter().flat_map(|object| {
            let (name, documents) = match object {
                text::PrGraphicObject::Text(text) => {
                    (text.name.as_str(), std::slice::from_ref(&text.document))
                }
                text::PrGraphicObject::TextLines(text) => {
                    (text.name.as_str(), text.documents.as_slice())
                }
                text::PrGraphicObject::Shape(_) | text::PrGraphicObject::Group(_) => ("", &[][..]),
            };
            documents.iter().map(move |document| (name, document))
        })
    }

    /// The object of a test graphic that holds one text.
    #[cfg(test)]
    pub(crate) fn text(&self) -> &PrText {
        match self.objects.as_slice() {
            [text::PrGraphicObject::Text(text)] => text,
            objects => panic!("test graphic holds one text, not {objects:?}"),
        }
    }

    #[cfg(test)]
    pub(crate) fn text_mut(&mut self) -> &mut PrText {
        match self.objects.as_mut_slice() {
            [text::PrGraphicObject::Text(text)] => text,
            objects => panic!("test graphic holds one text, not {objects:?}"),
        }
    }
}

/// Stable native media identity or Tesseract asset identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaId(pub(crate) String);

impl MediaId {
    /// Returns the native media identifier or Tesseract asset identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MediaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Source facts shared by all placements, including placements in other sequences.
#[derive(Debug, Clone)]
pub struct PrMedia {
    pub(crate) name: String,
    pub(crate) relative_path: Option<String>,
    pub(crate) relative_paths: Vec<String>,
    pub(crate) absolute_paths: Vec<(records::MediaPathField, PathBuf)>,
    pub(crate) video: Option<PrVideoStream>,
    pub(crate) audio: Option<PrAudioStream>,
}

/// Video-stream facts of one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrVideoStream {
    pub(crate) pixel_aspect: records::PixelAspectRatio,
    pub(crate) interpretation: SourceInterpretation,
    pub(crate) orientation: VideoOrientation,
    pub(crate) intrinsic_ticks: i64,
    pub(crate) frame_rate: SourceFrameRate,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) kind: PrMediaKind,
}

/// Saved picture declaration, never a measurement of the file's cadence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum SourceInterpretation {
    #[default]
    Original,
    Rate(SourceFrameRate),
    Invalid(String),
}

impl PrVideoStream {
    pub(crate) fn interpreted_duration(&self) -> crate::error::Result<i64> {
        use crate::error::{ensure, unsupported};
        let rate = match &self.interpretation {
            SourceInterpretation::Original => return Ok(self.intrinsic_ticks),
            SourceInterpretation::Invalid(reason) => return Err(unsupported(reason)),
            SourceInterpretation::Rate(rate) => rate,
        };
        let period = self.frame_rate.ticks_per_frame();
        ensure!(
            self.intrinsic_ticks > 0 && self.intrinsic_ticks % period == 0,
            "interpreted source Duration must contain an exact positive native frame count"
        );
        (self.intrinsic_ticks / period)
            .checked_mul(rate.ticks_per_frame())
            .ok_or_else(|| unsupported("interpreted source duration exceeds Premiere's tick range"))
    }

    pub(crate) fn display_dimensions(&self) -> [u32; 2] {
        self.orientation.display_dimensions(self.width, self.height)
    }

    /// Pixel interpretation follows the decoded picture's axes after its container rotation.
    pub(crate) fn pixel_scale(&self) -> [f64; 2] {
        let aspect = self.pixel_aspect.scale();
        match self.orientation {
            VideoOrientation::Clockwise | VideoOrientation::CounterClockwise => [1.0, aspect],
            VideoOrientation::Identity | VideoOrientation::HalfTurn => [aspect, 1.0],
        }
    }
}

impl PrMedia {
    /// Returns the source name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the native linked composition identity, when this is AEP media.
    /// Reading this identity does not import the linked composition into FX.
    pub fn after_effects_composition(&self) -> Option<PrAfterEffectsComposition> {
        match self.video.as_ref()?.kind {
            PrMediaKind::AfterEffectsComposition(composition) => Some(composition),
            _ => None,
        }
    }

    /// Returns whether this source is a Premiere still image.
    pub fn is_still(&self) -> bool {
        self.video
            .as_ref()
            .is_some_and(|video| video.kind.is_still())
    }

    /// Whether this is the Black Video generator media of an adjustment layer.
    pub(crate) fn is_adjustment(&self) -> bool {
        self.video
            .as_ref()
            .is_some_and(|video| video.kind.is_adjustment())
    }
}

/// Converter-local Stroke recipe and independently retained controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PrFilmImpactStroke {
    Outline99,
    Outline100,
    Frame99,
    Approximate {
        prescale: f64,
        hide_source: bool,
        opacity: f64,
    },
}

impl PrFilmImpactStroke {
    pub(crate) fn prescale(self) -> f64 {
        match self {
            Self::Outline100 => 100.0,
            Self::Outline99 | Self::Frame99 => 99.0,
            Self::Approximate { prescale, .. } => prescale,
        }
    }

    pub(crate) fn hides_source(self) -> bool {
        matches!(
            self,
            Self::Approximate {
                hide_source: true,
                ..
            }
        )
    }

    pub(crate) fn opacity(self) -> f64 {
        match self {
            Self::Approximate { opacity, .. } => opacity,
            _ => 100.0,
        }
    }
}

/// One supported video occurrence and its referenced source.
#[derive(Debug, Clone)]
pub struct PrVideoOccurrence {
    pub(crate) id: Option<String>,
    pub(crate) media: MediaId,
    pub(crate) start_ticks: i64,
    pub(crate) end_ticks: i64,
    pub(crate) in_ticks: i64,
    pub(crate) out_ticks: i64,
    /// Signed constant source-clock units per timeline-clock unit.
    pub(crate) playback_rate: f64,
    pub(crate) frame_blending: Option<fx_schema::FrameBlendingMode>,
    pub(crate) opacity: f64,
    pub(crate) blend_mode: PrBlendMode,
    pub(crate) transform: PrStaticTransform,
    /// The clip's one Crop, in its source frame: an active Crop effect's or
    /// the static Motion Crop of the Premiere 26.5 Motion layout, which
    /// applies with Motion after every effect.
    pub(crate) crop: PrStaticCrop,
    pub(crate) animations: Vec<PrPropertyAnimation>,
    /// Convertible standard effects in stack order, the order they apply.
    pub(crate) effects: Vec<PrEffect>,
    /// How many of `effects` apply before the Crop or Linear Wipe, at higher
    /// chain `Index`es than it; 0 without a Crop or Linear Wipe.
    pub(crate) effects_above_mask: usize,
    /// Import-only measured standard Stroke; geometry is owned by the occurrence.
    pub(crate) stroke: Option<PrFilmImpactStroke>,
    /// Active `AE.ADBE Geometry` records on the native chain, convertible or
    /// not; `effects` holds the convertible ones. The reader counts them
    /// before it drops unconvertible records, so that a second Transform
    /// omits the first even when it does not convert itself
    /// ([`Self::transform_stage`]). The count saturates at `u8::MAX`; only
    /// that rule's `> 1` reads it.
    pub(crate) active_transforms: u8,
    /// The source effects of the placement's master clip, apart from its own
    /// `effects`; `None` when the master clip names no chain. Import converts
    /// them on the placement's picture before its own effects, or reports why
    /// not (`convert::effects::import_source_effects`); export writes every
    /// effect of a picture in its placement's own chain.
    pub(crate) source_effects: Option<PrSourceEffects>,
    /// The FrameHold or native Time Remapping curve, with its key times in
    /// input ticks after the source In: the clip reaches a key
    /// `timeline_ticks / playback_rate` ticks after its start, so at unit
    /// speed, as always for a FrameHold, key times are on the clip's clock.
    pub(crate) time_remap: Option<PrTimeRemap>,
    pub(crate) linear_wipe: Option<PrLinearWipe>,
    /// The one mask on the clip's intrinsic Opacity, which Premiere
    /// applies after every standard effect, Crop and Linear Wipe included.
    pub(crate) opacity_mask: Option<PrMask>,
    /// The one active Track Matte Key, a standard effect at the chain position
    /// that `effects_above_mask` counts, like a Crop or Linear Wipe.
    pub(crate) track_matte: Option<PrTrackMatte>,
    /// Effective picture output: false when the native `ClipTrackItem/IsMuted`
    /// or the owning track's `Track/IsMuted` is `true` (picture verified by the
    /// AME render of `premiere_isolated_clip_disabled`; reading them as clip
    /// Enable and track output is inferred). The reader flattens both here
    /// because the FX document has per-layer `is_hidden` and no tracks.
    pub(crate) enabled: bool,
}

/// The standard effects of a master clip's own `VideoComponentChain`, which
/// Premiere applies to the source before each placement's own pipeline (the
/// pinned `premiere_isolated_source_effects_26_5` render: its Corner Pin before
/// the placement's Motion). Each placement reads them for itself, as it reads
/// [`PrVideoOccurrence::effects`]: in stack order, bypassed ones kept, keys on
/// the source clock, unconvertible ones reported and left out. The reader
/// admits no chain with an active effect that changes what the clip covers
/// or its transparency, or with an active Transform, or a Geometry2 that does
/// not convert, that can hide the picture at some source time: by an Opacity
/// or rendered Scale that reaches 0, by moving the whole picture out of the
/// clip's frame ([`PrTransform::hiding_geometry`]), or by moving, scaling or
/// turning it beside another effect that changes which part of the picture
/// shows, a Transform, Geometry2, Corner Pin or Mosaic, since their combined
/// geometry is not evaluated ([`PrTransform::changes_geometry`]); or with one
/// whose values cannot be read. Import converts no source Transform. One that
/// keeps part of the picture in the frame on its own is reported as not
/// converted, and the picture shows without it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrSourceEffects {
    /// The master clip that owns the chain (`MasterClip:<uid>`), which every
    /// placement of that master clip shares.
    pub(crate) master: String,
    pub(crate) effects: Vec<PrEffect>,
    /// Active Transform records of the chain, convertible or not, as
    /// [`PrVideoOccurrence::active_transforms`] counts them. The two counts
    /// stay apart: a source Transform is never the placement's one Transform.
    pub(crate) active_transforms: u8,
}

/// The report, on `MasterClip:<uid>`, of a master clip's `VideoComponentChain`
/// that no conversion applies: the reader's for a chain that it does not carry
/// (an element that is no graph edge, a nest placement's), the importer's for
/// source effects that it does not convert on any layer.
pub(crate) const SOURCE_CHAIN_NOT_CONVERTED: &str = "VideoComponentChain not converted";

/// The static Track Matte Key (`AE.ADBE Legacy Key Track Matte`) of a media
/// or nest placement: Premiere keys the placement's source frame by one
/// channel of the output of another video track at the same sequence time,
/// and does not draw the matte clip while it is consumed (fixture G1). The
/// matte track holds one enabled, unambiguous overlapping provider
/// ([`check_track_matte`]). Import binds its own identity and source clock;
/// where coverage is partial, only the covered consumer window is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrTrackMatte {
    /// Index of the matte track, strictly above the placement's own track.
    /// Premiere stores the track's persistent `Track/ID` in the record; the
    /// reader resolves it and the writer writes the index's ID.
    pub(crate) track_index: usize,
    pub(crate) channel: PrMatteChannel,
}

/// The matte channel that gates the placement: the Composite Using popup of
/// a Track Matte Key with its Reverse checkbox. Reverse with Matte Luma has no
/// variant: Premiere gives the matte item's zero-luma exterior full coverage
/// where FX's `lumaInverted` gives none (fixture G3b), so it fails closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrMatteChannel {
    /// Matte Alpha (0), Reverse false: the matte's alpha gates the placement.
    Alpha,
    /// Matte Alpha (0), Reverse true: one minus the matte's alpha gates the
    /// placement (fixture G3a: inside the opaque matte the base shows, outside
    /// it the placement).
    AlphaInverted,
    /// Matte Luma (1), Reverse false: the matte's luminance gates the
    /// placement. Premiere weights the matte's encoded RGB by Rec. 601
    /// (fixture G2), FX by Rec. 709: exact for neutral mattes, up to about 11
    /// levels apart on saturated colours.
    Luma,
}

impl PrMatteChannel {
    /// The native Composite Using popup value.
    pub(crate) fn composite_value(self) -> &'static str {
        match self {
            Self::Alpha | Self::AlphaInverted => "0",
            Self::Luma => "1",
        }
    }

    /// The native Reverse checkbox value.
    pub(crate) fn reverse_value(self) -> &'static str {
        match self {
            Self::Alpha | Self::Luma => "false",
            Self::AlphaInverted => "true",
        }
    }
}

/// Supported Adobe Linear Wipe state on one video occurrence.
#[derive(Debug, Clone)]
pub(crate) struct PrLinearWipe {
    pub(crate) initial_completion: f64,
    /// Empty for a constant `initial_completion`; otherwise source-clock keys.
    pub(crate) completion: Vec<PrScalarKeyframe>,
    pub(crate) angle_degrees: i16,
    pub(crate) feather: f64,
}

/// Static intrinsic Motion values in Premiere's normalized coordinate spaces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrStaticTransform {
    /// Canvas-normalized location where the source anchor lands.
    pub(crate) position: [f64; 2],
    /// Source-normalized pivot.
    pub(crate) anchor_point: [f64; 2],
    /// Horizontal and vertical percentages.
    pub(crate) scale: [f64; 2],
    pub(crate) rotation: f64,
}

impl Default for PrStaticTransform {
    fn default() -> Self {
        Self {
            position: [0.5, 0.5],
            anchor_point: [0.5, 0.5],
            scale: [100.0, 100.0],
            rotation: 0.0,
        }
    }
}

/// Largest difference, in units of the frame, between a Position and an
/// Anchor Point that name one point: single-precision noise. Premiere saves
/// the two 6e-9 apart on an unmoved adjustment layer.
const SAME_POINT_TOLERANCE: f64 = f32::EPSILON as f64;

impl PrStaticTransform {
    /// Whether this Motion keeps every pixel of a sequence-sized source in
    /// place, as the default does: Scale 100 and Rotation 0 with Position on
    /// the Anchor Point, which are then normalized to one frame.
    pub(crate) fn is_identity_on_canvas(&self) -> bool {
        self.scale == [100.0, 100.0]
            && self.rotation == 0.0
            && self
                .position
                .iter()
                .zip(self.anchor_point)
                .all(|(position, anchor)| (position - anchor).abs() <= SAME_POINT_TOLERANCE)
    }
}

/// One occurrence-local property animation, with key times on the source clock.
/// A point-valued property must receive its own key type, not be cast to a scalar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PrPropertyAnimation {
    Opacity(Vec<PrScalarKeyframe>),
    Position(Vec<PrPointKeyframe>),
    /// Motion Anchor Point keys, fractions of the source frame like the
    /// static [`PrStaticTransform::anchor_point`].
    AnchorPoint(Vec<PrPointKeyframe>),
    Rotation(Vec<PrScalarKeyframe>),
    /// Motion Scale keys under Uniform Scale, which scale both axes.
    UniformScale(Vec<PrScalarKeyframe>),
    /// Motion Scale Width keys, which scale the horizontal axis alone: Uniform
    /// Scale is off, so they never accompany [`Self::UniformScale`].
    ScaleWidth(Vec<PrScalarKeyframe>),
}

impl PrPropertyAnimation {
    pub(crate) fn property(&self) -> PrAnimatedProperty {
        match self {
            Self::Opacity(_) => PrAnimatedProperty::Opacity,
            Self::Position(_) => PrAnimatedProperty::Position,
            Self::AnchorPoint(_) => PrAnimatedProperty::AnchorPoint,
            Self::Rotation(_) => PrAnimatedProperty::Rotation,
            Self::UniformScale(_) => PrAnimatedProperty::UniformScale,
            Self::ScaleWidth(_) => PrAnimatedProperty::ScaleWidth,
        }
    }

    pub(crate) fn scalar_keys(&self) -> Option<&[PrScalarKeyframe]> {
        match self {
            Self::Opacity(keys)
            | Self::Rotation(keys)
            | Self::UniformScale(keys)
            | Self::ScaleWidth(keys) => Some(keys),
            Self::Position(_) | Self::AnchorPoint(_) => None,
        }
    }

    pub(crate) fn point_keys(&self) -> Option<&[PrPointKeyframe]> {
        match self {
            Self::Position(keys) | Self::AnchorPoint(keys) => Some(keys),
            Self::Opacity(_) | Self::Rotation(_) | Self::UniformScale(_) | Self::ScaleWidth(_) => {
                None
            }
        }
    }

    /// Why these keys are outside the one form that Adobe evidence measured
    /// for their property, if they are. Anchor Point and Scale Width keys
    /// convert Linear only, and Anchor Point keys without spatial tangents:
    /// AME's render of such keys in `feature_motion_anchor_scale_width_probe`
    /// is Position plus each axis's Scale times the source pixel less the
    /// source-normalized anchor, within 0.8 px at every frame. The first key's
    /// easing ends no segment and is not read. Other properties keep the rules
    /// of their own readers and exporters.
    pub(crate) fn unmeasured_form(&self) -> Option<&'static str> {
        let curved = |index: usize, easing: PrKeyframeEasing| {
            index > 0 && easing != PrKeyframeEasing::Linear
        };
        match self {
            Self::AnchorPoint(keys) => keys
                .iter()
                .enumerate()
                .any(|(index, key)| {
                    curved(index, key.easing)
                        || key.spatial_in_tangent.is_some()
                        || key.spatial_out_tangent.is_some()
                })
                .then_some("only Linear Anchor Point keys without spatial tangents convert; Premiere's other Anchor Point keys are unmeasured"),
            Self::ScaleWidth(keys) => keys
                .iter()
                .enumerate()
                .any(|(index, key)| curved(index, key.easing))
                .then_some("only Linear Scale Width keys convert; Premiere's other Scale Width keys are unmeasured"),
            Self::Opacity(_) | Self::Position(_) | Self::Rotation(_) | Self::UniformScale(_) => {
                None
            }
        }
    }

    /// Checks nonempty keys with finite values and spatial tangents, at
    /// strictly increasing source times.
    pub(crate) fn validate_keys(&self) -> crate::format::Result<()> {
        let count = match self {
            Self::Position(keys) | Self::AnchorPoint(keys) => keys.len(),
            Self::Opacity(keys)
            | Self::Rotation(keys)
            | Self::UniformScale(keys)
            | Self::ScaleWidth(keys) => keys.len(),
        };
        crate::format::ensure_valid!(count > 0, "animation must have at least one key");
        let valid = match self {
            Self::Position(keys) | Self::AnchorPoint(keys) => {
                keys.iter().all(|key| {
                    key.value
                        .iter()
                        .chain(key.spatial_in_tangent.iter().flatten())
                        .chain(key.spatial_out_tangent.iter().flatten())
                        .all(|value| value.is_finite())
                }) && keys
                    .windows(2)
                    .all(|pair| pair[0].source_ticks < pair[1].source_ticks)
            }
            Self::Opacity(keys)
            | Self::Rotation(keys)
            | Self::UniformScale(keys)
            | Self::ScaleWidth(keys) => {
                keys.iter().all(|key| key.value.is_finite())
                    && keys
                        .windows(2)
                        .all(|pair| pair[0].source_ticks < pair[1].source_ticks)
            }
        };
        crate::format::ensure_valid!(
            valid,
            "animation keys must have finite values and strictly increasing source times"
        );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn keys(&self) -> &[PrScalarKeyframe] {
        self.scalar_keys()
            .expect("test expected scalar Motion keys")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PrAnimatedProperty {
    Opacity,
    Position,
    AnchorPoint,
    Rotation,
    UniformScale,
    ScaleWidth,
}

#[derive(Debug, Clone)]
pub(crate) struct PrTimeRemap {
    pub(crate) keys: Vec<PrTimeRemapKeyframe>,
}

impl PrTimeRemap {
    /// Native Speed modes for forward ramps whose handles follow their plateaus.
    ///
    /// The saved Speed curve uses 6 for plateau keys and 7/8 for the start/end
    /// of a ramp. It stores no independent handles: the adjacent linear slopes
    /// determine the cubic with time controls at one third and two thirds.
    pub(crate) fn ramp_modes(&self) -> Option<Vec<u8>> {
        let keys = &self.keys;
        if keys.len() < 4
            || keys.first()?.easing != PrKeyframeEasing::Linear
            || keys.last()?.easing != PrKeyframeEasing::Linear
            || keys.iter().any(|key| key.source_ticks < 0)
            || keys.windows(2).any(|pair| {
                pair[0].timeline_ticks >= pair[1].timeline_ticks
                    || pair[0].source_ticks >= pair[1].source_ticks
            })
        {
            return None;
        }
        let slope = |a: &PrTimeRemapKeyframe, b: &PrTimeRemapKeyframe| {
            (i128::from(b.source_ticks) - i128::from(a.source_ticks)) as f64
                / (i128::from(b.timeline_ticks) - i128::from(a.timeline_ticks)) as f64
        };
        // Only floating-point roundoff is tolerated, not an edited curve or
        // the millisecond rounding of imported key times/source values.
        let same = |a: f64, b: f64| (a - b).abs() <= 64.0 * f64::EPSILON;
        let mut modes = vec![6; keys.len()];
        let mut has_ramp = false;
        for end in 1..keys.len() {
            match keys[end].easing {
                PrKeyframeEasing::Linear => {}
                PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 }
                    if end >= 2
                        && end + 1 < keys.len()
                        && keys[end - 1].easing == PrKeyframeEasing::Linear
                        && keys[end + 1].easing == PrKeyframeEasing::Linear =>
                {
                    let scale = 1.0 / slope(&keys[end - 1], &keys[end]);
                    let incoming = slope(&keys[end - 2], &keys[end - 1]) * scale / 3.0;
                    let outgoing = 1.0 - slope(&keys[end], &keys[end + 1]) * scale / 3.0;
                    if !same(x1, 1.0 / 3.0)
                        || !same(x2, 2.0 / 3.0)
                        || !same(y1, incoming)
                        || !same(y2, outgoing)
                        || !(0.0..=1.0).contains(&y1)
                        || !(0.0..=1.0).contains(&y2)
                    {
                        return None;
                    }
                    modes[end - 1] = 7;
                    modes[end] = 8;
                    has_ramp = true;
                }
                _ => return None,
            }
        }
        has_ramp.then_some(modes)
    }

    /// The measured explicit native Frame Hold over a placement of `duration`
    /// ticks: two Linear keys at its ends on the held `source_ticks`.
    pub(crate) fn frame_hold(source_ticks: i64, duration: i64) -> Self {
        Self {
            keys: [0, duration]
                .into_iter()
                .map(|timeline_ticks| PrTimeRemapKeyframe {
                    timeline_ticks,
                    source_ticks,
                    easing: PrKeyframeEasing::Linear,
                })
                .collect(),
        }
    }

    /// The held source tick when this curve is the explicit Frame Hold
    /// ([`Self::frame_hold`]) over a placement of `duration` ticks.
    pub(crate) fn held_source_ticks(&self, duration: i64) -> Option<i64> {
        match self.keys.as_slice() {
            [first, last]
                if first.timeline_ticks == 0
                    && last.timeline_ticks == duration
                    && first.source_ticks == last.source_ticks
                    && first.easing == PrKeyframeEasing::Linear
                    && last.easing == PrKeyframeEasing::Linear =>
            {
                Some(first.source_ticks)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrTimeRemapKeyframe {
    pub(crate) timeline_ticks: i64,
    pub(crate) source_ticks: i64,
    pub(crate) easing: PrKeyframeEasing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PrKeyframeEasing {
    Linear,
    Hold,
    CubicBezier { x1: f64, y1: f64, x2: f64, y2: f64 },
}

impl PrKeyframeEasing {
    pub(crate) fn native_outgoing_mode(self) -> u8 {
        match self {
            Self::Linear => 0,
            Self::Hold => 4,
            Self::CubicBezier { .. } => 5,
        }
    }

    /// The cubic Bézier timing handles `[x1, y1, x2, y2]`, `None` for Linear
    /// and Hold.
    pub(crate) fn bezier(self) -> Option<[f64; 4]> {
        match self {
            Self::CubicBezier { x1, y1, x2, y2 } => Some([x1, y1, x2, y2]),
            Self::Linear | Self::Hold => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrScalarKeyframe {
    pub(crate) source_ticks: i64,
    pub(crate) value: f64,
    pub(crate) easing: PrKeyframeEasing,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrPointKeyframe {
    pub(crate) source_ticks: i64,
    pub(crate) value: [f64; 2],
    pub(crate) easing: PrKeyframeEasing,
    pub(crate) spatial_in_tangent: Option<[f64; 2]>,
    pub(crate) spatial_out_tangent: Option<[f64; 2]>,
}

impl PrVideoOccurrence {
    /// A generated placement of `media` that shows `source` over `timeline`
    /// at unit speed: enabled, opaque, at default Motion, without Crop, mask,
    /// matte, effects or keys. Export sets what its layer carries.
    pub(crate) fn unedited(
        media: MediaId,
        timeline: std::ops::Range<i64>,
        source: std::ops::Range<i64>,
    ) -> Self {
        Self {
            id: None,
            media,
            start_ticks: timeline.start,
            end_ticks: timeline.end,
            in_ticks: source.start,
            out_ticks: source.end,
            playback_rate: 1.0,
            frame_blending: None,
            opacity: 100.0,
            blend_mode: PrBlendMode::Normal,
            transform: PrStaticTransform::default(),
            crop: PrStaticCrop::default(),
            animations: Vec::new(),
            effects: Vec::new(),
            effects_above_mask: 0,
            stroke: None,
            active_transforms: 0,
            source_effects: None,
            time_remap: None,
            linear_wipe: None,
            opacity_mask: None,
            track_matte: None,
            enabled: true,
        }
    }

    /// Returns the native occurrence identifier when loaded from Premiere.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Returns the timeline interval in Premiere ticks.
    pub fn timeline_ticks(&self) -> std::ops::Range<i64> {
        self.start_ticks..self.end_ticks
    }

    /// Returns the source interval in Premiere ticks.
    pub fn source_ticks(&self) -> std::ops::Range<i64> {
        self.in_ticks..self.out_ticks
    }

    /// The source tick shown over the whole placement when it is an explicit
    /// Frame Hold ([`PrTimeRemap::held_source_ticks`]).
    pub(crate) fn held_source_ticks(&self) -> Option<i64> {
        self.time_remap
            .as_ref()?
            .held_source_ticks(self.end_ticks - self.start_ticks)
    }

    /// Whether the clip plays a native Time Remapping curve from a source In
    /// other than 0 or at a speed other than 1, at input
    /// `In + speed × elapsed`. Adobe measured that clock on physical video
    /// only ([`Self::validate`]), and not the clock of the clip's effect keys.
    /// A FrameHold, which shows one source instant, is not such a curve.
    pub(crate) fn remaps_from_in_or_speed(&self) -> bool {
        (self.in_ticks != 0 || self.playback_rate != 1.0)
            && self.time_remap.as_ref().is_some_and(|remap| {
                remap
                    .keys
                    .windows(2)
                    .any(|pair| pair[0].source_ticks != pair[1].source_ticks)
            })
    }

    /// Linear Rotation keys on a strictly forward native remap use media time.
    /// Holds, other properties and masks retain their existing admission rules.
    /// The caller must also require physical video outside a nest or stage.
    pub(crate) fn has_media_clock_rotation(&self) -> bool {
        !self.animations.is_empty()
            && self.animations.iter().all(|animation| {
                matches!(animation, PrPropertyAnimation::Rotation(keys)
                    if keys.iter().all(|key| key.easing == PrKeyframeEasing::Linear))
            })
            && self.time_remap.as_ref().is_some_and(|remap| {
                remap.keys.len() >= 2
                    && remap
                        .keys
                        .windows(2)
                        .all(|pair| pair[0].source_ticks < pair[1].source_ticks)
            })
            && self.crop.is_default()
            && self.linear_wipe.is_none()
            && self.opacity_mask.is_none()
            && self.track_matte.is_none()
    }

    /// Returns the identity used to bind the referenced media bytes.
    pub fn media_id(&self) -> &MediaId {
        &self.media
    }

    /// Error and omission context: the native item, else its media identity.
    pub(crate) fn record(&self) -> &str {
        self.id.as_deref().unwrap_or(self.media.as_str())
    }

    /// Removes the effect at `index` from the stack, keeping the count of
    /// effects applied before the mask (`effects_above_mask`) consistent.
    pub(crate) fn remove_effect(&mut self, index: usize) -> PrEffect {
        self.effects_above_mask -= usize::from(index < self.effects_above_mask);
        self.effects.remove(index)
    }

    /// Every non-neutral edit on this occurrence; see [`occurrence_edits`].
    pub(crate) fn edits(&self) -> Vec<OccurrenceEdit> {
        occurrence_edits(ClipEdits {
            linear_wipe: self.linear_wipe.as_ref(),
            animations: &self.animations,
            transform: self.transform,
            crop: self.crop,
            opacity_mask: self.opacity_mask.as_ref(),
            track_matte: self.track_matte.as_ref(),
            opacity: self.opacity,
            playback_rate: self.playback_rate,
            time_remap: self.time_remap.as_ref(),
        })
    }

    /// The stack position (an index into `effects`) and values of the one
    /// Transform effect and its measured owner, or why the clip's Transforms
    /// are omitted instead; `Ok(None)` without a Transform.
    /// `source` is the media frame and `canvas` the sequence frame, in pixels.
    ///
    /// A Crop, Linear Wipe or Opacity mask keeps its stage and the Transform
    /// is omitted. Track Matte Key does likewise except for A4's
    /// measured Linear Position Transform on an Alpha-keyed picture: both
    /// saved effect orders move the video and its static matte together. A
    /// second active Transform on the clip (`active_transforms`),
    /// convertible or not, omits them all: native measurements cover one per
    /// clip, and their composition is unmeasured. The source frame must be the
    /// canvas: E11 could not separate the frame that Position is
    /// normalized to.
    pub(crate) fn transform_stage(
        &self,
        source: [u32; 2],
        canvas: [u32; 2],
    ) -> Result<Option<(usize, &PrTransform, TransformOwner)>, &'static str> {
        let stage =
            self.effects
                .iter()
                .enumerate()
                .find_map(|(index, effect)| match &effect.params {
                    PrEffectParams::Transform(transform) => Some((index, transform)),
                    _ => None,
                });
        let Some(stage) = stage else {
            return Ok(None);
        };
        if self.active_transforms > 1 {
            return Err(
                "another active Transform on the same clip is not converted with this one: native measurements cover one Transform per clip, and Premiere's composition of two is unmeasured",
            );
        }
        if !self.crop.is_default() || self.linear_wipe.is_some() || self.opacity_mask.is_some() {
            return Err(
                "a Transform with a Crop, Linear Wipe or Opacity mask on one clip is not converted: the mask keeps its stage group, which carries one shape",
            );
        }
        let owner = if let Some(matte) = self.track_matte {
            if !self.measured_matte_transform(stage.1, matte) {
                return Err(
                    "a Transform with a Track Matte Key on one clip is not converted outside the measured A4 form: one Linear Position Transform, Alpha key, default Motion and other Transform values, normal blend, opaque and unretimed clip",
                );
            }
            TransformOwner::KeyedPicture
        } else {
            TransformOwner::Video
        };

        if source != canvas {
            return Err(
                "a Transform on media that is not sequence-sized is not converted: the frame that Premiere normalizes its Position to is unmeasured there (native measurements cover 1920 x 1080 media on a 1920 x 1080 sequence)",
            );
        }
        Ok(Some((stage.0, stage.1, owner)))
    }

    /// A4 measured the same keyed-picture translation in both saved stack
    /// orders. Other Transform parameters and clip Motion remain unmeasured.
    fn measured_matte_transform(&self, transform: &PrTransform, matte: PrTrackMatte) -> bool {
        let [effect] = self.effects.as_slice() else {
            return false;
        };
        let [animation] = effect.animations.as_slice() else {
            return false;
        };
        let PrEffectParamKeys::Point(keys) = &animation.keys else {
            return false;
        };
        self.active_transforms == 1
            && effect.enabled
            && matte.channel == PrMatteChannel::Alpha
            && self.transform == PrStaticTransform::default()
            && self.animations.is_empty()
            && self.opacity == 100.0
            && self.blend_mode == PrBlendMode::Normal
            && self.playback_rate == 1.0
            && self.time_remap.is_none()
            && animation.param.id == TRANSFORM_POSITION.id
            && keys.len() >= 2
            && keys.iter().all(|key| {
                key.easing == PrKeyframeEasing::Linear
                    && key.spatial_in_tangent.is_none()
                    && key.spatial_out_tangent.is_none()
            })
            && transform.anchor_point == [0.5, 0.5]
            && !transform.uniform_scale
            && transform.scale_height == 100.0
            && transform.scale_width == 100.0
            && transform.rotation == 0.0
            && transform.skew == 0.0
            && transform.skew_axis == 0.0
            && transform.opacity == 100.0
            && transform.composition_shutter_angle
            && transform.shutter_angle == 0.0
            && !transform.bicubic_sampling
    }

    /// How the Crop, Linear Wipe, Opacity mask, Track Matte Key or Transform
    /// of this media occurrence converts when `source_effects` of its source
    /// effects apply on its picture before them, or why it does not convert
    /// so. `source` is the media frame and `canvas` the sequence frame, in
    /// pixels.
    ///
    /// Premiere applies a clip's source effects ([`Self::source_effects`]),
    /// then its standard effects in descending chain `Index`, Crop, Linear
    /// Wipe and Track Matte Key among them, and then Motion and Opacity, whose
    /// mask therefore applies after every effect; FX applies a layer's masks
    /// and track matte before its effects. Source effects are thus applied
    /// before any mask of the clip. Rules, in order:
    ///
    /// 1. Crop and Linear Wipe together, an Opacity mask with either, or a
    ///    Track Matte Key with any of the three: omitted. Export writes one
    ///    mask per clip, and FX's intersection of two masks is unverified
    ///    against Premiere.
    /// 2. A Linear Wipe or Track Matte Key on media that is not
    ///    sequence-sized: omitted. The wipe guide is a sequence-sized
    ///    rectangle, the matte is the canvas-sized matte track output, and the
    ///    frame that Premiere wipes or keys on such media is unverified.
    /// 3. Converted effects on both sides of the mask: the reader omits them
    ///    all (`MASK_EFFECT_ORDER_REASON`), so rules 4 to 8 see no effects.
    ///    Source effects are on both sides of it too when an effect of the
    ///    clip's own applies after its Crop, Linear Wipe or Track Matte Key:
    ///    that is refused, and the caller converts the clip without them
    ///    (`source_effects` 0). A Transform is never drawn as an effect after
    ///    the mask: it is a stage's transform or omitted.
    /// 4. No mask, with the one Transform of [`Self::transform_stage`]:
    ///    [`MaskBoundary::Staged`] without a mask; the video carries the
    ///    Transform.
    /// 5. No mask otherwise: [`MaskBoundary::Flat`].
    /// 6. A Crop or Opacity mask with no effect applied before it: flat. Its
    ///    guide shares the video's transform and Motion keys, so the mask
    ///    stays in the video's frame.
    /// 7. A Linear Wipe or Track Matte Key with no effect applied before it,
    ///    on a clip at default static Motion without Motion keys: flat. By
    ///    rule 2 its sequence-sized guide or matte is then the clip frame.
    /// 8. Otherwise [`MaskBoundary::Staged`]: so an Opacity mask with any
    ///    converted effect, since every effect applies before it, a mask
    ///    under source effects, and a Track Matte Key on a moved clip, whose
    ///    Motion moves the keyed picture and so the matte with it (fixture
    ///    G5).
    pub(crate) fn mask_boundary(
        &self,
        source: [u32; 2],
        canvas: [u32; 2],
        source_effects: usize,
    ) -> Result<MaskBoundary, &'static str> {
        let crop = !self.crop.is_default();
        let wipe = self.linear_wipe.is_some();
        let opacity_mask = self.opacity_mask.is_some();
        let track_matte = self.track_matte.is_some();
        if crop && wipe {
            return Err("Crop and Linear Wipe on one clip are not converted");
        }
        if opacity_mask && (crop || wipe) {
            return Err("an Opacity mask with a Crop or Linear Wipe on one clip is not converted");
        }
        if track_matte && (crop || wipe || opacity_mask) {
            return Err(
                "a Track Matte Key with a Crop, Linear Wipe or Opacity mask on one clip is not converted",
            );
        }
        if wipe && source != canvas {
            return Err("Linear Wipe on media that is not sequence-sized is not converted");
        }
        if track_matte && source != canvas {
            return Err("Track Matte Key on media that is not sequence-sized is not converted");
        }
        let moved = self.transform != PrStaticTransform::default()
            || self
                .animations
                .iter()
                .any(|animation| animation.property() != PrAnimatedProperty::Opacity);
        let masked = crop || wipe || opacity_mask || track_matte;
        let drawn_after_mask = self
            .effects
            .iter()
            .skip(self.effects_above_mask)
            .any(|effect| !matches!(effect.params, PrEffectParams::Transform(_)));
        if masked && source_effects > 0 && drawn_after_mask {
            return Err(
                "they apply before the Crop, Linear Wipe or Track Matte Key, and effects of the clip's own chain after it; one FX mask keeps the effects of only one side in order",
            );
        }
        Ok(match self.transform_stage(source, canvas) {
            // A4's Transform stages the whole Alpha-keyed picture; E11's
            // stages the video, and only without a mask (rule 4).
            Ok(Some((_, _, TransformOwner::KeyedPicture))) => MaskBoundary::Staged,
            Ok(Some((_, _, TransformOwner::Video))) if !masked => MaskBoundary::Staged,
            _ if !masked => MaskBoundary::Flat,
            _ if self.effects_above_mask + source_effects > 0
                || ((wipe || track_matte) && moved) =>
            {
                MaskBoundary::Staged
            }
            _ => MaskBoundary::Flat,
        })
    }
}

/// The layer whose transform carries the standard Transform effect on import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformOwner {
    /// The measured E11 video stage, inside clip Motion.
    Video,
    /// The measured A4 Alpha-keyed picture: fill and matte move together.
    KeyedPicture,
}

/// How the one mask (a Crop, Linear Wipe, Opacity mask or Track Matte Key) of
/// a media occurrence converts; see [`PrVideoOccurrence::mask_boundary`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaskBoundary {
    /// One video layer whose mask, if any, applies before all its effects.
    Flat,
    /// A group that carries the clip's Motion, Opacity, their keys and the
    /// mask. It holds the video layer, with the effects applied before the
    /// mask, and the mask guide or the matte source, both in the video's
    /// frame. Without a mask, the group holds the video alone, whose transform
    /// is the clip's one Transform effect ([`PrVideoOccurrence::transform_stage`]).
    Staged,
}

/// What a clip edit changes. A still picture, for example, loses nothing to a
/// clock edit, so its occurrence survives one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditKind {
    /// Motion, Crop, an Opacity mask, a Track Matte Key, Opacity or their keys.
    Picture,
    /// Playback rate or time remap.
    Clock,
    /// Linear Wipe.
    Transition,
}

/// One non-neutral edit of a clip placement, in [`occurrence_edits`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OccurrenceEdit {
    LinearWipe,
    /// Position, Anchor Point, Scale or Rotation keys.
    MotionKeys,
    /// Opacity keys, apart from Motion keys because an adjustment layer keeps
    /// them and refuses Motion keys.
    OpacityKeys,
    Position,
    AnchorPoint,
    Scale,
    Rotation,
    Crop,
    OpacityMask,
    TrackMatte,
    Opacity,
    PlaybackRate,
    TimeRemap,
}

impl OccurrenceEdit {
    pub(crate) fn kind(self) -> EditKind {
        match self {
            Self::LinearWipe => EditKind::Transition,
            Self::MotionKeys
            | Self::OpacityKeys
            | Self::Position
            | Self::AnchorPoint
            | Self::Scale
            | Self::Rotation
            | Self::Crop
            | Self::OpacityMask
            | Self::TrackMatte
            | Self::Opacity => EditKind::Picture,
            Self::PlaybackRate | Self::TimeRemap => EditKind::Clock,
        }
    }

    /// The edit as omission messages name it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::LinearWipe => "Linear Wipe",
            Self::MotionKeys => "Motion keyframes",
            Self::OpacityKeys => "Opacity keyframes",
            Self::Position => "nondefault Motion Position",
            Self::AnchorPoint => "nondefault Motion Anchor Point",
            Self::Scale => "nondefault Motion Scale",
            Self::Rotation => "nondefault Motion Rotation",
            Self::Crop => "nondefault Crop",
            Self::OpacityMask => "Opacity mask",
            Self::TrackMatte => "Track Matte Key",
            Self::Opacity => "nondefault Opacity",
            Self::PlaybackRate => "playback rate",
            Self::TimeRemap => "time remap",
        }
    }
}

/// The fields of one media or nest placement that [`occurrence_edits`]
/// classifies, with the meaning of the [`PrVideoOccurrence`] fields of the
/// same name.
pub(crate) struct ClipEdits<'a> {
    pub(crate) linear_wipe: Option<&'a PrLinearWipe>,
    pub(crate) animations: &'a [PrPropertyAnimation],
    pub(crate) transform: PrStaticTransform,
    pub(crate) crop: PrStaticCrop,
    pub(crate) opacity_mask: Option<&'a PrMask>,
    pub(crate) track_matte: Option<&'a PrTrackMatte>,
    pub(crate) opacity: f64,
    pub(crate) playback_rate: f64,
    pub(crate) time_remap: Option<&'a PrTimeRemap>,
}

/// Every non-neutral edit of one media or nest placement, in the order in
/// which the still, Color Matte, adjustment layer and nest checks report the
/// first one. Neutral values are those the reader gives a component that the
/// chain omits: default Motion and Crop, full opacity, unit forward playback,
/// and no keys, time remap, wipe, Opacity mask or Track Matte Key. Blend mode
/// is not listed: each of these placements carries it. Motion and Opacity
/// keys are separate edits because an adjustment layer keeps the latter and
/// refuses the former.
pub(crate) fn occurrence_edits(edits: ClipEdits<'_>) -> Vec<OccurrenceEdit> {
    let ClipEdits {
        linear_wipe,
        animations,
        transform,
        crop,
        opacity_mask,
        track_matte,
        opacity,
        playback_rate,
        time_remap,
    } = edits;
    let neutral = PrStaticTransform::default();
    let keyed = |opacity: bool| {
        animations
            .iter()
            .any(|animation| (animation.property() == PrAnimatedProperty::Opacity) == opacity)
    };
    [
        (linear_wipe.is_some(), OccurrenceEdit::LinearWipe),
        (keyed(false), OccurrenceEdit::MotionKeys),
        (keyed(true), OccurrenceEdit::OpacityKeys),
        (
            transform.position != neutral.position,
            OccurrenceEdit::Position,
        ),
        (
            transform.anchor_point != neutral.anchor_point,
            OccurrenceEdit::AnchorPoint,
        ),
        (transform.scale != neutral.scale, OccurrenceEdit::Scale),
        (
            transform.rotation != neutral.rotation,
            OccurrenceEdit::Rotation,
        ),
        (!crop.is_default(), OccurrenceEdit::Crop),
        (opacity_mask.is_some(), OccurrenceEdit::OpacityMask),
        (track_matte.is_some(), OccurrenceEdit::TrackMatte),
        (opacity != 100.0, OccurrenceEdit::Opacity),
        (playback_rate != 1.0, OccurrenceEdit::PlaybackRate),
        (time_remap.is_some(), OccurrenceEdit::TimeRemap),
    ]
    .into_iter()
    .filter_map(|(edited, edit)| edited.then_some(edit))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        EditKind::{Clock, Picture, Transition},
        PrKeyframeEasing, PrLinearWipe, PrPropertyAnimation, PrScalarKeyframe, PrTimeRemap,
        PrTimeRemapKeyframe, PrVideoOccurrence, TICKS,
    };
    use crate::tests::support::{clip_of, opacity_mask};

    #[test]
    fn occurrence_edits_name_each_non_neutral_field_in_report_order() {
        let neutral = clip_of("source", 0..TICKS, 0);
        assert_eq!(neutral.edits(), []);
        let table: [(fn(&mut PrVideoOccurrence), _, _); 12] = [
            (
                |clip| {
                    clip.linear_wipe = Some(PrLinearWipe {
                        initial_completion: 50.0,
                        completion: Vec::new(),
                        angle_degrees: 90,
                        feather: 0.0,
                    })
                },
                Transition,
                "Linear Wipe",
            ),
            (
                |clip| {
                    clip.animations = vec![PrPropertyAnimation::Rotation(vec![PrScalarKeyframe {
                        source_ticks: 0,
                        value: 15.0,
                        easing: PrKeyframeEasing::Linear,
                    }])]
                },
                Picture,
                "Motion keyframes",
            ),
            (
                |clip| {
                    clip.animations
                        .push(PrPropertyAnimation::Opacity(vec![PrScalarKeyframe {
                            source_ticks: 0,
                            value: 50.0,
                            easing: PrKeyframeEasing::Linear,
                        }]))
                },
                Picture,
                "Opacity keyframes",
            ),
            (
                |clip| clip.transform.position = [0.25, 0.5],
                Picture,
                "nondefault Motion Position",
            ),
            (
                |clip| clip.transform.anchor_point = [0.5, 0.25],
                Picture,
                "nondefault Motion Anchor Point",
            ),
            (
                |clip| clip.transform.scale = [150.0; 2],
                Picture,
                "nondefault Motion Scale",
            ),
            (
                |clip| clip.transform.rotation = 30.0,
                Picture,
                "nondefault Motion Rotation",
            ),
            (|clip| clip.crop.left = 10.0, Picture, "nondefault Crop"),
            (
                |clip| clip.opacity_mask = Some(opacity_mask()),
                Picture,
                "Opacity mask",
            ),
            (|clip| clip.opacity = 50.0, Picture, "nondefault Opacity"),
            // Reverse playback at unit speed is not neutral.
            (|clip| clip.playback_rate = -1.0, Clock, "playback rate"),
            (
                |clip| {
                    clip.time_remap = Some(PrTimeRemap {
                        keys: vec![PrTimeRemapKeyframe {
                            timeline_ticks: 0,
                            source_ticks: 0,
                            easing: PrKeyframeEasing::Linear,
                        }],
                    })
                },
                Clock,
                "time remap",
            ),
        ];
        let mut every_edit = neutral.clone();
        for (edit, kind, label) in table {
            let mut clip = neutral.clone();
            edit(&mut clip);
            let edits = clip.edits();
            let [only] = edits[..] else {
                panic!("{label}: {edits:?}");
            };
            assert_eq!((only.kind(), only.label()), (kind, label));
            edit(&mut every_edit);
        }
        let labels: Vec<_> = every_edit.edits().iter().map(|edit| edit.label()).collect();
        assert_eq!(labels, table.map(|(_, _, label)| label));
    }

    #[test]
    fn mask_boundary_omits_stages_or_keeps_one_layer_by_rule() {
        use super::{
            MaskBoundary::{Flat, Staged},
            PrEffect, PrEffectParams, PrGaussianBlur,
        };
        let key = |value| PrScalarKeyframe {
            source_ticks: 0,
            value,
            easing: PrKeyframeEasing::Linear,
        };
        let edited = |edit: &dyn Fn(&mut PrVideoOccurrence)| {
            let mut clip = clip_of("source", 0..TICKS, 0);
            clip.effects = vec![PrEffect {
                mask: None,
                enabled: true,
                params: PrEffectParams::GaussianBlur(PrGaussianBlur {
                    blurriness: 40.0,
                    repeat_edge_pixels: false,
                }),
                animations: Vec::new(),
            }];
            edit(&mut clip);
            clip
        };
        let crop = |clip: &mut PrVideoOccurrence| clip.crop.left = 20.0;
        // The Opacity mask applies after every effect, so the reader counts
        // them all as applied before it.
        let masked = |clip: &mut PrVideoOccurrence| {
            clip.opacity_mask = Some(opacity_mask());
            clip.effects_above_mask = clip.effects.len();
        };
        let wipe = |clip: &mut PrVideoOccurrence| {
            clip.linear_wipe = Some(PrLinearWipe {
                initial_completion: 0.0,
                completion: vec![key(0.0), key(60.0)],
                angle_degrees: 90,
                feather: 0.0,
            })
        };
        let frame = [1920, 1080];
        // "Above" and "below" in these case names are the application order:
        // a Crop above the effect applies before it (`effects_above_mask` 0).
        for (case, clip, source, expected) in [
            ("effects without a mask", edited(&|_| {}), frame, Ok(Flat)),
            ("Crop above the effect", edited(&crop), frame, Ok(Flat)),
            (
                "Crop below the effect",
                edited(&|clip| {
                    crop(clip);
                    clip.effects_above_mask = 1;
                }),
                frame,
                Ok(Staged),
            ),
            (
                "Crop with keyed Rotation and Scale",
                edited(&|clip| {
                    crop(clip);
                    clip.animations = vec![
                        PrPropertyAnimation::Rotation(vec![key(0.0), key(20.0)]),
                        PrPropertyAnimation::UniformScale(vec![key(100.0), key(80.0)]),
                    ];
                }),
                frame,
                Ok(Flat),
            ),
            (
                "Crop on portrait media with static Motion",
                edited(&|clip| {
                    crop(clip);
                    clip.transform.scale = [50.0; 2];
                }),
                [1080, 1920],
                Ok(Flat),
            ),
            ("wipe at default Motion", edited(&wipe), frame, Ok(Flat)),
            (
                "wipe with Opacity keys",
                edited(&|clip| {
                    wipe(clip);
                    clip.animations = vec![PrPropertyAnimation::Opacity(vec![key(50.0)])];
                }),
                frame,
                Ok(Flat),
            ),
            (
                "wipe with static Scale",
                edited(&|clip| {
                    wipe(clip);
                    clip.transform.scale = [80.0; 2];
                }),
                frame,
                Ok(Staged),
            ),
            (
                "wipe with Rotation keys",
                edited(&|clip| {
                    wipe(clip);
                    clip.animations = vec![PrPropertyAnimation::Rotation(vec![key(15.0)])];
                }),
                frame,
                Ok(Staged),
            ),
            (
                "wipe below the effect",
                edited(&|clip| {
                    wipe(clip);
                    clip.effects_above_mask = 1;
                }),
                frame,
                Ok(Staged),
            ),
            (
                "Crop and wipe",
                edited(&|clip| {
                    crop(clip);
                    wipe(clip);
                }),
                frame,
                Err("Crop and Linear Wipe on one clip are not converted"),
            ),
            (
                "wipe on 1280x720 media",
                edited(&wipe),
                [1280, 720],
                Err("Linear Wipe on media that is not sequence-sized is not converted"),
            ),
            (
                "Opacity mask over the effect",
                edited(&masked),
                frame,
                Ok(Staged),
            ),
            (
                "Opacity mask on a moved clip without effects",
                edited(&|clip| {
                    clip.effects.clear();
                    masked(clip);
                    clip.transform.scale = [50.0; 2];
                    clip.animations = vec![PrPropertyAnimation::Rotation(vec![key(15.0)])];
                }),
                frame,
                Ok(Flat),
            ),
            (
                "Opacity mask and Crop",
                edited(&|clip| {
                    crop(clip);
                    masked(clip);
                }),
                frame,
                Err("an Opacity mask with a Crop or Linear Wipe on one clip is not converted"),
            ),
            (
                "Opacity mask and wipe",
                edited(&|clip| {
                    wipe(clip);
                    masked(clip);
                }),
                frame,
                Err("an Opacity mask with a Crop or Linear Wipe on one clip is not converted"),
            ),
        ] {
            assert_eq!(clip.mask_boundary(source, frame, 0), expected, "{case}");
        }

        // Source effects apply before every effect of the clip's own, and so
        // before its mask.
        let bare = |edit: &dyn Fn(&mut PrVideoOccurrence)| {
            edited(&|clip| {
                clip.effects.clear();
                edit(clip);
            })
        };
        let both_sides = "they apply before the Crop, Linear Wipe or Track Matte Key, and effects of the clip's own chain after it; one FX mask keeps the effects of only one side in order";
        for (case, clip, expected) in [
            ("source effects without a mask", edited(&|_| {}), Ok(Flat)),
            ("source effects over a Crop", bare(&crop), Ok(Staged)),
            ("source effects over a wipe", bare(&wipe), Ok(Staged)),
            (
                "source effects over an Opacity mask",
                bare(&masked),
                Ok(Staged),
            ),
            (
                "source effects and the clip's effect over a Crop",
                edited(&|clip| {
                    crop(clip);
                    clip.effects_above_mask = 1;
                }),
                Ok(Staged),
            ),
            (
                "source effects over a Crop that the clip's effect follows",
                edited(&crop),
                Err(both_sides),
            ),
            (
                "source effects over a wipe that the clip's effect follows",
                edited(&wipe),
                Err(both_sides),
            ),
            (
                "source effects over Crop and wipe",
                bare(&|clip| {
                    crop(clip);
                    wipe(clip);
                }),
                Err("Crop and Linear Wipe on one clip are not converted"),
            ),
        ] {
            assert_eq!(clip.mask_boundary(frame, frame, 1), expected, "{case}");
        }
    }

    #[test]
    fn measured_matte_transform_rejects_other_picture_and_clock_edits() {
        use super::{
            PrEffectParamKeys, PrEffectParams, PrKeyframeEasing, PrMatteChannel, TransformOwner,
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/feature_transform_track_matte_26_5_strict.prproj");
        let (project, _) = super::PrProjectFile::load(&path).unwrap();
        let canvas = [1920, 1080];
        let clips: Vec<_> = project.sequences[0].video_tracks[1]
            .items
            .iter()
            .filter_map(super::PrVideoItem::media)
            .collect();
        assert_eq!(clips.len(), 2);
        for clip in &clips {
            assert!(matches!(
                clip.transform_stage(canvas, canvas),
                Ok(Some((_, _, TransformOwner::KeyedPicture)))
            ));
            // In both saved orders the Transform is the stage, never an
            // effect after the matte, so source effects stay before it.
            for source_effects in [0, 1] {
                assert_eq!(
                    clip.mask_boundary(canvas, canvas, source_effects),
                    Ok(super::MaskBoundary::Staged)
                );
            }
        }
        assert_eq!(
            clips
                .iter()
                .map(|clip| clip.effects_above_mask)
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([0, 1])
        );
        type Edit = fn(&mut PrVideoOccurrence);
        let cases: [(&str, Edit); 12] = [
            ("Scale", |clip| {
                if let PrEffectParams::Transform(t) = &mut clip.effects[0].params {
                    t.scale_height = 110.0;
                }
            }),
            ("Rotation", |clip| {
                if let PrEffectParams::Transform(t) = &mut clip.effects[0].params {
                    t.rotation = 15.0;
                }
            }),
            ("Shutter", |clip| {
                if let PrEffectParams::Transform(t) = &mut clip.effects[0].params {
                    t.shutter_angle = 180.0;
                }
            }),
            ("Transform Opacity", |clip| {
                if let PrEffectParams::Transform(t) = &mut clip.effects[0].params {
                    t.opacity = 50.0;
                }
            }),
            ("Hold Position", |clip| {
                if let PrEffectParamKeys::Point(keys) = &mut clip.effects[0].animations[0].keys {
                    keys[0].easing = PrKeyframeEasing::Hold;
                }
            }),
            ("spatial tangent", |clip| {
                if let PrEffectParamKeys::Point(keys) = &mut clip.effects[0].animations[0].keys {
                    keys[0].spatial_out_tangent = Some([0.1, 0.0]);
                }
            }),
            ("another Transform", |clip| clip.active_transforms = 2),
            ("another effect", |clip| {
                clip.effects.push(clip.effects[0].clone())
            }),
            ("clip Motion", |clip| clip.transform.position = [0.6, 0.5]),
            ("retime", |clip| clip.playback_rate = 2.0),
            ("Luma", |clip| {
                clip.track_matte.as_mut().unwrap().channel = PrMatteChannel::Luma
            }),
            ("Reverse", |clip| {
                clip.track_matte.as_mut().unwrap().channel = PrMatteChannel::AlphaInverted
            }),
        ];
        for (name, edit) in cases {
            let mut clip = clips[0].clone();
            edit(&mut clip);
            assert!(clip.transform_stage(canvas, canvas).is_err(), "{name}");
        }
    }

    #[test]
    fn one_transform_on_canvas_sized_media_without_a_mask_stages_the_clip() {
        use super::{
            MaskBoundary::{Flat, Staged},
            PrEffect, PrEffectParams, PrGaussianBlur, PrMatteChannel, PrTrackMatte,
        };
        use crate::tests::support::{transform_effect, DEFAULT_PR_TRANSFORM};
        let transform = || transform_effect(DEFAULT_PR_TRANSFORM, Vec::new());
        let blur = || PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::GaussianBlur(PrGaussianBlur {
                blurriness: 40.0,
                repeat_edge_pixels: false,
            }),
            animations: Vec::new(),
        };
        let edited = |effects: Vec<PrEffect>, edit: &dyn Fn(&mut PrVideoOccurrence)| {
            let mut clip = clip_of("source", 0..TICKS, 0);
            clip.active_transforms = u8::try_from(
                effects
                    .iter()
                    .filter(|effect| matches!(effect.params, PrEffectParams::Transform(_)))
                    .count(),
            )
            .unwrap();
            clip.effects = effects;
            edit(&mut clip);
            clip
        };
        let canvas = [1920, 1080];
        // (case, clip, source, the stage's index or the reason's start, boundary)
        for (case, clip, source, stage, boundary) in [
            (
                "no Transform",
                edited(vec![], &|_| {}),
                canvas,
                Ok(None),
                Flat,
            ),
            (
                "one Transform",
                edited(vec![transform()], &|_| {}),
                canvas,
                Ok(Some(0)),
                Staged,
            ),
            (
                "a Transform above another effect",
                edited(vec![blur(), transform()], &|_| {}),
                canvas,
                Ok(Some(1)),
                Staged,
            ),
            // The reader counted a second active Transform that it did not
            // convert.
            (
                "a Transform beside a rejected one",
                edited(vec![transform()], &|clip| clip.active_transforms = 2),
                canvas,
                Err("another active Transform on the same clip is not converted with this one"),
                Flat,
            ),
            (
                "a Transform with a Crop",
                edited(vec![transform()], &|clip| clip.crop.left = 20.0),
                canvas,
                Err(
                    "a Transform with a Crop, Linear Wipe or Opacity mask on one clip is not converted",
                ),
                Flat,
            ),
            // The Opacity mask applies after every effect, so the reader
            // counts them all as applied before it: the mask stages the clip.
            (
                "a Transform with an Opacity mask",
                edited(vec![transform()], &|clip| {
                    clip.opacity_mask = Some(opacity_mask());
                    clip.effects_above_mask = clip.effects.len();
                }),
                canvas,
                Err(
                    "a Transform with a Crop, Linear Wipe or Opacity mask on one clip is not converted",
                ),
                Staged,
            ),
            // A Transform that applies before the key stages the clip with
            // the key's matte, not as a Transform stage.
            (
                "a Transform with a Track Matte Key",
                edited(vec![transform()], &|clip| {
                    clip.track_matte = Some(PrTrackMatte {
                        track_index: 1,
                        channel: PrMatteChannel::Alpha,
                    });
                    clip.effects_above_mask = clip.effects.len();
                }),
                canvas,
                Err("a Transform with a Track Matte Key on one clip is not converted"),
                Staged,
            ),
            (
                "a Transform on portrait media",
                edited(vec![transform()], &|_| {}),
                [1080, 1920],
                Err("a Transform on media that is not sequence-sized is not converted"),
                Flat,
            ),
        ] {
            let actual = clip
                .transform_stage(source, canvas)
                .map(|stage| stage.map(|(index, _, _)| index));
            match stage {
                Ok(index) => assert_eq!(actual, Ok(index), "{case}"),
                Err(reason) => {
                    let actual = actual.err().unwrap_or_else(|| panic!("{case}: staged"));
                    assert!(actual.starts_with(reason), "{case}: {actual}");
                }
            }
            assert_eq!(
                clip.mask_boundary(source, canvas, 0),
                Ok(boundary),
                "{case}"
            );
        }
    }
}
