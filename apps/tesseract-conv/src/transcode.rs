//! Explicit single-file media conversion. Import never calls this path.

use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

use anyhow::{ensure, Context};
use clap::{Args, ValueEnum};
use media_transcode::{Backend, TranscodeRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BackendChoice {
    /// Call FFmpeg libraries directly (requires a build with ffmpeg-library enabled).
    Library,
    /// Explicit host ffmpeg and ffprobe processes.
    ExternalFfmpegCommand,
}

#[derive(Debug, Args)]
pub(super) struct TranscodeArgs {
    /// One local audio or video file, independent of any project.
    input: PathBuf,
    /// New media file (.mov/.mp4 for video, .wav for audio); never overwritten.
    #[arg(short, long)]
    output: PathBuf,
    /// No automatic fallback to another backend or encoder.
    #[arg(long, value_enum, default_value = "library")]
    backend: BackendChoice,
    /// External backend executable (default: ffmpeg on PATH).
    #[arg(long)]
    ffmpeg_path: Option<PathBuf>,
    /// External backend probe executable (default: ffprobe on PATH).
    #[arg(long)]
    ffprobe_path: Option<PathBuf>,
    /// JSON result on stdout; progress and errors as NDJSON on stderr.
    #[arg(long)]
    pub(super) json: bool,
}

pub(super) fn run(
    args: TranscodeArgs,
    reporter: Option<&crate::progress::Reporter>,
) -> anyhow::Result<String> {
    let backend = match args.backend {
        BackendChoice::Library => {
            ensure!(
                args.ffmpeg_path.is_none() && args.ffprobe_path.is_none(),
                "--ffmpeg-path/--ffprobe-path require --backend external-ffmpeg-command"
            );
            Backend::Library
        }
        BackendChoice::ExternalFfmpegCommand => Backend::External {
            ffmpeg: args.ffmpeg_path.unwrap_or_else(|| "ffmpeg".into()),
            ffprobe: args.ffprobe_path.unwrap_or_else(|| "ffprobe".into()),
        },
    };
    let input = std::path::absolute(&args.input).context("resolve input media path")?;
    let output = std::path::absolute(&args.output).context("resolve output media path")?;
    if args.json {
        serde_json::to_value((&input, &output))
            .context("JSON output requires UTF-8 media paths")?;
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let on_interrupt = Arc::clone(&cancelled);
    ctrlc::set_handler(move || on_interrupt.store(true, std::sync::atomic::Ordering::Relaxed))
        .context("install transcode cancellation handler")?;
    let result = media_transcode::run(
        TranscodeRequest {
            input: &input,
            output: &output,
            backend,
            cancelled: &cancelled,
        },
        &mut |progress| {
            if let Some(reporter) = reporter {
                reporter.update_media(progress);
            }
        },
    )?;
    if args.json {
        Ok(serde_json::to_string_pretty(&result)?)
    } else {
        let mut message = format!("Media written: {}", result.output.display());
        for warning in &result.warnings {
            message.push_str(&format!("\nWarning: {warning}"));
        }
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[test]
    fn explicit_backend_flags_reject_cross_backend_paths_before_input_io() {
        for options in [
            vec!["--backend", "library", "--ffmpeg-path", "anything"],
            vec!["--backend", "library", "--ffprobe-path", "anything"],
        ] {
            let mut argv = vec!["conv", "transcode", "missing.aep", "--output", "new"];
            argv.extend(options);
            let cli = crate::Cli::try_parse_from(argv).unwrap();
            let error = crate::run(cli).unwrap_err().to_string();
            assert!(error.contains("require"), "{error}");
        }
    }

    #[test]
    fn removed_helper_option_is_rejected() {
        assert!(crate::Cli::try_parse_from([
            "conv",
            "transcode",
            "source.aep",
            "--output",
            "new",
            "--library-helper",
            "unused"
        ])
        .is_err());
    }

    #[test]
    fn media_input_needs_no_project_and_rejects_project_options() {
        let argv = ["conv", "transcode", "input.mov", "--output", "output.mov"];
        assert!(crate::Cli::try_parse_from(argv).is_ok());
        for option in ["--composition", "--sequence", "--from", "--media-map"] {
            let mut arguments = argv.to_vec();
            arguments.extend([option, "unused"]);
            assert!(crate::Cli::try_parse_from(arguments).is_err(), "{option}");
        }
    }

    #[test]
    fn arbitrary_encoder_arguments_are_not_an_escape_hatch() {
        assert!(crate::Cli::try_parse_from([
            "conv",
            "transcode",
            "source.aep",
            "--output",
            "new",
            "--ffmpeg-args",
            "-r 15",
        ])
        .is_err());
    }
}
