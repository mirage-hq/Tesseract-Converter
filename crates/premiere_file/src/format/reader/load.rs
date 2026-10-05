//! Load the supported Premiere model from one native record graph.

use super::{read_sequence, read_xml};
use crate::{
    error::{ensure, unsupported, BuildError, Result},
    format::{cyclic_sequences, sequences, Graph, PrProjectFile},
    omit,
    schema::records,
    Omission, OmissionScope,
};
use std::path::Path;

/// Sequence metadata available without converting content or probing media.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NativeImportTarget {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) fps: Option<f64>,
    pub(crate) duration_secs: Option<f64>,
    pub(crate) video_track_count: Option<usize>,
    pub(crate) audio_track_count: Option<usize>,
}

impl PrProjectFile {
    /// Loads every supported top-level sequence from a Premiere project.
    ///
    /// # Errors
    /// Returns an error for malformed projects or unsupported selected content.
    pub fn load(
        path: impl AsRef<Path>,
    ) -> std::result::Result<(Self, Vec<Omission>), crate::ConversionError> {
        Ok(Self::load_selected(path.as_ref(), None)?)
    }

    pub(crate) fn load_selected(
        path: &Path,
        selection: Option<&str>,
    ) -> Result<(Self, Vec<Omission>)> {
        Self::load_xml(&read_xml(path)?, selection, path.parent())
    }

    /// Lists all sequences that have a stable GUID and nonempty name.
    pub(crate) fn import_targets(path: &Path) -> Result<Vec<NativeImportTarget>> {
        let xml = read_xml(path)?;
        let graph = Graph::parse(&xml)?;
        Ok(native_import_targets(&graph))
    }

    /// Loads exactly one import target; unrelated topology errors do not block selection.
    pub(crate) fn load_import(
        path: &Path,
        selection: Option<&str>,
    ) -> Result<(Self, Vec<Omission>)> {
        Self::load_import_with_media_relink(path, selection, None)
    }

    pub(crate) fn load_import_with_media_relink(
        path: &Path,
        selection: Option<&str>,
        relink: Option<&crate::ValidatedMediaRelink>,
    ) -> Result<(Self, Vec<Omission>)> {
        let xml = super::read_xml_with_media_relink(path, relink)?;
        let graph = Graph::parse(&xml)?.with_source_dir(path.parent());
        let targets = native_import_targets(&graph);
        let guid = match selection {
            Some(guid) => {
                ensure!(
                    targets.iter().any(|target| target.id == guid),
                    "no sequence matches GUID {guid:?}; run `tsrct-conv inspect INPUT` to inspect available sequences"
                );
                guid
            }
            None => match targets.as_slice() {
                [target] => &target.id,
                [] => {
                    return Err(unsupported(
                        "project has no selectable sequences; run `tsrct-conv inspect INPUT` to inspect available sequences",
                    ))
                }
                targets => {
                    return Err(unsupported(format!(
                        "project has {} selectable sequences; select one with --sequence <GUID>; run `tsrct-conv inspect INPUT` to inspect available sequences",
                        targets.len()
                    )))
                }
            },
        };
        if let Some(relink) = relink {
            relink.validate_for(path, guid)?;
        }
        let graph = graph.with_media_relink(relink)?;
        Self::load_one(&graph, guid)
    }

    fn load_one(graph: &Graph<'_>, guid: &str) -> Result<(Self, Vec<Omission>)> {
        let mut omissions = Vec::new();
        let mut media = std::collections::BTreeMap::new();
        // Preserve the existing cyclic-placement omission policy. If unrelated
        // topology is unreadable, the nested reader still detects active cycles.
        let cyclic = sequences(graph, &mut Vec::new())
            .map(|topology| cyclic_sequences(&topology))
            .unwrap_or_default();
        let mut sequence = read_sequence(graph, Some(guid), &cyclic, &mut media, &mut omissions)?;
        sequence.top_level = None;
        let referenced: std::collections::BTreeSet<_> =
            sequence.media_in_order().into_iter().cloned().collect();
        media.retain(|id, _| referenced.contains(id));
        let project = Self::from_sequences(vec![sequence], media);
        if let Err(error) = project.validate() {
            return Err(unsupported(format!(
                "Tesseract build blocked; no output published:\n{error}"
            )));
        }
        Ok((project, omissions))
    }

    /// Loads the selected timelines of `xml`.
    fn load_xml(
        xml: &str,
        selection: Option<&str>,
        source_dir: Option<&Path>,
    ) -> Result<(Self, Vec<Omission>)> {
        let graph = Graph::parse(xml)?.with_source_dir(source_dir);
        let mut omissions = Vec::new();
        let topology = sequences(&graph, &mut omissions);
        // Every read omits the placements of these timelines.
        let cyclic = topology
            .as_ref()
            .map(|topology| cyclic_sequences(topology))
            .unwrap_or_default();
        let selected: Vec<_> = match (topology, selection) {
            (Ok(sequences), _) => sequences
                .into_iter()
                .filter(|sequence| {
                    selection.map_or(sequence.top_level == Some(true), |id| sequence.guid == id)
                })
                .map(|sequence| (sequence.guid, sequence.name, sequence.top_level))
                .collect(),
            (Err(_), Some(guid)) => {
                // Explicit selection does not require topology from unrelated content.
                let record = graph
                    .locate_uid(guid, "explicit sequence selection")
                    .ok()
                    .filter(|record| record.tag() == records::SEQUENCE.tag)
                    .ok_or_else(|| unsupported("no sequence matches the explicit GUID"))?;
                let name = record
                    .element()
                    .child(records::NAME)
                    .and_then(|element| element.text())
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        unsupported(format!("{}: missing {}", record.identity(), records::NAME))
                    })?;
                vec![(guid.to_owned(), name.to_owned(), None)]
            }
            (Err(error), None) => {
                return Err(BuildError::Context {
                    context: "cannot identify all top-level timelines".into(),
                    source: Box::new(error.into()),
                })
            }
        };
        ensure!(
            !selected.is_empty(),
            "no proven top-level timeline; select an exact sequence GUID: {}",
            omissions
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
        let mut sequences = Vec::with_capacity(selected.len());
        let mut media = std::collections::BTreeMap::new();
        for (guid, name, top_level) in selected {
            let record = format!("{guid} ({name:?})");
            let mut sequence =
                match read_sequence(&graph, Some(&guid), &cyclic, &mut media, &mut omissions) {
                    Ok(sequence) => sequence,
                    Err(error) => {
                        omit(
                            &mut omissions,
                            OmissionScope::Sequence,
                            record,
                            format!("{error:#}"),
                        );
                        continue;
                    }
                };
            sequence.top_level = top_level;
            sequences.push(sequence);
        }
        ensure!(
            !sequences.is_empty(),
            "no convertible timelines; no output batch published:\n{}",
            omissions
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
        // A failed occurrence or sequence can leave media that no surviving clip uses.
        let referenced: std::collections::BTreeSet<_> = sequences
            .iter()
            .flat_map(|sequence| sequence.media_in_order().into_iter().cloned())
            .collect();
        media.retain(|id, _| referenced.contains(id));
        let project = Self::from_sequences(sequences, media);
        if let Err(error) = project.validate() {
            return Err(unsupported(format!(
                "Tesseract build blocked; no output batch published:\n{error}"
            )));
        }
        Ok((project, omissions))
    }
}

fn native_import_targets(graph: &Graph<'_>) -> Vec<NativeImportTarget> {
    let mut targets: Vec<_> = graph
        .records()
        .filter(|record| record.tag() == records::SEQUENCE.tag)
        .filter_map(|record| {
            let element = record.element();
            let id = element.attribute(records::OBJECT_UID)?.to_owned();
            let name = element
                .child(records::NAME)?
                .text()
                .filter(|name| !name.is_empty())?
                .to_owned();
            let metadata = native_target_metadata(graph, record);
            Some(NativeImportTarget {
                id,
                name,
                width: metadata.width,
                height: metadata.height,
                fps: metadata.fps,
                duration_secs: metadata.duration_secs,
                video_track_count: metadata.video_track_count,
                audio_track_count: metadata.audio_track_count,
            })
        })
        .collect();
    targets.sort_by(|left, right| left.id.cmp(&right.id));
    targets
}

#[derive(Default)]
struct NativeTargetMetadata {
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
    duration_secs: Option<f64>,
    video_track_count: Option<usize>,
    audio_track_count: Option<usize>,
}

fn native_target_metadata(
    graph: &Graph<'_>,
    sequence: crate::format::Record<'_>,
) -> NativeTargetMetadata {
    let Some(groups) = sequence.element().child("TrackGroups") else {
        return NativeTargetMetadata::default();
    };
    let mut metadata = NativeTargetMetadata {
        video_track_count: Some(0),
        audio_track_count: Some(0),
        ..Default::default()
    };
    let mut end_ticks = Some(0);
    for link in groups.children() {
        let Some(reference) = link.child("Second") else {
            return NativeTargetMetadata::default();
        };
        let Ok(group) = graph.locate(&reference.reference(), &sequence.identity()) else {
            return NativeTargetMetadata::default();
        };
        if group.tag() == records::VIDEO_TRACK_GROUP.tag {
            if let Some((width, height)) = group
                .element()
                .child("FrameRect")
                .and_then(|element| element.text())
                .and_then(frame_dimensions)
            {
                metadata.width = Some(width);
                metadata.height = Some(height);
            }
            metadata.fps = group
                .element()
                .child("TrackGroup")
                .and_then(|element| element.child("FrameRate"))
                .and_then(|element| element.text())
                .and_then(|text| text.parse::<u64>().ok())
                .filter(|ticks| *ticks != 0)
                .map(|ticks| crate::schema::TICKS as f64 / ticks as f64);
            collect_direct_tracks(
                graph,
                group,
                records::VIDEO_CLIP_TRACK.tag,
                &mut metadata.video_track_count,
                &mut end_ticks,
            );
        } else if group.tag() == records::AUDIO_TRACK_GROUP.tag {
            collect_direct_tracks(
                graph,
                group,
                records::AUDIO_CLIP_TRACK.tag,
                &mut metadata.audio_track_count,
                &mut end_ticks,
            );
        } else if group.tag() != records::DATA_TRACK_GROUP.tag
            || !group
                .track_references()
                .is_ok_and(|tracks| tracks.is_empty())
        {
            // Empty native data groups carry no timeline content. Other groups
            // may extend the duration, but do not invalidate known AV metadata.
            end_ticks = None;
        }
    }
    metadata.duration_secs = end_ticks.map(|ticks| ticks as f64 / crate::schema::TICKS as f64);
    metadata
}

fn collect_direct_tracks(
    graph: &Graph<'_>,
    group: crate::format::Record<'_>,
    expected_tag: &str,
    count: &mut Option<usize>,
    end_ticks: &mut Option<u64>,
) {
    let Ok(references) = group.track_references() else {
        *count = None;
        *end_ticks = None;
        return;
    };
    for reference in references {
        let Ok(track) = graph.locate(&reference, &group.identity()) else {
            *count = None;
            *end_ticks = None;
            continue;
        };
        if track.tag() != expected_tag {
            *count = None;
            *end_ticks = None;
            continue;
        }
        if let Some(value) = count.as_mut() {
            *value += 1;
        }
        let Some(clip_track) = track.element().child("ClipTrack") else {
            *end_ticks = None;
            continue;
        };
        let items = clip_track
            .child("ClipItems")
            .and_then(|items| items.child("TrackItems"))
            .into_iter()
            .flat_map(crate::format::graph::Element::children);
        for item in items {
            let end = graph
                .locate(&item.reference(), &track.identity())
                .ok()
                .and_then(native_item_end);
            *end_ticks = end_ticks.zip(end).map(|(current, end)| current.max(end));
        }
    }
}

fn native_item_end(item: crate::format::Record<'_>) -> Option<u64> {
    let range = item.element().child("ClipTrackItem")?.child("TrackItem")?;
    let start = match range.child("Start") {
        Some(start) => start.text()?.trim().parse::<u64>().ok()?,
        None => 0,
    };
    let end = range.child("End")?.text()?.trim().parse::<u64>().ok()?;
    (end > start).then_some(end)
}

fn frame_dimensions(value: &str) -> Option<(u32, u32)> {
    let mut values = value.split(',').map(str::parse::<i64>);
    let (left, top, right, bottom) = (
        values.next()?.ok()?,
        values.next()?.ok()?,
        values.next()?.ok()?,
        values.next()?.ok()?,
    );
    if values.next().is_some() {
        return None;
    }
    Some((
        u32::try_from(right.checked_sub(left)?).ok()?,
        u32::try_from(bottom.checked_sub(top)?).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_empty_data_group_preserves_scene_metadata() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/feature_adjacent_cut_strict.prproj");
        let targets = super::PrProjectFile::import_targets(&source).unwrap();
        assert_eq!(targets.len(), 1);
        let target = &targets[0];
        assert_eq!(target.name, "Adjacent cut");
        assert_eq!((target.width, target.height), (Some(1920), Some(1080)));
        assert_eq!(target.fps, Some(30.0));
        assert_eq!(target.duration_secs, Some(4.0));
        assert_eq!(target.video_track_count, Some(2));
        assert_eq!(target.audio_track_count, Some(4));
    }

    #[test]
    fn unknown_group_preserves_av_metadata_but_not_a_partial_duration() {
        let xml = include_str!("../../../tests/fixtures/one-clip.xml")
            .replace(
                "</TrackGroups>",
                "<TrackGroup><Second ObjectRef=\"999\"/></TrackGroup></TrackGroups>",
            )
            .replace(
                "</PremiereData>",
                "<UnknownTrackGroup ObjectID=\"999\"/></PremiereData>",
            );
        let graph = super::Graph::parse(&xml).unwrap();
        let targets = super::native_import_targets(&graph);
        assert_eq!(targets[0].width, Some(1920));
        assert_eq!(targets[0].fps, Some(30.0));
        assert_eq!(targets[0].video_track_count, Some(1));
        assert_eq!(targets[0].duration_secs, None);
    }

    #[test]
    fn import_metadata_is_native_and_unknown_values_are_not_defaulted() {
        let xml = include_str!("../../../tests/fixtures/one-clip.xml");
        let graph = super::Graph::parse(xml).unwrap();
        let targets = super::native_import_targets(&graph);
        let target = &targets[0];
        assert_eq!((target.width, target.height), (Some(1920), Some(1080)));
        assert_eq!(target.fps, Some(30.0));
        assert_eq!(target.duration_secs, Some(5.0));
        assert_eq!(target.video_track_count, Some(1));
        assert_eq!(target.audio_track_count, Some(0));

        let broken = xml
            .replace(
                "<FrameRate>8467200000</FrameRate>",
                "<FrameRate>bad</FrameRate>",
            )
            .replace("<End>1270080000000</End>", "<End>bad</End>");
        let graph = super::Graph::parse(&broken).unwrap();
        let targets = super::native_import_targets(&graph);
        assert_eq!(targets[0].fps, None);
        assert_eq!(targets[0].duration_secs, None);
        assert_eq!(targets[0].video_track_count, Some(1));

        let broken = xml.replace(
            "<Second ObjectRef=\"1\"/>",
            "<Second ObjectRef=\"missing\"/>",
        );
        let graph = super::Graph::parse(&broken).unwrap();
        let targets = super::native_import_targets(&graph);
        assert_eq!(targets[0].video_track_count, None);
        assert_eq!(targets[0].audio_track_count, None);
        assert_eq!(targets[0].duration_secs, None);
    }

    #[test]
    fn import_metadata_traverses_past_the_former_visit_quota() {
        const TRACKS: usize = 50_001;
        let tracks = r#"<Track ObjectURef="track-1"/>"#.repeat(TRACKS);
        let xml = include_str!("../../../tests/fixtures/one-clip.xml")
            .replace(r#"<Track ObjectURef="track-1"/>"#, &tracks);
        let graph = super::Graph::parse(&xml).unwrap();
        let targets = super::native_import_targets(&graph);
        let target = &targets[0];
        assert_eq!(target.video_track_count, Some(TRACKS));
        assert_eq!(target.duration_secs, Some(5.0));
    }

    use super::*;
    use crate::{
        format::tests::nested::{
            placement_records, sequence_records, sequence_source, with_records, Placement,
            ONE_CLIP_XML,
        },
        schema::{PrSequence, TICKS},
    };

    /// Top-level roots in GUID (selection) order: A to D each place "shared",
    /// whose 4 KiB name each copy repeats; E and `one-clip.xml`'s Main each
    /// hold one clip.
    fn roots_xml() -> String {
        let second = Placement {
            start: 0,
            end: TICKS,
            source_in: 0,
        };
        let mut records = sequence_records("shared", &"s".repeat(4096), 6000, &[6010])
            + &sequence_source(6002, "shared")
            + &placement_records(6010, 7, &second);
        for (root, base) in ["A", "B", "C", "D", "E"]
            .into_iter()
            .zip((7000_u32..).step_by(100))
        {
            let guid = format!("root-{}", root.to_lowercase());
            records.push_str(&sequence_records(
                &guid,
                &format!("Root {root}"),
                base,
                &[base + 10],
            ));
            let source = if root == "E" { 7 } else { 6002 };
            records.push_str(&placement_records(base + 10, source, &second));
        }
        with_records(ONE_CLIP_XML, &records)
    }

    fn names(project: &PrProjectFile) -> Vec<&str> {
        project.sequences().map(PrSequence::name).collect()
    }

    #[test]
    fn all_selected_timelines_are_retained_without_an_aggregate_byte_quota() {
        let (project, omissions) = PrProjectFile::load_xml(&roots_xml(), None, None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(
            names(&project),
            ["Root A", "Root B", "Root C", "Root D", "Root E", "Main"]
        );
        assert_eq!(project.media.len(), 1);
    }
}
