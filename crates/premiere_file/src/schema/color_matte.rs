//! Premiere's Color Matte generator: synthetic media that renders one opaque,
//! canvas-sized solid colour for an effectively infinite duration.
//!
//! The native shape is taken from Adobe-authored corpus projects (`lower_third`,
//! `credits`, `phone_title`, `countdown_title`, `horror_title`,
//! `corporate_slideshow`): the `Media` record carries the shared generator
//! `ImplementationID`, the `COLR` pseudo path, `Infinite=true`, and an
//! 8-byte `ImporterPrefs` blob holding the colour.

use super::TICKS;
use crate::error::{unsupported, Result};

/// `ImplementationID` of Premiere's synthetic generator importer.
///
/// It does not identify a Color Matte: corpus Graphic (`FilePath` `GRFV`,
/// 1196574294), Black Video (`BLAK`, 1112293707) and Color Matte (`COLR`,
/// [`COLOR_MATTE_FILE_PATH`]) media all carry it, e.g. in
/// `corporate_slideshow` and `food_promo`.
pub(crate) const GENERATOR_IMPLEMENTATION_ID: &str = "42008e7a-de6f-4270-96de-7e287abb9b4b";
/// `FilePath`/`ActualMediaFilePath` of Color Matte media: the `COLR`
/// four-character code, which distinguishes it from other generators.
pub(crate) const COLOR_MATTE_FILE_PATH: &str = "1129270354";
/// Native `Infinite` generator duration: 12 hours at every corpus frame rate.
pub(crate) const COLOR_MATTE_INTRINSIC_TICKS: i64 = 12 * 60 * 60 * TICKS;
/// Default project-item name; user renames are not round-tripped.
pub(crate) const COLOR_MATTE_NAME: &str = "Color Matte";

const PREFS_LEN: usize = 8;
const PREFS_VERSION: [u8; 4] = 1u32.to_le_bytes();

/// One authored solid colour as Premiere stores it: three 8-bit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrColorMatte {
    pub(crate) rgb: [u8; 3],
}

impl PrColorMatte {
    /// Decode the raw `ImporterPrefs` payload `[r, g, b, 0, 1, 0, 0, 0]`.
    ///
    /// The channel order was inferred from corpus colours (a steel-blue
    /// `corporate_slideshow` matte, a purple `countdown_title` background) and
    /// is verified by the AME render of the red/blue
    /// `premiere_isolated_color_matte` fixture: its red matte renders
    /// (255,0,0) and its blue matte (1,0,255), so the channels are not swapped.
    pub(crate) fn from_importer_prefs(bytes: &[u8]) -> Result<Self> {
        let Ok(bytes) = <[u8; PREFS_LEN]>::try_from(bytes) else {
            return Err(unsupported(format!(
                "Color Matte ImporterPrefs must be {PREFS_LEN} bytes, found {}",
                bytes.len()
            )));
        };
        if bytes[3] != 0 || bytes[4..] != PREFS_VERSION {
            return Err(unsupported(
                "Color Matte ImporterPrefs has an unknown layout; only version 1 opaque RGB is supported",
            ));
        }
        Ok(Self {
            rgb: [bytes[0], bytes[1], bytes[2]],
        })
    }

    pub(crate) fn importer_prefs(self) -> [u8; PREFS_LEN] {
        let [r, g, b] = self.rgb;
        let [v0, v1, v2, v3] = PREFS_VERSION;
        [r, g, b, 0, v0, v1, v2, v3]
    }

    /// The opaque fill colour with channels in `0..=1`.
    pub(crate) fn fill_color(self) -> [f64; 4] {
        let [r, g, b] = self.rgb.map(|channel| f64::from(channel) / 255.0);
        [r, g, b, 1.0]
    }

    /// Premiere mattes are opaque 8-bit RGB; channels round to the nearest step.
    pub(crate) fn from_fill_color(color: [f64; 4]) -> Result<Self> {
        if color[3] != 1.0 {
            return Err(unsupported(
                "Color Matte fill must be fully opaque; fill alpha is unsupported",
            ));
        }
        let mut rgb = [0u8; 3];
        for (channel, value) in rgb.iter_mut().zip(color) {
            if !(0.0..=1.0).contains(&value) {
                return Err(unsupported(
                    "Color Matte fill channels must lie within 0 to 1",
                ));
            }
            // The range check bounds the rounded value to 0..=255.
            *channel = (value * 255.0).round() as u8;
        }
        Ok(Self { rgb })
    }

    /// Lowercase `rrggbb`, used to share one generator record per colour.
    pub(crate) fn hex(self) -> String {
        let [r, g, b] = self.rgb;
        format!("{r:02x}{g:02x}{b:02x}")
    }
}
