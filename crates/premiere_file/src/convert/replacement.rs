//! Active replacement footage on export.
//!
//! `source.assetId` is a video layer's original asset. Eye Contact records a
//! generated replacement beside it; while the feature is enabled, the renderer
//! shows the replacement ([`active_asset_id`]). Premiere has no
//! replacement toggle, so export packages and references only the active asset
//! and reports the lineage that the Premiere clip cannot keep.

use crate::{export_loss::OmissionSink, omit, OmissionScope};
use fx_schema::{AssetId, VideoSource};

/// Returns the asset that the renderer shows for `source`: the Eye Contact
/// output while it is enabled, otherwise the original asset.
pub(crate) fn active_asset_id(source: &VideoSource) -> &AssetId {
    source
        .eye_contact
        .as_ref()
        .filter(|eye_contact| eye_contact.enabled)
        .map_or(&source.asset_id, |eye_contact| {
            &eye_contact.eye_contact_asset_id
        })
}

/// Returns the active asset for `source` and reports the lineage that export drops.
pub(super) fn active_video_asset<'a>(
    source: &'a VideoSource,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> &'a AssetId {
    if let Some(eye_contact) = &source.eye_contact {
        let reason = if eye_contact.enabled {
            format!(
                "Eye Contact exported as its active output {:?}; the original asset {:?} and the Eye Contact toggle were not exported",
                eye_contact.eye_contact_asset_id.as_str(),
                source.asset_id.as_str()
            )
        } else {
            format!(
                "inactive Eye Contact output {:?} was not exported",
                eye_contact.eye_contact_asset_id.as_str()
            )
        };
        omit(omissions, OmissionScope::Feature, record, reason);
    }
    active_asset_id(source)
}

/// Names the active Eye Contact output in a packaged-media mismatch: the
/// layer's `sourceIntrinsicDuration` describes the original upload, while
/// export checks the output that it packages.
pub(super) fn active_output_context(source: &VideoSource) -> String {
    let active = active_asset_id(source);
    if *active == source.asset_id {
        String::new()
    } else {
        format!(" of the active Eye Contact output {:?}", active.as_str())
    }
}

#[cfg(test)]
#[path = "tests/replacement.rs"]
mod tests;
