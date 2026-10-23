//! Per-effect AE Compositing Options (`ADBE Effect Built In Params`).
//!
//! The natively saved `color_ae_effect_compositing_options.aep` (AE 26.5,
//! Levels with Mask Reference 1 and Effect Opacity 50) pins the instance
//! layout written here:
//!
//! ```text
//! ADBE Effect Built In Params  tdgp "Compositing Options"
//!   ADBE Effect Mask Parade    tdgp "-_0_/-"
//!     ADBE Effect Mask         tdgp "Mask Reference <n>"
//!       ADBE Effect Path Stream Ref  tdbs (hidden-root descriptor, tdsb 1) + tdli <mask identity>
//!   ADBE Effect Mask Opacity   tdbs float slider, percent, bounds 0..100
//! ```
//!
//! AE omits each control while it holds its default (no masks, 100%), which
//! is also how the importer reads absence. Only the layout with both controls
//! present is pinned; each control alone is written by the same rule. The
//! `ADBE Force CPU GPU` render-device choice has no FX field and is never
//! written.

use super::views::{self, ValueKind};
use crate::rifx::{Chunk, RifxError};

pub(super) const MATCH_NAME: &str = "ADBE Effect Built In Params";

/// Effect Compositing Options of one fresh native effect instance.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeEffectCompositing {
    /// Native Effect Opacity percentage (`0..=100`; AE's default is 100).
    pub effect_opacity_percent: f64,
    /// Native identities (`mkif` identity words) of the owner's layer masks
    /// that restrict this effect, in Mask Reference order.
    pub mask_identities: Vec<u32>,
    /// FX masks referenced by the source effect that export has not yet
    /// resolved to native identities. Export resolves or diagnoses every one
    /// on the final layers; the writer never writes them (payload validation
    /// during lowering runs before resolution).
    pub unresolved_mask_references: Vec<fx_schema::FxItemId>,
}

impl Default for NativeEffectCompositing {
    fn default() -> Self {
        Self {
            effect_opacity_percent: 100.0,
            mask_identities: Vec::new(),
            unresolved_mask_references: Vec::new(),
        }
    }
}

impl NativeEffectCompositing {
    /// Reject values AE cannot store and references a layer of `mask_count`
    /// fresh masks does not have.
    pub(super) fn validate(&self, mask_count: usize) -> Result<(), RifxError> {
        if !(0.0..=100.0).contains(&self.effect_opacity_percent) {
            return Err(RifxError::Invalid("Effect Opacity outside 0..100"));
        }
        let count = u32::try_from(mask_count).unwrap_or(u32::MAX);
        if self
            .mask_identities
            .iter()
            .any(|identity| *identity == 0 || *identity > count)
        {
            return Err(RifxError::Invalid(
                "effect mask reference names no native layer mask",
            ));
        }
        Ok(())
    }
}

/// The instance's Compositing Options group.
pub(super) fn group(
    options: &NativeEffectCompositing,
    clock: super::super::keyframes::PropertyClock,
) -> Result<Chunk, RifxError> {
    let mut entries = Vec::new();
    if !options.mask_identities.is_empty() {
        let masks = options
            .mask_identities
            .iter()
            .enumerate()
            .map(|(index, identity)| {
                Ok((
                    "ADBE Effect Mask",
                    views::group(
                        1,
                        &format!("Mask Reference {}", index + 1),
                        vec![(
                            "ADBE Effect Path Stream Ref",
                            mask_reference(*identity, clock)?,
                        )],
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, RifxError>>()?;
        entries.push(("ADBE Effect Mask Parade", views::group(1, "-_0_/-", masks)?));
    }
    if options.effect_opacity_percent != 100.0 {
        entries.push((
            "ADBE Effect Mask Opacity",
            views::property_with_clock(
                ValueKind::EffectFloat,
                &[options.effect_opacity_percent],
                Some((0.0, 100.0)),
                None,
                clock,
            )?,
        ));
    }
    views::group(1, "Compositing Options", entries)
}

/// AE saves a mask reference with the hidden effect-root descriptor, leaf
/// flags 1 and the referenced mask identity in `tdli`.
fn mask_reference(
    identity: u32,
    clock: super::super::keyframes::PropertyClock,
) -> Result<Chunk, RifxError> {
    let mut leaf = views::property_with_clock(ValueKind::EffectRoot, &[0.0], None, None, clock)?;
    let children = leaf
        .children_mut()
        .ok_or(RifxError::Invalid("mask reference leaf is not a LIST"))?;
    let flags = children
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdsb")
        .ok_or(RifxError::Invalid("mask reference leaf has no flags"))?;
    *flags = Chunk::data(*b"tdsb", 1_u32.to_be_bytes())?;
    children.push(Chunk::data(*b"tdli", identity.to_be_bytes())?);
    Ok(leaf)
}
