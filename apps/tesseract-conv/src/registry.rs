//! Static format capabilities and their CLI presentation metadata.

use std::path::Path;

use anyhow::Context;
use clap::builder::PossibleValuesParser;
use fx_conv::{Diagnostic, DiagnosticKind};

use crate::formats::{
    self, after_effects_targets, export_after_effects, export_premiere, import_after_effects,
    import_premiere, premiere_targets, ConversionRequest, Handler, TargetLister,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConversionOption {
    Composition,
    ExpressionSamples,
    AvailableFonts,
    MediaMap,
    MediaRelink,
    Fps,
    Sequence,
}

impl ConversionOption {
    const ALL: [Self; 7] = [
        Self::Composition,
        Self::ExpressionSamples,
        Self::AvailableFonts,
        Self::MediaMap,
        Self::MediaRelink,
        Self::Fps,
        Self::Sequence,
    ];

    fn is_present(self, request: &ConversionRequest<'_>) -> bool {
        match self {
            Self::Composition => request.composition.is_some(),
            Self::ExpressionSamples => request.expression_samples.is_some(),
            Self::AvailableFonts => request.available_fonts.is_some(),
            Self::MediaMap => request.media_map.is_some(),
            Self::MediaRelink => request.media_relink.is_some(),
            Self::Fps => request.fps.is_some(),
            Self::Sequence => request.sequence.is_some(),
        }
    }

    const fn misuse_message(self) -> &'static str {
        match self {
            Self::Composition => {
                "--composition is only supported for After Effects to Tesseract conversion"
            }
            Self::ExpressionSamples => {
                "--expression-samples is only supported for After Effects to Tesseract conversion"
            }
            Self::AvailableFonts => {
                "--available-fonts is only supported for After Effects to Tesseract conversion"
            }
            Self::MediaMap => "--media-map is only supported for Adobe to Tesseract import",
            Self::MediaRelink => {
                "--media-relink is only supported for Premiere to Tesseract import"
            }
            Self::Fps => {
                "--fps is only supported for Tesseract to Premiere or After Effects conversion"
            }
            Self::Sequence => "--sequence is only supported for Premiere to Tesseract conversion",
        }
    }
}

#[derive(Debug)]
pub(super) struct Route {
    handler: Handler,
    allowed_options: &'static [ConversionOption],
    check_message: &'static str,
    write_message: &'static str,
    /// Ends every warning but an approximation, whose content converted.
    warning_suffix: &'static str,
    check_fx_schema_version: bool,
}

impl Route {
    pub(super) fn run(
        &self,
        request: &ConversionRequest<'_>,
        json: bool,
    ) -> anyhow::Result<String> {
        self.validate_options(request)?;
        validate_output_parent(request.output)?;
        if self.check_fx_schema_version {
            // Leave invalid-input errors to the format handler, which adds its own context.
            if let Ok(archive) = tesseract_file::TesseractFile::open(request.input) {
                if let Some(notice) = archive.metadata().fx_schema_version_notice("converter") {
                    if json {
                        crate::progress::write_event(
                            &serde_json::json!({"schemaVersion": 1, "type": "diagnostic", "command": "convert", "level": "info", "message": notice}),
                        );
                    } else {
                        eprintln!("{notice}");
                    }
                }
            }
        }
        let report = (self.handler)(request)
            .with_context(|| format!("convert {}", request.input.display()))?;
        let diagnostics: Vec<_> = report.diagnostics.iter().map(|diagnostic| {
            let value = serde_json::json!({
                "code": diagnostic.code,
                "kind": match diagnostic.kind {
                    DiagnosticKind::Omission => "omission",
                    DiagnosticKind::Approximation => "approximation",
                    DiagnosticKind::Warning => "warning",
                },
                "context": diagnostic.context,
                "message": self.warning(diagnostic),
            });
            if json {
                crate::progress::write_event(&serde_json::json!({"schemaVersion": 1, "type": "diagnostic", "command": "convert", "diagnostic": value}));
            } else {
                eprintln!("{}", self.warning(diagnostic));
            }
            value
        }).collect();
        let message = if request.mode.is_check() {
            self.check_message
        } else {
            self.write_message
        };
        if json {
            let artifacts: Vec<_> = report
                .artifacts
                .iter()
                .map(|artifact| {
                    serde_json::json!({
                        "path": artifact.path,
                        "kind": match artifact.kind {
                            fx_conv::ArtifactKind::Project => "project",
                            fx_conv::ArtifactKind::Media => "media",
                        },
                    })
                })
                .collect();
            Ok(serde_json::to_string_pretty(&serde_json::json!({
                "mode": if request.mode.is_check() { "check" } else { "write" },
                "input": request.input, "output": request.output,
                "artifactStatus": if request.mode.is_check() { "planned" } else { "published" },
                "artifacts": artifacts, "diagnostics": diagnostics, "message": message,
            }))?)
        } else {
            Ok(message.to_owned())
        }
    }

    /// The stderr line that reports `diagnostic`.
    fn warning(&self, diagnostic: &Diagnostic) -> String {
        let suffix = match diagnostic.kind {
            DiagnosticKind::Approximation => "",
            DiagnosticKind::Omission | DiagnosticKind::Warning => self.warning_suffix,
        };
        format!("warning: {diagnostic}{suffix}")
    }

    fn validate_options(&self, request: &ConversionRequest<'_>) -> anyhow::Result<()> {
        for option in ConversionOption::ALL {
            anyhow::ensure!(
                !option.is_present(request) || self.allowed_options.contains(&option),
                option.misuse_message()
            );
        }
        Ok(())
    }
}

/// Name the missing output directory before format handlers report a bare I/O error.
fn validate_output_parent(output: &Path) -> anyhow::Result<()> {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    anyhow::ensure!(
        parent.is_dir(),
        "output parent directory {} does not exist",
        parent.display()
    );
    Ok(())
}

struct ImportRoute {
    route: Route,
    list_targets: TargetLister,
}

pub(super) struct FormatRegistration {
    pub(super) id: &'static str,
    label: &'static str,
    extensions: &'static [&'static str],
    import: Option<&'static ImportRoute>,
    export: Option<&'static Route>,
    pub(super) inspect: Option<fn(&Path, bool) -> anyhow::Result<String>>,
    pub(super) media_inspector: Option<formats::MediaInspector>,
    hub: bool,
}

impl FormatRegistration {
    pub(super) fn list_targets(&self, input: &Path) -> anyhow::Result<Vec<fx_conv::ImportTarget>> {
        let import = self.import.ok_or_else(|| {
            anyhow::anyhow!("inspect is not supported for {} sources", self.label)
        })?;
        (import.list_targets)(input)
    }
}

const AFTER_EFFECTS_IMPORT_OPTIONS: &[ConversionOption] = &[
    ConversionOption::Composition,
    ConversionOption::ExpressionSamples,
    ConversionOption::AvailableFonts,
    ConversionOption::MediaMap,
];
const PREMIERE_IMPORT_OPTIONS: &[ConversionOption] = &[
    ConversionOption::Sequence,
    ConversionOption::MediaMap,
    ConversionOption::MediaRelink,
];
const EXPORT_OPTIONS: &[ConversionOption] = &[ConversionOption::Fps];

static AFTER_EFFECTS_IMPORT: ImportRoute = ImportRoute {
    list_targets: after_effects_targets,
    route: Route {
    handler: import_after_effects,
    allowed_options: AFTER_EFFECTS_IMPORT_OPTIONS,
    check_message: "After Effects structure can be imported with the reported approximations (visual fidelity not verified).",
    write_message: "Saved best-effort After Effects structure as project.tsrct (see warnings; visual fidelity not verified).",
    warning_suffix: "",
    check_fx_schema_version: false,
    },
};
static AFTER_EFFECTS_EXPORT: Route = Route {
    handler: export_after_effects,
    allowed_options: EXPORT_OPTIONS,
    check_message: "Experimental editable AEP export preflight completed (see omissions; Adobe acceptance unverified).",
    write_message: "Saved experimental project.aep (supported editable subset; see omissions; Adobe acceptance unverified).",
    warning_suffix: "",
    check_fx_schema_version: true,
};
static PREMIERE_IMPORT: ImportRoute = ImportRoute {
    list_targets: premiere_targets,
    route: Route {
        handler: import_premiere,
        allowed_options: PREMIERE_IMPORT_OPTIONS,
        check_message: "Premiere to Tesseract conversion is valid.",
        write_message: "Saved project.tsrct.",
        warning_suffix: " (not converted)",
        check_fx_schema_version: false,
    },
};
static PREMIERE_EXPORT: Route = Route {
    handler: export_premiere,
    allowed_options: EXPORT_OPTIONS,
    check_message: "Tesseract to Premiere conversion is valid.",
    write_message: "Saved Premiere project.",
    warning_suffix: "",
    check_fx_schema_version: true,
};

pub(super) static FORMATS: &[FormatRegistration] = &[
    FormatRegistration {
        id: "after-effects",
        label: "After Effects",
        extensions: &["aep"],
        import: Some(&AFTER_EFFECTS_IMPORT),
        export: Some(&AFTER_EFFECTS_EXPORT),
        inspect: Some(crate::inspect::inspect),
        media_inspector: Some(formats::after_effects_media),
        hub: false,
    },
    FormatRegistration {
        id: "premiere",
        label: "Premiere",
        extensions: &["prproj"],
        import: Some(&PREMIERE_IMPORT),
        export: Some(&PREMIERE_EXPORT),
        inspect: None,
        media_inspector: Some(formats::premiere_media),
        hub: false,
    },
    FormatRegistration {
        id: "tesseract",
        label: "Tesseract",
        extensions: &["tsrct"],
        import: None,
        export: None,
        inspect: None,
        media_inspector: None,
        hub: true,
    },
];

pub(super) fn format_value_parser() -> PossibleValuesParser {
    PossibleValuesParser::new(FORMATS.iter().map(|format| format.id))
}

pub(super) fn by_id(id: &str) -> anyhow::Result<&'static FormatRegistration> {
    FORMATS
        .iter()
        .find(|format| format.id == id)
        .with_context(|| format!("unknown format {id}"))
}

pub(super) fn infer(input: &Path) -> anyhow::Result<&'static FormatRegistration> {
    let extension = input.extension().and_then(|extension| extension.to_str());
    FORMATS
        .iter()
        .find(|format| {
            extension.is_some_and(|extension| {
                format
                    .extensions
                    .iter()
                    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
            })
        })
        .ok_or_else(|| {
            let mut choices = FORMATS
                .iter()
                .map(|format| format!("--from {}", format.id))
                .collect::<Vec<_>>();
            let last = choices
                .pop()
                .expect("the static format registry is nonempty");
            anyhow::anyhow!(
                "cannot infer input format; pass {}, or {last}",
                choices.join(", ")
            )
        })
}

pub(super) fn resolve_route(
    from: &FormatRegistration,
    to: &FormatRegistration,
) -> anyhow::Result<&'static Route> {
    anyhow::ensure!(
        from.id != to.id,
        "source and destination formats must differ"
    );

    if from.hub {
        return to
            .export
            .ok_or_else(|| anyhow::anyhow!("conversion to {} is not supported", to.label));
    }
    if to.hub {
        return from.import.map(|import| &import.route).ok_or_else(|| {
            anyhow::anyhow!(
                "{} is not supported as a source for Tesseract import",
                from.label
            )
        });
    }
    anyhow::bail!(
        "direct conversion from {} to {} is not supported; use Tesseract as the interchange format",
        from.label,
        to.label
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_conv::ConversionMode;

    #[test]
    fn missing_output_parent_is_named() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing");
        let error = validate_output_parent(&missing.join("out")).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "output parent directory {} does not exist",
                missing.display()
            )
        );
        assert!(validate_output_parent(&directory.path().join("out")).is_ok());
        assert!(validate_output_parent(Path::new("out")).is_ok());
    }

    #[test]
    fn infers_registered_extensions_case_insensitively() {
        assert_eq!(infer(Path::new("project.AEP")).unwrap().id, "after-effects");
        assert_eq!(infer(Path::new("project.PrPrOj")).unwrap().id, "premiere");
        assert_eq!(infer(Path::new("project.TSRCT")).unwrap().id, "tesseract");
        assert!(infer(Path::new("project.unknown")).is_err());
    }

    #[test]
    fn import_only_registration_exposes_only_its_registered_direction() {
        let hub = by_id("tesseract").unwrap();
        let svg = FormatRegistration {
            id: "svg",
            label: "SVG",
            extensions: &["svg"],
            // Route resolution only needs capability metadata; no SVG converter is implemented.
            import: by_id("after-effects").unwrap().import,
            export: None,
            inspect: None,
            media_inspector: None,
            hub: false,
        };

        assert!(resolve_route(&svg, hub).is_ok());
        assert_eq!(
            resolve_route(hub, &svg).unwrap_err().to_string(),
            "conversion to SVG is not supported"
        );
    }

    #[test]
    fn unavailable_and_same_format_directions_reject() {
        let after_effects = by_id("after-effects").unwrap();
        let premiere = by_id("premiere").unwrap();
        assert_eq!(
            resolve_route(after_effects, premiere)
                .unwrap_err()
                .to_string(),
            "direct conversion from After Effects to Premiere is not supported; use Tesseract as the interchange format"
        );
        assert_eq!(
            resolve_route(premiere, after_effects)
                .unwrap_err()
                .to_string(),
            "direct conversion from Premiere to After Effects is not supported; use Tesseract as the interchange format"
        );
        assert_eq!(
            resolve_route(premiere, premiere).unwrap_err().to_string(),
            "source and destination formats must differ"
        );
    }

    #[test]
    fn option_allowlist_rejects_before_calling_handler() {
        let request = ConversionRequest {
            input: Path::new("missing.tsrct"),
            output: Path::new("unused-output"),
            sequence: None,
            composition: Some(1),
            expression_samples: None,
            available_fonts: None,
            media_map: None,
            media_relink: None,
            fps: None,
            mode: ConversionMode::Check,
            progress: fx_conv::Progress::default(),
        };
        let route = resolve_route(by_id("tesseract").unwrap(), by_id("premiere").unwrap()).unwrap();
        assert_eq!(
            route.run(&request, false).unwrap_err().to_string(),
            "--composition is only supported for After Effects to Tesseract conversion"
        );
    }

    #[test]
    fn premiere_import_suffixes_every_warning_but_an_approximation() {
        let route = resolve_route(by_id("premiere").unwrap(), by_id("tesseract").unwrap()).unwrap();
        for (kind, line) in [
            (
                DiagnosticKind::Omission,
                "warning: feature clip: reason (not converted)",
            ),
            (
                DiagnosticKind::Warning,
                "warning: feature clip: reason (not converted)",
            ),
            (
                DiagnosticKind::Approximation,
                "warning: feature clip: reason",
            ),
        ] {
            let diagnostic = Diagnostic {
                code: "PREMIERE-FEATURE",
                kind,
                context: Some("feature clip".into()),
                message: "feature clip: reason".into(),
            };
            assert_eq!(route.warning(&diagnostic), line, "{kind:?}");
        }
    }
}
