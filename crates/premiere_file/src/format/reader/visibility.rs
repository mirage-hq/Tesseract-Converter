//! Native `IsMuted` on video clip items and video tracks.
//!
//! `ClipTrackItem/IsMuted=true` is read as clip Enable off and
//! `Track/IsMuted=true` as track video output (the timeline "eye") off. The
//! picture is verified by the AME render of the derived
//! `premiere_isolated_clip_disabled` fixture, which shows no clip that has
//! either field set. The field semantics are inferred, not Adobe-documented:
//! the clip field was observed on one production project, and the corpus has
//! the track field only on empty tracks. An absent field means enabled. Track
//! output is flattened onto every occurrence of the track because the FX
//! document has per-layer visibility and no track concept.

use super::video::native_bool;
use crate::error::Result;

/// Decodes one native `IsMuted` field; an absent field means enabled.
pub(super) fn is_muted(value: Option<&str>, identity: &str) -> Result<bool> {
    value.map_or(Ok(false), |value| native_bool(value, identity, "IsMuted"))
}
