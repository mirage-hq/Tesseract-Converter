//! CLI selection and presentation; native libraries own reachability and admission.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write,
    path::PathBuf,
};

use anyhow::{ensure, Context};
use clap::Args;
use fx_conv::{ImportTarget, MediaPreflight, MediaRemediation, MediaStatus, ValidatedMediaMap};

use crate::registry::{self, FormatRegistration};

#[derive(Debug, Args)]
pub(super) struct SourceArgs {
    /// Original Adobe project; preparation never rewrites it.
    pub input: PathBuf,
    /// Source format, inferred from extension if omitted.
    #[arg(long, value_parser = registry::format_value_parser())]
    pub from: Option<String>,
    /// Exact native AE composition item ID, not its name.
    #[arg(long, conflicts_with = "sequence")]
    pub composition: Option<u32>,
    /// Exact native Premiere sequence GUID, not its name.
    #[arg(long, conflicts_with = "composition")]
    pub sequence: Option<String>,
}

impl SourceArgs {
    pub fn format(&self) -> anyhow::Result<&'static FormatRegistration> {
        let format = self
            .from
            .as_deref()
            .map_or_else(|| registry::infer(&self.input), registry::by_id)?;
        ensure!(
            self.composition.is_none() || format.id == "after-effects",
            "--composition is only supported for After Effects sources"
        );
        ensure!(
            self.sequence.is_none() || format.id == "premiere",
            "--sequence is only supported for Premiere sources"
        );
        ensure!(
            format.media_inspector.is_some(),
            "media inspection is only supported for Adobe sources"
        );
        Ok(format)
    }

    pub fn targets(&self) -> anyhow::Result<Vec<ImportTarget>> {
        let format = self.format()?;
        let targets = format.list_targets(&self.input).with_context(|| {
            format!(
                "read {} structure from {}",
                if format.id == "after-effects" {
                    "AEP"
                } else {
                    "Premiere"
                },
                self.input.display(),
            )
        })?;
        let selected = self
            .composition
            .map(|id| id.to_string())
            .or_else(|| self.sequence.clone());
        if let Some(selected) = selected {
            let target = targets
                .into_iter()
                .find(|target| target.id == selected)
                .with_context(|| {
                    format!("native target {selected:?} does not exist in this source")
                })?;
            Ok(vec![target])
        } else {
            Ok(targets)
        }
    }

    pub fn inspect_target(
        &self,
        target: &str,
        map: Option<&ValidatedMediaMap>,
    ) -> anyhow::Result<MediaPreflight> {
        let inspect = self
            .format()?
            .media_inspector
            .context("format has no media inspector")?;
        inspect(&self.input, target, map)
    }
}

#[derive(Debug, Args)]
pub(super) struct InspectArgs {
    #[command(flatten)]
    pub source: SourceArgs,
    /// Keep the original version-1 metadata inventory; do not open media.
    #[arg(long, conflicts_with = "media_map")]
    pub metadata_only: bool,
    /// Inspect explicitly prepared replacements rather than just their originals.
    #[arg(long, value_name = "JSON")]
    pub media_map: Option<PathBuf>,
    /// Emit one versioned JSON object; completion does not imply readiness.
    #[arg(long)]
    pub json: bool,
}

pub(super) fn run(args: InspectArgs) -> anyhow::Result<String> {
    let format = args.source.format()?;
    // Legacy metadata output is kept byte-for-byte in metadata-only mode.
    if args.metadata_only {
        ensure!(
            args.source.composition.is_none() && args.source.sequence.is_none(),
            "--metadata-only lists all targets; omit --composition/--sequence"
        );
        return crate::inspect_source(format, &args.source.input, args.json);
    }
    let targets = args.source.targets()?;
    ensure!(
        args.media_map.is_none() || targets.len() == 1,
        "select one native target when inspecting with --media-map"
    );
    let map = args
        .media_map
        .as_deref()
        .map(ValidatedMediaMap::load)
        .transpose()?;
    let reports = targets
        .iter()
        .map(|target| args.source.inspect_target(&target.id, map.as_ref()))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let ready = reports.iter().all(MediaPreflight::is_ready);
    if args.json {
        let metadata = crate::inspect_source(format, &args.source.input, true)?;
        let mut inventory: serde_json::Value = serde_json::from_str(&metadata)?;
        inventory["schema_version"] = 2.into();
        inventory["media_admission"] = if ready { "ready" } else { "blocked" }.into();
        let video_ready = reports.iter().all(|report| {
            report.unassessed.is_empty()
                && report
                    .media
                    .iter()
                    .filter(|media| media.kind == fx_conv::MediaKind::Video)
                    .all(|media| media.status == fx_conv::MediaStatus::Supported)
        });
        inventory["video_admission"] = if video_ready { "ready" } else { "blocked" }.into();
        // This is an admission report, not whole-project conversion/render proof.
        inventory["media_preflight"] = serde_json::to_value(reports)?;
        Ok(serde_json::to_string_pretty(&inventory)?)
    } else {
        let mut admission = String::new();
        write_media_summary(&mut admission, &reports)?;
        if format.id == "after-effects" {
            let media_ready = reports
                .iter()
                .map(|report| Ok((report.target.parse::<u32>()?, report.is_ready())))
                .collect::<anyhow::Result<HashMap<_, _>>>()?;
            let mut text =
                crate::inspect::summary(&args.source.input, args.source.composition, &media_ready)?;
            text.push_str("\n\n");
            text.push_str(&admission);
            Ok(text)
        } else {
            let mut text = crate::inspect_source(format, &args.source.input, false)?;
            text.push_str("\n\n");
            text.push_str(&admission);
            Ok(text)
        }
    }
}

fn write_media_summary(text: &mut String, reports: &[MediaPreflight]) -> anyhow::Result<()> {
    let ready = reports.iter().filter(|report| report.is_ready()).count();
    writeln!(
        text,
        "Media check: {ready}/{} targets OK (source media only; not render proof)",
        reports.len()
    )?;

    // A precomp's source may be reached from many targets. Show each failure once.
    let mut issues = BTreeMap::new();
    let mut unassessed = BTreeMap::new();
    for report in reports {
        for issue in &report.unassessed {
            unassessed
                .entry(issue.as_str())
                .or_insert_with(Vec::new)
                .push(report.target.as_str());
        }
        for media in &report.media {
            if media.status == MediaStatus::Supported {
                continue;
            }
            let key = (
                &media.owner,
                &media.id,
                format!("{:?}", media.status),
                &media.selected,
                &media.reason,
            );
            let entry = issues.entry(key).or_insert_with(|| (media, Vec::new()));
            entry.1.push(report.target.as_str());
        }
    }
    if !issues.is_empty() || !unassessed.is_empty() {
        writeln!(text, "\nIssues ({}):", issues.len() + unassessed.len())?;
        let mut rows = Vec::new();
        let mut details = Vec::new();
        for (issue, targets) in unassessed {
            for (index, ids) in targets.chunks(3).enumerate() {
                rows.push([
                    if index == 0 { "UNASSESSED" } else { "" }.to_owned(),
                    String::new(),
                    ids.join(", "),
                    if index == 0 { issue } else { "" }.to_owned(),
                ]);
            }
        }
        for (_, (media, targets)) in issues {
            let status = media_issue_label(media.status, media.remediation);
            for (index, ids) in targets.chunks(3).enumerate() {
                rows.push([
                    if index == 0 { status } else { "" }.to_owned(),
                    if index == 0 { media.id.as_str() } else { "" }.to_owned(),
                    ids.join(", "),
                    if index == 0 { media.name.as_str() } else { "" }.to_owned(),
                ]);
            }
            if let Some(reason) = &media.reason {
                details.push(format!("{} [{}]: {reason}", media.name, media.id));
            }
            if let Some(path) = &media.selected {
                details.push(format!(
                    "{} [{}] selected: {}",
                    media.name,
                    media.id,
                    path.display()
                ));
            }
        }
        for line in crate::inspect::table(
            ["STATUS", "MEDIA ID", "AFFECTED COMPOSITIONS", "SOURCE"],
            rows,
        ) {
            writeln!(text, "{line}")?;
        }
        if !details.is_empty() {
            writeln!(text, "\nDetails:")?;
            for detail in details {
                writeln!(text, "  {detail}")?;
            }
        }
    }
    Ok(())
}

fn media_issue_label(status: MediaStatus, remediation: MediaRemediation) -> &'static str {
    match (status, remediation) {
        (MediaStatus::RequiresTranscode, MediaRemediation::TranscodeCandidate) => {
            "TRANSCODE CANDIDATE"
        }
        (MediaStatus::RequiresTranscode, MediaRemediation::ExternalRenderRequired) => {
            "EXTERNAL RENDER"
        }
        (MediaStatus::RequiresTranscode, _) => "PREPARATION NEEDED",
        (MediaStatus::Missing, _) => "PATH UNRESOLVED",
        (MediaStatus::Unreadable, _) => "UNREADABLE",
        (MediaStatus::InvalidMedia, _) => "INVALID MEDIA",
        (MediaStatus::Unassessed, _) => "UNASSESSED",
        (MediaStatus::Supported, _) => "OK",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_conv::{InspectedMedia, MediaKind, MediaRemediation, MediaStatus};

    #[test]
    fn preparation_labels_do_not_promise_a_successful_transcode() {
        assert_eq!(
            media_issue_label(
                MediaStatus::RequiresTranscode,
                MediaRemediation::TranscodeCandidate
            ),
            "TRANSCODE CANDIDATE"
        );
        assert_eq!(
            media_issue_label(
                MediaStatus::RequiresTranscode,
                MediaRemediation::ExternalRenderRequired
            ),
            "EXTERNAL RENDER"
        );
        assert_eq!(
            media_issue_label(MediaStatus::RequiresTranscode, MediaRemediation::Unknown),
            "PREPARATION NEEDED"
        );
    }

    #[test]
    fn shared_missing_source_is_reported_once_across_targets() {
        let media = InspectedMedia {
            owner: PathBuf::from("source.aep"),
            id: "25".to_owned(),
            name: "clip.mov".to_owned(),
            kind: MediaKind::Video,
            authored: None,
            original: None,
            selected: None,
            references: Vec::new(),
            status: MediaStatus::Missing,
            container: None,
            codec: None,
            reason: Some("local source not found".to_owned()),
            remediation: MediaRemediation::Unknown,
        };
        let reports = [
            MediaPreflight {
                format: "after-effects".to_owned(),
                target: "1".to_owned(),
                media: vec![media.clone()],
                unassessed: Vec::new(),
            },
            MediaPreflight {
                format: "after-effects".to_owned(),
                target: "2".to_owned(),
                media: vec![media],
                unassessed: Vec::new(),
            },
        ];
        let mut output = String::new();
        write_media_summary(&mut output, &reports).unwrap();
        assert!(output.contains("Media check: 0/2 targets OK"));
        assert!(output.contains("\nIssues (1):\nSTATUS"));
        let row = output
            .lines()
            .find(|line| line.starts_with("PATH UNRESOLVED"))
            .unwrap();
        assert!(row.contains("25"));
        assert!(row.contains("1, 2"));
        assert!(row.ends_with("clip.mov"));
        assert!(!output.contains("MISSING"));
        assert!(output.contains("Details:\n  clip.mov [25]: local source not found"));
    }
}
