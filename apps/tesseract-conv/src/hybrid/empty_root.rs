//! Preserve a genuinely empty source root as an editable AEP composition.
//! The linked occurrence represents that root, not invented source content.
use super::{automatic::StagedExport, package};
use aftereffects_file::{AfterEffects, AfterEffectsExportOptions};
use anyhow::{ensure, Context};
use premiere_file::{
    AfterEffectsPicture, FrameRate, PremiereExportOptions, PreparedPremiereExport,
};
use std::path::Path;
use tesseract_file::TesseractFile;

pub(super) fn stage(
    archive: &TesseractFile,
    prepared: PreparedPremiereExport<'_>,
    parent: &Path,
    output: &Path,
    options: &PremiereExportOptions,
    progress: fx_conv::Progress<'_>,
) -> anyhow::Result<StagedExport> {
    let document = archive.project();
    ensure!(
        document.composition().layers().is_empty(),
        "source root is not empty"
    );
    let rate = options.frame_rate.unwrap_or(FrameRate::Fps30);
    let fps = match rate {
        FrameRate::Fps24 => 24.0,
        FrameRate::Fps25 => 25.0,
        FrameRate::Fps30 => 30.0,
        FrameRate::Fps50 => 50.0,
        FrameRate::Fps60 => 60.0,
        _ => anyhow::bail!(
            "empty-root linked export requires an exact 24, 25, 30, 50 or 60 fps clock"
        ),
    };
    // This is the original full document, not a selected empty range or a
    // post-omission lowered view. The ordinary writer emits zero authored layers.
    let ae = AfterEffects.stage_picture_only_document_with_progress(
        archive,
        document,
        parent,
        &AfterEffectsExportOptions { fps },
        progress,
    )?;
    let composition = ae.root_composition();
    let dimensions = document.dimensions();
    let (width, height) = composition.dimensions();
    ensure!(
        [u32::from(width), u32::from(height)] == [dimensions.width, dimensions.height]
            && composition.frame_rate() == fps
            && composition.name() == document.composition().name(),
        "empty-root AEP metadata differs from its source"
    );
    let original = document.to_json_value()?;
    let seconds = original["duration"]
        .as_f64()
        .context("missing authored duration")?;
    ensure!(
        composition.duration_secs() == seconds,
        "empty-root linked export cannot preserve the authored duration on this frame clock"
    );
    let frames = (seconds * fps).round();
    ensure!(
        frames.is_finite() && frames > 0.0 && frames < (i64::MAX / rate.ticks_per_frame()) as f64,
        "empty-root duration exceeds native clock"
    );
    let end = (frames as i64)
        .checked_mul(rate.ticks_per_frame())
        .context("empty-root clock overflow")?;
    let picture = AfterEffectsPicture {
        composition_guid: composition.dynamic_link_guid().to_owned(),
        relative_path: package::scoped_aep_path(1)?,
        dimensions: [u32::from(width), u32::from(height)],
        frame_rate: rate,
        intrinsic_duration_ticks: end,
        timeline_ticks: 0..end,
        source_ticks: 0..end,
        enabled: true,
    };
    let native = prepared.stage_empty_root_with_after_effects(parent, output, &picture)?;
    Ok(StagedExport {
        native,
        scopes: vec![(1, ae)],
        diagnostics: Vec::new(),
    })
}
