//! Premiere's mask records: the Opacity mask that converts, the
//! four saved record forms and the Mask Path value with its keys.
//!
//! A mask is a standard-shaped `VideoFilterComponent` that its owner names in
//! `SubComponents`. Two forms are `AE.ADBE AEMask` from the corpus (57 masks in
//! 5 projects), following the owner's record generation: `Version` 7 with
//! `Component` 5 and 13 parameters, or `Version` 8 with `Component` 6 and the
//! same parameters plus two more tracking booleans (`ParameterID` 14 and 15)
//! and wider Feather and Expansion bounds. Premiere 26.5.1 re-saves every mask
//! as `AE.ADBE AEMask2`, `Version` 9 with `Component` 7, `DisplayName` Mask2
//! and 35 parameters, with no `Bypass` or `Intrinsic` ([`MaskForm`]): fixture
//! G5 shows it accepting and upgrading an XML-written v8/c6 record, and the
//! export gate the same for the v7 form the writer emits (the generation of
//! every component it writes): values and vertex bodies survive the re-save.
//! Premiere 26.3 saves the same record generation with 27 parameters: the
//! 26.5.1 layout without its sharpness and levels controls (`ParameterID` 30
//! to 37), observed on both masks of one native project last saved by 26.3
//! (no public fixture); the parameter count tells the two forms apart.
//!
//! Mask Path, Feather, Opacity, Expansion and Inverted convert directly.
//! Saved affine Tracker samples also lower to Path keys over an unchanged
//! reference outline; the resulting editable paths use linear interpolation.
//! Every other parameter stays at the one value each saved mask holds
//! ([`MaskControl::Default`], [`MaskParamRole::Binary`]), or the mask fails
//! closed: in the corpus forms the tracking booleans (`false`) and the
//! constants 2, 0 and 0.5 of unknown meaning; in the 26.5.1 form also the mask
//! transform (Scale 1:1, Scale Height and Width 100, Rotation 0), sharpness
//! and levels controls at their defaults, the mask Blend Mode and Type 0 and
//! three tracker values. The version-1 transform has two affine matrices and
//! reference controls; its samples convert through `decode_mask_tracker`.
//! Another value may name any tracker ([`MaskParamRole::TrackerState`]).
//! Constant scalar keys at supported defaults are inert and retained as those
//! defaults. Mask Position and Anchor Point are the
//! outline's centre and equal in every saved mask ([`MaskControl::Centre`]).
//! Inverted is parameter 10 in the corpus forms (the ten inverted Text masks
//! are its only `true` values) and 17 in the 26.5.1 form (fixture clip D).
//!
//! The corpus Mask Path value (`2cin`) is `2cin`, `u32 2`, a `u32` `z` that is
//! `1` on the 14 ellipse masks and `0` on rectangles and pen paths (meaning
//! unknown; both read, `0` is written), `u32 n`, then `n` vertices in the
//! graphic Path's 28-byte vertex layout (`format::shape_payload`) each
//! followed by `u32 1`. The 26.5.1 value keeps the same vertex bodies in a new
//! envelope: `u32 2`, `u32 1`, the byte `0x56`, `u32` payload length minus 13,
//! `u32 2`, `u32 n`, the `n` vertices and a final byte `1`; `z` is not kept
//! (field meanings observed on the five fixture masks, not inferred). There is
//! no closed byte: a mask outline is closed. Coordinates are unit fractions of
//! the clip's source frame, x by its width and y by its height (values outside
//! 0 to 1 occur): fixture G1 (clip B at Scale 50).
//!
//! A keyed Mask Path stores `ticks,base64;` per key in `Keyframes`, each a
//! complete value of the form's codec on the owner's source clock, with no
//! interpolation field, as Source Text keys are: without `IsTimeVarying` in a
//! v8 record that Premiere 26.5.1 opened and AME rendered, and with it `true`
//! in that record's 26.5.1 save, which also keys mask Position and Anchor
//! Point identically ([`PrMaskPathKey`], [`MaskControl::Centre`]).

mod tracker;
pub(crate) use tracker::{decode_mask_tracker, MaskTrackerTransform};

use super::{records, text::PrShapePath, PrKeyframeEasing, PrScalarKeyframe};
use crate::{
    error::{ensure, unsupported},
    format::{
        ensure_valid,
        shape_payload::{decode_vertex, encode_vertex, VERTEX_BYTES},
    },
};
use base64::{engine::general_purpose::STANDARD, Engine};

/// The Mask Path payload magic.
const MASK_PATH_MAGIC: &[u8; 4] = b"2cin";
const MASK_PATH_VERSION: u32 = 2;
const MASK_PATH_HEADER_BYTES: usize = 16;
/// Each vertex ends with this word in every corpus path.
const MASK_VERTEX_TRAILER: u32 = 1;
const MASK_VERTEX_BYTES: usize = VERTEX_BYTES + 4;
/// The 26.5.1 Mask Path envelope: two words, one byte, the length word and two
/// words before the vertices.
const MASK_PATH_26_5_HEADER: [u32; 2] = [2, 1];
const MASK_PATH_26_5_BYTE: u8 = 0x56;
const MASK_PATH_26_5_WORD: u32 = 2;
const MASK_PATH_26_5_HEADER_BYTES: usize = 21;
/// The 26.5.1 length word counts the payload after the envelope's first 13
/// bytes.
const MASK_PATH_26_5_LENGTH_OFFSET: usize = 13;
const MASK_PATH_26_5_TRAILER: u8 = 1;

/// Match name of the corpus mask records, which the writer emits.
pub(crate) const MASK_MATCH_NAME: &str = "AE.ADBE AEMask";
/// Match name of every mask Premiere 26.5.1 saves.
pub(crate) const MASK_MATCH_NAME_26_5: &str = "AE.ADBE AEMask2";

/// Mask Feather's upper bound in the written (v7) form, which both directions
/// enforce so that an imported mask exports again.
pub(crate) const MASK_FEATHER_MAX: f64 = 1000.0;

/// The editable value is retained, not calibrated to Premiere's edge radius.
pub(crate) const MASK_EXPANSION_APPROXIMATION: &str = "Mask Expansion converts one to one; positive FX expansion uses an approximate dilation kernel (half-value growth per axis), whose Premiere radius equivalence is unmeasured; negative expansion remains editable but does not shrink coverage in current FX";

/// Reported once per clip whose mask has a nonzero feather, in both
/// directions. Premiere's feather is a Gaussian edge of sigma about 0.37 times
/// Feather on the source frame (fixture G2: 22.1 px at 60, the one measured
/// radius); the FX mask feather is the renderer's separable blur at a step of
/// Feather / 2.4 px, whose width is quantized by iteration count above 9.6 px
/// and stops growing at the 64-iteration cap (`compute_blur_params`, about
/// Feather 77 and sigma 54 px), so no parameter transform makes the two equal
/// (D17-2).
pub(crate) const MASK_FEATHER_APPROXIMATION: &str = "Mask Feather converts one to one to the FX mask feather, an approximation; Premiere feather visuals are not preserved exactly";

/// The mask record match name that `match_name` is, in any saved form.
pub(crate) fn mask_match_name(match_name: Option<&str>) -> Option<&'static str> {
    [MASK_MATCH_NAME, MASK_MATCH_NAME_26_5]
        .into_iter()
        .find(|name| match_name == Some(name))
}

/// One Opacity mask: Premiere gates the clip's alpha by the path's coverage
/// times Mask Opacity, softened by Mask Feather (fixture G1, G2). An inverted
/// mask renders Mask Opacity times one minus the coverage (G3b), where FX
/// inverts the opacity-weighted coverage, so Inverted converts only at Mask
/// Opacity 100, including every key. Numeric controls retain their editable
/// keys; feather and nonzero expansion have explicit rendering approximations.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrMask {
    /// Native saved raster reference, replaced by inspected numbered media before import.
    pub(crate) raster: Option<RasterMask>,
    /// The closed outline in unit fractions of the clip's source frame; with
    /// keys, the first key's, which Premiere shows before that key.
    pub(crate) path: PrShapePath,
    /// The Mask Path keys in source-time order; empty for a static outline.
    /// The reader keeps them only on a video clip's mask: a still,
    /// composition or graphic whose mask is keyed is omitted, so a static
    /// consumer never meets them.
    pub(crate) path_keys: Vec<PrMaskPathKey>,
    /// Mask Feather, in source pixels, 0 to [`MASK_FEATHER_MAX`].
    pub(crate) feather: f64,
    pub(crate) feather_keys: Vec<PrScalarKeyframe>,
    /// Signed source-pixel expansion, bounded by the written v7 record.
    pub(crate) expansion: f64,
    pub(crate) expansion_keys: Vec<PrScalarKeyframe>,
    /// Mask Opacity, in percent.
    pub(crate) opacity: f64,
    pub(crate) opacity_keys: Vec<PrScalarKeyframe>,
    pub(crate) inverted: bool,
}

/// Converter-only recovery state; never persisted into FX or replayed on export.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RasterMask {
    Saved(crate::format::object_mask::Tracker),
    Prepared(super::MediaId),
}

/// One Mask Path key: the outline that the mask has at `source_ticks`.
/// Premiere 26.5.1 shows the first key's outline before it and the last key's
/// after it, moves each vertex linearly in time between two keys of one
/// vertex count, and holds the earlier outline between keys whose counts
/// differ (the stored value is not drawn; a translated rectangle
/// within 0.07 px of linear; a 4-vertex key held through at least 1.9 s of a
/// 1.5 s to 2.0 s interval and the 5-vertex key shown at 2.0 s). Inferred:
/// that the switch is at the later key, and that tangents and non-rigid
/// changes move as the translated vertices did.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrMaskPathKey {
    /// On the owner's source clock, as Motion keys.
    pub(crate) source_ticks: i64,
    pub(crate) path: PrShapePath,
}

/// Checks one mask outline: closed, of at least three finite vertices.
fn validate_outline(path: &PrShapePath) -> crate::format::Result<()> {
    ensure_valid!(
        path.closed && path.vertices.len() >= 3,
        "a mask path must be a closed outline of at least three vertices"
    );
    ensure_valid!(
        path.vertices
            .iter()
            .flat_map(|vertex| [vertex.point, vertex.in_tangent, vertex.out_tangent])
            .flatten()
            .all(f32::is_finite),
        "mask path vertices must be finite"
    );
    Ok(())
}

impl PrMask {
    pub(crate) fn numeric_keys(&self) -> [(&'static str, &[PrScalarKeyframe]); 3] {
        [
            ("feather", &self.feather_keys),
            ("expansion", &self.expansion_keys),
            ("opacity", &self.opacity_keys),
        ]
    }

    pub(crate) fn has_numeric_keys(&self) -> bool {
        self.numeric_keys().iter().any(|(_, keys)| !keys.is_empty())
    }

    pub(crate) fn approximations(&self) -> impl Iterator<Item = &'static str> {
        [
            (
                self.feather != 0.0 || self.feather_keys.iter().any(|key| key.value != 0.0),
                MASK_FEATHER_APPROXIMATION,
            ),
            (
                self.expansion != 0.0 || self.expansion_keys.iter().any(|key| key.value != 0.0),
                MASK_EXPANSION_APPROXIMATION,
            ),
        ]
        .into_iter()
        .filter_map(|(needed, message)| needed.then_some(message))
    }

    pub(crate) fn validate(&self) -> crate::format::Result<()> {
        for (name, value, keys, lower, upper) in [
            (
                "Feather",
                self.feather,
                &self.feather_keys,
                0.0,
                MASK_FEATHER_MAX,
            ),
            (
                "Expansion",
                self.expansion,
                &self.expansion_keys,
                -1000.0,
                1000.0,
            ),
            ("Opacity", self.opacity, &self.opacity_keys, 0.0, 100.0),
        ] {
            if name == "Expansion" {
                ensure_valid!(
                    value.is_finite() && (lower..=upper).contains(&value),
                    "Mask {name} must be finite and within {lower}..={upper}"
                );
            }
            ensure_valid!(
                keys.windows(2)
                    .all(|pair| pair[0].source_ticks < pair[1].source_ticks),
                "Mask {name} keys must have strictly increasing source times"
            );
            for key in keys {
                ensure_valid!(
                    key.value.is_finite() && (lower..=upper).contains(&key.value),
                    "Mask {name} key must be finite and within {lower}..={upper}"
                );
                ensure_valid!(
                    mask_numeric_easing(key.easing),
                    "Mask {name} supports only Linear, Hold or zero-speed Bezier keys"
                );
            }
            ensure_valid!(
                keys.windows(2)
                    .all(|pair| pair[1].easing != PrKeyframeEasing::Hold
                        || matches!(
                            pair[0].easing,
                            PrKeyframeEasing::Linear
                                | PrKeyframeEasing::Hold
                                | PrKeyframeEasing::CubicBezier {
                                    x2: 1.0,
                                    y2: 1.0,
                                    ..
                                }
                        )),
                "Mask {name} cannot preserve the curve arriving into a Hold key"
            );
        }
        if self.raster.is_some() {
            ensure_valid!(
                self.path.vertices.is_empty()
                    && self.path_keys.is_empty()
                    && !self.has_numeric_keys()
                    && self.feather == 0.0
                    && self.expansion == 0.0
                    && (!self.inverted || self.opacity == 100.0),
                "Object Mask raster requires zero Feather/Expansion and unkeyed coverage; inverted non-full Opacity is unproved (G3b)"
            );
        } else {
            validate_outline(&self.path)?;
        }
        for key in &self.path_keys {
            validate_outline(&key.path)?;
        }
        ensure_valid!(
            self.path_keys
                .windows(2)
                .all(|pair| pair[0].source_ticks < pair[1].source_ticks),
            "Mask Path keys must have strictly increasing source times"
        );
        ensure_valid!(
            self.path_keys
                .first()
                .is_none_or(|first| first.path == self.path),
            "a keyed mask's outline is its first key's"
        );
        ensure_valid!(
            self.feather.is_finite() && (0.0..=MASK_FEATHER_MAX).contains(&self.feather),
            "Mask Feather must be finite and within 0..={MASK_FEATHER_MAX}, the written record's bound"
        );
        ensure_valid!(
            self.opacity.is_finite() && (0.0..=100.0).contains(&self.opacity),
            "Mask Opacity must be finite and within 0..=100"
        );
        ensure_valid!(
            !self.inverted
                || (self.opacity == 100.0
                    && self.opacity_keys.iter().all(|key| key.value == 100.0)),
            "an inverted mask with Mask Opacity below 100 is not converted: Premiere renders Mask Opacity times the inverted coverage, FX inverts the Mask Opacity-weighted coverage"
        );
        Ok(())
    }
}

/// Zero-speed handles avoid inferring Premiere mask velocity units.
pub(crate) fn mask_numeric_easing(easing: PrKeyframeEasing) -> bool {
    match easing {
        PrKeyframeEasing::Linear | PrKeyframeEasing::Hold => true,
        PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            [x1, y1, x2, y2].into_iter().all(f64::is_finite)
                && (0.0..=1.0).contains(&x1)
                && (0.0..=1.0).contains(&x2)
                && y1 == 0.0
                && y2 == 1.0
        }
    }
}

/// Mask Feather's upper bound (also the Expansion magnitude) in the v7 form
/// and in the v8 and 26.5.1 forms.
const SLIDER_RANGE_V7: &str = "1000";
const SLIDER_RANGE_WIDE: &str = "5000";

/// One saved mask record form: its match and display names, the
/// `VideoFilterComponent` and `Component` versions, its parameters (the first
/// `param_count` of `params`), the Feather upper bound (also the Expansion
/// magnitude) and its Mask Path value. The 26.5.1 and 26.3 forms share their
/// names and versions and differ only in their parameters.
pub(crate) struct MaskForm {
    pub(crate) match_name: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) component_version: &'static str,
    pub(crate) body_version: &'static str,
    /// Whether the record writes `Bypass` and `Intrinsic` (both `false`; a
    /// bypassed mask writes `Bypass` `true`). Premiere 26.5.1 writes neither,
    /// and only there does a missing `Bypass` mean active (the 26.5.1 Crop
    /// rule); a bypassed 26.5.1 mask is unobserved and fails closed.
    pub(crate) flags_written: bool,
    params: &'static [MaskParamSpec],
    pub(crate) param_count: usize,
    pub(crate) slider_range: &'static str,
    pub(crate) decode_path: fn(&[u8]) -> crate::error::Result<PrShapePath>,
}

/// The form of `abstract_slideshow` (Premiere 12.1) and `vhs_slideshow`
/// (14.4), which the writer emits.
pub(crate) const MASK_FORM_V7: MaskForm = MaskForm {
    match_name: MASK_MATCH_NAME,
    display_name: "Mask",
    component_version: "7",
    body_version: "5",
    flags_written: true,
    params: &MASK_PARAMS,
    param_count: 13,
    slider_range: SLIDER_RANGE_V7,
    decode_path: decode_mask_path,
};

/// The form of `cinemagraph` (Premiere 13.1) and `visualizer_slideshow`.
pub(crate) const MASK_FORM_V8: MaskForm = MaskForm {
    match_name: MASK_MATCH_NAME,
    display_name: "Mask",
    component_version: "8",
    body_version: "6",
    flags_written: true,
    params: &MASK_PARAMS,
    param_count: MASK_PARAMS.len(),
    slider_range: SLIDER_RANGE_WIDE,
    decode_path: decode_mask_path,
};

/// The form Premiere 26.5.1 saves (fixture `feature_opacity_masks_26_5_strict`,
/// masks A to E).
pub(crate) const MASK_FORM_26_5: MaskForm = MaskForm {
    match_name: MASK_MATCH_NAME_26_5,
    display_name: "Mask2",
    component_version: "9",
    body_version: "7",
    flags_written: false,
    params: &MASK_PARAMS_26_5,
    param_count: MASK_PARAMS_26_5.len(),
    slider_range: SLIDER_RANGE_WIDE,
    decode_path: decode_mask_path_26_5,
};

/// The form Premiere 26.3 saves: the 26.5.1 record with [`MASK_PARAMS_26_3`].
pub(crate) const MASK_FORM_26_3: MaskForm = MaskForm {
    params: &MASK_PARAMS_26_3,
    param_count: MASK_PARAMS_26_3.len(),
    ..MASK_FORM_26_5
};

impl MaskForm {
    /// The parameters this form holds, in the saved `Params` order.
    pub(crate) fn params(&self) -> &'static [MaskParamSpec] {
        &self.params[..self.param_count]
    }

    /// The form with this match name and these record versions that holds
    /// `param_count` parameters, or else the first form with them, whose
    /// layout the reader then rejects; `None` if no form has them.
    pub(crate) fn of(
        match_name: Option<&str>,
        component_version: Option<&str>,
        body_version: Option<&str>,
        param_count: usize,
    ) -> Option<&'static Self> {
        let generation: Vec<&'static Self> = [
            &MASK_FORM_V7,
            &MASK_FORM_V8,
            &MASK_FORM_26_5,
            &MASK_FORM_26_3,
        ]
        .into_iter()
        .filter(|form| {
            match_name == Some(form.match_name)
                && component_version == Some(form.component_version)
                && body_version == Some(form.body_version)
        })
        .collect();
        generation
            .iter()
            .find(|form| form.params().len() == param_count)
            .or(generation.first())
            .copied()
    }
}

/// What one mask parameter holds: the Mask Path value, another binary value
/// (`ArbVideoComponentParam`) at the one base64 value every saved mask holds,
/// the tracker state, or one static control (`VideoComponentParam`,
/// `PointComponentParam`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MaskParamRole {
    Path,
    Binary(&'static str),
    /// The 26.5.1 and 26.3 tracker state (`ParameterID` 24): this base64
    /// value apart from the UUID of the tracker it names
    /// ([`is_saved_tracker_state`]).
    TrackerState(&'static str),
    Control(MaskControl),
}

/// What one static mask control means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MaskControl {
    Feather,
    Opacity,
    /// Mask Expansion; converts only at 0.
    Expansion,
    Inverted,
    /// Mask Position or Anchor Point (26.5.1): the outline's centre. Every
    /// saved mask holds the same point in both, so a mask translation is
    /// unobserved and the mask converts only when they are equal: two equal
    /// static points, or two identical keyed records, which Premiere 26.5.1
    /// saved beside a keyed Mask Path. Either way Position minus
    /// Anchor Point stays zero, so the mask transform stays the identity.
    Centre,
    /// A control with no FX counterpart, at the one `StartKeyframe` value
    /// every saved mask holds (a number, `false` or a point); another value
    /// fails closed.
    Default(&'static str),
}

/// Whether a `StartKeyframe` value is `default`, without imposing one number
/// formatting (Premiere writes `100.`, the writer `100`).
pub(crate) fn at_default(value: &str, default: &str) -> bool {
    value == default
        || value
            .parse::<f64>()
            .ok()
            .zip(default.parse::<f64>().ok())
            .is_some_and(|(value, default)| value == default)
}

/// One mask parameter record. `name` is absent from the unnamed booleans and
/// constants; `control`, `lower` and `upper` where the save omits them; on
/// Feather and Expansion [`Self::bounds`] takes the form's range instead of
/// `lower` and `upper`.
#[derive(Clone, Copy)]
pub(crate) struct MaskParamSpec {
    pub(crate) id: usize,
    pub(crate) tag: &'static str,
    pub(crate) name: Option<&'static str>,
    pub(crate) class_id: &'static str,
    pub(crate) control: Option<&'static str>,
    pub(crate) lower: Option<&'static str>,
    pub(crate) upper: Option<&'static str>,
    pub(crate) lower_ui: Option<&'static str>,
    pub(crate) upper_ui: Option<&'static str>,
    pub(crate) role: MaskParamRole,
}

impl MaskParamSpec {
    /// The `LowerBound` and `UpperBound` of this parameter in `form`.
    pub(crate) fn bounds(&self, form: &MaskForm) -> (Option<String>, Option<String>) {
        match self.role {
            MaskParamRole::Control(MaskControl::Feather) => {
                (Some("0".to_owned()), Some(form.slider_range.to_owned()))
            }
            MaskParamRole::Control(MaskControl::Expansion) => (
                Some(format!("-{}", form.slider_range)),
                Some(form.slider_range.to_owned()),
            ),
            _ => (self.lower.map(str::to_owned), self.upper.map(str::to_owned)),
        }
    }

    /// The `StartKeyframe` field count of this parameter's record.
    pub(crate) fn key_fields(&self) -> usize {
        if self.tag == records::POINT_COMPONENT_PARAM.tag {
            14
        } else {
            8
        }
    }
}

const SCALAR: &str = records::VIDEO_COMPONENT_PARAM.tag;
const POINT: &str = records::POINT_COMPONENT_PARAM.tag;
const BINARY: &str = "ArbVideoComponentParam";
const BOOLEAN_CLASS_ID: &str = records::VIDEO_BOOL_COMPONENT_PARAM.class_id;
const SLIDER_CLASS_ID: &str = records::VIDEO_FILTER_AMOUNT_PARAM.class_id;
const POPUP_CLASS_ID: &str = records::VIDEO_POPUP_PARAM.class_id;
const ROTATION_CLASS_ID: &str = records::VIDEO_COMPONENT_PARAM.class_id;
const POINT_CLASS_ID: &str = records::POINT_COMPONENT_PARAM.class_id;
/// The `ArbVideoComponentParam` class of binary values (also Source Text).
pub(crate) const MASK_PATH_CLASS_ID: &str = "313e54d4-6903-49ad-b0bf-8262cdd10f4e";
/// The `ArbVideoComponentParam` version the writer emits for the Mask Path.
pub(crate) const MASK_PATH_RECORD_VERSION: &str = "2";
const BINARY_CONTROL: &str = "22";

const fn boolean(
    id: usize,
    name: Option<&'static str>,
    control: Option<&'static str>,
    lower: Option<&'static str>,
    upper: Option<&'static str>,
    role: MaskParamRole,
) -> MaskParamSpec {
    MaskParamSpec {
        id,
        tag: SCALAR,
        name,
        class_id: BOOLEAN_CLASS_ID,
        control,
        lower,
        upper,
        lower_ui: None,
        upper_ui: None,
        role,
    }
}

/// A corpus tracking boolean, always `false`.
const fn tracking(id: usize, control: &'static str, upper: &'static str) -> MaskParamSpec {
    boolean(
        id,
        None,
        Some(control),
        Some("false"),
        Some(upper),
        MaskParamRole::Control(MaskControl::Default("false")),
    )
}

const fn slider(
    id: usize,
    name: Option<&'static str>,
    class_id: &'static str,
    control: Option<&'static str>,
    lower: &'static str,
    upper: &'static str,
    role: MaskParamRole,
) -> MaskParamSpec {
    MaskParamSpec {
        id,
        tag: SCALAR,
        name,
        class_id,
        control,
        lower: Some(lower),
        upper: Some(upper),
        lower_ui: None,
        upper_ui: None,
        role,
    }
}

/// A corpus constant of unknown meaning.
const fn constant(id: usize, upper: &'static str, value: &'static str) -> MaskParamSpec {
    slider(
        id,
        None,
        SLIDER_CLASS_ID,
        Some("8"),
        "0",
        upper,
        MaskParamRole::Control(MaskControl::Default(value)),
    )
}

const fn binary(id: usize, name: &'static str, role: MaskParamRole) -> MaskParamSpec {
    MaskParamSpec {
        id,
        tag: BINARY,
        name: Some(name),
        class_id: MASK_PATH_CLASS_ID,
        control: Some(BINARY_CONTROL),
        lower: None,
        upper: None,
        lower_ui: None,
        upper_ui: None,
        role,
    }
}

const fn point(id: usize, name: &'static str, control: MaskControl) -> MaskParamSpec {
    MaskParamSpec {
        id,
        tag: POINT,
        name: Some(name),
        class_id: POINT_CLASS_ID,
        control: None,
        lower: None,
        upper: None,
        lower_ui: None,
        upper_ui: None,
        role: MaskParamRole::Control(control),
    }
}

/// Every corpus mask parameter by `ParameterID`, as `cinemagraph`
/// `VideoFilterComponent:102` saves them; a v7 record holds the first 13.
pub(crate) const MASK_PARAMS: [MaskParamSpec; 15] = [
    tracking(1, "11", "false"),
    tracking(2, "16", "true"),
    tracking(3, "16", "true"),
    tracking(4, "16", "true"),
    tracking(5, "12", "false"),
    binary(6, "Mask Path", MaskParamRole::Path),
    MaskParamSpec {
        upper_ui: Some("300"),
        ..slider(
            7,
            Some("Mask Feather"),
            SLIDER_CLASS_ID,
            Some("8"),
            "0",
            SLIDER_RANGE_V7,
            MaskParamRole::Control(MaskControl::Feather),
        )
    },
    slider(
        8,
        Some("Mask Opacity"),
        SLIDER_CLASS_ID,
        Some("8"),
        "0",
        "100",
        MaskParamRole::Control(MaskControl::Opacity),
    ),
    MaskParamSpec {
        lower_ui: Some("-300"),
        upper_ui: Some("300"),
        ..slider(
            9,
            Some("Mask Expansion"),
            SLIDER_CLASS_ID,
            Some("8"),
            "-1000",
            SLIDER_RANGE_V7,
            MaskParamRole::Control(MaskControl::Expansion),
        )
    },
    boolean(
        10,
        None,
        Some("4"),
        Some("false"),
        Some("true"),
        MaskParamRole::Control(MaskControl::Inverted),
    ),
    constant(11, "3", "2"),
    constant(12, "4294967296", "0"),
    constant(13, "3.4028234663852886e+38", "0.5"),
    tracking(14, "16", "true"),
    tracking(15, "16", "true"),
];

/// A 26.5.1 unnamed boolean at `false`.
const fn boolean_26_5(
    id: usize,
    control: &'static str,
    upper: Option<&'static str>,
) -> MaskParamSpec {
    boolean(
        id,
        None,
        Some(control),
        None,
        upper,
        MaskParamRole::Control(MaskControl::Default("false")),
    )
}

/// A 26.5.1 slider at its default.
const fn default_26_5(
    id: usize,
    name: &'static str,
    class_id: &'static str,
    lower: &'static str,
    upper: &'static str,
    value: &'static str,
) -> MaskParamSpec {
    slider(
        id,
        Some(name),
        class_id,
        None,
        lower,
        upper,
        MaskParamRole::Control(MaskControl::Default(value)),
    )
}

/// The first tracker value (`ParameterID` 6) of every fixture mask.
const TRACKER_26_5: &str = "AQAAAAAAgD8AAAAAAAAAAAAAAAAAAIA/AAAAAAAAAAAAAAAAAACAPwAAgD8AAAAAAAAAAAAAAAAAAIA/AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAAAAAAIA/AACAPwAAAAA=";
/// The second tracker value (`ParameterID` 24) of every fixture mask: a
/// 100-byte table whose one string, its 36 bytes at [`TRACKER_STATE_UUID`],
/// is the canonical UUID of the tracker it names. Every 26.5.1 fixture mask
/// names `45054be8-1889-424f-97eb-f7f502ddb198`; the observed 26.3 mask
/// holds the same bytes naming another tracker, and no other record of either
/// project names one.
const TRACKER_STATE_26_5: &str = "DAAAAAgADAAEAAgACAAAABgAAABMAAAAEAAMAAAAAAAAAAcAAAAIABAAAAAAAAAABAAAACQAAAA0NTA1NGJlOC0xODg5LTQyNGYtOTdlYi1mN2Y1MDJkZGIxOTgAAAAAAAAAAA==";
/// The bytes of the tracker UUID in [`TRACKER_STATE_26_5`].
const TRACKER_STATE_UUID: std::ops::Range<usize> = 56..92;

/// Whether `payload` is the saved tracker state `expected` (base64) naming
/// any tracker: equal to it but for a canonical lowercase UUID at
/// [`TRACKER_STATE_UUID`]. The UUID identifies the tracker and is no mask
/// control; any other difference, such as tracking data, is not converted.
pub(crate) fn is_saved_tracker_state(payload: &[u8], expected: &str) -> bool {
    let Ok(expected) = STANDARD.decode(expected) else {
        return false;
    };
    let uuid = TRACKER_STATE_UUID;
    payload.len() == expected.len()
        && payload[..uuid.start] == expected[..uuid.start]
        && payload[uuid.end..] == expected[uuid.end..]
        && std::str::from_utf8(&payload[uuid]).is_ok_and(|text| {
            uuid::Uuid::parse_str(text).is_ok_and(|parsed| parsed.hyphenated().to_string() == text)
        })
}
/// The User Interactions value (`ParameterID` 23) of every fixture mask.
const USER_INTERACTIONS_26_5: &str = "EAAAAAAACgAMAAAABAAIAAoAAAAIAAAADAAAAAAAAAAAAAAAAAAAAA==";

/// The 26.5.1 Type control. The human-authored Object Mask fixture stores 4
/// here; static vector masks and the intrinsic selection shell store 0.
pub(crate) const MASK_TYPE_26_5: MaskParamSpec = MaskParamSpec {
    control: Some("10"),
    ..default_26_5(22, "Type", POPUP_CLASS_ID, "0", "4", "0")
};

/// Saved Object Mask selection/propagation, not automatic person segmentation.
pub(crate) const OBJECT_MASK_TYPE: f64 = 4.0;

/// Every 26.5.1 mask parameter in the saved `Params` order, as fixture mask A
/// (`VideoFilterComponent:157`) saves them: the observed defaults of every
/// control that does not convert, the outline's centre in Position (9) and
/// Anchor Point (10), the Path (7), Feather (14), Opacity (15), Expansion (16)
/// and Inverted (17).
pub(crate) const MASK_PARAMS_26_5: [MaskParamSpec; 35] = [
    boolean_26_5(1, "11", Some("false")),
    boolean_26_5(2, "16", None),
    boolean_26_5(3, "16", None),
    boolean_26_5(4, "16", None),
    boolean_26_5(19, "16", None),
    boolean_26_5(20, "16", None),
    boolean_26_5(5, "12", Some("false")),
    binary(6, "Tracker", MaskParamRole::Binary(TRACKER_26_5)),
    binary(
        24,
        "Tracker",
        MaskParamRole::TrackerState(TRACKER_STATE_26_5),
    ),
    binary(7, "Path", MaskParamRole::Path),
    boolean(
        8,
        Some("Transform"),
        Some("11"),
        None,
        Some("false"),
        MaskParamRole::Control(MaskControl::Default("false")),
    ),
    point(9, "Position", MaskControl::Centre),
    point(11, "Scale", MaskControl::Default("1:1")),
    MaskParamSpec {
        lower_ui: Some("0"),
        upper_ui: Some("200"),
        ..default_26_5(26, "Scale Height", SLIDER_CLASS_ID, "-5000", "5000", "100")
    },
    MaskParamSpec {
        lower_ui: Some("0"),
        upper_ui: Some("200"),
        ..default_26_5(27, "Scale Width", SLIDER_CLASS_ID, "-5000", "5000", "100")
    },
    boolean(
        28,
        Some("Uniform Scale"),
        None,
        None,
        None,
        MaskParamRole::Control(MaskControl::Default("false")),
    ),
    MaskParamSpec {
        control: Some("3"),
        ..default_26_5(12, "Rotation", ROTATION_CLASS_ID, "-32768", "32767", "0")
    },
    point(10, "Anchor Point", MaskControl::Centre),
    boolean_26_5(13, "12", Some("false")),
    boolean(
        30,
        Some("Sharpness"),
        Some("11"),
        None,
        Some("false"),
        MaskParamRole::Control(MaskControl::Default("false")),
    ),
    default_26_5(31, "Sharpness Method", POPUP_CLASS_ID, "0", "1", "0"),
    default_26_5(32, "Contrast", SLIDER_CLASS_ID, "0", "100", "0"),
    default_26_5(33, "Midpoint", SLIDER_CLASS_ID, "0", "100", "50"),
    default_26_5(34, "Input Black", SLIDER_CLASS_ID, "0", "255", "0"),
    default_26_5(35, "Input White", SLIDER_CLASS_ID, "0", "255", "255"),
    default_26_5(
        36,
        "Gamma",
        SLIDER_CLASS_ID,
        "0.10000000149011612",
        "4",
        "1",
    ),
    boolean_26_5(37, "12", Some("false")),
    MaskParamSpec {
        upper_ui: Some("300"),
        ..slider(
            14,
            Some("Feather"),
            SLIDER_CLASS_ID,
            None,
            "0",
            SLIDER_RANGE_WIDE,
            MaskParamRole::Control(MaskControl::Feather),
        )
    },
    slider(
        15,
        Some("Opacity"),
        SLIDER_CLASS_ID,
        None,
        "0",
        "100",
        MaskParamRole::Control(MaskControl::Opacity),
    ),
    MaskParamSpec {
        lower_ui: Some("-300"),
        upper_ui: Some("300"),
        ..slider(
            16,
            Some("Expansion"),
            SLIDER_CLASS_ID,
            None,
            "-5000",
            SLIDER_RANGE_WIDE,
            MaskParamRole::Control(MaskControl::Expansion),
        )
    },
    boolean(
        17,
        None,
        None,
        None,
        None,
        MaskParamRole::Control(MaskControl::Inverted),
    ),
    slider(
        25,
        None,
        SLIDER_CLASS_ID,
        None,
        "0",
        "192",
        MaskParamRole::Control(MaskControl::Default("32")),
    ),
    default_26_5(21, "Blend Mode", POPUP_CLASS_ID, "0", "2", "0"),
    MASK_TYPE_26_5,
    binary(
        23,
        "User Interactions",
        MaskParamRole::Binary(USER_INTERACTIONS_26_5),
    ),
];

/// Every 26.3 mask parameter in the saved `Params` order: the 26.5.1
/// parameters without the sharpness and levels controls that 26.5.1 added at
/// their defaults (`ParameterID` 30 to 37). Each kept record has the 26.5.1
/// name, class, control type and bounds (both masks of the observed 26.3
/// project).
pub(crate) const MASK_PARAMS_26_3: [MaskParamSpec; 27] = without_sharpness(MASK_PARAMS_26_5);

const fn without_sharpness(params: [MaskParamSpec; 35]) -> [MaskParamSpec; 27] {
    let mut kept = [params[0]; 27];
    let (mut from, mut to) = (0, 0);
    while from < params.len() {
        if !matches!(params[from].id, 30..=37) {
            kept[to] = params[from];
            to += 1;
        }
        from += 1;
    }
    assert!(
        to == kept.len(),
        "26.5.1 holds eight sharpness and levels controls"
    );
    kept
}

/// The `PremiereFilterPrivateData` that the writer stores on a mask: the
/// 88-byte `kcin` body of `vhs_slideshow` `VideoFilterComponent:2667`, whose
/// tracking state is all zero; other saves hold nonzero, undecoded state
/// (Premiere 26.5.1 re-saves it as 112 bytes).
pub(crate) const MASK_PRIVATE_DATA: &str = "a2NpbgEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIDl+f//////AQAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

/// Read the little-endian word at `at`.
fn word(payload: &[u8], at: usize) -> crate::error::Result<u32> {
    payload
        .get(at..at + 4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")))
        .ok_or_else(|| unsupported("truncated Mask Path payload"))
}

/// Decode `count` vertex bodies of `stride` bytes each from `vertices`, each
/// followed by `stride - VERTEX_BYTES` trailer bytes that `check_trailer`
/// accepts.
fn decode_vertices(
    vertices: &[u8],
    stride: usize,
    check_trailer: impl Fn(&[u8]) -> crate::error::Result<()>,
) -> crate::error::Result<PrShapePath> {
    let vertices = vertices
        .chunks_exact(stride)
        .map(|vertex| {
            check_trailer(&vertex[VERTEX_BYTES..])?;
            decode_vertex(&vertex[..VERTEX_BYTES])
        })
        .collect::<crate::error::Result<Vec<_>>>()?;
    Ok(PrShapePath {
        vertices,
        closed: true,
    })
}

/// Decode one corpus Mask Path value (`2cin`) into a closed outline in
/// unit-frame fractions.
///
/// # Errors
/// Rejects another magic or version, a `z` other than 0 or 1, fewer than
/// three vertices, a size that does not match the vertex count, a vertex
/// trailer other than 1, and what [`decode_vertex`] rejects.
pub(crate) fn decode_mask_path(payload: &[u8]) -> crate::error::Result<PrShapePath> {
    ensure!(
        payload.get(..4) == Some(MASK_PATH_MAGIC),
        "unknown Mask Path magic"
    );
    let version = word(payload, 4)?;
    ensure!(
        version == MASK_PATH_VERSION,
        "Mask Path version {version} is unsupported"
    );
    let z = word(payload, 8)?;
    ensure!(matches!(z, 0 | 1), "unknown Mask Path flag {z}");
    let count = usize::try_from(word(payload, 12)?)
        .map_err(|_| unsupported("Mask Path vertex count overflows"))?;
    ensure!(count >= 3, "a Mask Path needs at least three vertices");
    let vertex_bytes = count
        .checked_mul(MASK_VERTEX_BYTES)
        .filter(|bytes| MASK_PATH_HEADER_BYTES + bytes == payload.len())
        .ok_or_else(|| {
            unsupported(format!(
                "Mask Path size does not match its {count} vertices"
            ))
        })?;
    decode_vertices(
        &payload[MASK_PATH_HEADER_BYTES..MASK_PATH_HEADER_BYTES + vertex_bytes],
        MASK_VERTEX_BYTES,
        |trailer| {
            let trailer = u32::from_le_bytes(trailer.try_into().expect("four trailing bytes"));
            ensure!(
                trailer == MASK_VERTEX_TRAILER,
                "unknown Mask Path vertex trailer {trailer}"
            );
            Ok(())
        },
    )
}

/// Decode one Premiere 26.5.1 Path value into a closed outline in unit-frame
/// fractions.
///
/// # Errors
/// Rejects an envelope whose words, byte, length or final byte differ from
/// the five fixture masks', fewer than three vertices, and what
/// [`decode_vertex`] rejects.
pub(crate) fn decode_mask_path_26_5(payload: &[u8]) -> crate::error::Result<PrShapePath> {
    ensure!(
        [word(payload, 0)?, word(payload, 4)?] == MASK_PATH_26_5_HEADER
            && payload.get(8) == Some(&MASK_PATH_26_5_BYTE)
            && word(payload, 13)? == MASK_PATH_26_5_WORD,
        "unknown 26.5 Mask Path header"
    );
    let length = usize::try_from(word(payload, 9)?)
        .map_err(|_| unsupported("26.5 Mask Path length overflows"))?;
    ensure!(
        length + MASK_PATH_26_5_LENGTH_OFFSET == payload.len(),
        "26.5 Mask Path length {length} does not match its payload"
    );
    let count = usize::try_from(word(payload, 17)?)
        .map_err(|_| unsupported("Mask Path vertex count overflows"))?;
    ensure!(count >= 3, "a Mask Path needs at least three vertices");
    let vertex_bytes = count
        .checked_mul(VERTEX_BYTES)
        .filter(|bytes| MASK_PATH_26_5_HEADER_BYTES + bytes + 1 == payload.len())
        .ok_or_else(|| {
            unsupported(format!(
                "Mask Path size does not match its {count} vertices"
            ))
        })?;
    let trailer = payload[MASK_PATH_26_5_HEADER_BYTES + vertex_bytes];
    ensure!(
        trailer == MASK_PATH_26_5_TRAILER,
        "unknown 26.5 Mask Path trailer {trailer}"
    );
    decode_vertices(
        &payload[MASK_PATH_26_5_HEADER_BYTES..MASK_PATH_26_5_HEADER_BYTES + vertex_bytes],
        VERTEX_BYTES,
        |_| Ok(()),
    )
}

/// Encode one closed outline as a Mask Path value with `z` 0.
pub(crate) fn encode_mask_path(path: &PrShapePath) -> crate::format::Result<Vec<u8>> {
    ensure_valid!(
        path.closed && path.vertices.len() >= 3,
        "a Mask Path must be a closed outline of at least three vertices"
    );
    let count = u32::try_from(path.vertices.len())
        .map_err(|_| crate::format::invalid("Mask Path vertex count overflows"))?;
    let mut payload =
        Vec::with_capacity(MASK_PATH_HEADER_BYTES + MASK_VERTEX_BYTES * path.vertices.len());
    payload.extend_from_slice(MASK_PATH_MAGIC);
    payload.extend_from_slice(&MASK_PATH_VERSION.to_le_bytes());
    payload.extend_from_slice(&0_u32.to_le_bytes());
    payload.extend_from_slice(&count.to_le_bytes());
    for vertex in &path.vertices {
        encode_vertex(vertex, &mut payload);
        payload.extend_from_slice(&MASK_VERTEX_TRAILER.to_le_bytes());
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::{
        decode_mask_path, decode_mask_path_26_5, encode_mask_path, is_saved_tracker_state,
        MaskControl, MaskParamRole, PrMask, PrMaskPathKey, MASK_PARAMS, MASK_PARAMS_26_3,
        MASK_PARAMS_26_5, TRACKER_STATE_26_5, TRACKER_STATE_UUID,
    };
    use crate::schema::text::{PrPathVertex, PrShapePath};
    use base64::{engine::general_purpose::STANDARD, Engine};

    /// Fixture mask D (`ArbVideoComponentParam:299`, Premiere 26.5.1): the pen
    /// path of five vertices, the first smooth, authored as a
    /// `2cin` value and Premiere re-saved.
    const FIXTURE_PEN_PATH_26_5: &str = "AgAAAAEAAABWlQAAAAIAAAAFAAAAAQAAAM3MTD5mZmY/7FE4PtejcD+uR2E+9ihcPwAAAAAzM7M+mpkZPzMzsz6amRk/MzOzPpqZGT8AAAAAMzMzP83MzD4zMzM/zczMPjMzMz/NzMw+AAAAAGZmZj9cj8I+ZmZmP1yPwj5mZmY/XI/CPgAAAABmZmY/ZmZmP2ZmZj9mZmY/ZmZmP2ZmZj8B";
    /// The same authored outline (`D-written-param6`).
    const FIXTURE_PEN_PATH_WRITTEN: &str = "MmNpbgIAAAAAAAAABQAAAAEAAADNzEw+ZmZmP+xROD7Xo3A/rkdhPvYoXD8BAAAAAAAAADMzsz6amRk/MzOzPpqZGT8zM7M+mpkZPwEAAAAAAAAAMzMzP83MzD4zMzM/zczMPjMzMz/NzMw+AQAAAAAAAABmZmY/XI/CPmZmZj9cj8I+ZmZmP1yPwj4BAAAAAAAAAGZmZj9mZmY/ZmZmP2ZmZj9mZmY/ZmZmPwEAAAA=";

    /// `cinemagraph` `ArbVideoComponentParam:110`: the pen path of its
    /// Opacity mask, five vertices, the first smooth.
    const CINEMAGRAPH_PATH: &str = "MmNpbgIAAAAAAAAABQAAAAEAAADNzEw+LtiCP0kXSz7zkYE/UYJOPmoehD8BAAAAAAAAAFVVtT6Y0B4/VVW1PpjQHj9VVbU+mNAePwEAAAAAAAAAVVU3P/qkzz5VVTc/+qTPPlVVNz/6pM8+AQAAAAAAAABVVWc/XXnAPlVVZz9decA+VVVnP115wD4BAAAAAAAAADMzZT8ofYI/MzNlPyh9gj8zM2U/KH2CPwEAAAA=";

    fn corner(x: f32, y: f32) -> PrPathVertex {
        PrPathVertex {
            smooth: false,
            point: [x, y],
            in_tangent: [x, y],
            out_tangent: [x, y],
        }
    }

    #[test]
    fn corpus_pen_path_decodes_and_reencodes_byte_identically() {
        let payload = STANDARD.decode(CINEMAGRAPH_PATH).unwrap();
        let path = decode_mask_path(&payload).unwrap();
        assert!(path.closed);
        assert_eq!(path.vertices.len(), 5);
        assert_eq!(
            path.vertices
                .iter()
                .map(|vertex| vertex.smooth)
                .collect::<Vec<_>>(),
            [true, false, false, false, false]
        );
        assert_eq!(path.vertices[0].point, [0.2, 1.0222223]);
        assert_eq!(path.vertices[0].in_tangent, [0.19833101, 1.0122665]);
        assert_eq!(path.vertices[3].point, [0.9036458, 0.37592593]);
        assert_eq!(encode_mask_path(&path).unwrap(), payload);
    }

    #[test]
    fn mask_paths_outside_the_observed_form_fail_closed() {
        let payload = STANDARD.decode(CINEMAGRAPH_PATH).unwrap();
        let with = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut payload = payload.clone();
            edit(&mut payload);
            payload
        };
        for (case, payload, reason) in [
            (
                "magic",
                with(&|payload| payload[0] = b'1'),
                "unknown Mask Path magic",
            ),
            (
                "version",
                with(&|payload| payload[4] = 3),
                "Mask Path version 3 is unsupported",
            ),
            (
                "flag",
                with(&|payload| payload[8] = 2),
                "unknown Mask Path flag 2",
            ),
            (
                "count",
                with(&|payload| payload[12] = 4),
                "Mask Path size does not match its 4 vertices",
            ),
            (
                "two vertices",
                with(&|payload| {
                    payload[12] = 2;
                    payload.truncate(16 + 2 * 32);
                }),
                "a Mask Path needs at least three vertices",
            ),
            (
                "trailer",
                with(&|payload| payload[16 + 28] = 0),
                "unknown Mask Path vertex trailer 0",
            ),
            (
                "vertex flag",
                with(&|payload| payload[16 + 32] = 2),
                "unknown Path vertex flag 2",
            ),
            (
                "truncated",
                with(&|payload| payload.truncate(10)),
                "truncated Mask Path payload",
            ),
        ] {
            let error = decode_mask_path(&payload).unwrap_err().to_string();
            assert!(error.contains(reason), "{case}: {error}");
        }
    }

    #[test]
    fn premiere_26_5_path_holds_the_written_vertices_in_its_own_envelope() {
        let saved = STANDARD.decode(FIXTURE_PEN_PATH_26_5).unwrap();
        let written = STANDARD.decode(FIXTURE_PEN_PATH_WRITTEN).unwrap();
        let path = decode_mask_path_26_5(&saved).unwrap();
        assert_eq!(path, decode_mask_path(&written).unwrap());
        assert_eq!(path.vertices.len(), 5);
        assert_eq!(path.vertices[0].point, [0.2, 0.9]);
        assert_eq!(path.vertices[0].in_tangent, [0.18, 0.94]);
        assert_eq!(path.vertices[2].point, [0.7, 0.4]);
        let with = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut payload = saved.clone();
            edit(&mut payload);
            payload
        };
        for (case, payload, reason) in [
            (
                "first word",
                with(&|payload| payload[0] = 3),
                "unknown 26.5 Mask Path header",
            ),
            (
                "byte",
                with(&|payload| payload[8] = 0x57),
                "unknown 26.5 Mask Path header",
            ),
            (
                "length",
                with(&|payload| payload[9] = 0x94),
                "26.5 Mask Path length 148 does not match its payload",
            ),
            (
                "count",
                with(&|payload| payload[17] = 4),
                "Mask Path size does not match its 4 vertices",
            ),
            (
                "two vertices",
                with(&|payload| {
                    payload[9] = 2 * 28 + 9;
                    payload[17] = 2;
                    payload.truncate(21 + 2 * 28);
                    payload.push(1);
                }),
                "a Mask Path needs at least three vertices",
            ),
            (
                "trailer",
                with(&|payload| *payload.last_mut().unwrap() = 0),
                "unknown 26.5 Mask Path trailer 0",
            ),
            (
                "vertex flag",
                with(&|payload| payload[21 + 28] = 2),
                "unknown Path vertex flag 2",
            ),
            (
                "truncated",
                with(&|payload| payload.truncate(10)),
                "truncated Mask Path payload",
            ),
        ] {
            let error = decode_mask_path_26_5(&payload).unwrap_err().to_string();
            assert!(error.contains(reason), "{case}: {error}");
        }
    }

    #[test]
    fn mask_bounds_reject_open_or_small_paths_and_out_of_range_controls() {
        let triangle = PrShapePath {
            vertices: vec![corner(0.0, 0.0), corner(1.0, 0.0), corner(1.0, 1.0)],
            closed: true,
        };
        let valid = PrMask {
            raster: None,
            feather_keys: Vec::new(),
            expansion: 0.0,
            expansion_keys: Vec::new(),
            opacity_keys: Vec::new(),
            path: triangle.clone(),
            path_keys: Vec::new(),
            feather: 1000.0,
            opacity: 100.0,
            inverted: true,
        };
        let moved = PrShapePath {
            vertices: vec![corner(0.5, 0.0), corner(1.5, 0.0), corner(1.5, 1.0)],
            closed: true,
        };
        let keyed = |keys: [(i64, &PrShapePath); 2]| PrMask {
            raster: None,
            path: keys[0].1.clone(),
            path_keys: keys
                .iter()
                .map(|&(source_ticks, path)| PrMaskPathKey {
                    source_ticks,
                    path: path.clone(),
                })
                .collect(),
            ..valid.clone()
        };
        assert!(valid.validate().is_ok());
        assert!(PrMask {
            raster: None,
            opacity: 0.0,
            inverted: false,
            ..valid.clone()
        }
        .validate()
        .is_ok());
        assert!(keyed([(0, &triangle), (1, &moved)]).validate().is_ok());
        for (case, mask) in [
            ("keys at one time", keyed([(1, &triangle), (1, &moved)])),
            (
                "open key outline",
                keyed([
                    (0, &triangle),
                    (
                        1,
                        &PrShapePath {
                            closed: false,
                            ..moved.clone()
                        },
                    ),
                ]),
            ),
            (
                "outline other than the first key's",
                PrMask {
                    raster: None,
                    path: moved.clone(),
                    ..keyed([(0, &triangle), (1, &moved)])
                },
            ),
            (
                // Fixture clip D: Premiere renders 0.5 x (1 - coverage), FX
                // 1 - 0.5 x coverage (G3b).
                "inverted at Mask Opacity 50",
                PrMask {
                    raster: None,
                    opacity: 50.0,
                    ..valid.clone()
                },
            ),
            (
                "open path",
                PrMask {
                    raster: None,
                    path: PrShapePath {
                        closed: false,
                        ..triangle.clone()
                    },
                    ..valid.clone()
                },
            ),
            (
                "two vertices",
                PrMask {
                    raster: None,
                    path: PrShapePath {
                        vertices: triangle.vertices[..2].to_vec(),
                        closed: true,
                    },
                    ..valid.clone()
                },
            ),
            (
                "feather above the written bound",
                PrMask {
                    raster: None,
                    feather: 1000.5,
                    ..valid.clone()
                },
            ),
            (
                "negative feather",
                PrMask {
                    raster: None,
                    feather: -1.0,
                    ..valid.clone()
                },
            ),
            (
                "opacity above 100",
                PrMask {
                    raster: None,
                    opacity: 100.5,
                    ..valid.clone()
                },
            ),
        ] {
            assert!(mask.validate().is_err(), "{case}");
        }
        assert!(encode_mask_path(&PrShapePath {
            closed: false,
            ..triangle
        })
        .is_err());
    }

    #[test]
    fn written_layout_names_every_parameter_id_in_order() {
        for (index, spec) in MASK_PARAMS.iter().enumerate() {
            assert_eq!(spec.id, index + 1);
        }
    }

    #[test]
    fn premiere_26_5_layout_names_the_saved_parameter_ids_and_converted_controls() {
        // Fixture mask A's `Params` order (saved-param-table.md).
        assert_eq!(
            MASK_PARAMS_26_5
                .iter()
                .map(|spec| spec.id)
                .collect::<Vec<_>>(),
            [
                1, 2, 3, 4, 19, 20, 5, 6, 24, 7, 8, 9, 11, 26, 27, 28, 12, 10, 13, 30, 31, 32, 33,
                34, 35, 36, 37, 14, 15, 16, 17, 25, 21, 22, 23
            ]
        );
        let role = |id: usize| {
            MASK_PARAMS_26_5
                .iter()
                .find(|spec| spec.id == id)
                .unwrap()
                .role
        };
        assert_eq!(role(7), MaskParamRole::Path);
        assert_eq!(role(14), MaskParamRole::Control(MaskControl::Feather));
        assert_eq!(role(15), MaskParamRole::Control(MaskControl::Opacity));
        assert_eq!(role(16), MaskParamRole::Control(MaskControl::Expansion));
        assert_eq!(role(17), MaskParamRole::Control(MaskControl::Inverted));
        assert_eq!(role(9), MaskParamRole::Control(MaskControl::Centre));
        assert_eq!(role(10), MaskParamRole::Control(MaskControl::Centre));
        // Every other parameter converts only at one observed value, the
        // tracker state up to the tracker that it names.
        assert_eq!(
            MASK_PARAMS_26_5
                .iter()
                .filter(|spec| matches!(
                    spec.role,
                    MaskParamRole::Binary(_)
                        | MaskParamRole::TrackerState(_)
                        | MaskParamRole::Control(MaskControl::Default(_))
                ))
                .count(),
            35 - 7
        );
        assert!(super::at_default("100.", "100") && super::at_default("1:1", "1:1"));
        assert!(!super::at_default("1:1.5", "1:1") && !super::at_default("true", "false"));
    }

    #[test]
    fn premiere_26_3_layout_is_the_26_5_layout_without_sharpness_and_levels() {
        // The `Params` order of both masks of the observed 26.3 project.
        assert_eq!(
            MASK_PARAMS_26_3
                .iter()
                .map(|spec| spec.id)
                .collect::<Vec<_>>(),
            [
                1, 2, 3, 4, 19, 20, 5, 6, 24, 7, 8, 9, 11, 26, 27, 28, 12, 10, 13, 14, 15, 16, 17,
                25, 21, 22, 23
            ]
        );
        for spec in &MASK_PARAMS_26_3 {
            let saved = MASK_PARAMS_26_5
                .iter()
                .find(|saved| saved.id == spec.id)
                .unwrap();
            assert_eq!(
                (spec.role, spec.name, spec.class_id, spec.lower, spec.upper),
                (
                    saved.role,
                    saved.name,
                    saved.class_id,
                    saved.lower,
                    saved.upper
                )
            );
        }
        // The parameter count tells the two forms of one record generation
        // apart; another count meets the 26.5.1 layout check.
        let form = |count| {
            super::MaskForm::of(Some("AE.ADBE AEMask2"), Some("9"), Some("7"), count)
                .unwrap()
                .params()
                .len()
        };
        assert_eq!([form(27), form(35), form(28)], [27, 35, 35]);
        assert!(super::MaskForm::of(Some("AE.ADBE AEMask2"), Some("8"), Some("6"), 27).is_none());
    }

    #[test]
    fn a_saved_tracker_state_may_name_any_tracker_and_holds_nothing_else() {
        let saved = STANDARD.decode(TRACKER_STATE_26_5).unwrap();
        assert_eq!(
            &saved[TRACKER_STATE_UUID],
            b"45054be8-1889-424f-97eb-f7f502ddb198"
        );
        let naming = |uuid: &[u8]| {
            let mut state = saved.clone();
            state[TRACKER_STATE_UUID].copy_from_slice(uuid);
            state
        };
        assert!(is_saved_tracker_state(&saved, TRACKER_STATE_26_5));
        assert!(is_saved_tracker_state(
            &naming(b"0f8e3c55-2b1d-4a6e-9c7f-3d2a1b0c9e8d"),
            TRACKER_STATE_26_5
        ));
        let mut flagged = saved.clone();
        flagged[32] ^= 1;
        let mut longer = saved.clone();
        longer.push(0);
        for (case, state) in [
            ("capitals", naming(b"0F8E3C55-2B1D-4A6E-9C7F-3D2A1B0C9E8D")),
            ("no UUID", naming(b"0f8e3c55-2b1d-4a6e-9c7f-3d2a1b0c9e8z")),
            (
                "no hyphens",
                naming(b"0f8e3c552b1d4a6e9c7f3d2a1b0c9e8d0000"),
            ),
            ("other data", flagged),
            ("one byte longer", longer),
            ("truncated", saved[..91].to_vec()),
            ("empty", Vec::new()),
        ] {
            assert!(
                !is_saved_tracker_state(&state, TRACKER_STATE_26_5),
                "{case}"
            );
        }
    }
}
