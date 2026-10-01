//! Coordinate actual converter results; no parallel capability or proof model.
use super::{dependencies::GraphDependencies, owners::Owners, package};
use aftereffects_file::{AfterEffects, AfterEffectsExportOptions, StagedAfterEffectsPictureExport};
use anyhow::{ensure, Context};
use fx_conv::{ConversionDiagnostic, Diagnostic, DiagnosticKind};
use premiere_file::{
    AfterEffectsPicture, ExportLossDomain, ExportLossSource, FrameRate, PictureReplacement,
    Premiere, PremiereExportOptions, StagedPicturePremiereExport,
};
use std::{collections::BTreeSet, path::Path};
use tesseract_file::TesseractFile;

pub(super) struct StagedExport {
    pub native: StagedPicturePremiereExport,
    pub scopes: Vec<(u16, StagedAfterEffectsPictureExport)>,
    pub diagnostics: Vec<Diagnostic>,
}

pub(super) fn stage(
    archive: &TesseractFile,
    parent: &Path,
    output: &Path,
    options: &PremiereExportOptions,
    progress: fx_conv::Progress<'_>,
) -> anyhow::Result<StagedExport> {
    let document = archive.project();
    let prepared = Premiere.prepare_export_with_progress(archive, document, options, progress)?;
    progress.stage("plan Premiere picture scopes");
    let losses = prepared.losses();
    ensure!(
        !losses.losses_truncated,
        "hybrid export loss report exceeded its limit"
    );
    if !losses.losses.iter().any(|loss| {
        !matches!(
            loss.domain,
            ExportLossDomain::Audio | ExportLossDomain::Metadata
        ) && !matches!(loss.source, ExportLossSource::Document)
    }) {
        progress.stage("stage Premiere project");
        return Ok(StagedExport {
            native: prepared.stage_with_picture_replacements(parent, output, &[])?,
            scopes: Vec::new(),
            diagnostics: Vec::new(),
        });
    }
    let owners = Owners::new(document.composition().layers())?;
    let mut seeds = BTreeSet::new();
    for loss in &losses.losses {
        if matches!(
            loss.domain,
            ExportLossDomain::Audio | ExportLossDomain::Metadata
        ) {
            continue;
        }
        let root = match &loss.source {
            ExportLossSource::Layer(layer) | ExportLossSource::LayerSubtree(layer) => {
                owners.layer(*layer)?
            }
            ExportLossSource::Property(target) => owners.target(target)?,
            // Document/preparation diagnostics remain visible; they are not a
            // request to move the entire project into After Effects.
            ExportLossSource::Document => continue,
        };
        if !matches!(
            document.composition().layers()[root].data(),
            fx_schema::LayerData::Audio(_)
        ) {
            seeds.insert(root);
        }
    }
    if seeds.is_empty() {
        progress.stage("stage Premiere project");
        return Ok(StagedExport {
            native: prepared.stage_with_picture_replacements(parent, output, &[])?,
            scopes: Vec::new(),
            diagnostics: Vec::new(),
        });
    }
    let rate = options.frame_rate.unwrap_or(FrameRate::Fps30);
    let fps = match rate {
        FrameRate::Fps24 => 24.0,
        FrameRate::Fps25 => 25.0,
        FrameRate::Fps30 => 30.0,
        _ => anyhow::bail!("hybrid AEP scopes require an integral 24, 25 or 30 fps clock"),
    };
    let dependencies = GraphDependencies::new(document, &owners)?;
    let recipe = prepared.packing_recipe();
    ensure!(
        recipe.is_complete(),
        "picture packing capture is incomplete"
    );
    let root = recipe
        .containers()
        .into_iter()
        .find(|c| c.token == recipe.root())
        .context("missing root picture container")?;
    let boundaries = recipe.boundaries();
    let retained: BTreeSet<_> = recipe.retained_picture_boundaries(root.token).collect();
    let retained_roots: BTreeSet<_> = boundaries
        .iter()
        .filter(|boundary| retained.contains(&boundary.token))
        .map(|boundary| owners.layer(boundary.layer))
        .collect::<anyhow::Result<_>>()?;
    let mut replacements = Vec::new();
    let mut scopes = Vec::new();
    let mut diagnostics = Vec::new();
    while let Some(seed) = seeds.pop_first() {
        progress.stage("prepare linked AE scope");
        let connected = dependencies.connected(seed);
        for index in &connected {
            seeds.remove(index);
        }
        let first = *connected.first().context("empty picture scope")?;
        let last = *connected.last().context("empty picture scope")?;
        ensure!(scopes.len() < 256, "hybrid picture scope limit exceeded");
        let selected = &document.composition().layers()[first..=last];
        ensure!(selected.iter().enumerate().all(|(offset, layer)| {
            connected.contains(&(first + offset))
                || matches!(layer.data(), fx_schema::LayerData::Audio(_))
        }), "linked roots are interleaved with independent pictures; cannot extract scope {first}..={last}");
        // Audio contributes no picture boundary actions. It may lie inside
        // this interval: picture-only AE staging disables its native switches,
        // while Premiere replay retains its independent sound occurrences.
        let ids: BTreeSet<_> = selected.iter().map(fx_schema::Layer::id).collect();
        let slots: Vec<_> = root
            .boundaries
            .iter()
            .copied()
            .filter(|token| {
                boundaries
                    .iter()
                    .any(|boundary| boundary.token == *token && ids.contains(&boundary.layer))
            })
            .collect();
        ensure!(
            slots.len() == selected.len(),
            "selected roots are not independently replaceable native boundaries"
        );
        let scope = u16::try_from(scopes.len() + 1)?;
        let ae = match AfterEffects.stage_picture_layers_with_progress(
            archive,
            document,
            first..last + 1,
            parent,
            &AfterEffectsExportOptions { fps },
            progress,
        ) {
            Ok(stage) => stage,
            Err(error) => {
                let Some(omissions) = error.picture_scope_omissions() else {
                    return Err(error.into());
                };
                for omission in omissions {
                    let mut diagnostic = omission.diagnostic();
                    diagnostic.context = Some(format!(
                        "AE fallback for roots {first}..={last}: {}",
                        diagnostic.context.unwrap_or_default()
                    ));
                    diagnostics.push(diagnostic);
                }
                diagnostics.push(Diagnostic {
                    code: "HYBRID-NATIVE-RETAINED",
                    kind: DiagnosticKind::Warning,
                    context: Some(format!("roots {first}..={last}")),
                    message: "After Effects could not convert this picture scope; retained the native Premiere result with its reported limitations.".into(),
                });
                continue;
            }
        };
        let omitted_roots: BTreeSet<_> = ae
            .omitted_layer_ids()
            .iter()
            .map(|id| owners.layer(*id))
            .collect::<anyhow::Result<_>>()?;
        if !omitted_roots.is_disjoint(&retained_roots) {
            for omission in &ae.report().diagnostics {
                let mut diagnostic = omission.diagnostic();
                diagnostic.context = Some(format!(
                    "AE fallback for roots {first}..={last}: {}",
                    diagnostic.context.unwrap_or_default()
                ));
                diagnostics.push(diagnostic);
            }
            diagnostics.push(Diagnostic {
                code: "HYBRID-NATIVE-RETAINED",
                kind: DiagnosticKind::Warning,
                context: Some(format!("roots {first}..={last}")),
                message: "After Effects omitted a source layer from a root with retained native picture; kept the complete native scope with its reported limitations instead of deleting supported content.".into(),
            });
            continue;
        }
        let composition = ae.root_composition();
        let frames = (composition.duration_secs() * fps).round();
        ensure!(
            frames.is_finite()
                && frames > 0.0
                && frames <= (i64::MAX / rate.ticks_per_frame()) as f64,
            "AEP duration exceeds native clock"
        );
        let intrinsic = (frames as i64)
            .checked_mul(rate.ticks_per_frame())
            .context("AEP clock overflow")?;
        ensure!(
            root.timeline_end_ticks <= intrinsic,
            "AEP picture is shorter than its native container"
        );
        replacements.push(PictureReplacement {
            packing_id: recipe.id(),
            container: root.token,
            boundaries: slots,
            picture: AfterEffectsPicture {
                composition_guid: composition.dynamic_link_guid().to_owned(),
                relative_path: package::scoped_aep_path(scope)?,
                dimensions: root.dimensions,
                frame_rate: rate,
                intrinsic_duration_ticks: intrinsic,
                timeline_ticks: 0..root.timeline_end_ticks,
                source_ticks: 0..root.timeline_end_ticks,
                enabled: true,
            },
        });
        scopes.push((scope, ae));
    }
    progress.stage("stage Premiere project");
    let native = prepared.stage_with_picture_replacements(parent, output, &replacements)?;
    Ok(StagedExport {
        native,
        scopes,
        diagnostics,
    })
}
