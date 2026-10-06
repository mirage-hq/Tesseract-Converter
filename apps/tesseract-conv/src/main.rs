//! Standalone conversion of editable FX documents.

mod formats;
mod hybrid;
mod inspect;
mod preflight;
mod progress;
mod registry;
mod targets;
mod transcode;

use clap::{Args, CommandFactory, Parser, Subcommand};
use formats::ConversionRequest;
use fx_conv::ConversionMode;
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "tsrct-conv",
    version,
    about = "Convert editable projects to or from Tesseract",
    after_help = format!("fxSchemaVersion (this converter): {}", fx_schema::FX_SCHEMA_REVISION),
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct ConversionOptions {
    /// Source format (inferred from the input extension when omitted).
    #[arg(long, value_parser = registry::format_value_parser())]
    from: Option<String>,
    /// Select a Premiere sequence by GUID when converting to Tesseract.
    #[arg(long)]
    sequence: Option<String>,
    /// Opt in to the measured, sampled Film Impact Pop geometry approximation
    /// on Premiere to Tesseract import (disabled by default).
    #[arg(long)]
    allow_film_impact_pop: bool,
    /// Select an After Effects composition by its source project item ID.
    #[arg(long)]
    composition: Option<u32>,
    /// AE-evaluated expression values for this exact source AEP.
    #[arg(long, value_name = "JSON")]
    expression_samples: Option<PathBuf>,
    /// Available PostScript font names (JSON array); missing AE faces become Inter.
    #[arg(long, value_name = "JSON")]
    available_fonts: Option<PathBuf>,
    /// Source-bound replacements prepared separately from conversion.
    #[arg(long, value_name = "JSON")]
    media_map: Option<PathBuf>,
    /// Explicit source/sequence/Media-UID-bound Premiere file relocation.
    #[arg(long, value_name = "JSON")]
    media_relink: Option<PathBuf>,
    /// Frame rate of the export: a Premiere sequence at 23.976, 24, 25, 29.97,
    /// 30, 50, 59.94 or 60 fps (default 30), or an After Effects composition at
    /// any rate up to 240 fps (default 24).
    #[arg(long, value_name = "FPS")]
    fps: Option<String>,
    /// Validate the complete conversion without publishing output.
    #[arg(long)]
    check: bool,
}

#[derive(Args)]
struct ConvertArgs {
    /// Source project (.aep, .prproj, or .tsrct).
    input: PathBuf,
    /// Destination format.
    #[arg(long, value_parser = registry::format_value_parser())]
    to: String,
    /// Fresh output directory for the converted project and media.
    #[arg(short, long)]
    output: PathBuf,
    #[command(flatten)]
    options: ConversionOptions,
    /// JSON result on stdout; progress, diagnostics and errors as NDJSON on stderr.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Convert an editable project to another format.
    Convert(ConvertArgs),
    /// Inspect native scenes and used media without converting or transcoding.
    Inspect(preflight::InspectArgs),
    /// Convert one audio/video file into one new media file; no project required.
    Transcode(transcode::TranscodeArgs),
}

impl Cli {
    fn progress_mode(&self) -> Option<(&'static str, &'static str, bool)> {
        match &self.command {
            Command::Convert(args) => Some((
                "convert",
                if args.options.check {
                    "checking"
                } else {
                    "converting"
                },
                args.json,
            )),
            Command::Transcode(args) => Some(("transcode", "preparing", args.json)),
            _ => None,
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<String> {
    let mut reporter = cli
        .progress_mode()
        .map(|(command, phase, json)| progress::Reporter::start(command, phase, json))
        .transpose()?;
    let result = run_command(cli, reporter.as_ref());
    if let Some(reporter) = &mut reporter {
        reporter.finish(result.is_ok());
    }
    result
}

fn run_command(cli: Cli, reporter: Option<&progress::Reporter>) -> anyhow::Result<String> {
    match cli.command {
        Command::Convert(args) => convert(
            &args.input,
            &args.to,
            &args.output,
            &args.options,
            args.json,
            reporter,
        ),
        Command::Inspect(args) => preflight::run(args),
        Command::Transcode(args) => transcode::run(args, reporter),
    }
}

fn convert(
    input: &std::path::Path,
    to: &str,
    output: &std::path::Path,
    options: &ConversionOptions,
    json: bool,
    reporter: Option<&progress::Reporter>,
) -> anyhow::Result<String> {
    if json {
        // Validate representability before the conversion can publish files.
        serde_json::to_value((input, output))?;
    }
    let from = options
        .from
        .as_deref()
        .map_or_else(|| registry::infer(input), registry::by_id)?;
    let to = registry::by_id(to)?;
    let route = registry::resolve_route(from, to)?;
    let observe = |measurement| {
        if let Some(reporter) = reporter {
            reporter.update_conversion(measurement);
        }
    };
    let request = ConversionRequest {
        progress: reporter.map_or_else(fx_conv::Progress::default, |_| {
            fx_conv::Progress::new(&observe)
        }),
        input,
        output,
        sequence: options.sequence.as_deref(),
        allow_film_impact_pop: options.allow_film_impact_pop,
        composition: options.composition,
        expression_samples: options.expression_samples.as_deref(),
        available_fonts: options.available_fonts.as_deref(),
        media_map: options.media_map.as_deref(),
        media_relink: options.media_relink.as_deref(),
        fps: options.fps.as_deref(),
        mode: if options.check {
            ConversionMode::Check
        } else {
            ConversionMode::Write
        },
    };
    route.run(&request, json)
}

fn inspect_source(
    format: &registry::FormatRegistration,
    input: &std::path::Path,
    json: bool,
) -> anyhow::Result<String> {
    if let Some(inspect) = format.inspect {
        inspect(input, json)
    } else {
        targets::list(format, input, json)
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if error.kind() == clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand =>
        {
            // Clap's printer applies terminal styling; displaying render_help() directly does not.
            if let Err(error) = Cli::command().print_help() {
                eprintln!("failed to print help: {error}");
                return ExitCode::FAILURE;
            }
            return ExitCode::SUCCESS;
        }
        Err(error) => error.exit(),
    };
    let json_command = cli
        .progress_mode()
        .filter(|(_, _, json)| *json)
        .map(|(command, _, _)| command);
    match run(cli) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            if let Some(command) = json_command {
                progress::write_error(command, &error);
            } else {
                eprintln!("{error:#}");
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_subcommand_uses_the_existing_conversion_route() {
        let cli = Cli::try_parse_from([
            "tsrct-conv",
            "convert",
            "missing.tsrct",
            "--to",
            "premiere",
            "--output",
            "converted",
            "--media-map",
            "prepared/media-map.json",
        ])
        .unwrap();
        assert_eq!(
            run(cli).unwrap_err().to_string(),
            "--media-map is only supported for Adobe to Tesseract import"
        );
    }

    #[test]
    fn command_help_and_missing_conversion_arguments() {
        assert_eq!(
            Cli::try_parse_from(["tsrct-conv"])
                .err()
                .expect("missing command should show help")
                .kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        assert!(Cli::try_parse_from([
            "tsrct-conv",
            "source.aep",
            "--to",
            "tesseract",
            "-o",
            "out"
        ])
        .is_err());
        assert!(Cli::try_parse_from(["tsrct-conv", "source.aep", "--inspect"]).is_err());
        for args in [
            vec!["tsrct-conv", "convert"],
            vec!["tsrct-conv", "convert", "source.aep", "--to", "tesseract"],
        ] {
            assert_eq!(
                Cli::try_parse_from(args)
                    .err()
                    .expect("missing conversion arguments should be rejected")
                    .kind(),
                clap::error::ErrorKind::MissingRequiredArgument
            );
        }
    }

    #[test]
    fn hybrid_layer_selection_is_not_a_public_cli_option() {
        let error = Cli::try_parse_from([
            "tsrct-conv",
            "convert",
            "missing.tsrct",
            "--to",
            "premiere",
            "-o",
            "missing-output",
            "--after-effects-layer",
            "10",
        ])
        .err()
        .expect("manual selection is not a public option");
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn available_fonts_cli_publishes_inter_and_original_font_warning() {
        use fx_conv::ImportToTesseract as _;
        let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/aftereffects_file/tests/fixtures/pr4442_native/sources/text_document_point.aep");
        let target = aftereffects_file::AfterEffects
            .list_import_targets(&input)
            .unwrap()
            .remove(0);
        let root = tempfile::tempdir().unwrap();
        let inventory = root.path().join("fonts.json");
        std::fs::write(&inventory, r#"["Inter-Regular"]"#).unwrap();
        let output = root.path().join("converted");
        let cli = Cli::try_parse_from([
            "tsrct-conv",
            "convert",
            input.to_str().unwrap(),
            "--to",
            "tesseract",
            "--composition",
            &target.id,
            "--available-fonts",
            inventory.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--json",
        ])
        .unwrap();
        let report = run(cli).unwrap();
        assert!(report.contains("available-font inventory"));
        assert!(report.contains("ArialMT"));
        assert!(report.contains("Inter-Regular"));
        let archive = tesseract_file::TesseractFile::open(output.join("project.tsrct")).unwrap();
        assert!(archive
            .project_json()
            .unwrap()
            .to_string()
            .contains("\"fontFamily\":\"Inter-Regular\""));
    }

    #[test]
    fn available_fonts_parses_for_aep_and_rejects_other_routes_before_io() {
        let cli = Cli::try_parse_from([
            "tsrct-conv",
            "convert",
            "missing.aep",
            "--to",
            "tesseract",
            "--available-fonts",
            "fonts.json",
            "-o",
            "out",
        ])
        .unwrap();
        let Command::Convert(args) = cli.command else {
            panic!("convert command");
        };
        assert_eq!(
            args.options.available_fonts,
            Some(PathBuf::from("fonts.json"))
        );
        let root = tempfile::tempdir().unwrap();
        for (input, target) in [
            ("missing.prproj", "tesseract"),
            ("missing.tsrct", "after-effects"),
        ] {
            let cli = Cli::try_parse_from([
                "tsrct-conv",
                "convert",
                input,
                "--to",
                target,
                "--available-fonts",
                "missing.json",
                "-o",
                root.path().join("out").to_str().unwrap(),
            ])
            .unwrap();
            let error = run(cli).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("--available-fonts is only supported"),
                "{error:#}"
            );
        }
    }

    #[test]
    fn parses_expression_samples_for_after_effects_import() {
        let cli = Cli::try_parse_from([
            "tsrct-conv",
            "convert",
            "source.aep",
            "--to",
            "tesseract",
            "--output",
            "converted",
            "--expression-samples",
            "samples.json",
        ])
        .unwrap();
        assert_eq!(
            match cli.command {
                Command::Convert(args) => args.options.expression_samples,
                _ => panic!("expected convert command"),
            }
            .as_deref(),
            Some(std::path::Path::new("samples.json"))
        );
    }

    #[test]
    fn media_relink_is_premiere_import_only_and_composes_with_prepared_media() {
        for input in ["missing.tsrct", "missing.aep"] {
            let to = if input.ends_with("aep") {
                "tesseract"
            } else {
                "premiere"
            };
            let cli = Cli::try_parse_from([
                "tsrct-conv",
                "convert",
                input,
                "--to",
                to,
                "--output",
                "converted",
                "--media-relink",
                "bindings.json",
            ])
            .unwrap();
            assert_eq!(
                run(cli).unwrap_err().to_string(),
                "--media-relink is only supported for Premiere to Tesseract import"
            );
        }
        assert!(Cli::try_parse_from([
            "tsrct-conv",
            "convert",
            "source.prproj",
            "--to",
            "tesseract",
            "--output",
            "converted",
            "--media-relink",
            "bindings.json",
            "--media-map",
            "prepared.json"
        ])
        .is_ok());
        for (input, to) in [("missing.aep", "tesseract"), ("missing.tsrct", "premiere")] {
            let cli = Cli::try_parse_from([
                "tsrct-conv",
                "convert",
                input,
                "--to",
                to,
                "--output",
                "converted",
                "--media-relink",
                "bindings.json",
                "--media-map",
                "prepared.json",
            ])
            .unwrap();
            assert!(run(cli)
                .unwrap_err()
                .to_string()
                .contains("is only supported"));
        }
    }

    #[test]
    fn media_map_is_explicit_and_rejected_for_export_before_reading_input() {
        let cli = Cli::try_parse_from([
            "tesseract-conv",
            "convert",
            "missing.tsrct",
            "--to",
            "premiere",
            "--output",
            "converted",
            "--media-map",
            "prepared/media-map.json",
        ])
        .unwrap();
        assert_eq!(
            run(cli).unwrap_err().to_string(),
            "--media-map is only supported for Adobe to Tesseract import"
        );
        let cli = Cli::try_parse_from([
            "tesseract-conv",
            "convert",
            "source.aep",
            "--to",
            "tesseract",
            "--composition",
            "4",
            "--output",
            "converted",
            "--media-map",
            "prepared/media-map.json",
        ])
        .unwrap();
        assert_eq!(
            match cli.command {
                Command::Convert(args) => args.options.media_map,
                _ => panic!("expected convert command"),
            }
            .as_deref(),
            Some(std::path::Path::new("prepared/media-map.json"))
        );
    }

    #[test]
    fn rejects_expression_samples_for_other_conversion_modes() {
        let cli = Cli::try_parse_from([
            "tsrct-conv",
            "convert",
            "source.tsrct",
            "--to",
            "premiere",
            "--output",
            "converted",
            "--expression-samples",
            "samples.json",
        ])
        .unwrap();
        let error = run(cli).unwrap_err();
        assert_eq!(
            error.to_string(),
            "--expression-samples is only supported for After Effects to Tesseract conversion"
        );
    }

    #[test]
    fn removed_fast_aep_bake_flag_is_not_accepted() {
        for source in ["missing.aep", "missing.prproj"] {
            let error = Cli::try_parse_from([
                "tsrct-conv",
                "convert",
                source,
                "--to",
                "tesseract",
                "--output",
                "converted",
                "--fast-aep-bake",
            ])
            .err()
            .expect("removed flag must be rejected");
            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        }
        let error = Cli::try_parse_from([
            "tsrct-conv",
            "convert",
            "missing.tsrct",
            "--to",
            "after-effects",
            "--output",
            "converted",
            "--fast-aep-bake",
        ])
        .err()
        .expect("removed flag must be rejected");
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        let help = Cli::try_parse_from(["tsrct-conv", "convert", "--help"])
            .err()
            .expect("help should be displayed")
            .to_string();
        assert!(!help.contains("fast-aep-bake"));
    }

    #[test]
    fn inspect_rejects_expression_samples_before_reading_input() {
        let cli = Cli::try_parse_from([
            "tsrct-conv",
            "inspect",
            "missing.aep",
            "--expression-samples",
            "samples.json",
        ])
        .err()
        .expect("inspect must reject conversion flags");
        assert_eq!(cli.kind(), clap::error::ErrorKind::UnknownArgument);
    }
}
