//! Still-image media facts shared by the reader, converter, and writer.
//!
//! Premiere gives a still no media clock: its `VideoStream` carries `IsStill`
//! and, on every still in the corpus, a synthetic twelve-hour `Duration`; its
//! `Media` record is `Infinite`. A new placement's source range lasts as long
//! as the placement itself; a saved one may keep another span, which shows the
//! same picture. It starts one hour into that clock, rounded down to a
//! frame of the sequence rate
//! ([`FrameRate::generator_in_ticks`](super::FrameRate::generator_in_ticks)):
//! all 104 still placements of the 30 fps `corporate_slideshow` (93 of them on
//! still streams with `FrameRate` 8467200000) have `InPoint`
//! 914457600000000, and its still master clips span 0–5 s. The 29.97 fps
//! `stills_and_panorama` and `phone_title` start stills at 914456685542400
//! (107 892 frames). Premiere 26.5.1 can also keep a still's 5 s span on a
//! placement of another length: both shortening and lengthening convert at
//! unit forward rate; lengthening uses checked In plus placement duration
//! for the media-end bound. Other rates keep the source-span rule ([`PrVideoOccurrence::validate`](super::PrVideoOccurrence::validate)).

use super::{HdrProfile, PrAfterEffectsComposition, PrColorMatte, VideoCodec, TICKS};

/// Premiere's default channel selection when it first opens an OpenEXR file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenExrChannels {
    Rgb {
        red: bool,
        green: bool,
        blue: bool,
    },
    Luma,
    LumaChroma,
    /// Native input was classified before its file/import preferences were read.
    Unspecified,
}

/// Whether a media record is decoded video, a Premiere still image, a linked
/// After Effects composition, Color Matte generator media, or the Black Video
/// generator media of an adjustment layer.
///
/// A video's `codec` is the codec of the inspected file, and `hdr_profile` the
/// saved colour profile of an HDR source (`None` writes the BT.709 profile).
/// The reader leaves both unset because files are inspected after loading;
/// export takes them from the inspected packaged video, and the writer
/// requires the codec for `CodecType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrMediaKind {
    Video {
        codec: Option<VideoCodec>,
        hdr_profile: Option<HdrProfile>,
    },
    /// `alpha` is the straight-alpha declaration that native `AlphaType`
    /// carries: `true` for `1`, `false` when it is absent. A Tesseract document
    /// records no alpha fact, so export takes it from the inspected packaged
    /// image.
    Still { alpha: bool },
    /// Original OpenEXR bytes decoded by Premiere's native `oEXR` importer.
    /// `numbered` selects its finite image-sequence clock instead of a still.
    OpenExr {
        alpha: bool,
        numbered: bool,
        channels: OpenExrChannels,
    },
    /// Consecutive numbered image files on the stream's finite source clock.
    /// The linked filename supplies the first number; Duration/FrameRate the count.
    NumberedStills { alpha: bool },
    /// Editable AEP-backed source; never inspect it as an ordinary video file.
    AfterEffectsComposition(PrAfterEffectsComposition),
    /// Generator media with no file, paths or asset (`schema/color_matte.rs`).
    ColorMatte(PrColorMatte),
    /// Black Video generator media that every placement flags as an
    /// adjustment layer (`schema/adjustment.rs`); it has no picture of its own.
    Adjustment,
}

impl PrMediaKind {
    pub(crate) fn is_still(self) -> bool {
        matches!(
            self,
            Self::Still { .. }
                | Self::OpenExr {
                    numbered: false,
                    ..
                }
        )
    }

    pub(crate) fn is_numbered_stills(self) -> bool {
        matches!(
            self,
            Self::NumberedStills { .. } | Self::OpenExr { numbered: true, .. }
        )
    }

    pub(crate) fn is_adjustment(self) -> bool {
        matches!(self, Self::Adjustment)
    }
}

/// Premiere's synthetic still source duration: twelve hours.
pub(crate) const STILL_INTRINSIC_TICKS: i64 = 12 * 60 * 60 * TICKS;
/// Native `VideoStream.CodecType` for stills (`RAW ` four-character code).
pub(crate) const STILL_CODEC_TYPE: &str = "1380013856";
/// Native `VideoStream.AlphaType` for a still whose file carries straight alpha.
pub(crate) const STILL_STRAIGHT_ALPHA_TYPE: &str = "1";
/// Native `VideoStream.CodecType` from Premiere's bundled OpenEXR importer.
pub(crate) const OPENEXR_CODEC_TYPE: &str = "1281443650";
/// The bundled importer reports an EXR A channel as black-matte alpha.
pub(crate) const OPENEXR_ALPHA_TYPE: &str = "2";
