//! Format-specific CLI adapters around the typed conversion traits.

use std::path::Path;

use aftereffects_file::{AfterEffects, AfterEffectsExportOptions, AfterEffectsImportOptions};
use fx_conv::{
    ConversionMode, ConversionReport, ExportFromTesseract, ImportTarget, ImportToTesseract,
};
use premiere_file::{FrameRate, Premiere, PremiereExportOptions, PremiereImportOptions};

/// Conversion inputs shared by every registered format handler.
pub(super) struct ConversionRequest<'a> {
    pub(super) input: &'a Path,
    pub(super) output: &'a Path,
    pub(super) sequence: Option<&'a str>,
    pub(super) composition: Option<u32>,
    pub(super) expression_samples: Option<&'a Path>,
    pub(super) media_map: Option<&'a Path>,
    pub(super) fps: Option<&'a str>,
    pub(super) mode: ConversionMode,
    pub(super) progress: fx_conv::Progress<'a>,
}

pub(super) type Handler = fn(&ConversionRequest<'_>) -> anyhow::Result<ConversionReport>;
pub(super) type TargetLister = fn(&Path) -> anyhow::Result<Vec<ImportTarget>>;
pub(super) type MediaInspector =
    fn(&Path, &str, Option<&fx_conv::ValidatedMediaMap>) -> anyhow::Result<fx_conv::MediaPreflight>;

pub(super) fn after_effects_media(
    input: &Path,
    target: &str,
    map: Option<&fx_conv::ValidatedMediaMap>,
) -> anyhow::Result<fx_conv::MediaPreflight> {
    Ok(AfterEffects.inspect_media(
        input,
        &AfterEffectsImportOptions {
            composition: Some(target.parse()?),
            ..Default::default()
        },
        map,
    )?)
}

pub(super) fn premiere_media(
    input: &Path,
    target: &str,
    map: Option<&fx_conv::ValidatedMediaMap>,
) -> anyhow::Result<fx_conv::MediaPreflight> {
    Ok(Premiere.inspect_media(
        input,
        &PremiereImportOptions {
            sequence: Some(target.to_owned()),
        },
        map,
    )?)
}

pub(super) fn after_effects_targets(input: &Path) -> anyhow::Result<Vec<ImportTarget>> {
    Ok(AfterEffects.list_import_targets(input)?)
}

pub(super) fn premiere_targets(input: &Path) -> anyhow::Result<Vec<ImportTarget>> {
    Ok(Premiere.list_import_targets(input)?)
}

fn run_import<C>(
    converter: &C,
    request: &ConversionRequest<'_>,
    options: &C::Options,
) -> anyhow::Result<ConversionReport>
where
    C: ImportToTesseract,
    C::Error: Send + Sync,
{
    converter
        .import_to_tesseract_with_progress(
            request.input,
            request.output,
            options,
            request.mode,
            request.progress,
        )
        .map(ConversionReport::into_common)
        .map_err(anyhow::Error::new)
}

fn run_export<C>(
    converter: &C,
    request: &ConversionRequest<'_>,
    options: &C::Options,
) -> anyhow::Result<ConversionReport>
where
    C: ExportFromTesseract,
    C::Error: Send + Sync,
{
    converter
        .export_from_tesseract_with_progress(
            request.input,
            request.output,
            options,
            request.mode,
            request.progress,
        )
        .map(ConversionReport::into_common)
        .map_err(anyhow::Error::new)
}

pub(super) fn import_after_effects(
    request: &ConversionRequest<'_>,
) -> anyhow::Result<ConversionReport> {
    let options = AfterEffectsImportOptions {
        composition: request.composition,
        expression_samples: request.expression_samples.map(Path::to_path_buf),
    };
    if let Some(path) = request.media_map {
        let map = fx_conv::ValidatedMediaMap::load(path)?;
        return AfterEffects
            .import_with_media_map_with_progress(
                request.input,
                request.output,
                &options,
                request.mode,
                &map,
                request.progress,
            )
            .map(ConversionReport::into_common)
            .map_err(anyhow::Error::new);
    }
    run_import(&AfterEffects, request, &options)
}

pub(super) fn export_after_effects(
    request: &ConversionRequest<'_>,
) -> anyhow::Result<ConversionReport> {
    let options = request
        .fps
        .map(after_effects_options)
        .transpose()?
        .unwrap_or_default();
    run_export(&AfterEffects, request, &options)
}

pub(super) fn import_premiere(request: &ConversionRequest<'_>) -> anyhow::Result<ConversionReport> {
    let options = PremiereImportOptions {
        sequence: request.sequence.map(str::to_owned),
    };
    if let Some(path) = request.media_map {
        let map = fx_conv::ValidatedMediaMap::load(path)?;
        return Premiere
            .import_with_media_map_with_progress(
                request.input,
                request.output,
                &options,
                request.mode,
                &map,
                request.progress,
            )
            .map(ConversionReport::into_common)
            .map_err(anyhow::Error::new);
    }
    run_import(&Premiere, request, &options)
}

pub(super) fn export_premiere(request: &ConversionRequest<'_>) -> anyhow::Result<ConversionReport> {
    let options = PremiereExportOptions {
        frame_rate: request.fps.map(premiere_rate).transpose()?,
    };
    crate::hybrid::export(request, &options)
}

/// Premiere's exact sequence rate for an `--fps` value. 50 and 60 fps have no
/// Premiere-authored time display format, so they reject.
fn premiere_rate(fps: &str) -> anyhow::Result<FrameRate> {
    Ok(match fps {
        "23.976" => FrameRate::Fps24000Over1001,
        "24" => FrameRate::Fps24,
        "25" => FrameRate::Fps25,
        "29.97" => FrameRate::Fps30000Over1001,
        "30" => FrameRate::Fps30,
        "59.94" => FrameRate::Fps60000Over1001,
        _ => anyhow::bail!(
            "--fps {fps} is not supported for Premiere export; use 23.976, 24, 25, 29.97, 30 or 59.94"
        ),
    })
}

/// After Effects takes the decimal rate stored by its compositions; the
/// exporter performs the format-specific range validation.
fn after_effects_options(fps: &str) -> anyhow::Result<AfterEffectsExportOptions> {
    let fps = fps
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("--fps {fps} is not a number"))?;
    Ok(AfterEffectsExportOptions { fps })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_format_specific_numeric_options() {
        assert_eq!(
            premiere_rate("23.976").unwrap(),
            FrameRate::Fps24000Over1001
        );
        assert_eq!(premiere_rate("59.94").unwrap(), FrameRate::Fps60000Over1001);
        assert_eq!(after_effects_options("60").unwrap().fps, 60.0);
        assert!(premiere_rate("60")
            .unwrap_err()
            .to_string()
            .contains("not supported"));
        assert_eq!(
            after_effects_options("fast").unwrap_err().to_string(),
            "--fps fast is not a number"
        );
    }

    #[test]
    fn invalid_numeric_option_rejects_before_input_io() {
        let request = ConversionRequest {
            input: Path::new("missing.tsrct"),
            output: Path::new("unused-output"),
            sequence: None,
            composition: None,
            expression_samples: None,
            media_map: None,
            fps: Some("fast"),
            mode: ConversionMode::Check,
            progress: fx_conv::Progress::default(),
        };
        assert_eq!(
            export_after_effects(&request).unwrap_err().to_string(),
            "--fps fast is not a number"
        );
    }
}
