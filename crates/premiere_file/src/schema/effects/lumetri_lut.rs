//! Lumetri Color custom `.cube` LUTs: the Basic Correction Input LUT and the
//! Creative Look, as Premiere 26.5.2 saves a LUT browsed in its UI
//! (`color_lumetri_look_cube.prproj`).
//!
//! The browsed file is saved as its absolute path in a binary path slot:
//! UTF-16LE with one NUL terminator, base64 ([`aftereffects_file::lumetri::luts::INPUT_LUT_PATH`],
//! [`aftereffects_file::lumetri::luts::LOOK_PATH`]). A slot equal to another one is saved empty with
//! the other's `BinaryHash`. The popups ([`LUMETRI_INPUT_LUT_POPUP`],
//! [`LUMETRI_LOOK_POPUP`]) stay at 0 for a browsed file: a nonzero popup
//! selects one of Lumetri's built-in LUTs instead. Premiere also writes the
//! LUT paths into the `Blob` shader graph (ParameterID 1) and an embedded copy
//! of the samples into `Embedded LUTs` (ParameterID 98). The reader names the
//! file by its path slot and packages the file itself, falling back to the
//! embedded copy when the file is missing ([`crate::lut_assets`]). The slot
//! `ParameterID`s are [`aftereffects_file::lumetri::luts`]'s, shared with After Effects.

use std::path::PathBuf;

use fx_schema::AssetId;

use super::{EffectParamSpec, LUMETRI_PARAMS};

/// One of Lumetri's two custom-LUT slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LumetriLutSlot {
    /// Basic Correction > Input LUT, applied before the Basic Correction.
    Input,
    /// Creative > Look, applied after the Basic Correction at an Intensity.
    Look,
}

impl LumetriLutSlot {
    /// The Premiere control, as its UI names it.
    pub(crate) const fn control(self) -> &'static str {
        match self {
            Self::Input => "Lumetri Color > Basic Correction > Input LUT",
            Self::Look => "Lumetri Color > Creative > Look",
        }
    }
}

/// A custom `.cube` selected in a Lumetri LUT slot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrLumetriLut {
    /// The path as Premiere saved it (absolute on the authoring host).
    pub(crate) native_path: String,
    /// The `.cube` file and its package asset ([`crate::lut_assets`]).
    pub(crate) file: LutFile,
    /// Import: the copy of the LUT that Premiere embedded in the project
    /// for this file, when the `Blob` shader graph references it.
    pub(crate) embedded: Option<EmbeddedLut>,
}

/// LUT samples that Premiere 26.5.2 embeds in `Embedded LUTs` (ParameterID
/// 98) for a browsed `.cube`, shared with After Effects.
pub(crate) use aftereffects_file::lumetri::luts::EmbeddedLut;

impl PrLumetriLut {
    /// Whether the LUT names its `.cube`: a saved path, or (export, before
    /// the package file is chosen) the FX asset.
    pub(crate) fn is_named(&self) -> bool {
        !self.native_path.is_empty() || matches!(self.file, LutFile::Packaged(_))
    }
}

/// The `.cube` file of a [`PrLumetriLut`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LutFile {
    /// Import: not looked up; the effect was read but not prepared for
    /// packaging.
    Unresolved,
    /// Import: the located file and its FX asset. Export: the FX asset and
    /// the file it is written to in the Premiere package
    /// ([`crate::lut_assets::bind_luts`]), whose path is `native_path`.
    Found(ResolvedLut),
    /// Import: why no usable file was found.
    Missing(String),
    /// Export: the FX asset of a `lookTransform`, not yet given its file in
    /// the Premiere package.
    Packaged(AssetId),
}

/// A `.cube` file and the FX asset that holds its bytes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedLut {
    pub(crate) path: PathBuf,
    pub(crate) asset_id: AssetId,
}

/// A Lumetri Creative Look with a custom `.cube`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrLumetriLook {
    pub(crate) lut: PrLumetriLut,
    /// Look Intensity, 0 to 200 (100 applies the LUT once).
    pub(crate) intensity: f64,
}

/// Basic Correction Input LUT popup: 0 is None or a browsed file.
pub(crate) const LUMETRI_INPUT_LUT_POPUP: usize = 5;
/// Creative Look popup: 0 is None or a browsed file.
pub(crate) const LUMETRI_LOOK_POPUP: usize = 25;
/// The Look Color Space popup, and the value a Rec. 709 sequence saves.
pub(crate) const LUMETRI_LOOK_COLOR_SPACE: usize = 130;
pub(crate) const LUMETRI_LOOK_COLOR_SPACE_REC709: f64 = 28.0;

/// The pinned layout parameter `id` ([`LUMETRI_PARAMS`]).
pub(crate) fn lumetri_param(id: usize) -> Option<&'static EffectParamSpec> {
    LUMETRI_PARAMS.iter().find(|param| param.id == id)
}
