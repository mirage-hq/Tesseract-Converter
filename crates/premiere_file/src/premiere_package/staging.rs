//! Unpublished native Premiere content plus a separately supplied Dynamic Link source.

use std::{ops::Range, path::Path};

use fx_conv::ConversionReport;
use fx_schema::EditableFxCompositionDocument;
use tesseract_file::TesseractFile;

use super::{absolute_output_path, output_is_fresh, prepared::StagedNativePremiereExport};
use crate::{
    error::{unsupported, Result},
    schema::{records::MediaPathField, PrMediaKind, PrVideoStream, PrVideoTrack},
    ConversionError, FrameRate, MediaId, Omission, PrAfterEffectsComposition, PrMedia,
    PrProjectFile, PrVideoOccurrence, Premiere, PremiereExportOptions,
};

const AEP_NAME: &str = "compositions.aep";
const AEP_PATH: &str = "media/compositions.aep";
const TICKS_PER_SECOND: i64 = 254_016_000_000;

/// Independently established source facts for one full-canvas, topmost overlay.
///
/// The composition's clock must equal the sequence clock: source in/out equal
/// timeline in/out. This is not a partitioning API and cannot prove independence
/// from lower layers, parents, mattes, audio, or animation-graph dependencies.
/// No AEP is parsed here. The caller must pair the actual GUID with the exact
/// staged AEP and publish it at the result's `after_effects_path()`.
#[derive(Debug, Clone)]
pub struct AfterEffectsOverlay {
    /// Native Dynamic Link GUID, not an AEP item ID or a project/media UUID.
    pub composition_guid: String,
    /// Canvas of the actual generated composition; must match the sequence.
    pub dimensions: [u32; 2],
    /// Source rate; the caller must verify the native encoded rate agrees.
    pub frame_rate: FrameRate,
    /// Intrinsic duration read from the generated composition's writer plan.
    pub intrinsic_duration_secs: f64,
    /// Frame-aligned occurrence in the unchanged document/sequence clock.
    pub timeline_secs: Range<f64>,
}

/// Owned Premiere files, not a published or complete hybrid package.
///
/// The report lists only files that exist in this staging directory. The linked
/// AEP and its dependencies are owned by the other exporter and must be supplied
/// by the coordinator. Dropping this handle cleans only its private directory.
#[derive(Debug)]
pub struct StagedPremiereExport {
    pub(super) native: StagedNativePremiereExport,
}

impl StagedPremiereExport {
    /// Private files to copy, preserving the report's relative layout.
    pub fn directory(&self) -> &Path {
        self.native.directory()
    }

    /// Owned artifacts and native conversion diagnostics; excludes the foreign AEP.
    pub fn report(&self) -> &ConversionReport<Omission> {
        self.native.report()
    }

    /// Required AEP location relative to the final common package root.
    /// Its own relative media paths must be preserved beneath this file's parent.
    pub fn after_effects_path(&self) -> &Path {
        Path::new(AEP_PATH)
    }
}

impl Premiere {
    /// Prepare remaining native content and insert a separately staged AE overlay.
    ///
    /// `final_output` must be fresh with an existing parent. All authored paths
    /// target that final package, never the private staging directory. No final
    /// output is created. The caller must validate safe topmost scope extraction,
    /// source freshness, complete package inventory, and publication/rollback.
    /// This does not launch Adobe or establish acceptance/fidelity of either file.
    pub fn stage_with_after_effects_overlay(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        staging_parent: &Path,
        final_output: &Path,
        options: &PremiereExportOptions,
        overlay: &AfterEffectsOverlay,
    ) -> std::result::Result<StagedPremiereExport, ConversionError> {
        // Preserve the existing preflight order: collisions fail before JS/media work.
        let output = absolute_output_path(
            final_output,
            &std::env::current_dir().map_err(crate::error::BuildError::from)?,
        )?;
        output_is_fresh(&output)?;
        self.prepare_export(archive, document, options)?
            .stage_with_after_effects_overlay(staging_parent, &output, overlay)
    }
}

pub(super) fn insert_overlay(
    project: &mut PrProjectFile,
    output: &Path,
    overlay: &AfterEffectsOverlay,
) -> Result<()> {
    let identity =
        PrAfterEffectsComposition::parse(&overlay.composition_guid).ok_or_else(|| {
            unsupported("overlay requires a canonical non-nil native composition GUID")
        })?;
    let sequence = project
        .sequences
        .first_mut()
        .ok_or_else(|| unsupported("overlay requires one native sequence"))?;
    if overlay.dimensions != [sequence.width, sequence.height]
        || overlay.frame_rate != sequence.frame_rate
    {
        return Err(unsupported(
            "overlay canvas and frame rate must match the native sequence",
        ));
    }
    let intrinsic_ticks = ticks(overlay.intrinsic_duration_secs)?;
    let start_ticks = ticks(overlay.timeline_secs.start)?;
    let end_ticks = ticks(overlay.timeline_secs.end)?;
    let frame_ticks = sequence.frame_rate.ticks_per_frame();
    if start_ticks >= end_ticks
        || end_ticks > intrinsic_ticks
        || end_ticks > sequence.timeline_end_ticks
        || start_ticks % frame_ticks != 0
        || end_ticks % frame_ticks != 0
    {
        return Err(unsupported(
            "overlay requires a positive frame-aligned range within its unchanged source and sequence clocks",
        ));
    }
    if project
        .media
        .values()
        .any(|media| media.name.eq_ignore_ascii_case(AEP_NAME))
    {
        return Err(unsupported(
            "reserved linked AEP filename collides with native media",
        ));
    }
    let mut suffix = 0_u64;
    let media_id = loop {
        let candidate = MediaId(format!("hybrid-after-effects-{suffix}"));
        if !project.media.contains_key(&candidate) {
            break candidate;
        }
        suffix += 1;
    };
    let relative = format!("./{AEP_PATH}");
    let absolute = output.join(AEP_PATH);
    project.media.insert(
        media_id.clone(),
        PrMedia {
            name: AEP_NAME.into(),
            relative_path: Some(relative.clone()),
            relative_paths: vec![relative],
            absolute_paths: vec![
                (MediaPathField::ActualMediaFilePath, absolute.clone()),
                (MediaPathField::FilePath, absolute),
            ],
            video: Some(PrVideoStream {
                orientation: crate::schema::VideoOrientation::Identity,
                intrinsic_ticks,
                frame_rate: (overlay.frame_rate).into(),
                width: overlay.dimensions[0],
                height: overlay.dimensions[1],
                kind: PrMediaKind::AfterEffectsComposition(identity),
            }),
            audio: None,
        },
    );
    sequence.video_tracks.push(PrVideoTrack {
        nests: Vec::new(),
        transitions: Vec::new(),
        items: vec![crate::PrVideoItem::Media(PrVideoOccurrence::unedited(
            media_id,
            start_ticks..end_ticks,
            start_ticks..end_ticks,
        ))],
    });
    project.validate()?;
    Ok(())
}

fn ticks(seconds: f64) -> Result<i64> {
    let value = seconds * TICKS_PER_SECOND as f64;
    if !value.is_finite() || value < 0.0 || value >= i64::MAX as f64 {
        return Err(unsupported(
            "overlay time must be finite, nonnegative and within native tick range",
        ));
    }
    // Checked range above; round only to Premiere's sub-nanosecond native clock.
    Ok(value.round() as i64)
}

#[cfg(test)]
mod tests;
