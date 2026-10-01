//! Premiere CLI orchestration using native conversion and editable linked AEP scopes.
mod automatic;
mod dependencies;
mod owners;
mod package;

use crate::formats::ConversionRequest;
use anyhow::Context;
use fx_conv::{ConversionDiagnostic, ConversionReport, Diagnostic, DiagnosticKind};
use premiere_file::PremiereExportOptions;
use tesseract_file::TesseractFile;

pub(super) fn export(
    request: &ConversionRequest<'_>,
    options: &PremiereExportOptions,
) -> anyhow::Result<ConversionReport> {
    // As in Premiere's own export, an input path that is not UTF-8 rejects
    // before the filesystem resolves it.
    anyhow::ensure!(request.input.to_str().is_some(), "input path must be UTF-8");
    request.progress.stage("read Tesseract project");
    let input = request.input.canonicalize()?;
    let output = package::fresh_output(request.output)?;
    let source_hash = package::sha256(&input)?;
    let archive = TesseractFile::open(&input)?;
    let work = tempfile::Builder::new()
        .prefix(".conversion-hybrid-")
        .tempdir_in(output.parent().context("missing output parent")?)?;
    let staged = automatic::stage(&archive, work.path(), &output, options, request.progress)?;
    request.progress.stage("assemble Premiere package");
    let inputs = package::picture_stage_inputs(&staged.native, &staged.scopes)?;
    let assembly = work.path().join("package");
    std::fs::create_dir(&assembly)?;
    let artifacts = package::assemble(&assembly, &inputs)?;
    package::unchanged(&input, &source_hash)?;
    let mut diagnostics: Vec<_> = staged
        .native
        .report()
        .diagnostics
        .iter()
        .map(ConversionDiagnostic::diagnostic)
        .collect();
    diagnostics.extend(staged.diagnostics);
    for (scope, ae) in &staged.scopes {
        for warning in &ae.report().diagnostics {
            let mut diagnostic = warning.diagnostic();
            diagnostic.context = Some(format!(
                "AEP scope {scope}: {}",
                diagnostic.context.unwrap_or_default()
            ));
            diagnostics.push(diagnostic);
        }
    }
    if !staged.scopes.is_empty() {
        diagnostics.push(Diagnostic {
            code: "HYBRID-EXPERIMENTAL", kind: DiagnosticKind::Warning, context: None,
            message: "Editable linked-AEP package requires Premiere and After Effects. Conversion diagnostics are retained; Adobe acceptance, render/alpha/audio fidelity, edit propagation and relocation have not been measured.".into(),
        });
    }
    if !request.mode.is_check() {
        request.progress.stage("publish Premiere package");
        package::publish(&assembly, &output, &artifacts, &input, &source_hash)?;
    }
    Ok(ConversionReport {
        diagnostics,
        artifacts,
    })
}

#[cfg(test)]
mod tests;
