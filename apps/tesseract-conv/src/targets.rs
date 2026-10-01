//! Read-only, format-independent import-target presentation.

use std::path::Path;

use serde::Serialize;

use crate::registry::FormatRegistration;

#[derive(Serialize)]
struct Target<'a> {
    id: &'a str,
    name: &'a str,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
    duration_secs: Option<f64>,
    layer_count: Option<usize>,
    video_track_count: Option<usize>,
    audio_track_count: Option<usize>,
}

#[derive(Serialize)]
struct Inventory<'a> {
    schema_version: u32,
    format: &'a str,
    targets: Vec<Target<'a>>,
}

pub(super) fn list(
    format: &FormatRegistration,
    input: &Path,
    json: bool,
) -> anyhow::Result<String> {
    let targets = format.list_targets(input)?;
    if json {
        let inventory = Inventory {
            schema_version: 1,
            format: format.id,
            targets: targets
                .iter()
                .map(|target| Target {
                    id: &target.id,
                    name: &target.name,
                    width: target.width,
                    height: target.height,
                    fps: target.fps,
                    duration_secs: target.duration_secs,
                    layer_count: target.layer_count,
                    video_track_count: target.video_track_count,
                    audio_track_count: target.audio_track_count,
                })
                .collect(),
        };
        return Ok(serde_json::to_string_pretty(&inventory)?);
    }
    if targets.is_empty() {
        return Ok("No import targets.".into());
    }
    Ok(targets
        .iter()
        .map(|target| format!(
            "scene id={} name={:?} {}x{} fps={} duration_secs={} layers={} video_tracks={} audio_tracks={}",
            target.id, target.name, known(target.width), known(target.height),
            known(target.fps), known(target.duration_secs), known(target.layer_count),
            known(target.video_track_count), known(target.audio_track_count)
        ))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn known(value: Option<impl std::fmt::Display>) -> String {
    value.map_or_else(|| "unknown".into(), |value| value.to_string())
}
