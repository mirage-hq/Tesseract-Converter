//! Standard (non-intrinsic) clip effects: the typed stack model, the native
//! identity table shared by the reader and writer, and the effects that omit
//! their whole occurrence.
//!
//! A clip's `VideoComponentChain` gives any materialized intrinsic Motion/Opacity
//! components lower `Index`es than its standard effects, and Premiere renders
//! the chain in descending component `Index`. The stack order, the order in
//! which the effects apply, is therefore the reverse of the chain's `Index`
//! order ([`chain_render_order`]). Intrinsic components keep their own owner
//! (`reader::animation`); this model holds convertible standard effects.

use super::{
    records::{self, XmlRecordDefinition},
    seconds, PrKeyframeEasing, PrPointKeyframe, PrScalarKeyframe, TICKS,
};
use crate::format::{ensure_valid, invalid, Result};
use std::{cmp::Ordering, collections::BTreeSet, ops::RangeInclusive};

/// Maps a clip's components between their ascending `VideoComponentChain`
/// `Index` order and the order in which they apply. The mapping is its own
/// inverse: the reader applies it to a chain in `Index` order, and the writer
/// to the FX order, the order they apply, to get the chain's `Index` order.
///
/// Premiere renders a chain in descending `Index`: the component at Index 0
/// renders last. The native reference
/// (`premiere_isolated_stage_order_26_5`). AME rendered a Crop or Linear Wipe
/// at Index 0 and a Gaussian Blur at Index 1 with a sharp edge, so the blur
/// applied first, and the swapped layout with a soft edge. With the `ID`s
/// swapped, the render matched that one at all 12 compared frames
/// (mean difference 0.000), so `ID` does not order it. FX applies a layer's
/// effects in list order, so each order is the reverse of the other.
pub(crate) fn chain_render_order<I>(components: I) -> std::iter::Rev<I::IntoIter>
where
    I: IntoIterator,
    I::IntoIter: DoubleEndedIterator,
{
    components.into_iter().rev()
}

/// One convertible standard clip effect, in stack order (the order in which
/// Premiere applies it, which FX keeps).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrEffect {
    /// The inverse of Premiere's `Bypass`. A bypassed effect keeps its stack
    /// position and values, like a disabled FX `EffectRecord`.
    pub(crate) enabled: bool,
    /// Spatial scope of this effect, not the clip's intrinsic Opacity mask.
    pub(crate) mask: Option<super::PrMask>,
    pub(crate) params: PrEffectParams,
    /// Keyed parameters in native `Params` order. The static value of a keyed
    /// parameter in `params` is its first key's value: before a later first
    /// key, AME renders that key's value and ignores `StartKeyframe`.
    pub(crate) animations: Vec<PrEffectParamAnimation>,
}

/// The keys of one animated effect parameter, on the source clock like the
/// keys of intrinsic Motion.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrEffectParamAnimation {
    /// The keyed parameter, which has an [`EffectParamBinding`].
    pub(crate) param: &'static EffectParamSpec,
    pub(crate) keys: PrEffectParamKeys,
}

/// Keys of the kind that their parameter's [`EffectParamBinding`] names.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PrEffectParamKeys {
    Scalar(Vec<PrScalarKeyframe>),
    Point(Vec<PrPointKeyframe>),
    Colour(Vec<PrColourKeyframe>),
}

impl PrEffectParamKeys {
    pub(crate) fn scalar(&self) -> Option<&[PrScalarKeyframe]> {
        match self {
            Self::Scalar(keys) => Some(keys),
            Self::Point(_) | Self::Colour(_) => None,
        }
    }

    pub(crate) fn point(&self) -> Option<&[PrPointKeyframe]> {
        match self {
            Self::Point(keys) => Some(keys),
            Self::Scalar(_) | Self::Colour(_) => None,
        }
    }

    pub(crate) fn colour(&self) -> Option<&[PrColourKeyframe]> {
        match self {
            Self::Colour(keys) => Some(keys),
            Self::Scalar(_) | Self::Point(_) => None,
        }
    }
}

/// One key of a colour parameter. Premiere interpolates a Linear segment per
/// channel; a Bezier segment between colours is
/// unverified and rejected, so `easing` is Linear or Hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrColourKeyframe {
    pub(crate) source_ticks: i64,
    pub(crate) value: PrColour,
    pub(crate) easing: PrKeyframeEasing,
}

/// An opaque 8-bit RGB colour of a colour parameter.
///
/// Premiere stores a colour as a u64 of four 16-bit channels, alpha, red,
/// green and blue from the high end, with the 8-bit value in each channel's
/// high byte: every one of the 1,050 corpus channel values has a zero low
/// byte. Alpha is 0 on Tint's defaults and 0xff00 on every
/// authored colour, and the default white with alpha 0 renders as white,
/// so import ignores it and export writes 0xff00.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrColour {
    /// Red, green and blue.
    pub(crate) rgb: [u8; 3],
}

/// Bits of one 16-bit native channel.
const COLOUR_CHANNEL_BITS: u32 = 16;
/// Native alpha that export writes: opaque, in the channel's high byte.
const COLOUR_OPAQUE_ALPHA: u64 = 0xff00;

impl PrColour {
    pub(crate) const BLACK: Self = Self { rgb: [0, 0, 0] };
    pub(crate) const WHITE: Self = Self {
        rgb: [255, 255, 255],
    };

    /// The colour of a native ARGB value, or why it has no 8-bit colour.
    pub(crate) fn from_native(value: u64) -> std::result::Result<Self, String> {
        let channel = |index: u32| (value >> (COLOUR_CHANNEL_BITS * index)) & 0xffff;
        let mut rgb = [0; 3];
        // Red is the second channel from the high end, blue the last.
        for (slot, index) in rgb.iter_mut().zip([2, 1, 0]) {
            let native = channel(index);
            if native & 0xff != 0 {
                return Err(format!(
                    "colour {value:#018x} has a nonzero low byte in a channel; only 8-bit colours convert"
                ));
            }
            // Fits: the high byte of a 16-bit channel.
            *slot = (native >> 8) as u8;
        }
        Ok(Self { rgb })
    }

    /// The native ARGB value that export writes.
    pub(crate) fn native(self) -> u64 {
        let [red, green, blue] = self.rgb.map(|channel| u64::from(channel) << 8);
        (COLOUR_OPAQUE_ALPHA << (3 * COLOUR_CHANNEL_BITS))
            | (red << (2 * COLOUR_CHANNEL_BITS))
            | (green << COLOUR_CHANNEL_BITS)
            | blue
    }

    /// The FX channel values, each from 0 to 1.
    pub(crate) fn fx(self) -> [f64; 3] {
        self.rgb.map(|channel| f64::from(channel) / 255.0)
    }

    /// The colour of FX channel values from 0 to 1, rounded to 8 bits (at most
    /// 1/510 per channel), or `None` when a channel is outside that range or
    /// not finite: never clamped.
    pub(crate) fn from_fx(rgb: [f64; 3]) -> Option<Self> {
        let mut colour = Self::BLACK;
        for (slot, value) in colour.rgb.iter_mut().zip(rgb) {
            if !(0.0..=1.0).contains(&value) {
                return None;
            }
            // Fits: 0 to 255 after rounding.
            *slot = (value * 255.0).round() as u8;
        }
        Some(colour)
    }
}

/// Typed static parameter values. The variant selects the native identity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PrEffectParams {
    GaussianBlur(PrGaussianBlur),
    FilmImpactBlur(PrFilmImpactBlur),
    CornerPin(PrCornerPin),
    DirectionalBlur(PrDirectionalBlur),
    FilmImpactDirectionalBlur(PrFilmImpactDirectionalBlur),
    Levels(PrLevels),
    BrightnessContrast(PrBrightnessContrast),
    /// Import-only normalized Offset center; edited MotionTile uses linked export.
    Offset([f64; 2]),
    /// Import-only selected Lumetri controls. Never serialize as native Lumetri;
    /// edited FX exports through the linked-AEP route.
    LumetriExposure(f64),
    LumetriTemperature(f64),
    LumetriTint(f64),
    LumetriSaturation(f64),
    LumetriVignette([f64; 3]),
    Invert(PrInvert),
    FindEdges(PrFindEdges),
    Tint(PrTint),
    /// `AE.ADBE Black & White`, which has no parameters: Premiere 26.5.1's
    /// grayscale, the same render as a default Tint.
    BlackWhite,
    Ramp(PrRamp),
    Mosaic(PrMosaic),
    Replicate(PrReplicate),
    Posterize(PrPosterize),
    Sharpen(PrSharpen),
    AlphaGlow {
        size: f64,
        brightness: f64,
        color: PrColour,
    },
    LegacyLuma {
        threshold: f64,
        cutoff: f64,
    },
    LensDistortion(f64),
    /// Import-only modern controls; canonical export is Legacy Noise via Grain.
    ModernNoise {
        amount: f64,
        seed: f64,
    },
    /// Legacy strength; color/clipping modes use a diagnosed Grain surrogate.
    Noise {
        amount: f64,
    },

    /// Initial native rate; FX cannot animate its Posterize Time clock.
    PosterizeTime {
        frame_rate: f64,
    },
    Transform(PrTransform),
    /// Geometry2 on a flagged adjustment: transforms the composed lower picture.
    AdjustmentGeometry2(PrTransform),
}

/// Native Find Edges controls; blend keys are deliberately not mapped to FX.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrFindEdges {
    pub(crate) invert: bool,
    pub(crate) blend: f64,
    pub(crate) blend_animated: bool,
}

impl PrFindEdges {
    pub(crate) const APPROXIMATION: &str = "FX uses a grayscale Sobel edge detector, not Adobe's colored edge detector; edge/color and alpha fidelity are unmeasured";
    pub(crate) const BLEND_OMISSION: &str = "Blend With Original and its keys were not imported: FX Find Edges has no original-image blend control; full-strength edges were kept";
}

/// Static `AE.ADBE Geometry` ("Transform") values, which the video of a
/// mask-less stage group carries as its FX transform.
/// Premiere applies the effect in the clip's source frame before
/// Motion (the native Transform displacement follows the
/// rotated Motion axes) and clips nothing to the source frame (T2: 476,960
/// content pixels outside the Motion-only rectangle), so the effect is the
/// staged video's transform under the group's Motion: output = Position +
/// M · (source − Anchor Point) with both points normalized to the source
/// frame (T7: clip B's anchor 1440:540 lands on 960:540 within 0.75 px), a
/// clockwise Rotation in FX's y-down frame (T4: +30 renders +29.998°) and
/// Scale Height driving both axes under Uniform Scale (T3: clip B renders
/// 0.49997 × 0.49995 with Scale Width 100 saved). The source frame must be
/// the canvas: E11 measured 1920 × 1080 on 1920 × 1080 only.
///
/// What converts is decided once, for the reader and the exporter, by
/// [`PrTransform::ensure_convertible`], and what converts approximately by
/// [`PrTransform::approximations`]; a bypassed effect is the reader's rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrTransform {
    /// Anchor Point, normalized to the source frame.
    pub(crate) anchor_point: [f64; 2],
    /// Position, normalized to the source frame. Ordinary physical hosts require
    /// a matching canvas; nests separately bound differing-canvas admission.
    pub(crate) position: [f64; 2],
    /// The Uniform Scale checkbox (`ParameterID` 11): Scale Height drives
    /// both axes and Scale Width is saved but not rendered (T3).
    pub(crate) uniform_scale: bool,
    /// Scale Height in percent; the y scale, and the x scale under
    /// `uniform_scale`.
    pub(crate) scale_height: f64,
    /// Scale Width in percent; the x scale without `uniform_scale`
    /// (inferred from the two named axes: E11 has no non-uniform clip).
    pub(crate) scale_width: f64,
    /// Skew in degrees, an FX `skew` of the same value (T5: clip C's
    /// `[[1.2884, −0.2889], [0.2886, 0.7112]]` is
    /// R(45°) · [[1, −tan 30°], [0, 1]] · R(−45°) within 0.00026 per
    /// coefficient, the FX shear form).
    pub(crate) skew: f64,
    /// Skew Axis in degrees. At rotation 0 and scale 100 Premiere renders
    /// Skew `s` at Skew Axis `a` as R(90° − a) · [[1, −tan s], [0, 1]] ·
    /// R(a − 90°) (y down): the export comparison measured Skew 30 at
    /// Skew Axis −30 as `[[0.7498, −0.1444], [0.4329, 1.2497]]` within
    /// 0.0003 per coefficient, and the native Skew Axis 45 sample fits
    /// this form as it fits the negated axis, which the gate ruled out. FX
    /// shears by R(−axis) · [[1, −tan skew], [0, 1]] · R(axis)
    /// (`scene::Affine::from_components_with_skew`), so under a nonzero Skew
    /// the FX `skew_axis` is Skew Axis − 90° ([`Self::fx_skew_axis`]).
    pub(crate) skew_axis: f64,
    /// Rotation in degrees, clockwise, an FX `rotation` of the same value (T4).
    pub(crate) rotation: f64,
    /// Opacity in percent, the staged video's FX opacity of the same value.
    /// Premiere blends it in linear light (T6) and FX in encoded sRGB, so
    /// another value than [`TRANSFORM_OPAQUE`] is an approximation.
    pub(crate) opacity: f64,
    /// The Use Composition's Shutter Angle checkbox (`ParameterID` 9).
    pub(crate) composition_shutter_angle: bool,
    /// Shutter Angle in degrees, rendered only while the checkbox is off
    /// ([`Self::motion_blur_shutter_angle`]).
    pub(crate) shutter_angle: f64,
    /// Sampling 1 (Bicubic) instead of 0 (Bilinear); FX resamples
    /// bilinearly.
    pub(crate) bicubic_sampling: bool,
}

impl PrTransform {
    /// Why keyed Skew or Skew Axis is not converted in either direction.
    pub(crate) const KEYED_SKEW: &'static str = "keyed Skew or Skew Axis is not converted: native measurements cover static skew only, and Motion has no skew for the export to key";

    /// Degrees between a Skew Axis and the FX `skew_axis` that renders the
    /// same shear ([`Self::skew_axis`]).
    const SKEW_AXIS_OFFSET: f64 = 90.0;

    /// Degrees of axis after which the shear repeats: R(θ + 180°) = −R(θ) on
    /// both sides of the shear matrix, in Premiere's form and in FX's.
    const SKEW_AXIS_PERIOD: f64 = 180.0;

    /// The FX `skew_axis` of this Transform: Skew Axis − 90° under a nonzero
    /// Skew ([`Self::skew_axis`]), else 0. Skew 0 shears nothing at any axis,
    /// so import keeps no inert axis and reports a nonzero one
    /// ([`Self::unretained_skew_axis`]).
    pub(crate) fn fx_skew_axis(&self) -> f64 {
        if self.skew == 0.0 {
            0.0
        } else {
            self.skew_axis - Self::SKEW_AXIS_OFFSET
        }
    }

    /// Why import drops this Transform's Skew Axis: a nonzero one under
    /// Skew 0 ([`Self::fx_skew_axis`]); `None` otherwise.
    pub(crate) fn unretained_skew_axis(&self) -> Option<String> {
        (self.skew == 0.0 && self.skew_axis != 0.0).then(|| {
            format!(
                "Skew Axis {} without Skew is not retained (no render effect)",
                self.skew_axis
            )
        })
    }

    /// The Skew Axis that export writes for the FX `skew` and `fx_skew_axis`,
    /// the inverse of [`Self::fx_skew_axis`]: 90° more under a nonzero skew,
    /// else 0. Whole degrees round-trip exactly, fractions to within an ulp.
    /// An axis beyond the native bounds writes its equivalent from 0° to
    /// 180° ([`Self::SKEW_AXIS_PERIOD`]).
    pub(crate) fn native_skew_axis(skew: f64, fx_skew_axis: f64) -> f64 {
        if skew == 0.0 {
            return 0.0;
        }
        let axis = fx_skew_axis + Self::SKEW_AXIS_OFFSET;
        if TRANSFORM_SKEW_AXIS
            .value_range()
            .is_some_and(|bounds| bounds.contains(&axis))
        {
            axis
        } else {
            axis.rem_euclid(Self::SKEW_AXIS_PERIOD)
        }
    }

    /// The FX `[x, y]` scale: Scale Height on both axes under Uniform Scale
    /// (T3), else Scale Width and Scale Height (inferred).
    pub(crate) fn scale(&self) -> [f64; 2] {
        if self.uniform_scale {
            [self.scale_height; 2]
        } else {
            [self.scale_width, self.scale_height]
        }
    }

    /// The Shutter Angle at which Premiere blurs this Transform's motion: a
    /// nonzero one while the composition's is not used (T12: clip F's edges
    /// widen from 2.2 to 12.5 px at 180°); angle 0 is no blur by definition.
    /// It converts as FX motion blur on the staged video with the
    /// composition's shutter at this angle.
    pub(crate) fn motion_blur_shutter_angle(&self) -> Option<f64> {
        (!self.composition_shutter_angle && self.shutter_angle > 0.0).then_some(self.shutter_angle)
    }

    /// Reject what no FX value approximates: Skew or Skew Axis keys
    /// ([`Self::KEYED_SKEW`]). Called by the reader and the exporter on the
    /// values and keys each writes.
    pub(crate) fn ensure_convertible(
        &self,
        animations: &[PrEffectParamAnimation],
    ) -> std::result::Result<(), String> {
        let keyed = |param: &EffectParamSpec| {
            animations
                .iter()
                .any(|animation| animation.param.id == param.id)
        };
        if keyed(&TRANSFORM_SKEW) || keyed(&TRANSFORM_SKEW_AXIS) {
            return Err(Self::KEYED_SKEW.to_owned());
        }
        Ok(())
    }

    /// Reports each approximated parameter while preserving the rest of the Transform.
    ///
    /// Warnings follow the check order: Opacity other than 100 or keyed (sRGB for
    /// Premiere's linear light, T6); motion blur
    /// ([`Self::motion_blur_shutter_angle`], FX's blur measured against clip
    /// F's); keyed Shutter Angle with the checkbox off (its first key's value;
    /// with the checkbox on no angle renders); bicubic Sampling
    /// (bilinear); a nonzero Skew with a nonzero Rotation at any time or with
    /// axis scales whose equality at every time is unproved (FX's
    /// composition of skew, rotation and scale; T5 measured the shear at
    /// Rotation 0 and 100/100 only). Scale Width keys under Uniform Scale are
    /// an omission ([`Self::unimported_scale_width_keys`]). The axes are equal at
    /// every time under Uniform Scale, or when the two statics are equal and
    /// Scale Width and Scale Height are both static or carry identical native
    /// tracks (the same key times, values and modes); equal key ranges alone
    /// do not establish it, because two tracks that reach the same extrema at
    /// different times or on different easings differ in between. Import and
    /// export report the same warnings for the values and keys each converts.
    pub(crate) fn approximations(&self, animations: &[PrEffectParamAnimation]) -> Vec<String> {
        let track = |param: &EffectParamSpec| {
            animations
                .iter()
                .find(|animation| animation.param.id == param.id)
                .map(|animation| &animation.keys)
        };
        let keyed = |param: &EffectParamSpec| track(param).is_some();
        let mut warnings = Vec::new();
        if keyed(&TRANSFORM_OPACITY) || self.opacity != TRANSFORM_OPAQUE {
            let opacity = if keyed(&TRANSFORM_OPACITY) {
                "keyed Transform Opacity".to_owned()
            } else {
                format!("Transform Opacity {}", self.opacity)
            };
            warnings.push(format!("{opacity} blends in linear light in Premiere; converted as sRGB opacity (Transform Opacity 50 with clip Opacity 50: mean error ≈ 22 levels, p99 ≈ 80)"));
        }
        if let Some(angle) = self.motion_blur_shutter_angle() {
            warnings.push(format!("Transform motion blur (Shutter Angle {angle}) approximated by FX motion blur (at 180°: blur edges 13.1-13.7 px wide against Premiere's 12.0-12.2; FX's blur is one frame late at every start and stop of the motion)"));
        }
        if keyed(&TRANSFORM_SHUTTER_ANGLE) && !self.composition_shutter_angle {
            warnings.push(format!("keyed Transform Shutter Angle converts as its first key's value {}: the FX composition shutter has no keys (unmeasured against Premiere)", self.shutter_angle));
        }
        if self.bicubic_sampling {
            warnings.push(format!("Transform Sampling {TRANSFORM_SAMPLING_BICUBIC} (bicubic) has no FX equivalent; bilinear used (unmeasured against Premiere)"));
        }
        if self.skew != 0.0 {
            let rotation = track(&TRANSFORM_ROTATION)
                .and_then(PrEffectParamKeys::scalar)
                .map_or([self.rotation; 2], |keys| {
                    let keys: Vec<_> = keys
                        .iter()
                        .map(|key| (key.value, key.easing.bezier()))
                        .collect();
                    scalar_range(&keys)
                });
            let equal_axes = self.uniform_scale
                || (self.scale_width == self.scale_height
                    && track(&TRANSFORM_SCALE_WIDTH) == track(&TRANSFORM_SCALE_HEIGHT));
            let with = match (rotation != [0.0; 2], !equal_axes) {
                (false, false) => None,
                (true, true) => Some("a Rotation and unequal Scale Width and Scale Height"),
                (true, false) => Some("a Rotation"),
                (false, true) => Some("unequal Scale Width and Scale Height"),
            };
            if let Some(with) = with {
                warnings.push(format!(
                    "Transform Skew {} with {with} converts with FX's composition of skew, rotation and scale; skew with rotation or non-uniform scale is unmeasured (native measurements cover Rotation 0 and Scale 100/100 only)",
                    self.skew
                ));
            }
        }
        warnings
    }

    /// Why import drops this Transform's Scale Width keys: under Uniform
    /// Scale they do not render (inferred from T3's static Scale Width);
    /// `None` otherwise. Export writes Uniform
    /// Scale only with Scale Height keys.
    pub(crate) fn unimported_scale_width_keys(
        &self,
        animations: &[PrEffectParamAnimation],
    ) -> Option<&'static str> {
        (self.uniform_scale
            && animations
                .iter()
                .any(|animation| animation.param.id == TRANSFORM_SCALE_WIDTH.id))
        .then_some("Transform Scale Width keys under Uniform Scale were not imported: Premiere renders Scale Height on both axes (inferred from a static Scale Width sample)")
    }

    /// Why this Transform, with its keys `animations`, can hide its whole
    /// picture at some time by its geometry; `None` while part of the picture
    /// stays in the clip's frame at every time. Import converts no source
    /// Transform, and a Geometry2 only as a centered positive zoom, so the
    /// reader omits a placement whose picture a left-out one can hide
    /// (`SplitChain::reject_hiding_transforms`).
    ///
    /// The rendered Scale, Scale Height on both axes under Uniform Scale
    /// (T3), must not reach 0. Unrotated and unskewed, the picture spans
    /// Position + Scale × ([0, 1] − Anchor Point) on each axis in frame units
    /// (T7) and must overlap the frame [0, 1] there; keys are bounded by
    /// their extremes ([`scalar_range`], [`point_axis_range`]), whose
    /// pairings bound each edge because a Scale that does not reach 0 keeps
    /// its sign. Under a Rotation or Skew, whose composition with the Scale
    /// is unmeasured ([`Self::approximations`]), the picture point at the
    /// Anchor Point still lands on the Position, so only an Anchor Point on
    /// the picture with a Position inside the frame at every time keeps part
    /// of the picture there. The reader keeps no curved Position path and no
    /// Anchor Point keys for a Transform.
    pub(crate) fn hiding_geometry(&self, animations: &[PrEffectParamAnimation]) -> Option<String> {
        let TransformExtremes {
            scale,
            position,
            turned,
        } = self.extremes(animations);
        for (label, [least, greatest]) in scale {
            if (least..=greatest).contains(&0.0) {
                return Some(if least == greatest {
                    format!("its {label} is {least}")
                } else {
                    format!("its {label} keys reach {least} to {greatest}, which includes 0")
                });
            }
        }
        if turned {
            let anchored = self
                .anchor_point
                .iter()
                .all(|value| (0.0..=1.0).contains(value))
                && position
                    .iter()
                    .all(|&[least, greatest]| 0.0 < least && greatest < 1.0);
            return (!anchored).then(|| {
                "under its Rotation or Skew, its Anchor Point or Position can move the whole picture out of its frame".to_owned()
            });
        }
        for (axis, name) in ["x", "y"].into_iter().enumerate() {
            let anchor = self.anchor_point[axis];
            let outside = position[axis].into_iter().any(|at| {
                scale[axis].1.into_iter().any(|percent| {
                    let edges = [
                        at - percent / 100.0 * anchor,
                        at + percent / 100.0 * (1.0 - anchor),
                    ];
                    edges.iter().all(|&edge| edge <= 0.0) || edges.iter().all(|&edge| edge >= 1.0)
                })
            });
            if outside {
                return Some(format!(
                    "its Position, Anchor Point and Scale can move the whole picture out of its frame on the {name} axis"
                ));
            }
        }
        None
    }

    /// Whether this Transform, with its keys `animations`, moves, scales,
    /// mirrors or turns its picture at some time: a Position away from its
    /// Anchor Point, a rendered Scale other than 100 or a Rotation or Skew
    /// ([`Self::hiding_geometry`]'s extremes). [`Self::hiding_geometry`]
    /// checks the whole picture alone in its frame, so the reader omits a
    /// placement whose left-out Transform does so beside another effect that
    /// changes which part of the picture shows
    /// (`SplitChain::reject_hiding_transforms`).
    pub(crate) fn changes_geometry(&self, animations: &[PrEffectParamAnimation]) -> bool {
        let TransformExtremes {
            scale,
            position,
            turned,
        } = self.extremes(animations);
        turned
            || scale.iter().any(|&(_, range)| range != [100.0; 2])
            || position
                .iter()
                .zip(self.anchor_point)
                .any(|(&range, anchor)| range != [anchor; 2])
    }

    /// The extremes of this Transform's geometry over its keys `animations`.
    fn extremes(&self, animations: &[PrEffectParamAnimation]) -> TransformExtremes {
        let keys = |param: &EffectParamSpec| {
            animations
                .iter()
                .find(|animation| animation.param.id == param.id)
                .map(|animation| &animation.keys)
        };
        // The least and greatest value of the scalar `param`, static at `value`.
        let range = |param: &EffectParamSpec, value: f64| match keys(param)
            .and_then(PrEffectParamKeys::scalar)
        {
            Some(keys) => {
                let keys: Vec<_> = keys
                    .iter()
                    .map(|key| (key.value, key.easing.bezier()))
                    .collect();
                scalar_range(&keys)
            }
            None => [value; 2],
        };
        let axes = if self.uniform_scale {
            [(&TRANSFORM_SCALE_HEIGHT, self.scale_height); 2]
        } else {
            [
                (&TRANSFORM_SCALE_WIDTH, self.scale_width),
                (&TRANSFORM_SCALE_HEIGHT, self.scale_height),
            ]
        };
        TransformExtremes {
            scale: axes.map(|(param, value)| (param.label, range(param, value))),
            position: match keys(&TRANSFORM_POSITION).and_then(PrEffectParamKeys::point) {
                Some(keys) => [0, 1].map(|axis| point_axis_range(keys, axis)),
                None => self.position.map(|value| [value; 2]),
            },
            turned: range(&TRANSFORM_ROTATION, self.rotation) != [0.0; 2]
                || range(&TRANSFORM_SKEW, self.skew) != [0.0; 2],
        }
    }
}

/// The least and greatest values of a Transform's geometry over its keys
/// ([`scalar_range`], [`point_axis_range`]), for
/// [`PrTransform::hiding_geometry`] and [`PrTransform::changes_geometry`].
struct TransformExtremes {
    /// The label and range, in percent, of the Scale rendered on each axis:
    /// Scale Height on both under Uniform Scale (T3).
    scale: [(&'static str, [f64; 2]); 2],
    /// The range of the Position on each axis, in frame units.
    position: [[f64; 2]; 2],
    /// Whether a Rotation or Skew turns the picture at some time.
    turned: bool,
}

/// Static `AE.ADBE Mosaic` ("Mosaic (Legacy)") values with Sharp Colors on,
/// which FX `mosaic` uses unchanged: both divide the clip frame into
/// `horizontal` × `vertical` blocks from the top-left corner, with fractional
/// block widths when the counts do not divide the frame, and fill each block
/// with the source at its centre (flat blocks equal to the
/// centre pixel within 0.09–1.42 levels; the native 7 × 5 grid's
/// edges at k·1920/7 and k·216). Block counts are fractions of the frame in
/// both engines and Premiere renders clip effects before Motion, so the host's
/// Motion does not enter (inferred beyond the fixture's default Motion,
/// which the export gate measures on a Scale 50 clip).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrMosaic {
    /// Horizontal Blocks, 1 to 4000.
    pub(crate) horizontal: u32,
    /// Vertical Blocks, 1 to 4000.
    pub(crate) vertical: u32,
    /// Sharp Colors. Only `true` converts ([`Self::ensure_convertible`]).
    pub(crate) sharp_colors: bool,
}

impl PrMosaic {
    /// Why a Mosaic with Sharp Colors off is omitted in both directions.
    pub(crate) const SHARP_COLORS_OFF: &'static str = "Sharp Colors is off: Premiere then averages each block, while the FX mosaic samples the block's centre (native sample differences of 29.5–35.1 levels)";

    /// The whole block count that `value` of the count parameter `param` is,
    /// or why it is none, naming the value as `{label}{what} {value}`:
    /// Premiere counts whole blocks and export does not round.
    /// The range is the parameter's.
    pub(crate) fn count(
        param: &EffectParamSpec,
        what: &str,
        value: f64,
    ) -> std::result::Result<u32, String> {
        if value.fract() != 0.0 || !value.is_finite() {
            return Err(format!(
                "{}{what} {value} is not a whole number of blocks; Premiere counts whole blocks and no rounding is applied",
                param.label
            ));
        }
        if !param
            .value_range()
            .is_some_and(|range| range.contains(&value))
        {
            return Err(format!(
                "{}{what} {value} is outside Premiere's {} to {} range",
                param.label, param.lower_bound, param.upper_bound
            ));
        }
        Ok(value as u32)
    }

    /// Reject a Mosaic that the FX `mosaic` does not render as Premiere does:
    /// Sharp Colors off ([`Self::SHARP_COLORS_OFF`]), a count key that is not
    /// a whole number, or count keys that interpolate. Premiere holds each
    /// count until the next key (E8: 40 × 30 exactly from the key's frame) and
    /// the FX renderer would draw fractional counts between Linear or Bézier
    /// keys, where Premiere's stepping is unmeasured. Called by the
    /// reader and the exporter on the values and keys each writes.
    pub(crate) fn ensure_convertible(
        &self,
        animations: &[PrEffectParamAnimation],
    ) -> std::result::Result<(), String> {
        if !self.sharp_colors {
            return Err(Self::SHARP_COLORS_OFF.to_owned());
        }
        for animation in animations {
            let Some(keys) = animation.keys.scalar() else {
                continue;
            };
            for key in keys {
                Self::count(animation.param, " key value", key.value)?;
            }
            // A key's easing describes the segment that ends at it.
            if let Some(pair) = keys
                .windows(2)
                .find(|pair| pair[1].easing != PrKeyframeEasing::Hold)
            {
                let kind = match pair[1].easing {
                    PrKeyframeEasing::Linear => "Linear",
                    PrKeyframeEasing::CubicBezier { .. } => "Bézier",
                    PrKeyframeEasing::Hold => unreachable!("the pair was found by its easing"),
                };
                return Err(format!(
                    "{} keys are {kind} between source times {} s and {} s; only Hold keys convert, because the FX mosaic renders fractional block counts between keys and Premiere's stepping there is unmeasured",
                    animation.param.label,
                    seconds(pair[0].source_ticks),
                    seconds(pair[1].source_ticks)
                ));
            }
        }
        Ok(())
    }
}

/// Static `AE.ADBE Replicate` value, a whole Count, which FX `motionTile`
/// draws as Count × Count whole copies of its layer's frame: tiles of
/// [`Self::tile_size`] percent whose first tile is centred at
/// [`Self::tile_center`] of the frame on both axes, over the whole frame
/// ([`Self::FULL_FRAME_PERCENT`]), without mirrored edges or phase. The FX
/// shader samples each axis at `fract((uv − centre) / (size / 100) + 0.5)`,
/// which is `fract(Count · uv)` at that centre: copies that start at the
/// frame's top-left corner, where the FX default centre 0.5 would shift an
/// even Count by half a tile. Every converted Replicate is reported once as
/// an approximation ([`Self::TILING_APPROXIMATION`]); whole Counts with Hold
/// keys are the forms that convert ([`Self::new`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrReplicate {
    /// Count, the copies in each row and column: a whole number from 2 to 16.
    pub(crate) count: u8,
}

impl PrReplicate {
    /// What every converted Replicate approximates, which import and export
    /// report once per effect: the grid converts, the sampling of each copy
    /// is the FX shader's.
    pub(crate) const TILING_APPROXIMATION: &'static str = "the Count × Count grid of whole copies converts as FX motionTile tiles of 100/Count percent, the first centred at 1/(2·Count); each FX tile samples the layer's whole frame with samples clamped 1.5 source pixels inside its edges, while Premiere's grid origin, its resampling of each copy, its tile edges and alpha are unmeasured";

    /// The FX size of the whole frame, in percent: a Replicate's
    /// `outputWidth` and `outputHeight`, which its tiles divide by the Count.
    pub(crate) const FULL_FRAME_PERCENT: f64 = 100.0;

    /// The Replicate of the native `count`, a keyed Count's first key, with
    /// the Count keys in `animations`, or why it does not convert. A Count
    /// that is not a whole number, static or on a key, is not rounded. Keys
    /// must hold: Premiere's Count between interpolated keys is unmeasured,
    /// and the FX motionTile would interpolate the tile size and centre,
    /// reciprocals of the Count, linearly. Called by the reader and the
    /// exporter on the values and keys each writes.
    pub(crate) fn new(
        count: f64,
        animations: &[PrEffectParamAnimation],
    ) -> std::result::Result<Self, String> {
        for animation in animations {
            let Some(keys) = animation.keys.scalar() else {
                continue;
            };
            for key in keys {
                Self::whole_count(" key value", key.value)?;
            }
            // A key's easing describes the segment that ends at it.
            let interpolated = keys.windows(2).find_map(|pair| {
                let kind = match pair[1].easing {
                    PrKeyframeEasing::Hold => return None,
                    PrKeyframeEasing::Linear => "Linear",
                    PrKeyframeEasing::CubicBezier { .. } => "Bézier",
                };
                Some((kind, pair[0].source_ticks, pair[1].source_ticks))
            });
            if let Some((kind, start, end)) = interpolated {
                return Err(format!(
                    "{} keys are {kind} between source times {} s and {} s; only Hold keys convert, because Premiere's Count between interpolated keys is unmeasured and the FX motionTile would interpolate the tile size and centre, reciprocals of the Count, linearly",
                    animation.param.label,
                    seconds(start),
                    seconds(end)
                ));
            }
        }
        Ok(Self {
            count: Self::whole_count("", count)?,
        })
    }

    /// The whole Count that `value` is, or why it is none, naming it as
    /// `Count{what} {value}`. The range is the parameter's.
    fn whole_count(what: &str, value: f64) -> std::result::Result<u8, String> {
        let param = &REPLICATE_COUNT;
        if value.fract() != 0.0 || !value.is_finite() {
            return Err(format!(
                "{}{what} {value} is not a whole number; Premiere counts whole copies and no rounding is applied",
                param.label
            ));
        }
        if !param
            .value_range()
            .is_some_and(|range| range.contains(&value))
        {
            return Err(format!(
                "{}{what} {value} is outside Premiere's {} to {} range",
                param.label, param.lower_bound, param.upper_bound
            ));
        }
        // Fits: a whole number from 2 to 16.
        Ok(value as u8)
    }

    /// The FX `tileWidth` and `tileHeight` of Count `count`: the frame's
    /// [`Self::FULL_FRAME_PERCENT`] divided into Count tiles.
    pub(crate) fn tile_size(count: f64) -> f64 {
        Self::FULL_FRAME_PERCENT / count
    }

    /// The FX `tileCenterX` and `tileCenterY` of Count `count`: the centre of
    /// the first tile, half of its 1/Count of the frame.
    pub(crate) fn tile_center(count: f64) -> f64 {
        0.5 / count
    }

    /// The Count whose grid FX tiles of `size` percent (`[width, height]`)
    /// whose first tile is centred at `center` (`[x, y]`) draw, or `None`
    /// when they draw none: a whole Count from 2 to 16 whose
    /// [`Self::tile_size`] and [`Self::tile_center`] they equal exactly, as
    /// import writes them. Nothing is rounded, and another centre that draws
    /// the same grid, such as 0.5 for an odd Count, is not recognized.
    pub(crate) fn grid_count(size: [f64; 2], center: [f64; 2]) -> Option<u8> {
        let range = REPLICATE_COUNT.value_range()?;
        (0..=u8::MAX).find(|&count| {
            let count = f64::from(count);
            range.contains(&count)
                && size == [Self::tile_size(count); 2]
                && center == [Self::tile_center(count); 2]
        })
    }
}

/// Native integer Amount, in the same nominal units as FX `sharpen`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrSharpen {
    pub(crate) amount: u16,
}

impl PrSharpen {
    pub(crate) const KERNEL_APPROXIMATION: &'static str = "Sharpen Amount converts unchanged in nominal units, but Premiere and FX use different sharpening kernels; appearance, clipping and high-gain fidelity are not equivalent or certified";

    /// Reject edited fractions rather than silently rounding the native integer control.
    pub(crate) fn new(
        amount: f64,
        animations: &[PrEffectParamAnimation],
    ) -> std::result::Result<Self, String> {
        let whole = |value: f64| {
            if !value.is_finite() || value.fract() != 0.0 || !(0.0..=4000.0).contains(&value) {
                return Err(format!("Sharpen Amount {value} must be a whole number from 0 to 4000; no rounding or clamping is applied"));
            }
            // Checked above: an integer in 0..=4000 fits u16.
            Ok(value as u16)
        };
        for animation in animations {
            if let Some(keys) = animation.keys.scalar() {
                for key in keys {
                    whole(key.value)?;
                    if matches!(key.easing, PrKeyframeEasing::CubicBezier { .. }) {
                        return Err("Sharpen Amount keys must be Linear or Hold; Bezier fidelity is unverified".to_owned());
                    }
                }
            }
        }
        Ok(Self {
            amount: whole(amount)?,
        })
    }
}

/// Static `AE.ADBE Posterize` value, a whole Level, which FX `posterize`
/// keeps as `levels`. Both reduce each channel to Level values from black to
/// white, by different rules ([`Self::QUANTIZER_APPROXIMATION`]). Whole
/// Levels and Hold keys are the forms that convert ([`Self::new`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrPosterize {
    /// Level, the number of values in each channel: a whole number from 2 to
    /// 255.
    pub(crate) level: u8,
}

impl PrPosterize {
    /// What every converted Posterize approximates, which import and export
    /// report once per effect: the Level converts unchanged, the quantizer
    /// does not.
    pub(crate) const QUANTIZER_APPROXIMATION: &'static str = "Premiere Posterize quantizes each channel into Level equal input bins, floor(v·n/256)·255/(n − 1), and FX posterize rounds it to the nearest of as many levels, round(v·(n − 1)/255)·255/(n − 1); the Level converts unchanged and colours near a bin edge render differently";

    /// The Posterize of the native `level`, a keyed Level's first key, with
    /// the Level keys in `animations`, or why it does not convert. A Level
    /// that is not a whole number, static or on a key, is not rounded:
    /// Premiere's rendering of a fractional Level is unmeasured. Keys must
    /// hold: Premiere holds each Level until the next key, while its stepping
    /// between Linear or Bézier keys, where the FX shader floors the
    /// interpolated levels, is unmeasured. Called by the reader and the
    /// exporter on the values and keys each writes.
    pub(crate) fn new(
        level: f64,
        animations: &[PrEffectParamAnimation],
    ) -> std::result::Result<Self, String> {
        for animation in animations {
            let Some(keys) = animation.keys.scalar() else {
                continue;
            };
            for key in keys {
                Self::level(" key value", key.value)?;
            }
            // A key's easing describes the segment that ends at it.
            let interpolated = keys.windows(2).find_map(|pair| {
                let kind = match pair[1].easing {
                    PrKeyframeEasing::Hold => return None,
                    PrKeyframeEasing::Linear => "Linear",
                    PrKeyframeEasing::CubicBezier { .. } => "Bézier",
                };
                Some((kind, pair[0].source_ticks, pair[1].source_ticks))
            });
            if let Some((kind, start, end)) = interpolated {
                return Err(format!(
                    "{} keys are {kind} between source times {} s and {} s; only Hold keys convert, because the FX posterize floors the levels between keys and Premiere's stepping there is unmeasured",
                    animation.param.label,
                    seconds(start),
                    seconds(end)
                ));
            }
        }
        Ok(Self {
            level: Self::level("", level)?,
        })
    }

    /// The whole Level that `value` is, or why it is none, naming it as
    /// `Level{what} {value}`. The range is the parameter's.
    fn level(what: &str, value: f64) -> std::result::Result<u8, String> {
        let param = &POSTERIZE_LEVEL;
        if value.fract() != 0.0 || !value.is_finite() {
            return Err(format!(
                "{}{what} {value} is not a whole number; Premiere's rendering of a fractional Level is unmeasured and no rounding is applied",
                param.label
            ));
        }
        if !param
            .value_range()
            .is_some_and(|range| range.contains(&value))
        {
            return Err(format!(
                "{}{what} {value} is outside Premiere's {} to {} range",
                param.label, param.lower_bound, param.upper_bound
            ));
        }
        // Fits: a whole number from 2 to 255.
        Ok(value as u8)
    }
}

/// Static `AE.ADBE Ramp` values of a linear ramp with no scatter, which FX
/// `gradientRamp` uses as frame-UV points, channel shares of 255 and
/// `blend` = 1 − Blend With Original. Both mix the two colours on encoded
/// values along the axis from `start` to `end` and mix the result with the
/// original (mean differences of 0.26–0.75 levels on the native unblended frames).
///
/// Premiere measures the ramp in clip pixels and the FX shader in the layer
/// frame's UV, so the two agree only on an axis-aligned ramp on a clip whose
/// frame is the canvas ([`PrRamp::ensure_aligned`], the converter's host rule;
/// the E10 probe measured 50 and 13 levels on a diagonal and a radial ramp).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrRamp {
    /// Start of Ramp, normalized to the clip frame (origin top left, y down),
    /// possibly outside it.
    pub(crate) start: [f64; 2],
    /// Start Color.
    pub(crate) start_colour: PrColour,
    /// End of Ramp, normalized like `start`.
    pub(crate) end: [f64; 2],
    /// End Color.
    pub(crate) end_colour: PrColour,
    /// Blend With Original, from 0 (the ramp alone) to 1 (the original alone).
    pub(crate) blend: f64,
}

/// The axis of an aligned ramp: the coordinate that both endpoints share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RampAxis {
    /// Both endpoints share x: the ramp runs down the frame.
    Vertical,
    /// Both endpoints share y: the ramp runs across the frame.
    Horizontal,
}

impl PrRamp {
    /// The squared axis length below which the FX `gradient_ramp` shader
    /// stops dividing by the axis: it computes
    /// `dot(uv - sp, axis) / max(dot(axis, axis), 1e-5)` in f32
    /// (`crates/fx_composition/src/custom_shader_presets.rs`), so a shorter
    /// axis is stretched over √1e-5 of the frame instead of its own length
    /// (a 0.001 ramp renders a tenth of its gradient at its nominal end).
    const SHADER_AXIS_FLOOR: f64 = 1e-5;

    /// The least axis length, as a share of the frame, that converts: √1e-5
    /// = 0.003162… ([`Self::SHADER_AXIS_FLOOR`]) rounded up to 0.0032. The
    /// margin of 3.8e-5 exceeds by orders of magnitude the f32 rounding of
    /// the endpoints (2⁻²⁴ of their magnitude, endpoints within a few hundred
    /// frames of the canvas), of their difference and of the squared length,
    /// so a converted ramp never reaches the floor when the shader evaluates
    /// it; Premiere has no such floor (E10: a ramp is its pixel projection).
    pub(crate) const MIN_LENGTH: f64 = 0.0032;

    /// The FX values that the shader's `blend` and Premiere's Blend With
    /// Original exchange: each is one minus the other, an affine map under
    /// which normalized key easing carries over unchanged.
    pub(crate) fn fx_blend(blend: f64) -> f64 {
        1.0 - blend
    }

    /// Reject a ramp whose axis is not aligned with the frame at every time,
    /// whose endpoints coincide, meet or come within [`Self::MIN_LENGTH`] of
    /// each other, or whose keys leave the axis: Premiere measures the ramp in
    /// clip pixels and the FX shader in frame UV, which agree only along an
    /// axis-aligned line on a frame-size host
    /// and only while the shader divides by the
    /// axis ([`Self::SHADER_AXIS_FLOOR`]). A keyed coordinate is bounded by
    /// its keys, Bézier overshoot included ([`scalar_range`]).
    pub(crate) fn ensure_aligned(
        &self,
        animations: &[PrEffectParamAnimation],
    ) -> std::result::Result<(), String> {
        let ranges = |param: &EffectParamSpec, value: [f64; 2]| -> [[f64; 2]; 2] {
            let keys = animations
                .iter()
                .find(|animation| animation.param.id == param.id)
                .and_then(|animation| animation.keys.point());
            std::array::from_fn(|axis| match keys {
                Some(keys) => point_axis_range(keys, axis),
                None => [value[axis]; 2],
            })
        };
        let [start_x, start_y] = ranges(&RAMP_START, self.start);
        let [end_x, end_y] = ranges(&RAMP_END, self.end);
        let fixed = |start: [f64; 2], end: [f64; 2]| start[0] == start[1] && start == end;
        let axis = match (fixed(start_x, end_x), fixed(start_y, end_y)) {
            (true, true) => {
                return Err(format!(
                    "Start of Ramp and End of Ramp are both {}:{}; a ramp of zero length is not converted",
                    self.start[0], self.start[1]
                ))
            }
            (true, false) => RampAxis::Vertical,
            (false, true) => RampAxis::Horizontal,
            (false, false) => {
                return Err(format!(
                    "Start of Ramp {}:{} to End of Ramp {}:{} is not aligned with the frame at every time; Premiere measures a ramp in clip pixels and the FX gradientRamp in frame UV, which agree only along the frame's axes",
                    self.start[0], self.start[1], self.end[0], self.end[1]
                ))
            }
        };
        let (along_start, along_end, name) = match axis {
            RampAxis::Vertical => (start_y, end_y, "y"),
            RampAxis::Horizontal => (start_x, end_x, "x"),
        };
        // The least distance between the two endpoints at any time: the gap
        // between their ranges, negative when the ranges overlap.
        let length = (along_end[0] - along_start[1]).max(along_start[0] - along_end[1]);
        if length <= 0.0 {
            return Err(format!(
                "Start of Ramp and End of Ramp meet: their {name} coordinates reach {}..{} and {}..{} over their keys, and a ramp of zero length is not converted",
                along_start[0], along_start[1], along_end[0], along_end[1]
            ));
        }
        if length < Self::MIN_LENGTH {
            return Err(format!(
                "Start of Ramp and End of Ramp come within {length:.4} of the frame of each other along {name} (their coordinates reach {}..{} and {}..{} over their keys); a ramp shorter than {} of the frame is not converted, because the FX gradientRamp floors its squared length at {:e} and would stretch it over {:.4} of the frame",
                along_start[0], along_start[1], along_end[0], along_end[1],
                Self::MIN_LENGTH, Self::SHADER_AXIS_FLOOR, Self::SHADER_AXIS_FLOOR.sqrt()
            ));
        }
        Ok(())
    }
}

/// The least and greatest value of coordinate `axis` of point `keys` at any
/// time ([`scalar_range`] of that coordinate).
fn point_axis_range(keys: &[PrPointKeyframe], axis: usize) -> [f64; 2] {
    let coordinates: Vec<_> = keys
        .iter()
        .map(|key| (key.value[axis], key.easing.bezier()))
        .collect();
    scalar_range(&coordinates)
}

/// Static `AE.ADBE Tint` values, which FX `tintTritone` uses as channel
/// shares of 255 and an unchanged Amount: both map luma (Rec. 601 weights on
/// encoded values) from `black` to `white` and mix the result
/// with the original by `amount` percent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrTint {
    /// Map Black To.
    pub(crate) black: PrColour,
    /// Map White To.
    pub(crate) white: PrColour,
    /// Amount to Tint, in percent.
    pub(crate) amount: f64,
}

impl PrTint {
    /// Tint's defaults, a full grayscale: what a Black & White renders (E7).
    pub(crate) const GRAYSCALE: Self = Self {
        black: PrColour::BLACK,
        white: PrColour::WHITE,
        amount: 100.0,
    };
}

/// Static `AE.ADBE Gaussian Blur 2` values that FX `gaussianBlur` expresses.
///
/// Blur Dimensions is always Horizontal and Vertical: the FX variant has no
/// axis selection, so single-axis native modes are rejected rather than kept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrGaussianBlur {
    /// Native Blurriness, which FX `gaussianBlur.blurriness` uses unchanged.
    pub(crate) blurriness: f64,
    pub(crate) repeat_edge_pixels: bool,
}

/// Static `AE.Impact_Blur_FX` values that FX `gaussianBlur` expresses: a
/// uniform blur without Chromatic Aberration. Angle, Seed and Thickness are
/// not kept (see [`FILM_IMPACT_BLUR`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrFilmImpactBlur {
    /// Native Amount: FX `blurriness` is [`FILM_IMPACT_BLURRINESS_PER_AMOUNT`]
    /// times it.
    pub(crate) amount: f64,
    /// Edge Behavior 1 (repeat) rather than 2 (transparent exterior).
    pub(crate) repeat_edge_pixels: bool,
}

/// Static `AE.ADBE Motion Blur` values in the clip's own frame: Premiere blurs
/// a clip before its Motion (the native scaled and rotated clip
/// blurs along its own vertical axis). The converter maps them to FX
/// `directionalBlur`, which blurs in composition space, through the host's
/// static Scale and Rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrDirectionalBlur {
    /// Native Direction in degrees clockwise from up: 0 blurs vertically and
    /// 90 horizontally (E2).
    pub(crate) direction: f64,
    /// Native Blur Length in clip pixels.
    pub(crate) blur_length: f64,
}

/// Static `AE.Impact_Directional_Blur_FX` values in the clip's own frame, which
/// convert as a [`PrDirectionalBlur`] with Direction = Angle and Blur Length =
/// [`FILM_IMPACT_BLUR_LENGTH_PER_AMOUNT`] times Amount (see
/// [`FILM_IMPACT_DIRECTIONAL_BLUR`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrFilmImpactDirectionalBlur {
    /// Native Angle in degrees clockwise from up, as Legacy Direction.
    pub(crate) angle: f64,
    pub(crate) amount: f64,
}

/// Static `AE.ADBE Brightness & Contrast 2` values, which FX
/// `brightnessContrast` uses unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrBrightnessContrast {
    pub(crate) brightness: f64,
    pub(crate) contrast: f64,
}

/// Saved `AE.ADBE Invert` selection. RGB uses complementary Levels; Alpha
/// requires an ordinary-occurrence coverage graph, never RGB Levels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrInvert {
    /// Saved Premiere popup: RGB 0 or Alpha 15 (not AE's Alpha 16).
    pub(crate) channel: u8,
    /// Native Blend With Original, the original's share of the render in
    /// percent: 0 is the full inversion.
    pub(crate) blend: f64,
}

/// Static `AE.ADBE Corner Pin` corners, which FX `cornerPin` uses unchanged.
///
/// Each corner is normalized to the clip's own frame, origin top left and y
/// down, and may lie outside it: native measurements used a portrait
/// clip in a landscape sequence, and Premiere's warp as a perspective
/// (projective) one, as FX draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrCornerPin {
    /// Upper Left, Upper Right, Lower Left and Lower Right, in native `Params`
    /// order.
    pub(crate) corners: [[f64; 2]; 4],
}

impl PrCornerPin {
    /// Reject a quad that is degenerate or non-convex at any time: no
    /// perspective warp maps the clip frame onto one, and Premiere's rendering
    /// of one is unmeasured. The quad is checked at the static corners and at
    /// the time of every corner key, where each other corner takes its value
    /// at that time, and over every interval between two consecutive key times
    /// of any corner, as
    /// [`ensure_convex_between`] describes.
    ///
    /// Each of the quad's four turns must have one strict sign, the same for
    /// all four, computed in f64 with no tolerance: a zero or NaN turn fails.
    pub(crate) fn ensure_convex(
        &self,
        animations: &[PrEffectParamAnimation],
    ) -> std::result::Result<(), String> {
        let corner_keys: [Option<&[PrPointKeyframe]>; 4] = std::array::from_fn(|index| {
            animations
                .iter()
                .find(|animation| animation.param.id == CORNER_PIN.params[index].id)
                .and_then(|animation| animation.keys.point())
        });
        if !is_convex(self.corners) {
            return Err("the static corners form a degenerate or non-convex quad, which no perspective warp of the clip frame produces".to_owned());
        }
        let mut times: Vec<i64> = corner_keys
            .iter()
            .flatten()
            .flat_map(|keys| keys.iter().map(|key| key.source_ticks))
            .collect();
        for &ticks in &times {
            let corners = std::array::from_fn(|index| {
                corner_keys[index]
                    .and_then(|keys| corner_at(keys, ticks))
                    .unwrap_or(self.corners[index])
            });
            if !is_convex(corners) {
                return Err(format!(
                    "the corners form a degenerate or non-convex quad at the key at source time {:.3} s, which no perspective warp of the clip frame produces",
                    ticks as f64 / TICKS as f64
                ));
            }
        }
        times.sort_unstable();
        times.dedup();
        for pair in times.windows(2) {
            let interval = [pair[0], pair[1]];
            let motions = std::array::from_fn(|index| {
                corner_keys[index]
                    .and_then(|keys| corner_motion(keys, interval))
                    .unwrap_or(CornerMotion::Fixed(self.corners[index]))
            });
            ensure_convex_between(motions, interval)?;
        }
        Ok(())
    }
}

/// Native corner indices (upper left, upper right, lower left, lower right)
/// in drawing order: upper left, upper right, lower right, lower left.
const DRAWING_ORDER: [usize; 4] = [0, 1, 3, 2];

/// The native indices of the corners of turn `turn`: a corner's predecessor
/// in drawing order, the corner and its successor.
pub(crate) fn turn_corners(turn: usize) -> [usize; 3] {
    [0, 1, 2].map(|offset| DRAWING_ORDER[(turn + offset) % 4])
}

/// Each turn of the quad: the cross product of the edges into and out of a
/// corner, which is twice the signed area of the triangle of that corner and
/// its two neighbours. A Corner Pin's quad is convex when all four turn one
/// strict way.
pub(crate) fn turns(corners: [[f64; 2]; 4]) -> [f64; 4] {
    std::array::from_fn(|turn| {
        let [a, b, c] = turn_corners(turn).map(|index| corners[index]);
        cross(difference(b, a), difference(c, b))
    })
}

/// The strict way in which the quad turns at all four corners, or `None`
/// when a turn is zero, NaN or the other way.
fn turning(corners: [[f64; 2]; 4]) -> Option<Ordering> {
    let ways = turns(corners).map(|turn| turn.partial_cmp(&0.0));
    let way = ways[0].filter(|way| way.is_ne())?;
    ways.iter().all(|other| *other == Some(way)).then_some(way)
}

/// Whether the corners, taken in drawing order, turn the same strict way at
/// every corner.
fn is_convex(corners: [[f64; 2]; 4]) -> bool {
    turning(corners).is_some()
}

fn difference(to: [f64; 2], from: [f64; 2]) -> [f64; 2] {
    [to[0] - from[0], to[1] - from[1]]
}

fn cross(first: [f64; 2], second: [f64; 2]) -> f64 {
    first[0] * second[1] - first[1] * second[0]
}

/// `start` moved by `amount` times `direction`.
fn along(start: [f64; 2], direction: [f64; 2], amount: f64) -> [f64; 2] {
    [
        start[0] + direction[0] * amount,
        start[1] + direction[1] * amount,
    ]
}

/// A corner over an interval between two consecutive key times of any corner,
/// which therefore holds none of its own keys.
#[derive(Debug, Clone, Copy)]
enum CornerMotion {
    /// Unkeyed, before its first key or after its last, on a Hold (up to its
    /// next key), or between two keys on one point.
    Fixed([f64; 2]),
    Moving(CornerSegment),
}

/// A corner's straight path between two of its keys, over part of which the
/// interval lies.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CornerSegment {
    from: [f64; 2],
    to: [f64; 2],
    /// The source times of the two keys.
    keys: [i64; 2],
    /// The cubic Bézier timing handles, or `None` for Linear.
    bezier: Option<[f64; 4]>,
    /// The interval's ends as progress in time along the segment, 0 to 1.
    times: [f64; 2],
}

impl CornerSegment {
    /// The least and greatest eased progress along the segment over the
    /// interval.
    fn progress_range(&self) -> [f64; 2] {
        self.bezier.map_or(self.times, |handles| {
            cubic_bezier_range(handles, self.times)
        })
    }
}

/// How the corner with `keys` moves over `interval`, two consecutive key
/// times of any corner; `None` when it has no keys.
fn corner_motion(keys: &[PrPointKeyframe], [start, end]: [i64; 2]) -> Option<CornerMotion> {
    let next = keys.partition_point(|key| key.source_ticks <= start);
    let Some(to) = keys.get(next) else {
        return keys.last().map(|key| CornerMotion::Fixed(key.value));
    };
    let Some(from) = next.checked_sub(1).and_then(|index| keys.get(index)) else {
        return Some(CornerMotion::Fixed(to.value));
    };
    let bezier = match to.easing {
        PrKeyframeEasing::Hold => return Some(CornerMotion::Fixed(from.value)),
        PrKeyframeEasing::Linear => None,
        PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => Some([x1, y1, x2, y2]),
    };
    if from.value == to.value {
        return Some(CornerMotion::Fixed(from.value));
    }
    let span = (i128::from(to.source_ticks) - i128::from(from.source_ticks)) as f64;
    Some(CornerMotion::Moving(CornerSegment {
        from: from.value,
        to: to.value,
        keys: [from.source_ticks, to.source_ticks],
        bezier,
        times: [start, end]
            .map(|ticks| (i128::from(ticks) - i128::from(from.source_ticks)) as f64 / span),
    }))
}

/// Reject a quad that is not strictly convex, with one orientation, at some
/// time of `interval`, two consecutive key times of any corner over which the
/// corners move as `motions` say. The quad at an interval's end is its limit
/// there, before a Hold jumps. Each turn is twice the signed area of a
/// triangle of three distinct corners, so it is affine in each corner's
/// position:
/// - When every moving corner is Linear, or all move between the same keys
///   with one Bézier easing, every corner moves with one shared parameter and
///   each turn is a quadratic in it, extreme at an end of its range or at its
///   vertex: the check is exact.
/// - Otherwise each moving corner's own eased progress is bounded by its
///   range, and each turn is extreme at a vertex of the box of these ranges.
///   This is sound but conservative, because the box also holds progress
///   combinations that the corners never reach at one time; the shared
///   parameter is not relaxed to it, so that a translating quad converts.
fn ensure_convex_between(
    motions: [CornerMotion; 4],
    [start, end]: [i64; 2],
) -> std::result::Result<(), String> {
    let segments: Vec<(usize, CornerSegment)> = motions
        .iter()
        .enumerate()
        .filter_map(|(index, motion)| match motion {
            CornerMotion::Moving(segment) => Some((index, *segment)),
            CornerMotion::Fixed(_) => None,
        })
        .collect();
    let Some(&(_, first)) = segments.first() else {
        // A still quad is the quad at `start`, checked at that key.
        return Ok(());
    };
    let from = motions.map(|motion| match motion {
        CornerMotion::Fixed(point) => point,
        CornerMotion::Moving(segment) => segment.from,
    });
    let path = motions.map(|motion| match motion {
        CornerMotion::Fixed(_) => [0.0; 2],
        CornerMotion::Moving(segment) => difference(segment.to, segment.from),
    });
    let exact = if segments.iter().all(|(_, segment)| segment.bezier.is_none()) {
        // Linear corners move at constant speeds: all by the interval's own
        // progress in time.
        let [base, arrival] = [0, 1].map(|side| {
            std::array::from_fn(|index| match motions[index] {
                CornerMotion::Fixed(point) => point,
                CornerMotion::Moving(segment) => {
                    along(segment.from, path[index], segment.times[side])
                }
            })
        });
        let direction = std::array::from_fn(|index| difference(arrival[index], base[index]));
        Some(stays_convex_along(base, direction, [0.0, 1.0]))
    } else if segments
        .iter()
        .all(|(_, segment)| segment.bezier == first.bezier && segment.keys == first.keys)
    {
        Some(stays_convex_along(from, path, first.progress_range()))
    } else {
        None
    };
    let seconds = |ticks: i64| ticks as f64 / TICKS as f64;
    match exact {
        Some(true) => Ok(()),
        Some(false) => Err(format!(
            "the corners form a degenerate or non-convex quad between the keys at source times {:.3} s and {:.3} s, which no perspective warp of the clip frame produces",
            seconds(start),
            seconds(end)
        )),
        None => {
            let ranges = motions.map(|motion| match motion {
                CornerMotion::Fixed(_) => [0.0; 2],
                CornerMotion::Moving(segment) => segment.progress_range(),
            });
            if stays_convex_in_box(from, path, ranges) {
                return Ok(());
            }
            let names: Vec<&str> = segments
                .iter()
                .map(|(index, _)| CORNER_PIN.params[*index].label)
                .collect();
            Err(format!(
                "{} move with different easings between the keys at source times {:.3} s and {:.3} s, so their quad there cannot be proven convex",
                english_list(&names),
                seconds(start),
                seconds(end)
            ))
        }
    }
}

/// `names` joined as an English list: "A and B", "A, B and C".
fn english_list(names: &[&str]) -> String {
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

/// Whether the quad of the corners `base + w * direction` is strictly convex,
/// with one orientation, for every `w` in `range`. Each turn is then a
/// quadratic in `w`, extreme at an end of `range` or at its vertex.
fn stays_convex_along(
    base: [[f64; 2]; 4],
    direction: [[f64; 2]; 4],
    [low, high]: [f64; 2],
) -> bool {
    let quad = |w: f64| std::array::from_fn(|index| along(base[index], direction[index], w));
    let vertices = (0..4).filter_map(|turn| {
        let [a, b, c] = turn_corners(turn);
        let (into, out_of) = (difference(base[b], base[a]), difference(base[c], base[b]));
        let (into_moves, out_of_moves) = (
            difference(direction[b], direction[a]),
            difference(direction[c], direction[b]),
        );
        // The turn is cross(into + w * into_moves, out_of + w * out_of_moves).
        // One that is affine in `w` has an infinite or NaN vertex, outside
        // the range.
        let square = cross(into_moves, out_of_moves);
        let linear = cross(into, out_of_moves) + cross(into_moves, out_of);
        let vertex = -linear / (2.0 * square);
        (low < vertex && vertex < high).then_some(vertex)
    });
    turning(quad(low)).is_some_and(|way| {
        [high]
            .into_iter()
            .chain(vertices)
            .all(|w| turning(quad(w)) == Some(way))
    })
}

/// Whether the quad of the corners `base + w * direction` is strictly convex,
/// with one orientation, for every combination of each corner's own `w` in
/// its entry of `ranges`. Each turn is affine in each corner's `w`, so it is
/// extreme at one of the 16 vertices of that box.
fn stays_convex_in_box(
    base: [[f64; 2]; 4],
    direction: [[f64; 2]; 4],
    ranges: [[f64; 2]; 4],
) -> bool {
    let mut way = None;
    (0..16_usize).all(|vertex| {
        let quad = std::array::from_fn(|index| {
            along(
                base[index],
                direction[index],
                ranges[index][(vertex >> index) & 1],
            )
        });
        let this = turning(quad);
        this.is_some() && *way.get_or_insert(this) == this
    })
}

/// A keyed corner at `ticks`: its first key's value before that key, its last
/// key's after the last, and between two keys the eased progress along their
/// straight path.
fn corner_at(keys: &[PrPointKeyframe], ticks: i64) -> Option<[f64; 2]> {
    let next = keys.partition_point(|key| key.source_ticks <= ticks);
    let Some(end) = keys.get(next) else {
        return keys.last().map(|key| key.value);
    };
    let Some(start) = next.checked_sub(1).and_then(|index| keys.get(index)) else {
        return Some(end.value);
    };
    let progress = (i128::from(ticks) - i128::from(start.source_ticks)) as f64
        / (i128::from(end.source_ticks) - i128::from(start.source_ticks)) as f64;
    let eased = match end.easing {
        PrKeyframeEasing::Hold => 0.0,
        PrKeyframeEasing::Linear => progress,
        PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            cubic_bezier_progress([x1, y1, x2, y2], progress)
        }
    };
    Some(std::array::from_fn(|axis| {
        start.value[axis] + (end.value[axis] - start.value[axis]) * eased
    }))
}

/// The cubic Bézier timing curve at `progress`. Its x handles lie in 0 to 1,
/// so x grows with the curve parameter, which bisection finds.
fn cubic_bezier_progress([x1, y1, x2, y2]: [f64; 4], progress: f64) -> f64 {
    bezier_coordinate(y1, y2, bezier_parameter(x1, x2, progress))
}

/// The curve parameter at which the timing curve's x, which grows with it,
/// reaches `progress`.
fn bezier_parameter(x1: f64, x2: f64, progress: f64) -> f64 {
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..64 {
        let middle = (low + high) / 2.0;
        if bezier_coordinate(x1, x2, middle) < progress {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) / 2.0
}

/// One coordinate of the timing curve from 0 to 1 whose control points have
/// the coordinates `first` and `second`, at the curve parameter `parameter`.
fn bezier_coordinate(first: f64, second: f64, parameter: f64) -> f64 {
    let rest = 1.0 - parameter;
    3.0 * rest * rest * parameter * first
        + 3.0 * rest * parameter * parameter * second
        + parameter * parameter * parameter
}

/// The least and greatest value of the cubic Bézier timing curve while its
/// progress in time runs over `times`, 0 to 1: at those ends, and where the
/// curve turns back at a root of its derivative, which is how it overshoots
/// 0 or 1. Its x handles lie in 0 to 1, as for [`cubic_bezier_progress`].
/// Its y handles may take any finite value, as FX keys accept, and the range
/// is then finite.
fn cubic_bezier_range([x1, y1, x2, y2]: [f64; 4], times: [f64; 2]) -> [f64; 2] {
    let [low, high] = times.map(|time| {
        if time <= 0.0 {
            0.0
        } else if time >= 1.0 {
            1.0
        } else {
            bezier_parameter(x1, x2, time)
        }
    });
    // The derivative over 3 and over `size`: a * u^2 + b * u + c at the
    // curve parameter u. Dividing by `size` keeps the roots and bounds each
    // coefficient by 7, so that the discriminant cannot overflow. The roots
    // are q / a and c / q, whose q adds two terms of one sign, so that no
    // cancellation loses the root near -c / b when a is near 0. A NaN or
    // infinite root, when there is no real root or a is 0, is not between
    // `low` and `high`.
    let size = y1.abs().max(y2.abs()).max(1.0);
    let [first, second] = [y1, y2].map(|handle| handle / size);
    let (a, b, c) = (
        3.0 * (first - second) + 1.0 / size,
        2.0 * (second - 2.0 * first),
        first,
    );
    let q = -(b + (b * b - 4.0 * a * c).sqrt().copysign(b)) / 2.0;
    [low, high]
        .into_iter()
        .chain(
            [q / a, c / q]
                .into_iter()
                .filter(|root| low < *root && *root < high),
        )
        .map(|parameter| bezier_coordinate(y1, y2, parameter))
        .fold(
            [f64::INFINITY, f64::NEG_INFINITY],
            |[least, greatest], value| [least.min(value), greatest.max(value)],
        )
}

/// Static native Levels: the existing master mapping, or bounded RGB selectors
/// with a neutral master. Both forms serialize the same twenty native controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PrLevels {
    Master {
        rgb: [f64; 5],
    },
    Channels([PrLevelChannel; 3]),
    /// Static RGB corrections lowered to ordinary editable channel branches.
    Corrections([[f64; 5]; 3]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrLevelChannel {
    Keep,
    Off,
    On,
}

impl PrLevelChannel {
    pub(crate) fn values(self) -> [f64; 5] {
        match self {
            Self::Keep => LEVELS_NEUTRAL,
            Self::Off => [0.0, 255.0, 0.0, 0.0, 100.0],
            Self::On => [0.0, 255.0, 255.0, 255.0, 100.0],
        }
    }

    pub(crate) fn from_values(values: [f64; 5]) -> Option<Self> {
        [Self::Keep, Self::Off, Self::On]
            .into_iter()
            .find(|channel| channel.values() == values)
    }
}

impl PrLevels {
    /// Visible controls and private little-endian u16 data share this order.
    pub(crate) fn start_values(&self) -> [f64; 20] {
        let rows = match self {
            Self::Master { rgb } => [*rgb, LEVELS_NEUTRAL, LEVELS_NEUTRAL, LEVELS_NEUTRAL],
            Self::Corrections([red, green, blue]) => [LEVELS_NEUTRAL, *red, *green, *blue],
            Self::Channels(channels) => {
                let [red, green, blue] = channels.map(PrLevelChannel::values);
                [LEVELS_NEUTRAL, red, green, blue]
            }
        };
        std::array::from_fn(|index| rows[index / 5][index % 5])
    }

    /// Reject unmeasured forms: input black at or above input white, or output black above
    /// output white, at any time. A keyed level is bounded by its keys, Bézier
    /// overshoot included, so the check is conservative when both levels of
    /// a pair are keyed.
    pub(crate) fn ensure_rendered_form(
        &self,
        animations: &[PrEffectParamAnimation],
    ) -> std::result::Result<(), String> {
        let Self::Master { rgb } = self else {
            return if animations.is_empty() {
                Ok(())
            } else {
                Err("Levels channel selectors must be static".to_owned())
            };
        };
        let range = |index: usize| {
            animations
                .iter()
                .find(|animation| animation.param.id == LEVELS.params[index].id)
                .and_then(|animation| animation.keys.scalar())
                .map_or([rgb[index]; 2], |keys| {
                    let keys: Vec<_> = keys
                        .iter()
                        .map(|key| (key.value, key.easing.bezier()))
                        .collect();
                    scalar_range(&keys)
                })
        };
        let [input_black, input_white, output_black, output_white] = [0, 1, 2, 3].map(range);
        if input_black[1] >= input_white[0] {
            return Err(format!(
                "input black {} reaches input white {}, a Levels form that no Adobe render measured",
                input_black[1], input_white[0]
            ));
        }
        if output_black[1] > output_white[0] {
            return Err(format!(
                "output black {} exceeds output white {}, a Levels form that no Adobe render measured",
                output_black[1], output_white[0]
            ));
        }
        Ok(())
    }
}

/// The least and greatest value that scalar `keys` take at any time: their
/// values, and where a cubic Bézier overshoots them. Each key is its value
/// and the cubic Bézier timing handles that it arrives with, `None` for a
/// Linear or Hold arrival, which stays between the two values.
pub(crate) fn scalar_range(keys: &[(f64, Option<[f64; 4]>)]) -> [f64; 2] {
    let values = keys
        .iter()
        .map(|(value, _)| *value)
        .chain(keys.windows(2).flat_map(|pair| {
            let [(from, _), (to, bezier)] = [pair[0], pair[1]];
            bezier
                .map_or([0.0, 1.0], |handles| {
                    cubic_bezier_range(handles, [0.0, 1.0])
                })
                .map(|progress| from + (to - from) * progress)
        }));
    values.fold(
        [f64::INFINITY, f64::NEG_INFINITY],
        |[least, greatest], value| [least.min(value), greatest.max(value)],
    )
}

impl PrEffect {
    /// Native identity and parameter layout of this effect.
    pub(crate) fn spec(&self) -> &'static EffectSpec {
        match self.params {
            PrEffectParams::GaussianBlur(_) => &GAUSSIAN_BLUR,
            PrEffectParams::FilmImpactBlur(_) => &FILM_IMPACT_BLUR,
            PrEffectParams::CornerPin(_) => &CORNER_PIN,
            PrEffectParams::DirectionalBlur(_) => &DIRECTIONAL_BLUR,
            PrEffectParams::FilmImpactDirectionalBlur(_) => &FILM_IMPACT_DIRECTIONAL_BLUR,
            PrEffectParams::Levels(_) => &LEVELS,
            PrEffectParams::BrightnessContrast(_) => &BRIGHTNESS_CONTRAST,
            PrEffectParams::Offset(_) => &OFFSET,
            PrEffectParams::LumetriExposure(_) => &LUMETRI_EXPOSURE_SPEC,
            PrEffectParams::LumetriTemperature(_) => &LUMETRI_TEMPERATURE_SPEC,
            PrEffectParams::LumetriTint(_) => &LUMETRI_TINT_SPEC,
            PrEffectParams::LumetriSaturation(_) => &LUMETRI_SATURATION_SPEC,
            PrEffectParams::LumetriVignette(_) => &LUMETRI_VIGNETTE_SPEC,
            PrEffectParams::Invert(_) => &INVERT,
            PrEffectParams::FindEdges(_) => &FIND_EDGES,
            PrEffectParams::Tint(_) => &TINT,
            PrEffectParams::BlackWhite => &BLACK_WHITE,
            PrEffectParams::Ramp(_) => &RAMP,
            PrEffectParams::Mosaic(_) => &MOSAIC,
            PrEffectParams::Replicate(_) => &REPLICATE,
            PrEffectParams::Posterize(_) => &POSTERIZE,
            PrEffectParams::Sharpen(_) => &SHARPEN,
            PrEffectParams::AlphaGlow { .. } => &ALPHA_GLOW,
            PrEffectParams::LegacyLuma { .. } => &LEGACY_LUMA_KEY,
            PrEffectParams::LensDistortion(_) => &LENS_DISTORTION,
            PrEffectParams::Noise { .. } => &NOISE,
            PrEffectParams::ModernNoise { .. } => &MODERN_NOISE,

            PrEffectParams::PosterizeTime { .. } => &POSTERIZE_TIME,
            PrEffectParams::Transform(_) => &TRANSFORM,
            PrEffectParams::AdjustmentGeometry2(_) => &ADJUSTMENT_GEOMETRY2,
        }
    }

    /// The keys of `param`, when it is animated.
    pub(crate) fn keys(&self, param: &EffectParamSpec) -> Option<&PrEffectParamKeys> {
        self.animations
            .iter()
            .find(|animation| animation.param.id == param.id)
            .map(|animation| &animation.keys)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if let Some(mask) = &self.mask {
            mask.validate()?;
        }
        let spec = self.spec();
        if matches!(self.params, PrEffectParams::Levels(PrLevels::Channels(_))) {
            ensure_valid!(
                self.animations.is_empty(),
                "Levels channel selectors must be static"
            );
        }
        match &self.params {
            PrEffectParams::LumetriVignette(values) => {
                for (value, param) in values.iter().zip(LUMETRI_VIGNETTE_SPEC.params) {
                    ensure_valid!(param.value_range().is_some_and(|range| range.contains(value)), "Lumetri Vignette value outside saved range");
                }
            }
            PrEffectParams::LumetriExposure(value) | PrEffectParams::LumetriSaturation(value) | PrEffectParams::LumetriTemperature(value) | PrEffectParams::LumetriTint(value) => {
                ensure_valid!(spec.params[0].value_range().is_some_and(|range| range.contains(value)), "Lumetri replacement value is outside its saved control range");
            }
            PrEffectParams::GaussianBlur(blur) => ensure_valid!(
                blur.blurriness.is_finite()
                    && (0.0..=GAUSSIAN_BLUR_MAX_BLURRINESS).contains(&blur.blurriness),
                "Gaussian Blur Blurriness {} is outside Premiere's 0 to {GAUSSIAN_BLUR_MAX_BLURRINESS} range",
                blur.blurriness
            ),
            PrEffectParams::FilmImpactBlur(blur) => ensure_valid!(
                FILM_IMPACT_BLUR_AMOUNT
                    .value_range()
                    .is_some_and(|range| range.contains(&blur.amount)),
                "Gaussian Blur Amount {} is outside Premiere's {} to {} range",
                blur.amount,
                FILM_IMPACT_BLUR_AMOUNT.lower_bound,
                FILM_IMPACT_BLUR_AMOUNT.upper_bound
            ),
            PrEffectParams::Offset(center) => ensure_valid!(center.iter().all(|value| value.is_finite()), "Offset center must be finite"),
            PrEffectParams::CornerPin(pin) => ensure_valid!(
                pin.corners.iter().flatten().all(|value| value.is_finite()),
                "Corner Pin corners must be finite"
            ),
            // Two scalars in native `Params` order.
            PrEffectParams::DirectionalBlur(PrDirectionalBlur {
                direction: first,
                blur_length: second,
            })
            | PrEffectParams::BrightnessContrast(PrBrightnessContrast {
                brightness: first,
                contrast: second,
            }) => {
                for (param, value) in spec.params.iter().zip([first, second]) {
                    ensure_valid!(
                        param
                            .value_range()
                            .is_some_and(|range| range.contains(value)),
                        "{} {} {value} is outside Premiere's {} to {} range",
                        spec.display_name,
                        param.label,
                        param.lower_bound,
                        param.upper_bound
                    );
                }
            }
            PrEffectParams::FilmImpactDirectionalBlur(blur) => {
                for (param, value) in [
                    (&FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE, blur.angle),
                    (&FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT, blur.amount),
                ] {
                    ensure_valid!(
                        param
                            .value_range()
                            .is_some_and(|range| range.contains(&value)),
                        "{} {} {value} is outside Premiere's {} to {} range",
                        spec.display_name,
                        param.label,
                        param.lower_bound,
                        param.upper_bound
                    );
                }
            }
            PrEffectParams::Levels(levels) => ensure_valid!(
                LEVELS.params.iter().zip(levels.start_values()).all(|(param, value)| param
                    .value_range()
                    .is_some_and(|range| range.contains(&value))),
                "Levels values {:?} are outside Premiere's ranges",
                levels.start_values()
            ),
            PrEffectParams::Invert(PrInvert { blend, .. }) => ensure_valid!(
                INVERT_BLEND
                    .value_range()
                    .is_some_and(|range| range.contains(blend)),
                "{} {} {blend} is outside Premiere's {} to {} range",
                spec.display_name,
                INVERT_BLEND.label,
                INVERT_BLEND.lower_bound,
                INVERT_BLEND.upper_bound
            ),
            // The colours are 8-bit by construction.
            PrEffectParams::Tint(PrTint { amount, .. }) => ensure_valid!(
                TINT_AMOUNT
                    .value_range()
                    .is_some_and(|range| range.contains(amount)),
                "{} {} {amount} is outside Premiere's {} to {} range",
                spec.display_name,
                TINT_AMOUNT.label,
                TINT_AMOUNT.lower_bound,
                TINT_AMOUNT.upper_bound
            ),
            PrEffectParams::FindEdges(edges) => {
                ensure_valid!(
                    FIND_EDGES_BLEND
                        .value_range()
                        .is_some_and(|range| range.contains(&edges.blend)),
                    "Find Edges Blend With Original must be from 0 to 1"
                );
                ensure_valid!(
                    self.animations.is_empty(),
                    "Find Edges controls have no keyframe mapping"
                );
            }
            PrEffectParams::ModernNoise { amount, seed } => {
                ensure_valid!(
                    (0.0..=100.0).contains(amount),
                    "invalid Modern Noise intensity"
                );
                ensure_valid!((0.0..=99999.0).contains(seed), "invalid Modern Noise seed");
            }
            PrEffectParams::Noise { amount } => ensure_valid!(
                NOISE_AMOUNT
                    .value_range()
                    .is_some_and(|range| range.contains(amount)),
                "Noise Amount of Noise must be finite and within 0 to 100"
            ),
            PrEffectParams::BlackWhite => {}
            // The colours are 8-bit by construction; the axis rule is the
            // reader's and the exporter's (`PrRamp::ensure_aligned`).
            PrEffectParams::Ramp(PrRamp {
                start, end, blend, ..
            }) => {
                ensure_valid!(
                    start.iter().chain(end).all(|value| value.is_finite()),
                    "Ramp points must be finite"
                );
                ensure_valid!(
                    RAMP_BLEND
                        .value_range()
                        .is_some_and(|range| range.contains(blend)),
                    "{} {} {blend} is outside Premiere's {} to {} range",
                    spec.display_name,
                    RAMP_BLEND.label,
                    RAMP_BLEND.lower_bound,
                    RAMP_BLEND.upper_bound
                );
            }
            // Whole counts by construction; Sharp Colors and the Hold-only
            // keys are the reader's and the exporter's (`PrMosaic::ensure_convertible`).
            PrEffectParams::Mosaic(PrMosaic {
                horizontal,
                vertical,
                ..
            }) => {
                for (param, value) in [
                    (&MOSAIC_HORIZONTAL_BLOCKS, horizontal),
                    (&MOSAIC_VERTICAL_BLOCKS, vertical),
                ] {
                    ensure_valid!(
                        param
                            .value_range()
                            .is_some_and(|range| range.contains(&f64::from(*value))),
                        "{} {} {value} is outside Premiere's {} to {} range",
                        spec.display_name,
                        param.label,
                        param.lower_bound,
                        param.upper_bound
                    );
                }
            }
            // A whole Count by construction; the Hold-only keys are the
            // reader's and the exporter's (`PrReplicate::new`).
            PrEffectParams::Replicate(PrReplicate { count }) => ensure_valid!(
                REPLICATE_COUNT
                    .value_range()
                    .is_some_and(|range| range.contains(&f64::from(*count))),
                "{} {} {count} is outside Premiere's {} to {} range",
                spec.display_name,
                REPLICATE_COUNT.label,
                REPLICATE_COUNT.lower_bound,
                REPLICATE_COUNT.upper_bound
            ),
            PrEffectParams::Sharpen(sharpen) => {
                ensure_valid!(
                    PrSharpen::new(f64::from(sharpen.amount), &self.animations).is_ok(),
                    "Sharpen requires integer Amount 0 to 4000 and Linear/Hold keys"
                );
            }
            PrEffectParams::AlphaGlow { size, brightness, .. } => {
                alpha_glow_size(*size, &self.animations).map_err(invalid)?;
                ensure_valid!((0.0..=255.0).contains(brightness) && brightness.fract() == 0.0, "Alpha Glow Brightness must be whole in 0..255");
            }
            PrEffectParams::LegacyLuma { threshold, cutoff } => {
                validate_legacy_luma(*threshold, *cutoff, &self.animations).map_err(invalid)?;
            }
            PrEffectParams::LensDistortion(curvature) => {
                lens_curvature(*curvature, &self.animations).map_err(invalid)?;
            }
            // A whole Level by construction; the Hold-only keys are the
            // reader's and the exporter's (`PrPosterize::new`).
            PrEffectParams::Posterize(PrPosterize { level }) => ensure_valid!(
                POSTERIZE_LEVEL
                    .value_range()
                    .is_some_and(|range| range.contains(&f64::from(*level))),
                "{} {} {level} is outside Premiere's {} to {} range",
                spec.display_name,
                POSTERIZE_LEVEL.label,
                POSTERIZE_LEVEL.lower_bound,
                POSTERIZE_LEVEL.upper_bound
            ),
            PrEffectParams::PosterizeTime { frame_rate } => ensure_valid!(
                POSTERIZE_TIME_FRAME_RATE
                    .value_range()
                    .is_some_and(|range| range.contains(frame_rate)),
                "Posterize Time Frame Rate {frame_rate} is outside Premiere's range"
            ),
            // The skew key rule is the reader's and the exporter's
            // (`PrTransform::ensure_convertible`).
            PrEffectParams::Transform(transform) | PrEffectParams::AdjustmentGeometry2(transform) => {
                ensure_valid!(
                    transform
                        .anchor_point
                        .iter()
                        .chain(&transform.position)
                        .all(|value| value.is_finite()),
                    "Transform points must be finite"
                );
                for (param, value) in [
                    (&TRANSFORM_SCALE_HEIGHT, transform.scale_height),
                    (&TRANSFORM_SCALE_WIDTH, transform.scale_width),
                    (&TRANSFORM_SKEW, transform.skew),
                    (&TRANSFORM_SKEW_AXIS, transform.skew_axis),
                    (&TRANSFORM_ROTATION, transform.rotation),
                    (&TRANSFORM_OPACITY, transform.opacity),
                    (&TRANSFORM_SHUTTER_ANGLE, transform.shutter_angle),
                ] {
                    ensure_valid!(
                        param
                            .value_range()
                            .is_some_and(|range| range.contains(&value)),
                        "{} {} {value} is outside Premiere's {} to {} range",
                        spec.display_name,
                        param.label,
                        param.lower_bound,
                        param.upper_bound
                    );
                }
            }
        }
        let mut keyed = BTreeSet::new();
        for animation in &self.animations {
            let param = animation.param;
            ensure_valid!(
                param.binding.is_some() && spec.params.contains(param),
                "{} {} has no keyframe mapping",
                spec.display_name,
                param.label
            );
            ensure_valid!(
                keyed.insert(param.id),
                "duplicate {} {} animation",
                spec.display_name,
                param.label
            );
            match (&animation.keys, param.binding) {
                (
                    PrEffectParamKeys::Scalar(keys),
                    Some(
                        EffectParamBinding::Scalar(_)
                        | EffectParamBinding::ScaledScalar { .. }
                        | EffectParamBinding::Integer { .. }
                        | EffectParamBinding::TileCount { .. },
                    ),
                ) => {
                    let range = param.value_range();
                    ensure_valid!(
                        !keys.is_empty()
                            && keys.iter().all(|key| {
                                range
                                    .as_ref()
                                    .is_some_and(|range| range.contains(&key.value))
                            })
                            && keys
                                .windows(2)
                                .all(|pair| pair[0].source_ticks < pair[1].source_ticks),
                        "{} {} keys must be one or more values from {} to {} at strictly increasing source times",
                        spec.display_name,
                        param.label,
                        param.lower_bound,
                        param.upper_bound
                    );
                }
                (PrEffectParamKeys::Point(keys), Some(EffectParamBinding::Point { .. })) => {
                    ensure_valid!(
                        !keys.is_empty()
                            && keys
                                .iter()
                                .all(|key| key.value.iter().all(|value| value.is_finite()))
                            && keys
                                .windows(2)
                                .all(|pair| pair[0].source_ticks < pair[1].source_ticks),
                        "{} {} keys must be one or more finite points at strictly increasing source times",
                        spec.display_name,
                        param.label
                    );
                }
                // The values are 8-bit colours by construction.
                (PrEffectParamKeys::Colour(keys), Some(EffectParamBinding::Colour { .. })) => {
                    ensure_valid!(
                        !keys.is_empty()
                            && keys
                                .windows(2)
                                .all(|pair| pair[0].source_ticks < pair[1].source_ticks),
                        "{} {} keys must be one or more colours at strictly increasing source times",
                        spec.display_name,
                        param.label
                    );
                    ensure_valid!(
                        keys.iter().all(|key| !matches!(
                            key.easing,
                            PrKeyframeEasing::CubicBezier { .. }
                        )),
                        "{} {} keys must be Linear or Hold; Premiere's Bezier interpolation between colours is unverified",
                        spec.display_name,
                        param.label
                    );
                }
                _ => {
                    return Err(invalid(format!(
                        "{} {} keys are not of its binding's kind",
                        spec.display_name, param.label
                    )))
                }
            }
        }
        Ok(())
    }
}

/// Native identity and writer layout of one supported standard effect.
///
/// The reader identifies an effect by `match_name` and its parameters by
/// `ParameterID` and `Name`; it does not check record versions. The writer
/// emits the other fields in the record generation of the intrinsic Motion
/// writer (`VideoFilterComponent` 7 with `Component`
/// [`records::VIDEO_FILTER_COMPONENT_BODY_VERSION`]). For standard effects that
/// generation is inferred: Premiere's own generation varies by version (9/7 in
/// 26.5.1), and Premiere 26.5.1 reopens exported Gaussian Blurs, which AME
/// renders (the `premiere_isolated_gaussian_blur_keys_26_5` export gate).
#[derive(Debug)]
pub(crate) struct EffectSpec {
    pub(crate) match_name: &'static str,
    /// English Premiere name written on export. The reader does not require
    /// it, because Premiere localizes display names.
    pub(crate) display_name: &'static str,
    pub(crate) filter_type: &'static str,
    /// Parameters in native `Params` order.
    pub(crate) params: &'static [EffectParamSpec],
    /// A Premiere-native filter in the form Premiere 26.5.1 saves `PR.ADBE
    /// Levels`: records [`PREMIERE_NATIVE_FILTER_VERSIONS`],
    /// every `ParameterID` [`PREMIERE_NATIVE_PARAMETER_ID`], so parameters are
    /// identified by `Name`, a `PremiereFilterPrivateData`, and no `Bypass` or
    /// `Intrinsic`, whose form on such a filter is unverified.
    /// Lens uses the same native IDs/versions but resolves slots by Index and
    /// does not interpret or author its unestablished private data.
    /// Other effects number their parameters from 1.
    pub(crate) premiere_native: bool,
    /// Whether the record carries a `PremiereFilterPrivateData` that holds no
    /// parameter data, which the reader accepts and ignores and the writer
    /// does not write: Invert's is uninitialized process memory, saved once
    /// and named by `BinaryHash` from the other records. A
    /// Premiere-native filter's private data holds its values instead.
    pub(crate) opaque_private_data: bool,
}

/// One native effect parameter, as the corpus Gaussian Blur records (saved by
/// Premiere 14.4) serialize it, a point parameter as the corpus Corner Pin
/// records (Premiere 12.1) do, or a Directional Blur or Brightness & Contrast
/// parameter as its corpus records (Premiere 12.1 and 14.4) do.
#[derive(Debug, PartialEq)]
pub(crate) struct EffectParamSpec {
    pub(crate) id: usize,
    pub(crate) name: &'static str,
    /// Effect Controls label, used in messages. Checkbox parameters have a
    /// blank native `name`.
    pub(crate) label: &'static str,
    pub(crate) record: XmlRecordDefinition,
    pub(crate) control: &'static str,
    /// Native `LowerBound` and `UpperBound`. Both are empty for a point
    /// parameter, whose `PointComponentParam` record has neither.
    pub(crate) lower_bound: &'static str,
    pub(crate) upper_bound: &'static str,
    /// Native `LowerUIBound` and `UpperUIBound`, the Effect Controls slider
    /// range, where the native records carry them.
    pub(crate) lower_ui_bound: Option<&'static str>,
    pub(crate) upper_ui_bound: Option<&'static str>,
    /// Popup parameters step between keys instead of interpolating.
    pub(crate) discontinuous_interpolate: bool,
    /// The FX parameter that this parameter's keys animate. Keys on a
    /// parameter without a binding omit the effect.
    pub(crate) binding: Option<EffectParamBinding>,
}

/// How the keys of a native effect parameter map to an FX effect parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum EffectParamBinding {
    /// Scalar keys on the FX parameter of this name, the `paramName` of an
    /// `effectProperty` animation target.
    Scalar(&'static str),
    /// Scalar keys on the FX parameter `name` whose FX values are `multiplier`
    /// times the native values, without rounding: Film Impact Amount 20 is FX
    /// `blurriness` 114.
    ScaledScalar { name: &'static str, multiplier: f64 },
    /// Point keys whose coordinates are keys on the FX scalar parameters `x`
    /// and `y`, at the same times and with the same easing.
    Point { x: &'static str, y: &'static str },
    /// Scalar keys on the FX parameter `name` whose native values are whole
    /// numbers, `divisor` times the FX values: Levels Gamma 150 is FX 1.5.
    /// Export rounds to the nearest whole native value.
    Integer { name: &'static str, divisor: u16 },
    /// Colour keys ([`PrColour`]) whose channels are keys on the FX scalar
    /// parameters `red`, `green` and `blue`, each a share of 255, at the same
    /// times and with the same easing.
    Colour {
        red: &'static str,
        green: &'static str,
        blue: &'static str,
    },
    /// Scalar keys of a whole Replicate Count whose FX values are its grid
    /// ([`PrReplicate`]): keys on the FX scalar parameters `width` and
    /// `height` of [`PrReplicate::tile_size`] and on `center_x` and
    /// `center_y` of [`PrReplicate::tile_center`], at the same times and with
    /// the same easing.
    TileCount {
        width: &'static str,
        height: &'static str,
        center_x: &'static str,
        center_y: &'static str,
    },
}

impl EffectParamBinding {
    /// The FX parameters that this binding's keys animate.
    pub(crate) fn fx_params(self) -> Vec<&'static str> {
        match self {
            Self::Scalar(name) | Self::ScaledScalar { name, .. } | Self::Integer { name, .. } => {
                vec![name]
            }
            Self::Point { x, y } => vec![x, y],
            Self::Colour { red, green, blue } => vec![red, green, blue],
            Self::TileCount {
                width,
                height,
                center_x,
                center_y,
            } => vec![width, height, center_x, center_y],
        }
    }
}

impl EffectSpec {
    /// The parameter whose keys animate the FX parameter `fx_param`.
    pub(crate) fn bound_param(&self, fx_param: &str) -> Option<&'static EffectParamSpec> {
        self.params.iter().find(|param| {
            param
                .binding
                .is_some_and(|binding| binding.fx_params().contains(&fx_param))
        })
    }
}

impl EffectParamSpec {
    /// Whether a native parameter's `Name` is this parameter's: equal to the
    /// spec name, or blank or absent for a spec whose name is blank, the
    /// checkbox forms: Premiere 26.5.1 saves Mosaic's Sharp Colors without
    /// the element and the corpus Gaussian Blur checkbox has
    /// `<Name> </Name>`. The writer writes the spec name when it is not empty.
    pub(crate) fn accepts_name(&self, name: Option<&str>) -> bool {
        name == Some(self.name)
            || (self.name.trim().is_empty() && name.is_none_or(|name| name.trim().is_empty()))
    }

    /// Native `LowerBound` to `UpperBound`, when both are numbers.
    pub(crate) fn value_range(&self) -> Option<RangeInclusive<f64>> {
        Some(self.lower_bound.parse().ok()?..=self.upper_bound.parse().ok()?)
    }

    /// The FX value of the native `value`.
    pub(crate) fn fx_value(&self, value: f64) -> f64 {
        match self.binding {
            Some(EffectParamBinding::Integer { divisor, .. }) => value / f64::from(divisor),
            // Rounding to 1e-9 reimports an exported FX value unchanged: its
            // native value, FX / multiplier, is inexact in f64.
            Some(EffectParamBinding::ScaledScalar { multiplier, .. }) => {
                (value * multiplier * 1e9).round() / 1e9
            }
            _ => value,
        }
    }

    /// The unrounded native value of the FX `value`, or `None` when it is
    /// outside [`Self::value_range`]: never clamped.
    pub(crate) fn native_value(&self, value: f64) -> Option<f64> {
        let native = match self.binding {
            Some(EffectParamBinding::Integer { divisor, .. }) => value * f64::from(divisor),
            Some(EffectParamBinding::ScaledScalar { multiplier, .. }) => value / multiplier,
            _ => value,
        };
        self.value_range()?.contains(&native).then_some(native)
    }

    /// The native `value` as export writes it: the nearest whole number for an
    /// [`EffectParamBinding::Integer`] parameter.
    pub(crate) fn written_value(&self, value: f64) -> f64 {
        match self.binding {
            Some(EffectParamBinding::Integer { .. }) => value.round(),
            _ => value,
        }
    }
}

/// A converted clip keeps its effects on one side of its Crop, Linear Wipe or
/// Track Matte Key: applying after the mask on its video layer, or before it
/// under a stage group. Effects on both sides of it have no ordered
/// representation in FX.
pub(crate) const MASK_EFFECT_ORDER_REASON: &str = "converted effects are on both sides of the Crop, Linear Wipe or Track Matte Key in the native chain; one FX mask keeps the effects of only one side in order";

/// Blur Dimensions popup value for Horizontal and Vertical, the only mode FX
/// `gaussianBlur` renders.
pub(crate) const BLUR_DIMENSIONS_HORIZONTAL_AND_VERTICAL: &str = "0";

/// Native Blurriness `UpperBound`.
pub(crate) const GAUSSIAN_BLUR_MAX_BLURRINESS: f64 = 30000.0;

/// Gaussian Blur Blurriness, the only blur parameter that the corpus keys.
/// The written static record shape (`IsTimeVarying` false, no `Keyframes`) is
/// inferred from the blur's static Blur Dimensions and Repeat Edge Pixels
/// records.
pub(crate) const GAUSSIAN_BLUR_BLURRINESS: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "Blurriness",
    label: "Blurriness",
    record: records::VIDEO_FILTER_AMOUNT_PARAM,
    control: "8",
    lower_bound: "0",
    upper_bound: "30000",
    lower_ui_bound: None,
    upper_ui_bound: Some("50"),
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::Scalar("blurriness")),
};

/// Gaussian Blur Blur Dimensions popup.
pub(crate) const GAUSSIAN_BLUR_DIMENSIONS: EffectParamSpec = EffectParamSpec {
    id: 2,
    name: "Blur Dimensions",
    label: "Blur Dimensions",
    record: records::VIDEO_POPUP_PARAM,
    control: "7",
    lower_bound: "0",
    upper_bound: "2",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: true,
    binding: None,
};

/// Gaussian Blur Repeat Edge Pixels checkbox.
pub(crate) const GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS: EffectParamSpec = EffectParamSpec {
    id: 3,
    name: " ",
    label: "Repeat Edge Pixels",
    record: records::VIDEO_BOOL_COMPONENT_PARAM,
    control: "4",
    lower_bound: "false",
    upper_bound: "true",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: None,
};

/// `AE.ADBE Gaussian Blur 2`. The identity and parameter layout are the same
/// in the corpus Gaussian Blurs and in Premiere 26.5.1, which names it
/// "Gaussian Blur (Legacy)". Premiere 26's default "Gaussian Blur" is Film
/// Impact's [`FILM_IMPACT_BLUR`].
pub(crate) const GAUSSIAN_BLUR: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Gaussian Blur 2",
    display_name: "Gaussian Blur",
    filter_type: "2",
    params: &[
        GAUSSIAN_BLUR_BLURRINESS,
        GAUSSIAN_BLUR_DIMENSIONS,
        GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS,
    ],
    premiere_native: false,
    opaque_private_data: false,
};

/// FX `blurriness` per Film Impact Amount, so that FX `blurriness` keeps its
/// Legacy (AE) meaning. On the pinned chart at 1080p, AME renders Amount `a`
/// with the edge spread of Legacy Blurriness 5.63a to 5.70a (gamma-2.4 fits)
/// or 5.78a to 5.81a (encoded fits) for Amount 5 to 50
/// (case `premiere_isolated_film_impact_blur_26_5`). The kernels
/// differ, and resolution scaling is unmeasured.
pub(crate) const FILM_IMPACT_BLURRINESS_PER_AMOUNT: f64 = 5.7;

/// A Film Impact parameter record as Premiere 26.5.1 saves it: version 10 of
/// `record`. The writer omits an empty name, control type or bound.
const fn film_impact_param(
    id: usize,
    name: &'static str,
    record: XmlRecordDefinition,
    control: &'static str,
    lower_bound: &'static str,
    upper_bound: &'static str,
) -> EffectParamSpec {
    EffectParamSpec {
        id,
        name,
        label: name,
        record: XmlRecordDefinition {
            version: "10",
            ..record
        },
        control,
        lower_bound,
        upper_bound,
        lower_ui_bound: None,
        upper_ui_bound: None,
        discontinuous_interpolate: false,
        binding: None,
    }
}

const fn film_impact_flag(id: usize, name: &'static str, control: &'static str) -> EffectParamSpec {
    film_impact_param(
        id,
        name,
        records::VIDEO_BOOL_COMPONENT_PARAM,
        control,
        "",
        "",
    )
}

const fn film_impact_number(
    id: usize,
    name: &'static str,
    lower_bound: &'static str,
    upper_bound: &'static str,
) -> EffectParamSpec {
    film_impact_param(
        id,
        name,
        records::VIDEO_FILTER_AMOUNT_PARAM,
        "",
        lower_bound,
        upper_bound,
    )
}

const fn film_impact_popup(id: usize, name: &'static str) -> EffectParamSpec {
    EffectParamSpec {
        discontinuous_interpolate: true,
        ..film_impact_param(id, name, records::VIDEO_POPUP_PARAM, "", "0", "2")
    }
}

pub(crate) const FILM_IMPACT_BLUR_SEED: EffectParamSpec =
    film_impact_number(8041, "Seed", "0", "99999");
pub(crate) const FILM_IMPACT_BLUR_ANGLE: EffectParamSpec = film_impact_param(
    2,
    "Angle",
    records::VIDEO_COMPONENT_PARAM,
    "3",
    "-32768",
    "32767",
);
pub(crate) const FILM_IMPACT_BLUR_AMOUNT: EffectParamSpec = EffectParamSpec {
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "blurriness",
        multiplier: FILM_IMPACT_BLURRINESS_PER_AMOUNT,
    }),
    ..film_impact_number(3, "Amount", "0", "1000")
};
pub(crate) const FILM_IMPACT_BLUR_THICKNESS: EffectParamSpec =
    film_impact_number(4, "Thickness", "0", "1000");
pub(crate) const FILM_IMPACT_BLUR_UNIFORM: EffectParamSpec =
    film_impact_flag(5, "Uniform Blur", "");
pub(crate) const FILM_IMPACT_BLUR_CHROMATIC: EffectParamSpec =
    film_impact_number(6, "Chromatic Aberration", "0", "100");
pub(crate) const FILM_IMPACT_BLUR_EDGE: EffectParamSpec = film_impact_popup(8, "Edge Behavior");
pub(crate) const FILM_IMPACT_BLUR_APPLIED_VERSION: EffectParamSpec =
    film_impact_number(8140, "_ Applied Version", "0", "999999");

/// The parameters of a Film Impact effect in the order Premiere 26.5.1 saves
/// them: its `controls` between the hidden parameters that both Film Impact
/// blurs save alike. The `@26_2` form lacks the hidden
/// ParameterIDs 8300 and 8301, as Film Impact 26.2 saves its blur.
macro_rules! film_impact_params {
    ($($control:expr),+ $(,)?) => {
        film_impact_params!(@layout [
            film_impact_number(8300, "", "0", "16777215"),
            film_impact_number(8301, "", "0", "16777215"),
        ] $($control),+)
    };
    (@26_2 $($control:expr),+ $(,)?) => {
        film_impact_params!(@layout [] $($control),+)
    };
    (@layout [$($later:expr),* $(,)?] $($control:expr),+) => {
        &[
            film_impact_flag(8100, "Error occurred", "16"),
            EffectParamSpec {
                upper_bound: "false",
                ..film_impact_flag(1, "Controls", "11")
            },
            film_impact_flag(8040, "", "16"),
            $($control,)+
            film_impact_flag(8240, "", "16"),
            EffectParamSpec {
                upper_bound: "false",
                ..film_impact_flag(7, "Controls", "12")
            },
            film_impact_popup(8280, "_ Overlay Mode"),
            film_impact_flag(8281, "_ Overlay Info", ""),
            film_impact_flag(8141, "", "16"),
            FILM_IMPACT_BLUR_APPLIED_VERSION,
            $($later,)*
            film_impact_flag(9020, "_ Overlay Enabled", ""),
            film_impact_number(9040, "_ Sequence Width", "-1", "1000000000"),
            film_impact_number(9041, "_ Sequence Height", "-1", "1000000000"),
            film_impact_number(9042, "_ Sequence Pixel Ratio", "-1", "1000000000"),
        ]
    };
}

/// Premiere 26.5.1's current "Gaussian Blur", Film Impact `AE.Impact_Blur_FX`,
/// in the 9/7 form it saves: 22 parameters, of which 14 are hidden. With
/// Uniform Blur on, it blurs by Amount; Thickness and Seed leave that blur
/// unchanged, and Angle 45 widens it by 3 % (probe
/// `premiere_isolated_film_impact_blur_26_5`). With Uniform Blur off, Amount
/// blurs along Angle and Thickness across it (at Angle 0, vertically and
/// horizontally).
pub(crate) const FILM_IMPACT_BLUR: EffectSpec = EffectSpec {
    match_name: "AE.Impact_Blur_FX",
    display_name: "Gaussian Blur",
    filter_type: "2",
    params: film_impact_params![
        FILM_IMPACT_BLUR_SEED,
        FILM_IMPACT_BLUR_ANGLE,
        FILM_IMPACT_BLUR_AMOUNT,
        FILM_IMPACT_BLUR_THICKNESS,
        FILM_IMPACT_BLUR_UNIFORM,
        FILM_IMPACT_BLUR_CHROMATIC,
        FILM_IMPACT_BLUR_EDGE,
    ],
    premiere_native: false,
    opaque_private_data: false,
};

/// The static values of Premiere 26.5.1's default current Gaussian Blur, in
/// `Params` order. Export writes them with its Amount and Edge Behavior.
pub(crate) const FILM_IMPACT_BLUR_DEFAULTS: [&str; 22] = [
    "false", "false", "false", "0.", "0.", "20.", "20.", "true", "0.", "1", "false", "false", "0",
    "false", "false", "260501.", "0.", "0.", "false", "-1.", "-1.", "-1.",
];

/// [`FILM_IMPACT_BLUR`] as Film Impact 26.2 saves it (`_ Applied Version`
/// 260200): the same 9/7 record and parameter identities without the hidden
/// ParameterIDs 8300 and 8301.
/// Import reads it as the 26.5.1 blur; export writes only the 26.5.1 layout.
pub(crate) const FILM_IMPACT_BLUR_26_2: EffectSpec = EffectSpec {
    params: film_impact_params![
        @26_2
        FILM_IMPACT_BLUR_SEED,
        FILM_IMPACT_BLUR_ANGLE,
        FILM_IMPACT_BLUR_AMOUNT,
        FILM_IMPACT_BLUR_THICKNESS,
        FILM_IMPACT_BLUR_UNIFORM,
        FILM_IMPACT_BLUR_CHROMATIC,
        FILM_IMPACT_BLUR_EDGE,
    ],
    ..FILM_IMPACT_BLUR
};

/// [`FILM_IMPACT_BLUR_DEFAULTS`] in [`FILM_IMPACT_BLUR_26_2`] order, without
/// the values of ParameterIDs 8300 and 8301 and with the 26.2 `_ Applied
/// Version` 260200. Import compares no stamp: it is one of the
/// [`FILM_IMPACT_BLUR_CONTROLS`].
pub(crate) const FILM_IMPACT_BLUR_26_2_DEFAULTS: [&str; 20] = [
    "false", "false", "false", "0.", "0.", "20.", "20.", "true", "0.", "1", "false", "false", "0",
    "false", "false", "260200.", "false", "-1.", "-1.", "-1.",
];

/// The parameters that import reads or ignores in either Film Impact blur.
/// Every other parameter must have its default value, because no probe shows
/// that another value leaves the picture unchanged. Applied Version stamps the
/// applying release.
pub(crate) const FILM_IMPACT_BLUR_CONTROLS: [&EffectParamSpec; 8] = [
    &FILM_IMPACT_BLUR_SEED,
    &FILM_IMPACT_BLUR_ANGLE,
    &FILM_IMPACT_BLUR_AMOUNT,
    &FILM_IMPACT_BLUR_THICKNESS,
    &FILM_IMPACT_BLUR_UNIFORM,
    &FILM_IMPACT_BLUR_CHROMATIC,
    &FILM_IMPACT_BLUR_EDGE,
    &FILM_IMPACT_BLUR_APPLIED_VERSION,
];

/// A point parameter (a Corner Pin corner, a Ramp endpoint) whose keys move
/// the FX scalars `x` and `y`. The written record follows the corpus Corner
/// Pin and Ramp records of Premiere 12.1 (`PointComponentParam` with
/// `ParameterControlType` 6, the Motion writer's generation); Premiere 26.5.1
/// saves version 4 without the control type.
const fn point_param(
    id: usize,
    name: &'static str,
    x: &'static str,
    y: &'static str,
) -> EffectParamSpec {
    EffectParamSpec {
        id,
        name,
        label: name,
        record: records::POINT_COMPONENT_PARAM,
        control: "6",
        lower_bound: "",
        upper_bound: "",
        lower_ui_bound: None,
        upper_ui_bound: None,
        discontinuous_interpolate: false,
        binding: Some(EffectParamBinding::Point { x, y }),
    }
}

/// Selected native Offset control; import-only replacement metadata.
/// Blend With Original is deliberately ignored, not serialized from this spec.
pub(crate) const OFFSET_CENTER: EffectParamSpec =
    point_param(1, "Shift Center To", "tileCenterX", "tileCenterY");
pub(crate) const OFFSET: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Offset",
    display_name: "Offset",
    filter_type: "2",
    params: &[OFFSET_CENTER],
    premiere_native: false,
    opaque_private_data: false,
};

/// `AE.ADBE Corner Pin`, the only corner-pin effect in the corpus (Premiere
/// 12.1, 7/5 records) and in Premiere 26.5.1 (9/7 records),
/// with the same four parameters. Its corner defaults are 0:0, 1:0, 0:1
/// and 1:1.
pub(crate) const CORNER_PIN: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Corner Pin",
    display_name: "Corner Pin",
    filter_type: "2",
    params: &[
        point_param(1, "Upper Left", "upperLeftX", "upperLeftY"),
        point_param(2, "Upper Right", "upperRightX", "upperRightY"),
        point_param(3, "Lower Left", "lowerLeftX", "lowerLeftY"),
        point_param(4, "Lower Right", "lowerRightX", "lowerRightY"),
    ],
    premiere_native: false,
    opaque_private_data: false,
};

/// `VideoFilterComponent` and `Component` versions of a Premiere-native
/// filter, as Premiere 26.5.1 saves `PR.ADBE Levels`.
pub(crate) const PREMIERE_NATIVE_FILTER_VERSIONS: [&str; 2] = ["9", "7"];

/// The `ParameterID` of every parameter of a Premiere-native filter.
pub(crate) const PREMIERE_NATIVE_PARAMETER_ID: &str = "-1";

/// The neutral value of each Levels row, in native `Params` order: black
/// input 0, white input 255, black output 0, white output 255 and Gamma 100
/// (1.0), Premiere's defaults.
pub(crate) const LEVELS_NEUTRAL: [f64; 5] = [0.0, 255.0, 0.0, 255.0, 100.0];

/// A Levels parameter as Premiere 26.5.1 saves it: `VideoComponentParam`
/// version 10 with `ParameterControlType` 1 and whole-number values. The ids
/// number the parameters in `Params` order; the records store
/// [`PREMIERE_NATIVE_PARAMETER_ID`].
const fn levels_param(
    id: usize,
    name: &'static str,
    upper_bound: &'static str,
    binding: Option<EffectParamBinding>,
) -> EffectParamSpec {
    EffectParamSpec {
        id,
        name,
        label: name,
        record: XmlRecordDefinition::new(
            "VideoComponentParam",
            "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
            "10",
        ),
        control: "1",
        lower_bound: "0",
        upper_bound,
        lower_ui_bound: None,
        upper_ui_bound: None,
        discontinuous_interpolate: false,
        binding,
    }
}

/// A master (RGB) level, a whole number from 0 to 255 in native and FX units.
const fn levels_master(id: usize, name: &'static str, fx_param: &'static str) -> EffectParamSpec {
    levels_param(
        id,
        name,
        "255",
        Some(EffectParamBinding::Integer {
            name: fx_param,
            divisor: 1,
        }),
    )
}

/// `PR.ADBE Levels`, Premiere's own Levels (Premiere 26.5.1):
/// the (RGB), (R), (G) and (B) rows of Black Input Level, White Input Level,
/// Black Output Level, White Output Level and Gamma. Only the master (RGB)
/// row converts; FX `levels` has no per-channel rows.
pub(crate) const LEVELS: EffectSpec = EffectSpec {
    match_name: "PR.ADBE Levels",
    display_name: "Levels",
    filter_type: "1",
    params: &[
        levels_master(1, "(RGB) Black Input Level", "inputBlack"),
        levels_master(2, "(RGB) White Input Level", "inputWhite"),
        levels_master(3, "(RGB) Black Output Level", "outputBlack"),
        levels_master(4, "(RGB) White Output Level", "outputWhite"),
        // Native Gamma is FX gamma in hundredths: 150 is an exponent of 1 / 1.5
        // on encoded values.
        levels_param(
            5,
            "(RGB) Gamma",
            "1000",
            Some(EffectParamBinding::Integer {
                name: "gamma",
                divisor: 100,
            }),
        ),
        levels_param(6, "(R) Black Input Level", "255", None),
        levels_param(7, "(R) White Input Level", "255", None),
        levels_param(8, "(R) Black Output Level", "255", None),
        levels_param(9, "(R) White Output Level", "255", None),
        levels_param(10, "(R) Gamma", "1000", None),
        levels_param(11, "(G) Black Input Level", "255", None),
        levels_param(12, "(G) White Input Level", "255", None),
        levels_param(13, "(G) Black Output Level", "255", None),
        levels_param(14, "(G) White Output Level", "255", None),
        levels_param(15, "(G) Gamma", "1000", None),
        levels_param(16, "(B) Black Input Level", "255", None),
        levels_param(17, "(B) White Input Level", "255", None),
        levels_param(18, "(B) Black Output Level", "255", None),
        levels_param(19, "(B) White Output Level", "255", None),
        levels_param(20, "(B) Gamma", "1000", None),
    ],
    premiere_native: true,
    opaque_private_data: false,
};

/// Directional Blur Direction, an angle control (`ParameterControlType` 3).
pub(crate) const DIRECTIONAL_BLUR_DIRECTION: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "Direction",
    label: "Direction",
    record: records::VIDEO_COMPONENT_PARAM,
    control: "3",
    lower_bound: "-32768",
    upper_bound: "32767",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::Scalar("direction")),
};

/// Directional Blur Blur Length. The written record follows the corpus
/// records (`ParameterControlType` 2); Premiere 26.5.1 saves it without a
/// control type.
pub(crate) const DIRECTIONAL_BLUR_LENGTH: EffectParamSpec = EffectParamSpec {
    id: 2,
    name: "Blur Length",
    label: "Blur Length",
    record: records::VIDEO_COMPONENT_PARAM,
    control: "2",
    lower_bound: "0",
    upper_bound: "1000",
    lower_ui_bound: None,
    upper_ui_bound: Some("20"),
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::Scalar("blurLength")),
};

/// `AE.ADBE Motion Blur`, "Directional Blur" in the corpus records (Premiere
/// 12.1 7/5 and 14.4 8/6) and "Directional Blur (Legacy)" in Premiere 26.5.1
/// (9/7), with the same two parameters and bounds. Export writes
/// the corpus name, which Premiere 26.5.1 shows as "Directional Blur (Legacy)".
/// Premiere 26's default "Directional Blur" is Film Impact's
/// [`FILM_IMPACT_DIRECTIONAL_BLUR`].
pub(crate) const DIRECTIONAL_BLUR: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Motion Blur",
    display_name: "Directional Blur",
    filter_type: "2",
    params: &[DIRECTIONAL_BLUR_DIRECTION, DIRECTIONAL_BLUR_LENGTH],
    premiere_native: false,
    opaque_private_data: false,
};

/// Legacy Blur Length per current Directional Blur Amount, so that FX
/// `blurLength` keeps its Legacy meaning. On the pinned chart at 1080p, AME
/// renders Amount `a` with the edge spread of Legacy Blur Length 1.59a to
/// 1.63a (gamma-2.4 fits), and Amount 35 with a transparent exterior as Legacy
/// Blur Length 55 (case `premiere_isolated_film_impact_directional_blur_26_5`).
/// Resolution scaling is unmeasured.
pub(crate) const FILM_IMPACT_BLUR_LENGTH_PER_AMOUNT: f64 = 1.6;

/// The current Directional Blur's Angle, which sets FX `direction` as Legacy
/// Direction does.
pub(crate) const FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE: EffectParamSpec = EffectParamSpec {
    binding: Some(EffectParamBinding::Scalar("direction")),
    ..FILM_IMPACT_BLUR_ANGLE
};

/// The current Directional Blur's Amount, which sets FX `blurLength`.
pub(crate) const FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT: EffectParamSpec = EffectParamSpec {
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "blurLength",
        multiplier: FILM_IMPACT_BLUR_LENGTH_PER_AMOUNT,
    }),
    ..FILM_IMPACT_BLUR_AMOUNT
};

/// Premiere 26.5.1's current "Directional Blur", Film Impact
/// `AE.Impact_Directional_Blur_FX`, in the 9/7 form it saves: the current
/// Gaussian Blur's parameters without Thickness and Uniform Blur.
/// Like Legacy, it blurs in the clip's frame before Motion, and
/// Angle 0 blurs vertically and Angle 90 horizontally, clockwise positive.
/// Edge Behavior 2, a transparent exterior, is the Legacy blur's; Seed leaves
/// the blur unchanged (same probe).
pub(crate) const FILM_IMPACT_DIRECTIONAL_BLUR: EffectSpec = EffectSpec {
    match_name: "AE.Impact_Directional_Blur_FX",
    display_name: "Directional Blur",
    filter_type: "2",
    params: film_impact_params![
        FILM_IMPACT_BLUR_SEED,
        FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE,
        FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT,
        FILM_IMPACT_BLUR_CHROMATIC,
        FILM_IMPACT_BLUR_EDGE,
    ],
    premiere_native: false,
    opaque_private_data: false,
};

/// The static values of Premiere 26.5.1's default current Directional Blur, in
/// `Params` order. Export writes them with its Angle, Amount and Edge Behavior
/// 2.
pub(crate) const FILM_IMPACT_DIRECTIONAL_BLUR_DEFAULTS: [&str; 20] = [
    "false", "false", "false", "0.", "0.", "35.", "0.", "1", "false", "false", "0", "false",
    "false", "260501.", "0.", "0.", "false", "-1.", "-1.", "-1.",
];

/// A Brightness & Contrast parameter, from -100 to 100, whose keys move the FX
/// scalar `fx_param`. The written record follows the corpus records
/// (`ParameterControlType` 2); Premiere 26.5.1 saves it without a control
/// type.
const fn brightness_contrast_param(
    id: usize,
    name: &'static str,
    fx_param: &'static str,
) -> EffectParamSpec {
    EffectParamSpec {
        id,
        name,
        label: name,
        record: records::VIDEO_COMPONENT_PARAM,
        control: "2",
        lower_bound: "-100",
        upper_bound: "100",
        lower_ui_bound: None,
        upper_ui_bound: None,
        discontinuous_interpolate: false,
        binding: Some(EffectParamBinding::Scalar(fx_param)),
    }
}

pub(crate) const BRIGHTNESS_CONTRAST_BRIGHTNESS: EffectParamSpec =
    brightness_contrast_param(1, "Brightness", "brightness");

pub(crate) const BRIGHTNESS_CONTRAST_CONTRAST: EffectParamSpec =
    brightness_contrast_param(2, "Contrast", "contrast");

/// `AE.ADBE Brightness & Contrast 2`, "Brightness & Contrast" in the corpus
/// records (Premiere 12.1 7/5 and 14.4 8/6) and in Premiere 26.5.1 (9/7),
/// with the same two parameters and bounds.
pub(crate) const BRIGHTNESS_CONTRAST: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Brightness & Contrast 2",
    display_name: "Brightness & Contrast",
    filter_type: "2",
    params: &[BRIGHTNESS_CONTRAST_BRIGHTNESS, BRIGHTNESS_CONTRAST_CONTRAST],
    premiere_native: false,
    opaque_private_data: false,
};

// Import metadata only: these specs describe selected saved controls, not a
// complete Lumetri record layout. The native writer explicitly rejects them.
pub(crate) const LUMETRI_TEMPERATURE: EffectParamSpec = EffectParamSpec {
    id: 7,
    name: "Temperature",
    label: "Lumetri saved Temperature",
    lower_bound: "-300",
    upper_bound: "300",
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "temperature",
        multiplier: 1.0 / 3.0,
    }),
    ..BRIGHTNESS_CONTRAST_CONTRAST
};
pub(crate) const LUMETRI_TINT: EffectParamSpec = EffectParamSpec {
    id: 8,
    name: "Tint",
    label: "Lumetri saved Tint",
    lower_bound: "-300",
    upper_bound: "300",
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "tint",
        multiplier: -1.0 / 3.0,
    }),
    ..BRIGHTNESS_CONTRAST_CONTRAST
};
const LUMETRI_TEMPERATURE_SPEC: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Lumetri",
    display_name: "Lumetri Temperature replacement",
    params: &[LUMETRI_TEMPERATURE],
    ..BRIGHTNESS_CONTRAST
};
const LUMETRI_TINT_SPEC: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Lumetri",
    display_name: "Lumetri Tint replacement",
    params: &[LUMETRI_TINT],
    ..BRIGHTNESS_CONTRAST
};
pub(crate) const LUMETRI_EXPOSURE: EffectParamSpec = EffectParamSpec {
    id: 11,
    name: "Exposure",
    label: "Lumetri saved Exposure",
    lower_bound: "-5",
    upper_bound: "5",
    binding: Some(EffectParamBinding::Scalar("exposure")),
    ..BRIGHTNESS_CONTRAST_CONTRAST
};
pub(crate) const LUMETRI_SATURATION: EffectParamSpec = EffectParamSpec {
    id: 20,
    name: "Saturation",
    label: "Lumetri saved Saturation",
    lower_bound: "0",
    upper_bound: "200",
    binding: Some(EffectParamBinding::Scalar("saturation")),
    ..BRIGHTNESS_CONTRAST_CONTRAST
};
pub(crate) const LUMETRI_VIGNETTE_AMOUNT: EffectParamSpec = EffectParamSpec {
    id: 51,
    name: "Amount",
    label: "Lumetri Vignette Amount",
    lower_bound: "-5",
    upper_bound: "5",
    binding: None,
    ..BRIGHTNESS_CONTRAST_CONTRAST
};
pub(crate) const LUMETRI_VIGNETTE_MIDPOINT: EffectParamSpec = EffectParamSpec {
    id: 52,
    name: "Midpoint",
    label: "Lumetri Vignette Midpoint",
    lower_bound: "0",
    upper_bound: "100",
    binding: None,
    ..BRIGHTNESS_CONTRAST_CONTRAST
};
pub(crate) const LUMETRI_VIGNETTE_FEATHER: EffectParamSpec = EffectParamSpec {
    id: 54,
    name: "Feather",
    label: "Lumetri Vignette Feather",
    lower_bound: "0",
    upper_bound: "100",
    binding: None,
    ..BRIGHTNESS_CONTRAST_CONTRAST
};
const LUMETRI_VIGNETTE_SPEC: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Lumetri",
    display_name: "Lumetri Vignette replacement",
    params: &[
        LUMETRI_VIGNETTE_AMOUNT,
        LUMETRI_VIGNETTE_MIDPOINT,
        LUMETRI_VIGNETTE_FEATHER,
    ],
    ..BRIGHTNESS_CONTRAST
};

const LUMETRI_EXPOSURE_SPEC: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Lumetri",
    display_name: "Lumetri Exposure replacement",
    params: &[LUMETRI_EXPOSURE],
    ..BRIGHTNESS_CONTRAST
};
const LUMETRI_SATURATION_SPEC: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Lumetri",
    display_name: "Lumetri Saturation replacement",
    params: &[LUMETRI_SATURATION],
    ..BRIGHTNESS_CONTRAST
};

/// Invert Channel popup value for RGB, the only channel FX `levels` inverts:
/// its outputs apply to all three channels.
pub(crate) const INVERT_CHANNEL_RGB: &str = "0";

/// Invert Channel popup (RGB, R, G, B, and the HLS, YIQ and Alpha channels
/// up to 15). The written record follows the corpus records (Premiere 14.4,
/// `ParameterControlType` 7); Premiere 26.5.1 saves it without a control type.
pub(crate) const INVERT_CHANNEL: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "Channel",
    label: "Channel",
    record: records::VIDEO_POPUP_PARAM,
    control: "7",
    lower_bound: "0",
    upper_bound: "15",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: true,
    binding: None,
};

/// Invert Blend With Original, in percent of the original. Its keys animate
/// the FX `levels` output white, whose value and complementary output black
/// `convert::effects` maps (255 levels per 100 percent); the written record
/// follows the corpus records (`ParameterControlType` 2), which Premiere
/// 26.5.1 saves without a control type.
pub(crate) const INVERT_BLEND: EffectParamSpec = EffectParamSpec {
    id: 2,
    name: "Blend With Original",
    label: "Blend With Original",
    record: records::VIDEO_COMPONENT_PARAM,
    control: "2",
    lower_bound: "0",
    upper_bound: "100",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::Scalar("outputWhite")),
};

/// `AE.ADBE Invert`, "Invert" in the corpus records (Premiere 14.4, 8/6) and in
/// Premiere 26.5.1 (9/7), with the same two parameters and
/// bounds. Every saved record carries an opaque `PremiereFilterPrivateData`.
pub(crate) const INVERT: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Invert",
    display_name: "Invert",
    filter_type: "2",
    params: &[INVERT_CHANNEL, INVERT_BLEND],
    premiere_native: false,
    opaque_private_data: true,
};

/// The human-authored Premiere 2026 checkbox, ParameterID 1, without Name,
/// bounds or control type. Adobe checked means bright edges on black, the
/// opposite polarity to the existing FX shader's `invert > 0.5`.
pub(crate) const FIND_EDGES_INVERT: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "",
    label: "Invert",
    record: XmlRecordDefinition {
        version: "10",
        ..records::VIDEO_BOOL_COMPONENT_PARAM
    },
    control: "",
    lower_bound: "",
    upper_bound: "",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: None,
};

/// Native fraction of original image. The reader admits its scalar keys to
/// diagnose their loss; neither this control nor its keys exist in FX Find Edges.
pub(crate) const FIND_EDGES_BLEND: EffectParamSpec = EffectParamSpec {
    id: 2,
    name: "Blend With Original",
    label: "Blend With Original",
    record: XmlRecordDefinition {
        version: "10",
        ..records::VIDEO_COMPONENT_PARAM
    },
    control: "",
    lower_bound: "0",
    upper_bound: "1",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: None,
};

/// `AE.ADBE Find Edges`, pinned native component 537 and parameters 714/715
/// in `tests/fixtures/find-edges-26.5.xml` (component versions 9/7).
pub(crate) const FIND_EDGES: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Find Edges",
    display_name: "Find Edges",
    filter_type: "2",
    params: &[FIND_EDGES_INVERT, FIND_EDGES_BLEND],
    premiere_native: false,
    opaque_private_data: false,
};

/// A colour parameter (a Tint map colour, a Ramp colour), whose keys move the
/// FX channel scalars `red`, `green` and `blue`. The written record follows
/// the corpus Tint and Ramp records (Premiere 12.1: `ParameterControlType` 5,
/// bounds 0 to 2^64 − 1); Premiere 26.5.1 saves version 10 without a control
/// type or bounds.
const fn colour_param(
    id: usize,
    name: &'static str,
    red: &'static str,
    green: &'static str,
    blue: &'static str,
) -> EffectParamSpec {
    EffectParamSpec {
        id,
        name,
        label: name,
        record: records::VIDEO_COLOR_PARAM,
        control: "5",
        lower_bound: "0",
        upper_bound: "18446744073709551615",
        lower_ui_bound: None,
        upper_ui_bound: None,
        discontinuous_interpolate: false,
        binding: Some(EffectParamBinding::Colour { red, green, blue }),
    }
}

pub(crate) const TINT_MAP_BLACK_TO: EffectParamSpec =
    colour_param(1, "Map Black To", "blackR", "blackG", "blackB");

pub(crate) const TINT_MAP_WHITE_TO: EffectParamSpec =
    colour_param(2, "Map White To", "whiteR", "whiteG", "whiteB");

/// Tint Amount to Tint, in percent, whose keys move FX `amount`. The written
/// record follows the corpus records (`ParameterControlType` 2); Premiere
/// 26.5.1 saves it without a control type.
pub(crate) const TINT_AMOUNT: EffectParamSpec = EffectParamSpec {
    id: 3,
    name: "Amount to Tint",
    label: "Amount to Tint",
    record: records::VIDEO_COMPONENT_PARAM,
    control: "2",
    lower_bound: "0",
    upper_bound: "100",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::Scalar("amount")),
};

/// `AE.ADBE Tint`, "Tint" in the corpus records (Premiere 12.1, 7/5) and in
/// Premiere 26.5.1 (9/7), with the same three parameters.
pub(crate) const TINT: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Tint",
    display_name: "Tint",
    filter_type: "2",
    params: &[TINT_MAP_BLACK_TO, TINT_MAP_WHITE_TO, TINT_AMOUNT],
    premiere_native: false,
    opaque_private_data: false,
};

/// `AE.ADBE Black & White`, Premiere 26.5.1's "Black & White" (9/7): a record without parameters (no `Params` element). The corpus's older
/// `PR.ADBE Black & White` (`VideoFilterType` 1, one unnamed
/// `ArbVideoComponentParam`) is another effect and stays unknown.
pub(crate) const BLACK_WHITE: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Black & White",
    display_name: "Black & White",
    filter_type: "2",
    params: &[],
    premiere_native: false,
    opaque_private_data: false,
};

pub(crate) const RAMP_START: EffectParamSpec = point_param(1, "Start of Ramp", "startX", "startY");

pub(crate) const RAMP_START_COLOR: EffectParamSpec =
    colour_param(2, "Start Color", "startR", "startG", "startB");

pub(crate) const RAMP_END: EffectParamSpec = point_param(3, "End of Ramp", "endX", "endY");

pub(crate) const RAMP_END_COLOR: EffectParamSpec =
    colour_param(4, "End Color", "endR", "endG", "endB");

/// Ramp Shape popup: 0 linear, 1 radial. Only [`RAMP_SHAPE_LINEAR`] converts.
pub(crate) const RAMP_SHAPE: EffectParamSpec = EffectParamSpec {
    id: 5,
    name: "Ramp Shape",
    label: "Ramp Shape",
    record: records::VIDEO_POPUP_PARAM,
    control: "7",
    lower_bound: "0",
    upper_bound: "1",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: true,
    binding: None,
};

/// The Ramp Shape value of a linear ramp, the one that converts and the one
/// export writes.
pub(crate) const RAMP_SHAPE_LINEAR: &str = "0";

/// Ramp Scatter, which FX has no counterpart for: only 0 converts, and export
/// writes 0.
pub(crate) const RAMP_SCATTER: EffectParamSpec = EffectParamSpec {
    id: 6,
    name: "Ramp Scatter",
    label: "Ramp Scatter",
    record: records::VIDEO_COMPONENT_PARAM,
    control: "2",
    lower_bound: "0",
    upper_bound: "512",
    lower_ui_bound: None,
    upper_ui_bound: Some("50"),
    discontinuous_interpolate: false,
    binding: None,
};

/// Blend With Original, a fraction from 0 to 1 (not a percentage), whose keys
/// move FX `blend` through [`PrRamp::fx_blend`].
pub(crate) const RAMP_BLEND: EffectParamSpec = EffectParamSpec {
    id: 7,
    name: "Blend With Original",
    label: "Blend With Original",
    record: records::VIDEO_COMPONENT_PARAM,
    control: "2",
    lower_bound: "0",
    upper_bound: "1",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::Scalar("blend")),
};

/// `AE.ADBE Ramp`, "Ramp" in the corpus records (Premiere 12.1 and 14.x, 7/5
/// and 8/5–6) and in Premiere 26.5.1 (9/7), with the same
/// seven parameters. Its defaults are a vertical black-to-white ramp, 0.5:0 to
/// 0.5:1, linear, no scatter, Blend 0.
pub(crate) const RAMP: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Ramp",
    display_name: "Ramp",
    filter_type: "2",
    params: &[
        RAMP_START,
        RAMP_START_COLOR,
        RAMP_END,
        RAMP_END_COLOR,
        RAMP_SHAPE,
        RAMP_SCATTER,
        RAMP_BLEND,
    ],
    premiere_native: false,
    opaque_private_data: false,
};

/// A Mosaic block count: class `6e02e8bb`, the integer parameter class that
/// popups also use, with control type 1 (integer slider), whole values from
/// 1 to 4000 and a UI range to 200. `fx_name` is the FX `mosaic` parameter.
const fn mosaic_count(id: usize, name: &'static str, fx_name: &'static str) -> EffectParamSpec {
    EffectParamSpec {
        id,
        name,
        label: name,
        record: records::VIDEO_POPUP_PARAM,
        control: "1",
        lower_bound: "1",
        upper_bound: "4000",
        lower_ui_bound: None,
        upper_ui_bound: Some("200"),
        discontinuous_interpolate: false,
        binding: Some(EffectParamBinding::Scalar(fx_name)),
    }
}

pub(crate) const MOSAIC_HORIZONTAL_BLOCKS: EffectParamSpec =
    mosaic_count(1, "Horizontal Blocks", "horizontalBlocks");
pub(crate) const MOSAIC_VERTICAL_BLOCKS: EffectParamSpec =
    mosaic_count(2, "Vertical Blocks", "verticalBlocks");

/// Mosaic Sharp Colors checkbox. Premiere 26.5.1 saves it without a `Name`
/// element and the corpus records likewise, so its spec name
/// is empty: the reader accepts a missing name for it
/// ([`EffectParamSpec::accepts_name`]) and the writer omits the element.
pub(crate) const MOSAIC_SHARP_COLORS: EffectParamSpec = EffectParamSpec {
    id: 3,
    name: "",
    label: "Sharp Colors",
    record: records::VIDEO_BOOL_COMPONENT_PARAM,
    control: "4",
    lower_bound: "false",
    upper_bound: "true",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: None,
};

/// `AE.ADBE Mosaic`, "Mosaic (Legacy)" in Premiere 26.5.1 (the
/// corpus records are the 8/5 and 7/5 generations of the same layout).
/// Premiere 26's default "Mosaic" is the unrelated Film Impact
/// `AE.Impact_Mosaic_FX`, which stays an unknown effect.
pub(crate) const MOSAIC: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Mosaic",
    display_name: "Mosaic (Legacy)",
    filter_type: "2",
    params: &[
        MOSAIC_HORIZONTAL_BLOCKS,
        MOSAIC_VERTICAL_BLOCKS,
        MOSAIC_SHARP_COLORS,
    ],
    premiere_native: false,
    opaque_private_data: false,
};

/// Replicate Count, whose keys move the four FX `motionTile` tile fields
/// together ([`EffectParamBinding::TileCount`]): the integer class
/// `6e02e8bb` with control type 1 (integer slider), whole values from 2 to 16
/// and the default 2, which Premiere 26.5.1 saves as version 10 without UI
/// bounds. The written record follows the writer's corpus generation of that
/// class, version 9, as Mosaic's counts do.
pub(crate) const REPLICATE_COUNT: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "Count",
    label: "Count",
    record: records::VIDEO_POPUP_PARAM,
    control: "1",
    lower_bound: "2",
    upper_bound: "16",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::TileCount {
        width: "tileWidth",
        height: "tileHeight",
        center_x: "tileCenterX",
        center_y: "tileCenterY",
    }),
};

/// `AE.ADBE Replicate`, "Replicate" in Premiere 26.5.1 (saved as 9/7), with
/// its one parameter, Count.
pub(crate) const REPLICATE: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Replicate",
    display_name: "Replicate",
    filter_type: "2",
    params: &[REPLICATE_COUNT],
    premiere_native: false,
    opaque_private_data: false,
};

/// Premiere's integer Sharpen slider; Scalar binding deliberately avoids rounding.
/// Reads the saved version 10; writes the shared version 9 integer record,
/// whose Adobe acceptance for Sharpen remains unverified.
pub(crate) const SHARPEN_AMOUNT: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "Sharpen Amount",
    label: "Sharpen Amount",
    record: records::VIDEO_POPUP_PARAM,
    control: "1",
    lower_bound: "0",
    upper_bound: "4000",
    lower_ui_bound: None,
    upper_ui_bound: Some("100"),
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::Scalar("amount")),
};

pub(crate) const SHARPEN: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Sharpen",
    display_name: "Sharpen",
    filter_type: "2",
    params: &[SHARPEN_AMOUNT],
    premiere_native: false,
    opaque_private_data: false,
};

/// Deliberate amplitude normalization, not a measured native calibration.
pub(crate) const NOISE_APPROXIMATION: &str = "Noise and Grain use different random kernels and temporal patterns; monochrome and wrapping modes approximate color/clipped noise; modern tonal weighting, Uniform Intensity, Saturation, Blend Mode and Master are not reproduced; Preserve Alpha off approximates alpha-preserving Grain; modern Intensity uses the same monotonic 0.4 strength surrogate, not calibrated equivalence; Legacy Grain amount = native Amount of Noise × 0.4, size 1, softness 0, aspectRatio 1, seed 0; Modern retains numeric Seed but not its native RNG sequence; strength, spatial behavior and alpha fidelity are unmeasured";

pub(crate) const NOISE_AMOUNT: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "Amount of Noise",
    label: "Amount of Noise",
    record: XmlRecordDefinition::new(
        "VideoComponentParam",
        "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        "10",
    ),
    control: "",
    lower_bound: "0",
    upper_bound: "100",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::ScaledScalar {
        // Grain persists amount, but its authorable shader target is intensity.
        name: "intensity",
        multiplier: 0.4,
    }),
};
pub(crate) const NOISE_COLOR: EffectParamSpec = EffectParamSpec {
    id: 2,
    name: "Noise Type",
    label: "Noise Type",
    record: XmlRecordDefinition {
        version: "10",
        ..records::VIDEO_BOOL_COMPONENT_PARAM
    },
    control: "",
    lower_bound: "",
    upper_bound: "",
    ..MOSAIC_SHARP_COLORS
};
pub(crate) const NOISE_CLIPPING: EffectParamSpec = EffectParamSpec {
    id: 3,
    name: "Clipping",
    label: "Clipping",
    record: XmlRecordDefinition {
        version: "10",
        ..records::VIDEO_BOOL_COMPONENT_PARAM
    },
    control: "",
    lower_bound: "",
    upper_bound: "",
    ..MOSAIC_SHARP_COLORS
};
pub(crate) const NOISE: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Noise2",
    display_name: "Noise (Legacy)",
    filter_type: "2",
    params: &[NOISE_AMOUNT, NOISE_COLOR, NOISE_CLIPPING],
    premiere_native: false,
    opaque_private_data: false,
};

/// Import-only ABI from noise-native-records.xml, records 733–757.
pub(crate) const MODERN_NOISE: EffectSpec = EffectSpec {
    match_name: "AE.ADBE_Noise_FX",
    display_name: "Noise",
    params: &[
        EffectParamSpec {
            id: 8100,
            name: "Error occurred",
            label: "Error occurred",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "16",
            lower_bound: "",
            upper_bound: "",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 1,
            name: "Controls",
            label: "Controls",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "11",
            lower_bound: "",
            upper_bound: "false",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 8040,
            name: "",
            label: "internal control",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "16",
            lower_bound: "",
            upper_bound: "",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 8041,
            name: "Seed",
            label: "Seed",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "99999",
            discontinuous_interpolate: false,
            binding: Some(EffectParamBinding::Scalar("seed")),
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 3,
            name: "Intensity",
            label: "Intensity",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "100",
            discontinuous_interpolate: false,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 4,
            name: "Shadows",
            label: "Shadows",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "100",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 5,
            name: "Midtones",
            label: "Midtones",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "100",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 6,
            name: "Highlights",
            label: "Highlights",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "100",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 7,
            name: "Uniform Intensity",
            label: "Uniform Intensity",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "",
            lower_bound: "",
            upper_bound: "",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 8,
            name: "Saturation",
            label: "Saturation",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "100",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 9,
            name: "Blend Mode",
            label: "Blend Mode",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "4",
            discontinuous_interpolate: true,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 10,
            name: "Master",
            label: "Master",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "100",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 11,
            name: "Preserve Alpha",
            label: "Preserve Alpha",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "",
            lower_bound: "",
            upper_bound: "",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 8240,
            name: "",
            label: "internal control",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "16",
            lower_bound: "",
            upper_bound: "",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 2,
            name: "Controls",
            label: "Controls",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "12",
            lower_bound: "",
            upper_bound: "false",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 8280,
            name: "_ Overlay Mode",
            label: "_ Overlay Mode",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "2",
            discontinuous_interpolate: true,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 8281,
            name: "_ Overlay Info",
            label: "_ Overlay Info",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "",
            lower_bound: "",
            upper_bound: "",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 8141,
            name: "",
            label: "internal control",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "16",
            lower_bound: "",
            upper_bound: "",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 8140,
            name: "_ Applied Version",
            label: "_ Applied Version",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "999999",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 8300,
            name: "",
            label: "internal control",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "16777215",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 8301,
            name: "",
            label: "internal control",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "0",
            upper_bound: "16777215",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 9020,
            name: "_ Overlay Enabled",
            label: "_ Overlay Enabled",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "",
            lower_bound: "",
            upper_bound: "",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 9040,
            name: "_ Sequence Width",
            label: "_ Sequence Width",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "-1",
            upper_bound: "1000000000",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 9041,
            name: "_ Sequence Height",
            label: "_ Sequence Height",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "-1",
            upper_bound: "1000000000",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
        EffectParamSpec {
            id: 9042,
            name: "_ Sequence Pixel Ratio",
            label: "_ Sequence Pixel Ratio",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
                "10",
            ),
            control: "",
            lower_bound: "-1",
            upper_bound: "1000000000",
            discontinuous_interpolate: false,
            binding: None,
            ..NOISE_AMOUNT
        },
    ],
    ..NOISE
};

/// Posterize Level, whose keys move FX `levels`: the float slider class
/// `a4ff2d6e` from 2 to 255 with a UI bound of 32 and the default 7, which
/// Premiere 26.5.1 saves as version 10 without a control type. The written
/// record follows the writer's corpus generation of that class, version 9
/// with `ParameterControlType` 8 as the corpus Gaussian Blur Blurriness
/// records have it; Premiere's reading of a written one is unverified.
pub(crate) const POSTERIZE_LEVEL: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "Level",
    label: "Level",
    record: records::VIDEO_FILTER_AMOUNT_PARAM,
    control: "8",
    lower_bound: "2",
    upper_bound: "255",
    lower_ui_bound: None,
    upper_ui_bound: Some("32"),
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::Scalar("levels")),
};

/// `AE.ADBE Posterize`, "Posterize" in Premiere 26.5.1 (saved as 9/7), with
/// its one parameter, Level.
pub(crate) const POSTERIZE: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Posterize",
    display_name: "Posterize",
    filter_type: "2",
    params: &[POSTERIZE_LEVEL],
    premiere_native: false,
    opaque_private_data: false,
};

/// A deliberate signed slider normalization, not a measured lens equation.
/// Positive native convex distortion expands the centre. FX inverse sampling
/// expands it with a negative coefficient (sample UV moves toward the centre).
pub(crate) const LENS_CURVATURE: EffectParamSpec = EffectParamSpec {
    lower_bound: "-100",
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "amount",
        multiplier: -0.01,
    }),
    ..levels_param(1, "Curvature", "100", None)
};

pub(crate) const LENS_APPROXIMATION: &str = "centered radial approximation: FX amount = -Curvature/100 (export uses the inverse), a deliberate slider normalization, not a measured coefficient law; FX uses UV cubic inverse sampling and transparent bounds, not Premiere's native kernel; only an identity canvas-sized input plane, zero decentering/prism and Fill Alpha on convert; native private data is not replayed or authored and native acceptance is unverified";

/// Native seven-control layout from the human-authored Lens fixture. Only
/// Curvature is editable in this bounded mapping; other controls are guarded.
pub(crate) const LENS_DISTORTION: EffectSpec = EffectSpec {
    match_name: "PR.ADBE Lens Distortion",
    display_name: "Lens Distortion",
    filter_type: "1",
    params: &[
        LENS_CURVATURE,
        EffectParamSpec {
            lower_bound: "-100",
            ..levels_param(2, "Vertical Decentering", "100", None)
        },
        EffectParamSpec {
            lower_bound: "-100",
            ..levels_param(3, "Horizontal Decentering", "100", None)
        },
        EffectParamSpec {
            lower_bound: "-100",
            ..levels_param(4, "Vertical Prism FX", "100", None)
        },
        EffectParamSpec {
            lower_bound: "-100",
            ..levels_param(5, "Horizontal Prism FX", "100", None)
        },
        EffectParamSpec {
            id: 6,
            name: "",
            label: "Fill Alpha",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "cc12343e-f113-4d3b-ae05-b287db77d461",
                "10",
            ),
            control: "",
            lower_bound: "",
            upper_bound: "",
            lower_ui_bound: None,
            upper_ui_bound: None,
            discontinuous_interpolate: false,
            binding: None,
        },
        EffectParamSpec {
            id: 7,
            name: "Fill Color",
            label: "Fill Color",
            record: XmlRecordDefinition::new(
                "VideoComponentParam",
                "0fde4e9f-f895-4ba3-b0fe-9a6feafda583",
                "10",
            ),
            control: "",
            lower_bound: "",
            upper_bound: "",
            lower_ui_bound: None,
            upper_ui_bound: Some("0"),
            discontinuous_interpolate: false,
            binding: None,
        },
    ],
    premiere_native: true,
    opaque_private_data: false,
};

/// Curvature supports only bounded Linear/Hold tracks, without Bézier overshoot.
pub(crate) fn lens_curvature(
    curvature: f64,
    animations: &[PrEffectParamAnimation],
) -> std::result::Result<f64, String> {
    if !(-100.0..=100.0).contains(&curvature) {
        return Err("Lens Curvature must be finite and within -100 to 100".to_owned());
    }
    for animation in animations {
        let Some(keys) = animation.keys.scalar() else {
            return Err("Lens Curvature requires scalar keys".to_owned());
        };
        if animation.param != &LENS_CURVATURE
            || keys.iter().any(|key| {
                !(-100.0..=100.0).contains(&key.value)
                    || matches!(key.easing, PrKeyframeEasing::CubicBezier { .. })
            })
        {
            return Err(
                "Lens Distortion supports only Curvature Linear/Hold keys within -100 to 100"
                    .to_owned(),
            );
        }
    }
    Ok(curvature)
}

/// Human-saved Premiere 2026 Frame Rate (parameter 717). The binding reads
/// native keys only; the converter reports and keeps their initial rate,
/// because FX `frameRate` is not animatable.
pub(crate) const POSTERIZE_TIME_FRAME_RATE: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "Frame Rate",
    label: "Frame Rate",
    record: XmlRecordDefinition {
        version: "10",
        ..records::VIDEO_COMPONENT_PARAM
    },
    control: "",
    lower_bound: "0.0099945068359375",
    upper_bound: "99",
    lower_ui_bound: Some("1"),
    upper_ui_bound: Some("64"),
    discontinuous_interpolate: true,
    binding: Some(EffectParamBinding::Scalar("frameRate")),
};

/// Human-saved Premiere 2026 Posterize Time (component 540, versions 9/7).
pub(crate) const POSTERIZE_TIME: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Posterize Time",
    display_name: "Posterize Time",
    filter_type: "2",
    params: &[POSTERIZE_TIME_FRAME_RATE],
    premiere_native: false,
    opaque_private_data: false,
};

/// A Transform scalar (class `fe47129e`) whose keys, when `fx_property` names
/// a layer property, become the staged video's keys on it. The written
/// record follows the corpus records (Premiere 12.1 7/5 and 14.4 8/6:
/// `ParameterControlType` 2, or 3 for the angles) with Premiere 26.5.1's
/// bounds (`A-static.xml`), which saves no control type but
/// the angles'.
const fn transform_scalar(
    id: usize,
    name: &'static str,
    control: &'static str,
    [lower_bound, upper_bound]: [&'static str; 2],
    fx_property: Option<&'static str>,
) -> EffectParamSpec {
    EffectParamSpec {
        id,
        name,
        label: name,
        record: records::VIDEO_COMPONENT_PARAM,
        control,
        lower_bound,
        upper_bound,
        lower_ui_bound: None,
        upper_ui_bound: None,
        discontinuous_interpolate: false,
        binding: match fx_property {
            Some(property) => Some(EffectParamBinding::Scalar(property)),
            None => None,
        },
    }
}

/// A Transform checkbox (class `cc12343e`). Premiere saves both without a
/// `Name` element in 26.5.1 (E11) and in the corpus generations, so the spec
/// name is empty ([`EffectParamSpec::accepts_name`]) and `label` is the
/// Effect Controls name.
const fn transform_checkbox(id: usize, label: &'static str) -> EffectParamSpec {
    EffectParamSpec {
        id,
        name: "",
        label,
        record: records::VIDEO_BOOL_COMPONENT_PARAM,
        control: "4",
        lower_bound: "false",
        upper_bound: "true",
        lower_ui_bound: None,
        upper_ui_bound: None,
        discontinuous_interpolate: false,
        binding: None,
    }
}

/// Transform Anchor Point, source-normalized. Its keys are not converted:
/// E11 keyed Position, Scale Height and Rotation only.
pub(crate) const TRANSFORM_ANCHOR_POINT: EffectParamSpec = EffectParamSpec {
    binding: None,
    ..point_param(1, "Anchor Point", "", "")
};

/// Transform Position, source-normalized; its keys move the staged video's
/// `positionX` and `positionY` (E11 T9, clip F's 14-field point keys).
pub(crate) const TRANSFORM_POSITION: EffectParamSpec =
    point_param(2, "Position", "positionX", "positionY");

/// Transform Uniform Scale checkbox, `ParameterID` 11 at `Params` index 2.
pub(crate) const TRANSFORM_UNIFORM_SCALE: EffectParamSpec = transform_checkbox(11, "Uniform Scale");

/// A Transform scale axis: bounds ±30000, slider ±200 (E11).
const fn transform_scale(
    id: usize,
    name: &'static str,
    fx_property: &'static str,
) -> EffectParamSpec {
    EffectParamSpec {
        lower_ui_bound: Some("-200"),
        upper_ui_bound: Some("200"),
        ..transform_scalar(id, name, "2", ["-30000", "30000"], Some(fx_property))
    }
}

/// Transform Scale Height; its keys move the staged video's `scaleY`, and
/// its `scaleX` too under Uniform Scale (T3).
pub(crate) const TRANSFORM_SCALE_HEIGHT: EffectParamSpec =
    transform_scale(3, "Scale Height", "scaleY");

/// Transform Scale Width; its keys move the staged video's `scaleX` without
/// Uniform Scale, and are not imported under it
/// ([`PrTransform::unimported_scale_width_keys`]).
pub(crate) const TRANSFORM_SCALE_WIDTH: EffectParamSpec =
    transform_scale(4, "Scale Width", "scaleX");

/// Transform Skew, from −70 to 70 degrees. The binding lets the reader read
/// its keys, which [`PrTransform::ensure_convertible`] rejects
/// ([`PrTransform::KEYED_SKEW`]).
pub(crate) const TRANSFORM_SKEW: EffectParamSpec =
    transform_scalar(5, "Skew", "2", ["-70", "70"], Some("skew"));

/// Transform Skew Axis, an angle control, keyed like [`TRANSFORM_SKEW`].
pub(crate) const TRANSFORM_SKEW_AXIS: EffectParamSpec =
    transform_scalar(6, "Skew Axis", "3", ["-32768", "32767"], Some("skewAxis"));

/// Transform Rotation, an angle control; its keys move the staged video's
/// `rotation` (T9, clip D).
pub(crate) const TRANSFORM_ROTATION: EffectParamSpec =
    transform_scalar(7, "Rotation", "3", ["-32768", "32767"], Some("rotation"));

/// Transform Opacity; it and its keys are the staged video's `opacity`, an
/// approximation: Premiere blends it in linear light (E11 T6: gamma 2.4 fits
/// within 1.0 level) and the FX SDR composite blends encoded values.
pub(crate) const TRANSFORM_OPACITY: EffectParamSpec =
    transform_scalar(8, "Opacity", "2", ["0", "100"], Some("opacity"));

/// Transform Use Composition's Shutter Angle checkbox, `ParameterID` 9.
pub(crate) const TRANSFORM_COMPOSITION_SHUTTER_ANGLE: EffectParamSpec =
    transform_checkbox(9, "Use Composition's Shutter Angle");

/// Transform Shutter Angle, rendered while the checkbox is off
/// ([`PrTransform::motion_blur_shutter_angle`]) as the FX composition's
/// `motionBlur.shutterAngle`, which has no keys: a keyed angle converts as
/// its first key's value.
pub(crate) const TRANSFORM_SHUTTER_ANGLE: EffectParamSpec =
    transform_scalar(10, "Shutter Angle", "2", ["0", "360"], Some("shutterAngle"));

/// Transform Sampling popup: [`TRANSFORM_SAMPLING_BILINEAR`] or
/// [`TRANSFORM_SAMPLING_BICUBIC`].
pub(crate) const TRANSFORM_SAMPLING: EffectParamSpec = EffectParamSpec {
    id: 12,
    name: "Sampling",
    label: "Sampling",
    record: records::VIDEO_POPUP_PARAM,
    control: "7",
    lower_bound: "0",
    upper_bound: "1",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: true,
    binding: None,
};

/// The Sampling value of bilinear sampling, which FX resamples with and
/// export writes.
pub(crate) const TRANSFORM_SAMPLING_BILINEAR: &str = "0";

/// The Sampling value of bicubic sampling, which converts as bilinear.
pub(crate) const TRANSFORM_SAMPLING_BICUBIC: &str = "1";

/// The Transform Opacity that blends nothing: any other value converts
/// approximately ([`TRANSFORM_OPACITY`]).
pub(crate) const TRANSFORM_OPAQUE: f64 = 100.0;

/// `AE.ADBE Geometry`, "Transform" in Premiere 26.5.1 (9/7 records) and in the corpus (Premiere 12.1 7/5 and 14.4 8/6), with the same
/// 12 parameters in this `Params` order: the checkboxes are `ParameterID` 11
/// and 9 at indexes 2 and 9. Its defaults are Anchor Point and Position
/// 0.5:0.5, Scale 100/100, Uniform Scale off, Skew, Skew Axis and Rotation 0,
/// Opacity 100, the composition's shutter angle, Shutter Angle 0 and
/// bilinear Sampling. The measured centered uniform zoom of `AE.ADBE Geometry2`
/// uses this parameter layout and imports as Corner Pin. A parameter's binding names
/// the staged video's layer property that its keys animate, not an effect
/// parameter: the effect converts to the video's transform.
pub(crate) const TRANSFORM: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Geometry",
    display_name: "Transform",
    filter_type: "2",
    params: &[
        TRANSFORM_ANCHOR_POINT,
        TRANSFORM_POSITION,
        TRANSFORM_UNIFORM_SCALE,
        TRANSFORM_SCALE_HEIGHT,
        TRANSFORM_SCALE_WIDTH,
        TRANSFORM_SKEW,
        TRANSFORM_SKEW_AXIS,
        TRANSFORM_ROTATION,
        TRANSFORM_OPACITY,
        TRANSFORM_COMPOSITION_SHUTTER_ANGLE,
        TRANSFORM_SHUTTER_ANGLE,
        TRANSFORM_SAMPLING,
    ],
    premiere_native: false,
    opaque_private_data: false,
};

/// The same native controls as Transform, with editable Anchor keys for the
/// adjustment-composite mapping. Other Geometry2 hosts retain zoom admission.
pub(crate) const ADJUSTMENT_GEOMETRY2: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Geometry2",
    params: &[
        EffectParamSpec {
            binding: Some(EffectParamBinding::Point {
                x: "anchorPointX",
                y: "anchorPointY",
            }),
            ..TRANSFORM_ANCHOR_POINT
        },
        TRANSFORM_POSITION,
        TRANSFORM_UNIFORM_SCALE,
        TRANSFORM_SCALE_HEIGHT,
        TRANSFORM_SCALE_WIDTH,
        TRANSFORM_SKEW,
        TRANSFORM_SKEW_AXIS,
        TRANSFORM_ROTATION,
        TRANSFORM_OPACITY,
        TRANSFORM_COMPOSITION_SHUTTER_ANGLE,
        TRANSFORM_SHUTTER_ANGLE,
        TRANSFORM_SAMPLING,
    ],
    ..TRANSFORM
};

/// Track Matte Key's Matte popup: the persistent `Track/ID` of the matte
/// video track (`horror_title` stores 7 for the track at `Index` 5 after a
/// track deletion; `food_promo` and `corporate_slideshow` store `Index` + 1).
/// Its record class (`ParameterControlType` 13) occurs on no other corpus
/// parameter.
pub(crate) const TRACK_MATTE_KEY_MATTE: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "Matte:",
    label: "Matte",
    record: records::VIDEO_TRACK_POPUP_PARAM,
    control: "13",
    lower_bound: "0",
    upper_bound: "4294967295",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: None,
};

/// Track Matte Key's Composite Using popup ([`super::PrMatteChannel`]).
pub(crate) const TRACK_MATTE_KEY_COMPOSITE: EffectParamSpec = EffectParamSpec {
    id: 2,
    name: "Composite Using:",
    label: "Composite Using",
    record: records::VIDEO_POPUP_PARAM,
    control: "7",
    lower_bound: "0",
    upper_bound: "1",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: true,
    binding: None,
};

/// Track Matte Key's Reverse checkbox ([`super::PrMatteChannel`]).
pub(crate) const TRACK_MATTE_KEY_REVERSE: EffectParamSpec = EffectParamSpec {
    id: 3,
    name: "Reverse",
    label: "Reverse",
    record: records::VIDEO_BOOL_COMPONENT_PARAM,
    control: "4",
    lower_bound: "false",
    upper_bound: "true",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: None,
};

/// `AE.ADBE Legacy Key Track Matte`, "Track Matte Key": the 20 corpus records
/// (Premiere 12.x saves 7/5, Premiere 14.4 saves 8/6) share these three static
/// parameters and carry no private data. It is a mask like a Crop or Linear
/// Wipe: the Motion reader reads it and the chain's mask slot writes it
/// (`PrTrackMatte`), so it is not a [`PrEffectParams`] variant. Premiere
/// 26.5.1 saves the same three parameters in a 9/7 record without
/// `ParameterControlType`, `IsTimeVarying`, `Bypass` or `Intrinsic`
/// (fixture G6).
pub(crate) const TRACK_MATTE_KEY: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Legacy Key Track Matte",
    display_name: "Track Matte Key",
    filter_type: "2",
    params: &[
        TRACK_MATTE_KEY_MATTE,
        TRACK_MATTE_KEY_COMPOSITE,
        TRACK_MATTE_KEY_REVERSE,
    ],
    premiere_native: false,
    opaque_private_data: false,
};

/// Premiere's saved two-control keyer, not After Effects' `ADBE Luma Key`.
pub(crate) const LEGACY_LUMA_KEY_MATCH_NAME: &str = "AE.ADBE Legacy Key Luma";

/// Deliberate percent normalization, not an Adobe coverage equation or AE Tolerance.
pub(crate) const LEGACY_LUMA_KEY_MAPPING_REASON: &str = "Legacy Luma Key approximation: Threshold transparency level / 100 maps to FX threshold and Cutoff falloff / 100 to softness; export uses the inverse normalization, not an established Adobe coverage equation or AE Tolerance. FX uses smoothstep over premultiplied Rec. 601 luma; native edge, polarity and alpha fidelity are unmeasured. No native invert control is invented";

// At threshold <= 1 this produces distinct f32 smoothstep edges, even at 1.
pub(crate) const LEGACY_LUMA_MIN_CUTOFF: f64 = 0.01;
pub(crate) const LEGACY_LUMA_THRESHOLD: EffectParamSpec = EffectParamSpec {
    id: 1,
    name: "Threshold",
    label: "Threshold",
    record: XmlRecordDefinition {
        version: "10",
        ..records::VIDEO_COMPONENT_PARAM
    },
    control: "",
    lower_bound: "0",
    upper_bound: "100",
    lower_ui_bound: None,
    upper_ui_bound: None,
    discontinuous_interpolate: false,
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "threshold",
        multiplier: 0.01,
    }),
};
pub(crate) const LEGACY_LUMA_CUTOFF: EffectParamSpec = EffectParamSpec {
    id: 2,
    name: "Cutoff",
    label: "Cutoff",
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "softness",
        multiplier: 0.01,
    }),
    ..LEGACY_LUMA_THRESHOLD
};
pub(crate) const LEGACY_LUMA_KEY: EffectSpec = EffectSpec {
    match_name: LEGACY_LUMA_KEY_MATCH_NAME,
    display_name: "Luma Key",
    filter_type: "2",
    params: &[LEGACY_LUMA_THRESHOLD, LEGACY_LUMA_CUTOFF],
    premiere_native: false,
    opaque_private_data: false,
};

/// Validate endpoint domains before the bounded easing/falloff approximation.
pub(crate) fn validate_legacy_luma(
    threshold: f64,
    cutoff: f64,
    animations: &[PrEffectParamAnimation],
) -> std::result::Result<(), String> {
    if ![threshold, cutoff]
        .into_iter()
        .all(|value| (0.0..=100.0).contains(&value))
    {
        return Err("Legacy Luma Threshold/Cutoff must be finite percentages in 0..100".to_owned());
    }
    for animation in animations {
        let Some(keys) = animation.keys.scalar() else {
            return Err("Legacy Luma requires scalar keys".to_owned());
        };
        if keys.iter().any(|key| !(0.0..=100.0).contains(&key.value)) {
            return Err("Legacy Luma keys must be finite percentages in 0..100".to_owned());
        }
    }
    Ok(())
}

/// Normalize only at the native/FX boundary in either direction. Linearizing
/// Bezier keeps authored endpoints/times without permitting width overshoot.
pub(crate) fn normalize_legacy_luma(effect: &mut PrEffect) -> Vec<String> {
    let PrEffectParams::LegacyLuma { cutoff, .. } = &mut effect.params else {
        return Vec::new();
    };
    let mut notes = Vec::new();
    let mut floored = *cutoff < LEGACY_LUMA_MIN_CUTOFF;
    *cutoff = cutoff.max(LEGACY_LUMA_MIN_CUTOFF);
    for animation in &mut effect.animations {
        let PrEffectParamKeys::Scalar(keys) = &mut animation.keys else {
            continue;
        };
        let mut linearized = false;
        for key in keys {
            if matches!(key.easing, PrKeyframeEasing::CubicBezier { .. }) {
                key.easing = PrKeyframeEasing::Linear;
                linearized = true;
            }
            if animation.param == &LEGACY_LUMA_CUTOFF && key.value < LEGACY_LUMA_MIN_CUTOFF {
                key.value = LEGACY_LUMA_MIN_CUTOFF;
                floored = true;
            }
        }
        if linearized {
            notes.push(format!("{} Bezier keys approximated as Linear with values/times retained; bounded interpolation prevents falloff overshoot", animation.param.label));
        }
    }
    if floored {
        notes.push("Cutoff falloff below 0.01 percent was raised to 0.01 percent (FX softness 0.0001) at affected static values/key endpoints to avoid equal smoothstep edges; near-zero intervals are approximate".to_owned());
    }
    notes
}

impl PrEffect {
    pub(crate) fn is_legacy_luma(&self) -> bool {
        matches!(self.params, PrEffectParams::LegacyLuma { .. })
    }

    /// Dropping an enabled key must never leave an opaque occurrence behind.
    pub(crate) fn requires_coverage(&self) -> bool {
        self.enabled && self.is_legacy_luma()
    }
}

/// Standard effects that change what their clip covers or its transparency,
/// each named by a prefix of its match name or English display name.
///
/// An active such effect omits its whole occurrence: converting the clip
/// without it may expose previously keyed pixels or cover underlying content. Every other
/// standard effect keeps the clip and omits only itself. The list is
/// Premiere's Keying effects (Ultra Key, Luma Key, Chroma Key, Color Key,
/// Image Matte Key, Difference Matte, Non Red Key, Alpha Adjust), and the
/// Radial Wipe clip effects from the Transition effects. A masked effect
/// (`mask_match_name`) counts too. An active Track Matte Key
/// ([`TRACK_MATTE_KEY`]) matches the first entry but converts as the clip's
/// mask, so the chain reader skips it before this list applies. Premiere
/// renders a clip without its bypassed effects, so a bypassed entry keeps its
/// clip and is reported like any unmapped effect. Supported Legacy Luma controls
/// convert before this guard; a missing `Bypass` reads as
/// active (inferred) and an invalid one counts as active.
///
/// Failure direction: a prefix can only catch more effects, so a wrong entry
/// omits more occurrences than necessary (fail-closed). A display-name entry
/// misses a localized project; that project keeps the clip, and the
/// unknown-effect omission still reports the effect.
const COVERAGE_EFFECTS: [&str; 10] = [
    // The Legacy Key family, inferred from Track Matte Key's match name
    // (20 corpus records in 3 projects); Legacy Luma is also native-fixture-backed.
    "AE.ADBE Legacy Key ",
    // Radial Wipe: 2 corpus clip-effect records, both in `transition_countdown`.
    "AE.ADBE Radial Wipe",
    // English display-name fallbacks: apart from Legacy Luma above, no native
    // fixture or Premiere 26 bundle string identifies these effects' match names.
    "Ultra Key",
    "Luma Key",
    "Chroma Key",
    "Color Key",
    "Image Matte Key",
    "Difference Matte",
    "Non Red Key",
    "Alpha Adjust",
];

/// Whether a standard effect is one of the [`COVERAGE_EFFECTS`].
pub(crate) fn is_coverage_effect(match_name: Option<&str>, display_name: Option<&str>) -> bool {
    [match_name, display_name]
        .into_iter()
        .flatten()
        .any(|name| COVERAGE_EFFECTS.iter().any(|entry| name.starts_with(entry)))
}

/// Deliberate single-colour fading surrogate, not a calibrated pixel transfer.
pub(crate) const ALPHA_GLOW_APPROXIMATION: &str = "Alpha Glow uses a single-color soft normal OuterGlow surrogate: one Glow unit per intrinsic FX pixel, Brightness/255 as opacity, spread 0 and range 0.5; radius, falloff and transformed/nested halo extent are uncalibrated. Export uses current size/RGBA, rounds static sliders to integers and RGB to 8 bits, limits size to 100 and writes inactive End Color from current Start Color; unsupported style controls and animation are approximated with diagnostics";
pub(crate) const ALPHA_GLOW_SIZE: EffectParamSpec = EffectParamSpec {
    record: XmlRecordDefinition {
        version: "10",
        ..records::VIDEO_POPUP_PARAM
    },
    lower_bound: "0",
    upper_bound: "100",
    upper_ui_bound: None,
    ..mosaic_count(1, "Glow", "size")
};
pub(crate) const ALPHA_GLOW_BRIGHTNESS: EffectParamSpec = EffectParamSpec {
    id: 2,
    name: "Brightness",
    label: "Brightness",
    upper_bound: "255",
    binding: None,
    ..ALPHA_GLOW_SIZE
};
pub(crate) const ALPHA_GLOW_START: EffectParamSpec = EffectParamSpec {
    record: XmlRecordDefinition {
        version: "10",
        ..records::VIDEO_COLOR_PARAM
    },
    control: "",
    lower_bound: "",
    upper_bound: "",
    binding: None,
    ..colour_param(3, "Start Color", "", "", "")
};
pub(crate) const ALPHA_GLOW_END: EffectParamSpec = EffectParamSpec {
    id: 4,
    name: "End Color",
    label: "End Color",
    ..ALPHA_GLOW_START
};
pub(crate) const ALPHA_GLOW_USE_END: EffectParamSpec = EffectParamSpec {
    record: XmlRecordDefinition {
        version: "10",
        ..records::VIDEO_BOOL_COMPONENT_PARAM
    },
    control: "",
    lower_bound: "",
    upper_bound: "",
    id: 5,
    name: "Use End Color",
    label: "Use End Color",
    ..MOSAIC_SHARP_COLORS
};
pub(crate) const ALPHA_GLOW_FADE: EffectParamSpec = EffectParamSpec {
    id: 6,
    name: "Fade Out",
    label: "Fade Out",
    ..ALPHA_GLOW_USE_END
};
pub(crate) const ALPHA_GLOW: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Alpha Glow",
    display_name: "Alpha Glow",
    filter_type: "2",
    params: &[
        ALPHA_GLOW_SIZE,
        ALPHA_GLOW_BRIGHTNESS,
        ALPHA_GLOW_START,
        ALPHA_GLOW_END,
        ALPHA_GLOW_USE_END,
        ALPHA_GLOW_FADE,
    ],
    premiere_native: false,
    opaque_private_data: false,
};
/// Integer native slider endpoints; interpolation between endpoints stays continuous.
pub(crate) fn alpha_glow_size(
    size: f64,
    animations: &[PrEffectParamAnimation],
) -> std::result::Result<(), String> {
    let valid = |v: f64| (0.0..=100.0).contains(&v) && v.fract() == 0.0;
    if !valid(size)
        || animations.iter().any(|a| {
            a.param.id != 1
                || a.keys
                    .scalar()
                    .is_none_or(|keys| keys.iter().any(|k| !valid(k.value)))
        })
    {
        return Err(
            "Alpha Glow requires whole size endpoints in 0..100 and only size animation".to_owned(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        corner_at, cubic_bezier_progress, cubic_bezier_range, point_axis_range, stays_convex_along,
        stays_convex_in_box, EffectParamBinding, EffectParamSpec, PrColour, PrColourKeyframe,
        PrCornerPin, PrDirectionalBlur, PrEffect, PrEffectParamAnimation, PrEffectParamKeys,
        PrEffectParams, PrGaussianBlur, PrInvert, PrMosaic, PrPosterize, PrRamp, PrReplicate,
        PrTint, PrTransform, CORNER_PIN, DIRECTIONAL_BLUR, DIRECTIONAL_BLUR_DIRECTION,
        DIRECTIONAL_BLUR_LENGTH, FILM_IMPACT_BLUR, FILM_IMPACT_BLUR_AMOUNT, GAUSSIAN_BLUR,
        GAUSSIAN_BLUR_BLURRINESS, GAUSSIAN_BLUR_DIMENSIONS, GAUSSIAN_BLUR_MAX_BLURRINESS,
        GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS, INVERT, INVERT_BLEND, MOSAIC, MOSAIC_HORIZONTAL_BLOCKS,
        MOSAIC_SHARP_COLORS, MOSAIC_VERTICAL_BLOCKS, POSTERIZE, POSTERIZE_LEVEL, RAMP, RAMP_BLEND,
        RAMP_END, RAMP_END_COLOR, RAMP_START, RAMP_START_COLOR, REPLICATE, REPLICATE_COUNT, TINT,
        TINT_AMOUNT, TINT_MAP_BLACK_TO, TINT_MAP_WHITE_TO, TRANSFORM, TRANSFORM_ANCHOR_POINT,
        TRANSFORM_COMPOSITION_SHUTTER_ANGLE, TRANSFORM_OPACITY, TRANSFORM_ROTATION,
        TRANSFORM_SCALE_HEIGHT, TRANSFORM_SCALE_WIDTH, TRANSFORM_SHUTTER_ANGLE, TRANSFORM_SKEW,
        TRANSFORM_SKEW_AXIS, TRANSFORM_UNIFORM_SCALE,
    };
    use crate::schema::{PrKeyframeEasing, PrPointKeyframe, PrScalarKeyframe, TICKS};
    use std::collections::BTreeSet;

    const IDENTITY_CORNERS: [[f64; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];

    /// 5,000 keys, above the 4,096 that these checks once allowed, a
    /// millisecond apart: `key(index)` at each time.
    fn past_the_former_limit<T>(key: impl Fn(i64, i64) -> T) -> Vec<T> {
        (0..5000)
            .map(|index| key(index * TICKS / 1000, index))
            .collect()
    }

    /// `keys` less the last key, with the last two times repeated and with
    /// them swapped: every change leaves the keys out of strict time order.
    fn out_of_order<T: Clone>(keys: &[T], ticks: impl Fn(&mut T) -> &mut i64) -> [Vec<T>; 2] {
        let mut repeated = keys.to_vec();
        let last = repeated.len() - 1;
        *ticks(&mut repeated[last]) = *ticks(&mut repeated[last - 1]);
        let mut swapped = keys.to_vec();
        swapped.swap(last - 1, last);
        [repeated, swapped]
    }

    #[test]
    fn scalar_effect_keys_past_the_former_limit_keep_their_range_and_order_checks() {
        let blur = |keys: Vec<PrScalarKeyframe>| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::GaussianBlur(PrGaussianBlur {
                blurriness: 0.0,
                repeat_edge_pixels: false,
            }),
            animations: vec![PrEffectParamAnimation {
                param: &GAUSSIAN_BLUR_BLURRINESS,
                keys: PrEffectParamKeys::Scalar(keys),
            }],
        };
        let keys = past_the_former_limit(|source_ticks, index| PrScalarKeyframe {
            source_ticks,
            value: (index % 100) as f64,
            easing: PrKeyframeEasing::Linear,
        });
        assert!(blur(keys.clone()).validate().is_ok());
        let with_last = |value| {
            let mut keys = keys.clone();
            keys[4999].value = value;
            keys
        };
        let [repeated, swapped] = out_of_order(&keys, |key| &mut key.source_ticks);
        for (case, keys) in [
            ("empty", Vec::new()),
            ("above the range", with_last(30000.5)),
            ("nonfinite", with_last(f64::NAN)),
            ("repeated time", repeated),
            ("swapped times", swapped),
        ] {
            let error = blur(keys).validate().unwrap_err().to_string();
            assert!(
                error.contains("Blurriness keys must be one or more values from 0 to 30000 at strictly increasing source times"),
                "{case}: {error}"
            );
        }
    }

    #[test]
    fn point_effect_keys_past_the_former_limit_keep_their_finite_and_order_checks() {
        let ramp = |keys: Vec<PrPointKeyframe>| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Ramp(PrRamp {
                start: [0.5, 0.0],
                start_colour: PrColour::BLACK,
                end: [0.5, 1.0],
                end_colour: PrColour::WHITE,
                blend: 0.0,
            }),
            animations: vec![PrEffectParamAnimation {
                param: &RAMP_END,
                keys: PrEffectParamKeys::Point(keys),
            }],
        };
        // End of Ramp along the ramp's axis.
        let keys = past_the_former_limit(|source_ticks, index| PrPointKeyframe {
            source_ticks,
            value: [0.5, if index % 2 == 0 { 1.0 } else { 0.6 }],
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        });
        assert!(ramp(keys.clone()).validate().is_ok());
        let mut nonfinite = keys.clone();
        nonfinite[4999].value = [0.5, f64::NAN];
        let [repeated, swapped] = out_of_order(&keys, |key| &mut key.source_ticks);
        for (case, keys) in [
            ("empty", Vec::new()),
            ("nonfinite", nonfinite),
            ("repeated time", repeated),
            ("swapped times", swapped),
        ] {
            let error = ramp(keys).validate().unwrap_err().to_string();
            assert!(
                error.contains("End of Ramp keys must be one or more finite points at strictly increasing source times"),
                "{case}: {error}"
            );
        }
    }

    #[test]
    fn colour_effect_keys_past_the_former_limit_keep_their_easing_and_order_checks() {
        let tint = |keys: Vec<PrColourKeyframe>| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Tint(PrTint {
                amount: 100.0,
                ..PrTint::GRAYSCALE
            }),
            animations: vec![PrEffectParamAnimation {
                param: &TINT_MAP_WHITE_TO,
                keys: PrEffectParamKeys::Colour(keys),
            }],
        };
        let keys = past_the_former_limit(|source_ticks, index| PrColourKeyframe {
            source_ticks,
            value: if index % 2 == 0 {
                PrColour::WHITE
            } else {
                PrColour::BLACK
            },
            easing: if index % 3 == 0 {
                PrKeyframeEasing::Hold
            } else {
                PrKeyframeEasing::Linear
            },
        });
        assert!(tint(keys.clone()).validate().is_ok());
        let [repeated, swapped] = out_of_order(&keys, |key| &mut key.source_ticks);
        for (case, keys) in [
            ("empty", Vec::new()),
            ("repeated time", repeated),
            ("swapped times", swapped),
        ] {
            let error = tint(keys).validate().unwrap_err().to_string();
            assert!(
                error.contains("Map White To keys must be one or more colours at strictly increasing source times"),
                "{case}: {error}"
            );
        }
        let mut bezier = keys;
        bezier[4999].easing = PrKeyframeEasing::CubicBezier {
            x1: 0.3,
            y1: 0.3,
            x2: 0.7,
            y2: 0.7,
        };
        let error = tint(bezier).validate().unwrap_err().to_string();
        assert!(
            error.contains("Map White To keys must be Linear or Hold"),
            "{error}"
        );
    }

    #[test]
    fn shared_progress_proves_a_translating_quad_that_the_box_cannot() {
        let translation = [[2.0, 0.0]; 4];
        assert!(stays_convex_along(
            IDENTITY_CORNERS,
            translation,
            [0.0, 1.0]
        ));
        // Bounded one by one, the corners' progress admits Upper Left moved to
        // 2:0 while Upper Right is still at 1:0, a crossed quad: this is why
        // corners that share one progress are not checked in a box.
        assert!(!stays_convex_in_box(
            IDENTITY_CORNERS,
            translation,
            [[0.0, 1.0]; 4]
        ));
    }

    #[test]
    fn bezier_progress_range_includes_overshoot() {
        for (handles, times) in [
            ([0.3, 0.0, 0.6, 1.0], [0.0, 1.0]),
            ([0.2, 0.0, 0.6, 2.2], [0.0, 1.0]),
            ([0.4, -0.8, 0.7, 1.0], [0.0, 1.0]),
            ([0.5, -0.5, 0.5, 1.5], [0.25, 0.8]),
        ] {
            let [low, high] = cubic_bezier_range(handles, times);
            let samples: Vec<f64> = (0..=10_000)
                .map(|step| {
                    let time = times[0] + (times[1] - times[0]) * f64::from(step) / 10_000.0;
                    cubic_bezier_progress(handles, time)
                })
                .collect();
            let least = samples.iter().copied().fold(f64::INFINITY, f64::min);
            let greatest = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            // Every sample lies in the range, whose ends the samples reach.
            assert!(
                low <= least + 1e-12 && greatest <= high + 1e-12,
                "{handles:?} {times:?}: [{low}, {high}] against [{least}, {greatest}]"
            );
            assert!(
                least - low < 1e-6 && high - greatest < 1e-6,
                "{handles:?} {times:?}: [{low}, {high}] against [{least}, {greatest}]"
            );
        }
        // The overshoot that the export test of Upper Left past the diagonal
        // needs: 0.4 times at least 1.25.
        let [_, high] = cubic_bezier_range([0.2, 0.0, 0.6, 2.2], [0.0, 1.0]);
        assert!(high > 1.25, "{high}");
    }

    /// SplitMix64, a small deterministic generator for the seeded check.
    struct SplitMix64(u64);

    impl SplitMix64 {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut mixed = self.0;
            mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            mixed ^ (mixed >> 31)
        }

        /// A value from `low` up to `high`.
        fn uniform(&mut self, low: f64, high: f64) -> f64 {
            // The top 53 bits convert to f64 exactly.
            let unit = (self.next() >> 11) as f64 / (1_u64 << 53) as f64;
            low + (high - low) * unit
        }

        /// A whole number below `count`.
        fn below(&mut self, count: u32) -> u32 {
            u32::try_from(self.next() % u64::from(count)).expect("a remainder below a u32 count")
        }

        fn bezier(&mut self) -> PrKeyframeEasing {
            PrKeyframeEasing::CubicBezier {
                x1: self.uniform(0.0, 1.0),
                y1: self.uniform(-0.6, 1.6),
                x2: self.uniform(0.0, 1.0),
                y2: self.uniform(-0.6, 1.6),
            }
        }

        /// A convex quad: the identity quad with every corner moved by up to
        /// 0.15, mirrored for one seed in four, turned about its centre by up
        /// to a third of a turn either way and shifted by up to 0.5 on each
        /// axis.
        fn quad(&mut self) -> [[f64; 2]; 4] {
            let mirror = if self.below(4) == 0 { -1.0 } else { 1.0 };
            let third = std::f64::consts::TAU / 3.0;
            let (sin, cos) = self.uniform(-third, third).sin_cos();
            let shift = [self.uniform(-0.5, 0.5), self.uniform(-0.5, 0.5)];
            std::array::from_fn(|index| {
                let [x, y] =
                    IDENTITY_CORNERS[index].map(|value| value - 0.5 + self.uniform(-0.15, 0.15));
                let x = mirror * x;
                [
                    0.5 + shift[0] + x * cos - y * sin,
                    0.5 + shift[1] + x * sin + y * cos,
                ]
            })
        }

        /// `count` distinct source times on the quarter seconds from 0 to
        /// `quarters` quarter seconds, in order.
        fn quarter_seconds(&mut self, count: usize, quarters: u32) -> Vec<i64> {
            let mut times = Vec::with_capacity(count);
            while times.len() < count {
                let time = i64::from(self.below(quarters + 1)) * TICKS / 4;
                if !times.contains(&time) {
                    times.push(time);
                }
            }
            times.sort_unstable();
            times
        }
    }

    /// How the corners of a seeded Corner Pin move.
    #[derive(Debug, Clone, Copy)]
    enum SeededMotion {
        /// Linear or Hold keys, at times that differ between corners.
        Linear,
        /// Keys at 0 and 1 s with one Bezier easing that every moving corner
        /// shares.
        SharedBezier,
        /// Keys at each corner's own times, each with its own Linear or
        /// Bezier easing.
        DifferentEasings,
    }

    /// A seeded Corner Pin: one to four corners keyed from the first of three
    /// seeded convex quads through the corresponding corners of the others,
    /// between which the quad can flip or collapse.
    fn seeded_corner_pin(
        random: &mut SplitMix64,
        motion: SeededMotion,
    ) -> (PrCornerPin, Vec<PrEffectParamAnimation>) {
        let quads = [random.quad(), random.quad(), random.quad()];
        let corners = quads[0];
        let shared = random.bezier();
        let shared_times = random.quarter_seconds(3, 8);
        // All four corners for one seed in two, so that every keyed quad is
        // one of the convex quads.
        let moving = match (random.below(2), motion) {
            (0, _) => 4,
            (_, SeededMotion::DifferentEasings) => 2 + random.below(2),
            (_, SeededMotion::Linear | SeededMotion::SharedBezier) => 1 + random.below(3),
        };
        let first = random.below(4);
        let animations = (0..moving)
            .map(|offset| {
                let index = usize::try_from((first + offset) % 4).expect("a corner index");
                let (times, easings) = match motion {
                    SeededMotion::Linear => {
                        let count = 2 + usize::from(random.below(2) == 1);
                        let times = if random.below(2) == 0 {
                            shared_times.clone()
                        } else {
                            random.quarter_seconds(count, 8)
                        };
                        let easings: Vec<PrKeyframeEasing> = times
                            .iter()
                            .map(|_| {
                                if random.below(5) == 0 {
                                    PrKeyframeEasing::Hold
                                } else {
                                    PrKeyframeEasing::Linear
                                }
                            })
                            .collect();
                        (times, easings)
                    }
                    SeededMotion::SharedBezier => {
                        (vec![0, TICKS], vec![PrKeyframeEasing::Linear, shared])
                    }
                    SeededMotion::DifferentEasings => {
                        let times = if random.below(2) == 0 {
                            vec![0, TICKS]
                        } else {
                            random.quarter_seconds(2, 4)
                        };
                        let easing = if random.below(4) == 0 {
                            PrKeyframeEasing::Linear
                        } else {
                            random.bezier()
                        };
                        (times, vec![PrKeyframeEasing::Linear, easing])
                    }
                };
                let keys = times
                    .into_iter()
                    .zip(easings)
                    .zip(quads)
                    .map(|((source_ticks, easing), quad)| PrPointKeyframe {
                        source_ticks,
                        value: quad[index],
                        easing,
                        spatial_in_tangent: None,
                        spatial_out_tangent: None,
                    })
                    .collect();
                PrEffectParamAnimation {
                    param: &CORNER_PIN.params[index],
                    keys: PrEffectParamKeys::Point(keys),
                }
            })
            .collect();
        (PrCornerPin { corners }, animations)
    }

    /// The common strict sign of the quad's four turns, `Some(true)` when
    /// positive, computed here independently of the check.
    fn sampled_orientation(corners: [[f64; 2]; 4]) -> Option<bool> {
        let quad = [corners[0], corners[1], corners[3], corners[2]];
        let turns: Vec<f64> = (0..4)
            .map(|index| {
                let [a, b, c] = [0, 1, 2].map(|offset| quad[(index + offset) % 4]);
                (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
            })
            .collect();
        if turns.iter().all(|turn| *turn > 0.0) {
            Some(true)
        } else if turns.iter().all(|turn| *turn < 0.0) {
            Some(false)
        } else {
            None
        }
    }

    /// A sanity check of the implementation, not the proof: whenever the
    /// check accepts a seeded Corner Pin, dense sampling between its key times
    /// finds no quad that is not strictly convex, and one orientation within
    /// each interval, where the corners move continuously.
    #[test]
    fn seeded_corner_pins_that_pass_stay_strictly_convex_when_sampled() {
        const PINS: u32 = 1000;
        const SAMPLES: i64 = 48;
        let mut random = SplitMix64(2008);
        for motion in [
            SeededMotion::Linear,
            SeededMotion::SharedBezier,
            SeededMotion::DifferentEasings,
        ] {
            let mut accepted = 0;
            for _ in 0..PINS {
                let (pin, animations) = seeded_corner_pin(&mut random, motion);
                if pin.ensure_convex(&animations).is_err() {
                    continue;
                }
                accepted += 1;
                let quad_at = |ticks: i64| {
                    std::array::from_fn(|index| {
                        animations
                            .iter()
                            .find(|animation| animation.param.id == CORNER_PIN.params[index].id)
                            .and_then(|animation| corner_at(animation.keys.point()?, ticks))
                            .unwrap_or(pin.corners[index])
                    })
                };
                let mut times: Vec<i64> = animations
                    .iter()
                    .flat_map(|animation| animation.keys.point().unwrap_or_default())
                    .map(|key| key.source_ticks)
                    .collect();
                times.sort_unstable();
                times.dedup();
                for pair in times.windows(2) {
                    let orientation = sampled_orientation(quad_at(pair[0]));
                    assert!(
                        orientation.is_some(),
                        "{motion:?} at {}: {animations:?}",
                        pair[0]
                    );
                    for step in 1..=SAMPLES {
                        let ticks = pair[0] + (pair[1] - pair[0]) * step / (SAMPLES + 1);
                        let quad = quad_at(ticks);
                        assert_eq!(
                            sampled_orientation(quad),
                            orientation,
                            "{motion:?} at {ticks}: {quad:?} {animations:?}"
                        );
                    }
                }
            }
            // Both outcomes occur, so the seed exercises the check.
            assert!(
                (PINS / 10..=PINS * 9 / 10).contains(&accepted),
                "{motion:?}: {accepted} of {PINS} accepted"
            );
        }
    }

    #[test]
    fn gaussian_blur_layout_is_dense_and_uniquely_named() {
        assert_eq!(
            GAUSSIAN_BLUR.params[0].upper_bound.parse::<f64>(),
            Ok(GAUSSIAN_BLUR_MAX_BLURRINESS)
        );
        let ids: BTreeSet<_> = GAUSSIAN_BLUR.params.iter().map(|spec| spec.id).collect();
        assert_eq!(ids, (1..=3).collect());
        let names: BTreeSet<_> = GAUSSIAN_BLUR.params.iter().map(|spec| spec.name).collect();
        assert_eq!(names.len(), GAUSSIAN_BLUR.params.len());
        assert!(GAUSSIAN_BLUR
            .params
            .iter()
            .all(|spec| !spec.label.trim().is_empty()));
        // Only Blurriness converts with keys; key checks read its numeric bounds.
        let bound: Vec<_> = GAUSSIAN_BLUR
            .params
            .iter()
            .filter(|spec| spec.binding.is_some())
            .collect();
        assert_eq!(bound, [&GAUSSIAN_BLUR_BLURRINESS]);
        assert_eq!(
            GAUSSIAN_BLUR.bound_param("blurriness"),
            Some(&GAUSSIAN_BLUR_BLURRINESS)
        );
        assert_eq!(
            GAUSSIAN_BLUR_BLURRINESS.value_range(),
            Some(0.0..=GAUSSIAN_BLUR_MAX_BLURRINESS)
        );
    }

    #[test]
    fn scaled_scalar_binding_resolves_by_its_fx_parameter_name() {
        assert_eq!(
            FILM_IMPACT_BLUR.bound_param("blurriness"),
            Some(&FILM_IMPACT_BLUR_AMOUNT)
        );
    }

    #[test]
    fn blurriness_outside_the_native_range_is_invalid() {
        for blurriness in [-1.0, 30000.5, f64::NAN, f64::INFINITY] {
            let effect = PrEffect {
                mask: None,
                enabled: true,
                params: PrEffectParams::GaussianBlur(PrGaussianBlur {
                    blurriness,
                    repeat_edge_pixels: false,
                }),
                animations: Vec::new(),
            };
            assert!(effect.validate().is_err(), "{blurriness}");
        }
    }

    #[test]
    fn keys_need_a_bound_parameter_and_native_range() {
        let keyed = |param, values: &[f64]| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::GaussianBlur(PrGaussianBlur {
                blurriness: values[0],
                repeat_edge_pixels: false,
            }),
            animations: vec![PrEffectParamAnimation {
                param,
                keys: PrEffectParamKeys::Scalar(
                    (0..)
                        .zip(values)
                        .map(|(second, &value)| PrScalarKeyframe {
                            source_ticks: second * TICKS,
                            value,
                            easing: PrKeyframeEasing::Linear,
                        })
                        .collect(),
                ),
            }],
        };
        // 606 is above the renderer cap of 300 but within Premiere's range.
        assert!(keyed(&GAUSSIAN_BLUR_BLURRINESS, &[606.0, 0.0])
            .validate()
            .is_ok());
        for (param, values) in [
            (&GAUSSIAN_BLUR_BLURRINESS, [0.0, 30000.5]),
            (&GAUSSIAN_BLUR_DIMENSIONS, [0.0, 1.0]),
        ] {
            let error = keyed(param, &values).validate().unwrap_err().to_string();
            assert!(error.contains(param.label), "{error}");
        }
    }

    #[test]
    fn directional_blur_layout_binds_both_parameters_within_premiere_bounds() {
        let ids: BTreeSet<_> = DIRECTIONAL_BLUR.params.iter().map(|spec| spec.id).collect();
        assert_eq!(ids, (1..=2).collect());
        for (fx_param, param, range) in [
            ("direction", &DIRECTIONAL_BLUR_DIRECTION, -32768.0..=32767.0),
            ("blurLength", &DIRECTIONAL_BLUR_LENGTH, 0.0..=1000.0),
        ] {
            assert_eq!(DIRECTIONAL_BLUR.bound_param(fx_param), Some(param));
            assert_eq!(param.value_range(), Some(range));
        }
        let effect = |direction, blur_length| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::DirectionalBlur(PrDirectionalBlur {
                direction,
                blur_length,
            }),
            animations: Vec::new(),
        };
        assert!(effect(-32768.0, 1000.0).validate().is_ok());
        for (direction, blur_length, label) in [
            (32767.5, 10.0, "Direction"),
            (f64::NAN, 10.0, "Direction"),
            (90.0, -0.5, "Blur Length"),
            (90.0, 1000.5, "Blur Length"),
        ] {
            let error = effect(direction, blur_length).validate().unwrap_err();
            assert!(error.to_string().contains(label), "{error}");
        }
    }

    #[test]
    fn invert_blend_outside_the_native_range_is_invalid() {
        let effect = |blend| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Invert(PrInvert { blend, channel: 0 }),
            animations: Vec::new(),
        };
        assert_eq!(INVERT.bound_param("outputWhite"), Some(&INVERT_BLEND));
        assert!(effect(100.0).validate().is_ok());
        for blend in [-0.5, 100.5, f64::NAN] {
            let error = effect(blend).validate().unwrap_err().to_string();
            assert!(
                error.contains("Invert Blend With Original") && error.contains("0 to 100"),
                "{error}"
            );
        }
    }

    #[test]
    fn colours_map_between_native_argb_and_eight_bit_channels() {
        // (native value as saved, the colour, the value export writes): Tint's
        // defaults with alpha 0, the effect_stack pair, and the run E6 orange.
        #[rustfmt::skip]
        let colours = [
            (0, [0, 0, 0], 0xff00_0000_0000_0000),
            (0x0000_ff00_ff00_ff00, [255, 255, 255], 0xff00_ff00_ff00_ff00),
            (18374865704210960128, [163, 247, 143], 18374865704210960128),
            (18374950366522381824, [240, 242, 22], 18374950366522381824),
            (18374966857284190208, [255, 128, 0], 18374966857284190208),
        ];
        for (native, rgb, written) in colours {
            let colour = PrColour::from_native(native).unwrap();
            assert_eq!(colour.rgb, rgb);
            assert_eq!(colour.native(), written);
            assert_eq!(PrColour::from_fx(colour.fx()), Some(colour));
        }
        // A nonzero low byte in any channel, alpha excepted, has no 8-bit colour.
        for native in [1, 0x0001_0000, 0x0001_0000_0000] {
            assert!(PrColour::from_native(native).is_err(), "{native:#x}");
        }
        assert_eq!(
            PrColour::from_native(0x0001_0000_0000_0000).unwrap(),
            PrColour::BLACK
        );
        // FX channels round to 8 bits; outside 0 to 1 or not finite, none.
        assert_eq!(
            PrColour::from_fx([0.5, 0.9, 1.0 / 510.0]).unwrap().rgb,
            [128, 230, 1]
        );
        for rgb in [[1.001, 0.0, 0.0], [0.0, -0.001, 0.0], [0.0, 0.0, f64::NAN]] {
            assert_eq!(PrColour::from_fx(rgb), None, "{rgb:?}");
        }
    }

    #[test]
    fn tint_layout_binds_colours_and_amount_and_rejects_bezier_colour_keys() {
        for (name, param) in [
            ("blackR", &TINT_MAP_BLACK_TO),
            ("blackG", &TINT_MAP_BLACK_TO),
            ("blackB", &TINT_MAP_BLACK_TO),
            ("whiteR", &TINT_MAP_WHITE_TO),
            ("whiteB", &TINT_MAP_WHITE_TO),
            ("amount", &TINT_AMOUNT),
        ] {
            assert_eq!(TINT.bound_param(name), Some(param), "{name}");
        }
        assert_eq!(TINT.bound_param("blackA"), None);
        let effect = |amount, easing| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Tint(PrTint {
                amount,
                ..PrTint::GRAYSCALE
            }),
            animations: vec![PrEffectParamAnimation {
                param: &TINT_MAP_WHITE_TO,
                keys: PrEffectParamKeys::Colour(vec![
                    PrColourKeyframe {
                        source_ticks: 0,
                        value: PrColour::WHITE,
                        easing: PrKeyframeEasing::Linear,
                    },
                    PrColourKeyframe {
                        source_ticks: TICKS,
                        value: PrColour::BLACK,
                        easing,
                    },
                ]),
            }],
        };
        assert!(effect(100.0, PrKeyframeEasing::Hold).validate().is_ok());
        for amount in [-0.5, 100.5, f64::NAN] {
            let error = effect(amount, PrKeyframeEasing::Linear)
                .validate()
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("Tint Amount to Tint") && error.contains("0 to 100"),
                "{error}"
            );
        }
        let bezier = PrKeyframeEasing::CubicBezier {
            x1: 0.3,
            y1: 0.3,
            x2: 0.7,
            y2: 0.7,
        };
        let error = effect(100.0, bezier).validate().unwrap_err().to_string();
        assert!(
            error.contains("Tint Map White To keys must be Linear or Hold"),
            "{error}"
        );
    }

    #[test]
    fn ramp_layout_binds_points_colours_and_blend() {
        for (name, param) in [
            ("startX", &RAMP_START),
            ("startY", &RAMP_START),
            ("startR", &RAMP_START_COLOR),
            ("startB", &RAMP_START_COLOR),
            ("endX", &RAMP_END),
            ("endG", &RAMP_END_COLOR),
            ("blend", &RAMP_BLEND),
        ] {
            assert_eq!(RAMP.bound_param(name), Some(param), "{name}");
        }
        // Shape and Scatter have no FX counterpart to key.
        assert_eq!(RAMP.bound_param("shape"), None);
        assert_eq!(RAMP.bound_param("scatter"), None);
        assert_eq!(PrRamp::fx_blend(0.25), 0.75);
        let effect = |blend, start: [f64; 2]| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Ramp(PrRamp {
                start,
                start_colour: PrColour::BLACK,
                end: [0.5, 1.0],
                end_colour: PrColour::WHITE,
                blend,
            }),
            animations: Vec::new(),
        };
        assert!(effect(0.0, [0.5, 0.0]).validate().is_ok());
        for blend in [-0.5, 1.5, f64::NAN] {
            let error = effect(blend, [0.5, 0.0])
                .validate()
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("Ramp Blend With Original") && error.contains("0 to 1"),
                "{error}"
            );
        }
        let error = effect(0.0, [f64::INFINITY, 0.0])
            .validate()
            .unwrap_err()
            .to_string();
        assert!(error.contains("Ramp points must be finite"), "{error}");
    }

    /// Point keys of End of Ramp with Linear easing into every key but the
    /// one that `easing` enters.
    fn end_keys(points: &[[f64; 2]], easing: PrKeyframeEasing) -> Vec<PrEffectParamAnimation> {
        let keys = points
            .iter()
            .enumerate()
            .map(|(index, &value)| PrPointKeyframe {
                source_ticks: index as i64 * TICKS,
                value,
                easing: if index + 1 == points.len() {
                    easing
                } else {
                    PrKeyframeEasing::Linear
                },
                spatial_in_tangent: None,
                spatial_out_tangent: None,
            })
            .collect();
        vec![PrEffectParamAnimation {
            param: &RAMP_END,
            keys: PrEffectParamKeys::Point(keys),
        }]
    }

    #[test]
    fn point_axis_range_preserves_bezier_overshoot_for_each_coordinate() {
        let keys = [
            PrPointKeyframe {
                source_ticks: 0,
                value: [0.5, 1.0],
                easing: PrKeyframeEasing::Linear,
                spatial_in_tangent: None,
                spatial_out_tangent: None,
            },
            PrPointKeyframe {
                source_ticks: 1,
                value: [0.6, 0.5],
                easing: PrKeyframeEasing::CubicBezier {
                    x1: 0.3,
                    y1: -0.1,
                    x2: 0.7,
                    y2: 1.1,
                },
                spatial_in_tangent: None,
                spatial_out_tangent: None,
            },
        ];
        let [x_min, x_max] = point_axis_range(&keys, 0);
        let [y_min, y_max] = point_axis_range(&keys, 1);
        assert!(x_min < 0.5 && x_max > 0.6, "x range: {x_min}..{x_max}");
        assert!(y_min < 0.5 && y_max > 1.0, "y range: {y_min}..{y_max}");
    }

    #[test]
    fn ramp_axis_rule_holds_at_every_time() {
        let ramp = |start: [f64; 2], end: [f64; 2]| PrRamp {
            start,
            start_colour: PrColour::BLACK,
            end,
            end_colour: PrColour::WHITE,
            blend: 0.0,
        };
        let linear = PrKeyframeEasing::Linear;
        // Overshoots the key values by about 0.6 % of the segment on both sides.
        let overshoot = PrKeyframeEasing::CubicBezier {
            x1: 0.3,
            y1: -0.1,
            x2: 0.7,
            y2: 1.1,
        };
        // (start, end, End of Ramp keys, the reason or "")
        type Case<'a> = ([f64; 2], [f64; 2], Vec<PrEffectParamAnimation>, &'a str);
        #[rustfmt::skip]
        let cases: [Case<'_>; 16] = [
            ([0.5, 0.0], [0.5, 1.0], vec![], ""),
            ([0.2, 0.5], [0.8, 0.5], vec![], ""),
            // Off-frame endpoints on an axis, and a reversed axis.
            ([0.5, -0.2], [0.5, 1.4], vec![], ""),
            ([0.9, 0.5], [0.1, 0.5], vec![], ""),
            ([0.5, 0.0], [0.5, 1.0], end_keys(&[[0.5, 1.0], [0.5, 0.6]], linear), ""),
            ([0.3406, 0.4426], [0.5693, 0.7602], vec![], "is not aligned with the frame at every time"),
            ([0.5, 0.5], [0.5, 0.5], vec![], "Start of Ramp and End of Ramp are both 0.5:0.5"),
            ([0.5, 0.0], [0.5, 1.0], end_keys(&[[0.5, 1.0], [0.6, 0.6]], linear), "is not aligned with the frame at every time"),
            ([0.5, 0.0], [0.5, 1.0], end_keys(&[[0.5, 1.0], [0.5, 0.0]], linear), "meet: their y coordinates reach 0..0 and 0..1"),
            // The Bézier overshoots 0.1 down to about 0.095, past the start at 0.1.
            ([0.5, 0.1], [0.5, 1.0], end_keys(&[[0.5, 1.0], [0.5, 0.1]], overshoot), "meet: their y coordinates reach"),
            // The shader's axis floor: a static ramp of MIN_LENGTH converts, a
            // shorter one (0.001, and one just below √1e-5)
            // does not, on either axis.
            ([0.5, 0.0], [0.5, PrRamp::MIN_LENGTH], vec![], ""),
            ([0.5, 0.5], [0.5, 0.501], vec![], "come within 0.0010 of the frame of each other along y"),
            ([0.5, 0.5], [0.5031, 0.5], vec![], "come within 0.0031 of the frame of each other along x"),
            // Linear keys that bring the end into the band without meeting the start.
            ([0.5, 0.5], [0.5, 1.0], end_keys(&[[0.5, 1.0], [0.5, 0.502]], linear), "a ramp shorter than 0.0032 of the frame is not converted"),
            // Linear keys that stop 0.005 short of the start convert; the same
            // keys with the Bézier overshoot (about 0.003 more) enter the band.
            ([0.5, 0.5], [0.5, 1.0], end_keys(&[[0.5, 1.0], [0.5, 0.505]], linear), ""),
            ([0.5, 0.5], [0.5, 1.0], end_keys(&[[0.5, 1.0], [0.5, 0.505]], overshoot), "a ramp shorter than 0.0032 of the frame is not converted"),
        ];
        for (start, end, animations, reason) in cases {
            let result = ramp(start, end).ensure_aligned(&animations);
            match reason {
                "" => assert!(result.is_ok(), "{start:?} {end:?}: {result:?}"),
                reason => {
                    let error = result.unwrap_err();
                    assert!(error.contains(reason), "{start:?} {end:?}: {error}");
                }
            }
        }
        // The same overshoot on an end that stays clear of the start converts.
        assert!(ramp([0.5, 0.0], [0.5, 1.0])
            .ensure_aligned(&end_keys(&[[0.5, 1.0], [0.5, 0.5]], overshoot))
            .is_ok());
    }

    #[test]
    fn mosaic_layout_binds_counts_and_accepts_the_unnamed_checkbox() {
        assert_eq!(
            MOSAIC.bound_param("horizontalBlocks"),
            Some(&MOSAIC_HORIZONTAL_BLOCKS)
        );
        assert_eq!(
            MOSAIC.bound_param("verticalBlocks"),
            Some(&MOSAIC_VERTICAL_BLOCKS)
        );
        assert_eq!(MOSAIC.bound_param("sharpColors"), None);
        // A blank spec name accepts a missing or blank native `Name`; a named
        // parameter accepts only its name, as with Transform.
        for (spec, name, accepted) in [
            (&MOSAIC_SHARP_COLORS, None, true),
            (&MOSAIC_SHARP_COLORS, Some(" "), true),
            (&MOSAIC_SHARP_COLORS, Some(""), true),
            (&MOSAIC_SHARP_COLORS, Some("Sharp Colors"), false),
            (&GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS, Some(" "), true),
            (&GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS, None, true),
            (&MOSAIC_HORIZONTAL_BLOCKS, Some("Horizontal Blocks"), true),
            (&MOSAIC_HORIZONTAL_BLOCKS, None, false),
            (&MOSAIC_HORIZONTAL_BLOCKS, Some(""), false),
        ] {
            assert_eq!(spec.accepts_name(name), accepted, "{} {name:?}", spec.label);
        }
        // Whole counts from 1 to 4000, in either message form.
        assert_eq!(PrMosaic::count(&MOSAIC_HORIZONTAL_BLOCKS, "", 16.0), Ok(16));
        assert_eq!(
            PrMosaic::count(&MOSAIC_VERTICAL_BLOCKS, "", 4000.0),
            Ok(4000)
        );
        for (what, value, expected) in [
            ("", 12.5, "Horizontal Blocks 12.5 is not a whole number of blocks; Premiere counts whole blocks and no rounding is applied"),
            (" key value", 0.0, "Horizontal Blocks key value 0 is outside Premiere's 1 to 4000 range"),
            ("", 4001.0, "Horizontal Blocks 4001 is outside Premiere's 1 to 4000 range"),
            ("", f64::NAN, "Horizontal Blocks NaN is not a whole number of blocks; Premiere counts whole blocks and no rounding is applied"),
        ] {
            assert_eq!(
                PrMosaic::count(&MOSAIC_HORIZONTAL_BLOCKS, what, value).unwrap_err(),
                expected
            );
        }
        let effect = |horizontal| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Mosaic(PrMosaic {
                horizontal,
                vertical: 9,
                sharp_colors: true,
            }),
            animations: Vec::new(),
        };
        assert!(effect(16).validate().is_ok());
        let error = effect(0).validate().unwrap_err().to_string();
        assert!(
            error.contains("Mosaic (Legacy) Horizontal Blocks 0"),
            "{error}"
        );
    }

    #[test]
    fn mosaic_converts_with_sharp_colours_on_and_held_whole_counts() {
        use PrKeyframeEasing::{CubicBezier, Hold, Linear};
        let mosaic = |sharp_colors| PrMosaic {
            horizontal: 10,
            vertical: 10,
            sharp_colors,
        };
        let keys = |values: [(f64, PrKeyframeEasing); 3]| {
            vec![PrEffectParamAnimation {
                param: &MOSAIC_HORIZONTAL_BLOCKS,
                keys: PrEffectParamKeys::Scalar(
                    values
                        .into_iter()
                        .zip([TICKS, 3 * TICKS / 2, 5 * TICKS / 2])
                        .map(|((value, easing), source_ticks)| PrScalarKeyframe {
                            source_ticks,
                            value,
                            easing,
                        })
                        .collect(),
                ),
            }]
        };
        let bezier = CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.75,
            y2: 0.9,
        };
        let hold_rule = "; only Hold keys convert, because the FX mosaic renders fractional block counts between keys and Premiere's stepping there is unmeasured";
        // (Sharp Colors, keys, the reason or "")
        #[rustfmt::skip]
        let cases = [
            (true, vec![], ""),
            // The first key's own easing has no segment before it.
            (true, keys([(10.0, Linear), (40.0, Hold), (20.0, Hold)]), ""),
            (true, keys([(10.0, Hold), (40.0, Hold), (20.0, Hold)]), ""),
            (false, vec![], PrMosaic::SHARP_COLORS_OFF),
            (false, keys([(10.0, Linear), (40.0, Hold), (20.0, Hold)]), PrMosaic::SHARP_COLORS_OFF),
            (true, keys([(10.0, Linear), (40.0, Linear), (20.0, Hold)]), "Horizontal Blocks keys are Linear between source times 1.000 s and 1.500 s"),
            (true, keys([(10.0, Linear), (40.0, Hold), (20.0, bezier)]), "Horizontal Blocks keys are Bézier between source times 1.500 s and 2.500 s"),
            (true, keys([(10.0, Linear), (40.5, Hold), (20.0, Hold)]), "Horizontal Blocks key value 40.5 is not a whole number of blocks"),
            (true, keys([(10.0, Linear), (4001.0, Hold), (20.0, Hold)]), "Horizontal Blocks key value 4001 is outside Premiere's 1 to 4000 range"),
        ];
        for (sharp_colors, animations, reason) in cases {
            let result = mosaic(sharp_colors).ensure_convertible(&animations);
            match reason {
                "" => assert!(result.is_ok(), "{sharp_colors} {animations:?}: {result:?}"),
                reason => {
                    let error = result.unwrap_err();
                    assert!(error.contains(reason), "{error}");
                    if reason.contains("keys are") {
                        assert!(error.ends_with(hold_rule), "{error}");
                    }
                }
            }
        }
    }

    #[test]
    fn replicate_converts_whole_counts_with_hold_keys_only() {
        use PrKeyframeEasing::{CubicBezier, Hold, Linear};
        // One Count keys the four tile fields; no other motionTile field is bound.
        for field in ["tileWidth", "tileHeight", "tileCenterX", "tileCenterY"] {
            assert_eq!(
                REPLICATE.bound_param(field),
                Some(&REPLICATE_COUNT),
                "{field}"
            );
        }
        for field in ["outputWidth", "outputHeight", "mirrorEdges", "phase"] {
            assert_eq!(REPLICATE.bound_param(field), None, "{field}");
        }
        assert!(REPLICATE_COUNT.accepts_name(Some("Count")));
        // Key times on the source clock: 1, 1.5 and 2.5 s.
        let keys = |values: [(f64, PrKeyframeEasing); 3]| {
            vec![PrEffectParamAnimation {
                param: &REPLICATE_COUNT,
                keys: PrEffectParamKeys::Scalar(
                    values
                        .into_iter()
                        .zip([TICKS, 3 * TICKS / 2, 5 * TICKS / 2])
                        .map(|((value, easing), source_ticks)| PrScalarKeyframe {
                            source_ticks,
                            value,
                            easing,
                        })
                        .collect(),
                ),
            }]
        };
        let bezier = CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.75,
            y2: 0.9,
        };
        let whole =
            " is not a whole number; Premiere counts whole copies and no rounding is applied";
        let hold_rule = "; only Hold keys convert, because Premiere's Count between interpolated keys is unmeasured and the FX motionTile would interpolate the tile size and centre, reciprocals of the Count, linearly";
        // (the static Count, keys, the Count or the reason)
        #[rustfmt::skip]
        let cases: [(f64, Vec<PrEffectParamAnimation>, Result<u8, String>); 11] = [
            (2.0, vec![], Ok(2)),
            (16.0, vec![], Ok(16)),
            // The first key's own easing has no segment before it.
            (3.0, keys([(3.0, Linear), (5.0, Hold), (2.0, Hold)]), Ok(3)),
            (2.5, vec![], Err(format!("Count 2.5{whole}"))),
            (f64::NAN, vec![], Err(format!("Count NaN{whole}"))),
            (1.0, vec![], Err("Count 1 is outside Premiere's 2 to 16 range".to_owned())),
            (17.0, vec![], Err("Count 17 is outside Premiere's 2 to 16 range".to_owned())),
            (3.0, keys([(3.0, Linear), (5.0, Linear), (2.0, Hold)]), Err(format!("Count keys are Linear between source times 1.000 s and 1.500 s{hold_rule}"))),
            (3.0, keys([(3.0, Linear), (5.0, Hold), (2.0, bezier)]), Err(format!("Count keys are Bézier between source times 1.500 s and 2.500 s{hold_rule}"))),
            (3.0, keys([(3.0, Linear), (4.5, Hold), (2.0, Hold)]), Err(format!("Count key value 4.5{whole}"))),
            (3.0, keys([(3.0, Linear), (17.0, Hold), (2.0, Hold)]), Err("Count key value 17 is outside Premiere's 2 to 16 range".to_owned())),
        ];
        for (count, animations, expected) in cases {
            assert_eq!(
                PrReplicate::new(count, &animations).map(|replicate| replicate.count),
                expected,
                "{count} {animations:?}"
            );
        }
        let effect = |count| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Replicate(PrReplicate { count }),
            animations: Vec::new(),
        };
        assert!(effect(16).validate().is_ok());
        let error = effect(1).validate().unwrap_err().to_string();
        assert!(error.contains("Replicate Count 1 is outside"), "{error}");
    }

    #[test]
    fn replicate_grids_start_at_the_frame_corner_at_even_and_odd_counts() {
        // The FX motionTile's position in its tile on one axis, without
        // mirror or phase: fract((uv - centre) / (size / 100) + 0.5).
        let tile_position = |uv: f64, (size, center): (f64, f64)| {
            ((uv - center) / (size / 100.0) + 0.5).rem_euclid(1.0)
        };
        // Distance on the unit circle, so that 0 and almost 1 agree.
        let near = |a: f64, b: f64| {
            let distance = (a - b).abs();
            distance.min(1.0 - distance) < 1e-9
        };
        // Whole copies from the frame's top-left corner sit at fract(Count · uv)
        // at every Count, even or odd.
        for count in [2.0, 3.0, 4.0, 7.0, 16.0] {
            let tiles = (
                PrReplicate::tile_size(count),
                PrReplicate::tile_center(count),
            );
            for uv in [0.0, 0.125, 0.3, 0.7, 0.99] {
                let expected = (count * uv).rem_euclid(1.0);
                assert!(
                    near(tile_position(uv, tiles), expected),
                    "Count {count} at {uv}: {tiles:?}"
                );
            }
        }
        assert_eq!(
            [2.0, 4.0, 16.0].map(|count| (
                PrReplicate::tile_size(count),
                PrReplicate::tile_center(count)
            )),
            [(50.0, 0.25), (25.0, 0.125), (6.25, 0.03125)]
        );
        // The FX default centre 0.5 shifts an even Count by half a tile (0.75
        // and 0 instead of 0.25 and 0.5 at uv 0.125) and draws the corner
        // grid at an odd one.
        assert!(near(tile_position(0.125, (50.0, 0.5)), 0.75));
        assert!(near(tile_position(0.125, (25.0, 0.5)), 0.0));
        assert!(near(tile_position(0.125, (100.0 / 3.0, 0.5)), 0.375));
        // Every Count's tiles convert back exactly, and nothing else does.
        for count in 2..=16 {
            let tiles = f64::from(count);
            assert_eq!(
                PrReplicate::grid_count(
                    [PrReplicate::tile_size(tiles); 2],
                    [PrReplicate::tile_center(tiles); 2]
                ),
                Some(count)
            );
        }
        #[rustfmt::skip]
        let others = [
            // The default centre at an even Count, and at an odd one, which
            // draws the same grid but is not import's form.
            ([50.0; 2], [0.5; 2]),
            ([100.0 / 3.0; 2], [0.5; 2]),
            // Unequal sizes or centres, and a rounded size.
            ([50.0, 25.0], [0.25; 2]),
            ([50.0; 2], [0.25, 0.125]),
            ([33.33; 2], [1.0 / 6.0; 2]),
            // Counts 1 and 17 are outside Premiere's range.
            ([100.0; 2], [0.5; 2]),
            ([100.0 / 17.0; 2], [0.5 / 17.0; 2]),
        ];
        for (size, center) in others {
            assert_eq!(
                PrReplicate::grid_count(size, center),
                None,
                "{size:?} {center:?}"
            );
        }
    }

    #[test]
    fn posterize_converts_whole_levels_with_hold_keys_only() {
        use PrKeyframeEasing::{CubicBezier, Hold, Linear};
        assert_eq!(POSTERIZE.bound_param("levels"), Some(&POSTERIZE_LEVEL));
        assert!(POSTERIZE_LEVEL.accepts_name(Some("Level")));
        // The fixture's Level key times on the source clock: 1, 1.5 and 2.5 s.
        let keys = |values: [(f64, PrKeyframeEasing); 3]| {
            vec![PrEffectParamAnimation {
                param: &POSTERIZE_LEVEL,
                keys: PrEffectParamKeys::Scalar(
                    values
                        .into_iter()
                        .zip([TICKS, 3 * TICKS / 2, 5 * TICKS / 2])
                        .map(|((value, easing), source_ticks)| PrScalarKeyframe {
                            source_ticks,
                            value,
                            easing,
                        })
                        .collect(),
                ),
            }]
        };
        let bezier = CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.75,
            y2: 0.9,
        };
        let whole = " is not a whole number; Premiere's rendering of a fractional Level is unmeasured and no rounding is applied";
        let hold_rule = "; only Hold keys convert, because the FX posterize floors the levels between keys and Premiere's stepping there is unmeasured";
        // (the static Level, keys, the Level or the reason)
        #[rustfmt::skip]
        let cases: [(f64, Vec<PrEffectParamAnimation>, Result<u8, String>); 12] = [
            (2.0, vec![], Ok(2)),
            (255.0, vec![], Ok(255)),
            // The first key's own easing has no segment before it.
            (3.0, keys([(3.0, Linear), (8.0, Hold), (5.0, Hold)]), Ok(3)),
            (3.0, keys([(3.0, Hold), (8.0, Hold), (5.0, Hold)]), Ok(3)),
            (7.5, vec![], Err(format!("Level 7.5{whole}"))),
            (f64::NAN, vec![], Err(format!("Level NaN{whole}"))),
            (1.0, vec![], Err("Level 1 is outside Premiere's 2 to 255 range".to_owned())),
            (256.0, vec![], Err("Level 256 is outside Premiere's 2 to 255 range".to_owned())),
            (3.0, keys([(3.0, Linear), (8.0, Linear), (5.0, Hold)]), Err(format!("Level keys are Linear between source times 1.000 s and 1.500 s{hold_rule}"))),
            (3.0, keys([(3.0, Linear), (8.0, Hold), (5.0, bezier)]), Err(format!("Level keys are Bézier between source times 1.500 s and 2.500 s{hold_rule}"))),
            (3.0, keys([(3.0, Linear), (8.5, Hold), (5.0, Hold)]), Err(format!("Level key value 8.5{whole}"))),
            (3.0, keys([(3.0, Linear), (300.0, Hold), (5.0, Hold)]), Err("Level key value 300 is outside Premiere's 2 to 255 range".to_owned())),
        ];
        for (level, animations, expected) in cases {
            assert_eq!(
                PrPosterize::new(level, &animations).map(|posterize| posterize.level),
                expected,
                "{level} {animations:?}"
            );
        }
        let effect = |level| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Posterize(PrPosterize { level }),
            animations: Vec::new(),
        };
        assert!(effect(7).validate().is_ok());
        let error = effect(1).validate().unwrap_err().to_string();
        assert!(
            error.contains("Posterize Level 1 is outside Premiere's 2 to 255 range"),
            "{error}"
        );
    }

    #[test]
    fn transform_layout_names_the_staged_properties_and_accepts_the_unnamed_checkboxes() {
        // The 12 parameters in `Params` order with their `ParameterID`s
        // (`A-static.xml`); the keyed ones name the staged
        // video's layer property, the checkboxes and the static-only
        // parameters bind nothing.
        let layout: Vec<_> = TRANSFORM
            .params
            .iter()
            .map(|param| {
                (
                    param.id,
                    param.label,
                    param.binding.map(EffectParamBinding::fx_params),
                )
            })
            .collect();
        assert_eq!(
            layout,
            [
                (1, "Anchor Point", None),
                (2, "Position", Some(vec!["positionX", "positionY"])),
                (11, "Uniform Scale", None),
                (3, "Scale Height", Some(vec!["scaleY"])),
                (4, "Scale Width", Some(vec!["scaleX"])),
                (5, "Skew", Some(vec!["skew"])),
                (6, "Skew Axis", Some(vec!["skewAxis"])),
                (7, "Rotation", Some(vec!["rotation"])),
                (8, "Opacity", Some(vec!["opacity"])),
                (9, "Use Composition's Shutter Angle", None),
                (10, "Shutter Angle", Some(vec!["shutterAngle"])),
                (12, "Sampling", None),
            ]
        );
        // The checkboxes are saved without a `Name` (E11 and the corpus);
        // the scale axes keep their 26.5.1 names, which Uniform Scale does not
        // rename (`B-uniform.xml`).
        for (spec, name, accepted) in [
            (&TRANSFORM_UNIFORM_SCALE, None, true),
            (&TRANSFORM_COMPOSITION_SHUTTER_ANGLE, None, true),
            (&TRANSFORM_UNIFORM_SCALE, Some("Uniform Scale"), false),
            (&TRANSFORM_SCALE_HEIGHT, Some("Scale Height"), true),
            (&TRANSFORM_SCALE_HEIGHT, Some("Scale"), false),
            (&TRANSFORM_SCALE_WIDTH, Some(""), false),
        ] {
            assert_eq!(spec.accepts_name(name), accepted, "{} {name:?}", spec.label);
        }
        let bounds: Vec<_> = TRANSFORM
            .params
            .iter()
            .map(|param| {
                (
                    param.lower_bound,
                    param.upper_bound,
                    param.lower_ui_bound,
                    param.upper_ui_bound,
                    param.control,
                )
            })
            .collect();
        assert_eq!(
            bounds,
            [
                ("", "", None, None, "6"),
                ("", "", None, None, "6"),
                ("false", "true", None, None, "4"),
                ("-30000", "30000", Some("-200"), Some("200"), "2"),
                ("-30000", "30000", Some("-200"), Some("200"), "2"),
                ("-70", "70", None, None, "2"),
                ("-32768", "32767", None, None, "3"),
                ("-32768", "32767", None, None, "3"),
                ("0", "100", None, None, "2"),
                ("false", "true", None, None, "4"),
                ("0", "360", None, None, "2"),
                ("0", "1", None, None, "7"),
            ]
        );
        let popups: Vec<_> = TRANSFORM
            .params
            .iter()
            .filter(|param| param.discontinuous_interpolate)
            .map(|param| param.label)
            .collect();
        assert_eq!(popups, ["Sampling"]);
        assert_eq!(TRANSFORM_ANCHOR_POINT.record.tag, "PointComponentParam");
        // Values outside the native bounds are invalid.
        let effect = |skew| PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Transform(PrTransform {
                skew,
                ..CENTERED_TRANSFORM
            }),
            animations: Vec::new(),
        };
        assert!(effect(30.0).validate().is_ok());
        let error = effect(71.0).validate().unwrap_err().to_string();
        assert!(error.contains("Transform Skew 71"), "{error}");
    }

    /// A Transform at the defaults but Position 0.75:0.5 (clip A's placement).
    const CENTERED_TRANSFORM: PrTransform = PrTransform {
        anchor_point: [0.5, 0.5],
        position: [0.75, 0.5],
        uniform_scale: false,
        scale_height: 100.0,
        scale_width: 100.0,
        skew: 0.0,
        skew_axis: 0.0,
        rotation: 0.0,
        opacity: 100.0,
        composition_shutter_angle: true,
        shutter_angle: 0.0,
        bicubic_sampling: false,
    };

    #[test]
    fn transform_rejects_skew_keys_and_reports_each_approximated_parameter() {
        let keys_at = |param: &'static EffectParamSpec,
                       values: [f64; 2],
                       times: [i64; 2],
                       easing: PrKeyframeEasing| PrEffectParamAnimation {
            param,
            keys: PrEffectParamKeys::Scalar(
                values
                    .into_iter()
                    .zip(times)
                    .map(|(value, source_ticks)| PrScalarKeyframe {
                        source_ticks,
                        value,
                        easing,
                    })
                    .collect(),
            ),
        };
        let keys = |param: &'static EffectParamSpec, values: [f64; 2]| {
            keys_at(param, values, [TICKS, 2 * TICKS], PrKeyframeEasing::Linear)
        };
        // Scale Width and Scale Height tracks that reach the same extrema.
        let both_axes = |width: PrEffectParamAnimation| {
            vec![keys(&TRANSFORM_SCALE_HEIGHT, [100.0, 200.0]), width]
        };
        let transform =
            |uniform_scale, scale_width, skew, rotation, shutter: (bool, f64)| PrTransform {
                uniform_scale,
                scale_width,
                skew,
                rotation,
                composition_shutter_angle: shutter.0,
                shutter_angle: shutter.1,
                ..CENTERED_TRANSFORM
            };
        let rotation = "Transform Skew 30 with a Rotation converts with FX's composition of skew, rotation and scale; skew with rotation or non-uniform scale is unmeasured (native measurements cover Rotation 0 and Scale 100/100 only)";
        let stretch = "Transform Skew 30 with unequal Scale Width and Scale Height converts";
        let opacity_error = "converted as sRGB opacity (Transform Opacity 50 with clip Opacity 50: mean error ≈ 22 levels, p99 ≈ 80)";
        /// (values, keys, the rejection or the start of each warning)
        type Case<'a> = (
            PrTransform,
            Vec<PrEffectParamAnimation>,
            Result<&'a [&'a str], &'a str>,
        );
        #[rustfmt::skip]
        let cases: [Case<'_>; 22] = [
            // Clip C: Skew 30 / Axis 45 at rotation 0 and 100/100.
            (transform(false, 100.0, 30.0, 0.0, (true, 0.0)), vec![], Ok(&[])),
            // Clip B's uniform 50 turned 30 without skew; the axis alone is inert.
            (transform(true, 100.0, 0.0, 30.0, (true, 0.0)), vec![], Ok(&[])),
            (PrTransform { skew_axis: 45.0, ..transform(true, 100.0, 0.0, 30.0, (true, 0.0)) }, vec![], Ok(&[])),
            // Uniform Scale commutes with the shear, keyed or not.
            (transform(true, 100.0, 30.0, 0.0, (true, 0.0)), vec![keys(&TRANSFORM_SCALE_HEIGHT, [100.0, 200.0])], Ok(&[])),
            // Box on: the saved Shutter Angle, static or keyed, is not rendered; box off, angle 0: no blur.
            (transform(false, 100.0, 0.0, 0.0, (true, 180.0)), vec![], Ok(&[])),
            (transform(false, 100.0, 0.0, 0.0, (true, 180.0)), vec![keys(&TRANSFORM_SHUTTER_ANGLE, [180.0, 90.0])], Ok(&[])),
            (transform(false, 100.0, 0.0, 0.0, (false, 0.0)), vec![], Ok(&[])),
            (transform(false, 100.0, 30.0, 30.0, (true, 0.0)), vec![], Ok(&[rotation])),
            (transform(false, 100.0, 30.0, 0.0, (true, 0.0)), vec![keys(&TRANSFORM_ROTATION, [0.0, 90.0])], Ok(&[rotation])),
            (transform(false, 70.0, 30.0, 0.0, (true, 0.0)), vec![], Ok(&[stretch])),
            (transform(false, 100.0, 30.0, 0.0, (true, 0.0)), vec![keys(&TRANSFORM_SCALE_WIDTH, [100.0, 70.0])], Ok(&[stretch])),
            // Without Uniform Scale the axes are equal only on identical
            // tracks: the same extrema at other times or on another easing
            // differ between the keys.
            (transform(false, 100.0, 30.0, 0.0, (true, 0.0)), both_axes(keys(&TRANSFORM_SCALE_WIDTH, [100.0, 200.0])), Ok(&[])),
            (transform(false, 100.0, 30.0, 0.0, (true, 0.0)), both_axes(keys_at(&TRANSFORM_SCALE_WIDTH, [100.0, 200.0], [TICKS, 3 * TICKS], PrKeyframeEasing::Linear)), Ok(&[stretch])),
            (transform(false, 100.0, 30.0, 0.0, (true, 0.0)), both_axes(keys_at(&TRANSFORM_SCALE_WIDTH, [100.0, 200.0], [TICKS, 2 * TICKS], PrKeyframeEasing::Hold)), Ok(&[stretch])),
            (transform(false, 100.0, 30.0, 30.0, (true, 0.0)), vec![keys(&TRANSFORM_SCALE_WIDTH, [100.0, 70.0])], Ok(&["Transform Skew 30 with a Rotation and unequal Scale Width and Scale Height converts"])),
            (transform(false, 100.0, 30.0, 0.0, (true, 0.0)), vec![keys(&TRANSFORM_SKEW, [30.0, 40.0])], Err(PrTransform::KEYED_SKEW)),
            (transform(false, 100.0, 0.0, 0.0, (true, 0.0)), vec![keys(&TRANSFORM_SKEW_AXIS, [0.0, 45.0])], Err(PrTransform::KEYED_SKEW)),
            // Scale Width keys under Uniform Scale are an omission (below).
            (transform(true, 100.0, 0.0, 0.0, (true, 0.0)), vec![keys(&TRANSFORM_SCALE_WIDTH, [100.0, 70.0])], Ok(&[])),
            // Clip F: box off at 180 blurs; a keyed angle converts as its first key.
            (transform(false, 100.0, 0.0, 0.0, (false, 180.0)), vec![], Ok(&["Transform motion blur (Shutter Angle 180) approximated by FX motion blur (at 180°: blur edges 13.1-13.7 px wide against Premiere's 12.0-12.2; FX's blur is one frame late at every start and stop of the motion)"])),
            (transform(false, 100.0, 0.0, 0.0, (false, 180.0)), vec![keys(&TRANSFORM_SHUTTER_ANGLE, [180.0, 90.0])], Ok(&["Transform motion blur (Shutter Angle 180)", "keyed Transform Shutter Angle converts as its first key's value 180: the FX composition shutter has no keys (unmeasured against Premiere)"])),
            // Clip A: Opacity 50 (T6), static or keyed; bicubic Sampling.
            (PrTransform { opacity: 50.0, bicubic_sampling: true, ..CENTERED_TRANSFORM }, vec![], Ok(&["Transform Opacity 50 blends in linear light in Premiere; ", "Transform Sampling 1 (bicubic) has no FX equivalent; bilinear used (unmeasured against Premiere)"])),
            (CENTERED_TRANSFORM, vec![keys(&TRANSFORM_OPACITY, [100.0, 50.0])], Ok(&["keyed Transform Opacity blends in linear light in Premiere; "])),
        ];
        for (transform, animations, expected) in cases {
            let result = transform
                .ensure_convertible(&animations)
                .map(|()| transform.approximations(&animations));
            match (result, expected) {
                (Ok(warnings), Ok(starts)) => {
                    assert_eq!(warnings.len(), starts.len(), "{transform:?}: {warnings:?}");
                    for (warning, start) in warnings.iter().zip(starts) {
                        assert!(warning.starts_with(start), "{transform:?}: {warning}");
                        if warning.contains("Opacity") {
                            assert!(warning.ends_with(opacity_error), "{warning}");
                        }
                    }
                }
                (Err(error), Err(reason)) => assert_eq!(error, reason, "{transform:?}"),
                (result, expected) => panic!("{transform:?}: {result:?}, expected {expected:?}"),
            }
        }
        assert_eq!(
            transform(true, 100.0, 0.0, 0.0, (true, 0.0))
                .unimported_scale_width_keys(&[keys(&TRANSFORM_SCALE_WIDTH, [100.0, 70.0])]),
            Some("Transform Scale Width keys under Uniform Scale were not imported: Premiere renders Scale Height on both axes (inferred from a static Scale Width sample)")
        );
        // The FX scale: Scale Height on both axes under Uniform Scale.
        assert_eq!(
            transform(true, 70.0, 0.0, 0.0, (true, 0.0)).scale(),
            [100.0, 100.0]
        );
        assert_eq!(
            transform(false, 70.0, 0.0, 0.0, (true, 0.0)).scale(),
            [70.0, 100.0]
        );
    }
}
