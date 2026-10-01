//! Selected native source reachability, independent of editable placement support.

use super::{read_xml, video};
use crate::{
    error::{unsupported, Result},
    format::{Graph, PrProjectFile, Record},
    schema::{MediaId, PrMedia, PrMediaKind, PrSequence},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::Path,
};

#[derive(Default)]
pub(crate) struct NativeMediaScope {
    pub(crate) media: BTreeMap<MediaId, PrMedia>,
    pub(crate) unassessed: Vec<String>,
    picture_items: BTreeMap<String, BTreeSet<String>>,
    audio_items: BTreeSet<String>,
    nested_audio: bool,
}

impl NativeMediaScope {
    /// Every parsed copy must retain its native physical/nest placements.
    /// A retained copy of a reused sequence cannot cover an omitted copy.
    pub(crate) fn covers(&self, sequence: &PrSequence) -> bool {
        fn pictures(scope: &NativeMediaScope, sequence: &PrSequence) -> bool {
            let Some(expected) = sequence
                .id
                .as_ref()
                .and_then(|id| scope.picture_items.get(id))
            else {
                return false;
            };
            let retained: BTreeSet<_> = sequence
                .video_occurrences()
                .filter_map(|clip| clip.id.as_ref())
                .chain(
                    sequence
                        .nest_occurrences()
                        .filter_map(|nest| nest.id.as_ref()),
                )
                .collect();
            expected.iter().all(|id| retained.contains(id))
                && sequence
                    .nest_occurrences()
                    .all(|nest| pictures(scope, &nest.sequence))
        }
        fn sounds<'a>(sequence: &'a PrSequence, retained: &mut BTreeSet<&'a str>) {
            retained.extend(sequence.audio.iter().filter_map(|clip| clip.id.as_deref()));
            for nest in sequence.nest_occurrences() {
                sounds(&nest.sequence, retained);
            }
        }
        let mut retained_audio = BTreeSet::new();
        sounds(sequence, &mut retained_audio);
        self.unassessed.is_empty()
            && !self.nested_audio
            && pictures(self, sequence)
            && self
                .audio_items
                .iter()
                .all(|id| retained_audio.contains(id.as_str()))
    }
}

impl PrProjectFile {
    /// Inspect membership before unsupported placement properties can omit a clip.
    pub(crate) fn native_media_scope(path: &Path, target: &str) -> Result<NativeMediaScope> {
        let xml = read_xml(path)?;
        let graph = Graph::parse(&xml)?;
        let root = graph.locate_uid(target, "media inspection target")?;
        let mut scope = NativeMediaScope::default();
        let mut pending = vec![root];
        let mut visited = HashSet::new();
        while let Some(sequence) = pending.pop() {
            if !visited.insert(sequence.identity()) {
                continue;
            }
            let Some(guid) = sequence.element().attribute("ObjectUID") else {
                scope.unassessed.push(format!(
                    "{}: native sequence identity is unavailable",
                    sequence.identity()
                ));
                continue;
            };
            let picture_items = scope.picture_items.entry(guid.to_owned()).or_default();
            let Some(groups) = sequence.element().child("TrackGroups") else {
                scope.unassessed.push(format!(
                    "{}: native track groups are unavailable",
                    sequence.identity()
                ));
                continue;
            };
            for entry in groups.children() {
                let Some(reference) = entry.child("Second") else {
                    scope.unassessed.push(format!(
                        "{}: native track group reference is unavailable",
                        sequence.identity()
                    ));
                    continue;
                };
                let result = (|| -> Result<()> {
                    let group = graph.locate(&reference.reference(), &sequence.identity())?;
                    if !matches!(group.tag(), "VideoTrackGroup" | "AudioTrackGroup") {
                        return Ok(());
                    }
                    for reference in group.track_references()? {
                        let track = match graph.locate(&reference, &group.identity()) {
                            Ok(track) => track,
                            Err(error) => {
                                scope.unassessed.push(error.to_string());
                                continue;
                            }
                        };
                        for reference in track.track_item_references() {
                            let result = (|| -> Result<()> {
                                let item = graph.locate(&reference, &track.identity())?;
                                if !matches!(
                                    item.tag(),
                                    "VideoClipTrackItem" | "AudioClipTrackItem"
                                ) {
                                    return Ok(());
                                }
                                if item.tag() == "VideoClipTrackItem" {
                                    let decoded = graph.decode(item)?;
                                    if super::graphic::graphic_clip(&graph, &decoded).is_some()
                                        && !super::adjustment::is_flagged(&graph, &decoded)
                                    {
                                        return Ok(());
                                    }
                                }
                                let sub = follow(&graph, item, &["ClipTrackItem", "SubClip"])?;
                                let clip = follow(&graph, sub, &["Clip"])?;
                                let source = follow(&graph, clip, &["Clip", "Source"])?;
                                if source.element().child("SequenceSource").is_some() {
                                    if item.tag() == "VideoClipTrackItem" {
                                        picture_items.insert(item.identity());
                                    } else {
                                        // Flattened nested sound no longer retains its native owner.
                                        // Its unknown placement coverage keeps whole-source admission.
                                        scope.nested_audio = true;
                                    }
                                    pending.push(follow(
                                        &graph,
                                        source,
                                        &["SequenceSource", "Sequence"],
                                    )?);
                                } else if matches!(
                                    source.tag(),
                                    "VideoMediaSource" | "AudioMediaSource"
                                ) {
                                    let media = source
                                        .element()
                                        .child("MediaSource")
                                        .and_then(|source| source.child("Media"))
                                        .ok_or_else(|| {
                                            unsupported(format!(
                                                "{}: native media reference is unavailable",
                                                source.identity()
                                            ))
                                        })?;
                                    // Generator/adjustment records keep their existing admission policy.
                                    let adjustment = if item.tag() == "VideoClipTrackItem" {
                                        let decoded = graph.decode(item)?;
                                        super::adjustment::is_flagged(&graph, &decoded)
                                    } else {
                                        false
                                    };
                                    let id = video::read_source_media(
                                        &graph,
                                        &media.reference(),
                                        &source.identity(),
                                        &mut scope.media,
                                        adjustment,
                                        &mut Vec::new(),
                                    )?;
                                    if item.tag() == "AudioClipTrackItem" {
                                        scope.audio_items.insert(item.identity());
                                    } else if scope.media[&id].video.as_ref().is_some_and(|video| {
                                        matches!(video.kind, PrMediaKind::Video { .. })
                                    }) {
                                        picture_items.insert(item.identity());
                                    }
                                } else {
                                    scope.unassessed.push(format!(
                                        "{}: native source {} is not assessed",
                                        item.identity(),
                                        source.tag()
                                    ));
                                }
                                Ok(())
                            })();
                            if let Err(error) = result {
                                scope.unassessed.push(error.to_string());
                            }
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    scope.unassessed.push(error.to_string());
                }
            }
        }
        Ok(scope)
    }
}

fn follow<'a>(graph: &'a Graph<'_>, record: Record<'a>, path: &[&str]) -> Result<Record<'a>> {
    let reference = path
        .iter()
        .try_fold(record.element(), |element, name| element.child(name))
        .ok_or_else(|| {
            unsupported(format!(
                "{}: missing native {}",
                record.identity(),
                path.join("/")
            ))
        })?;
    Ok(graph.locate(&reference.reference(), &record.identity())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::{nest_of, video_sequence};

    #[test]
    fn every_reused_nest_instance_must_cover_its_native_placements() {
        let mut inner = video_sequence();
        inner.id = Some("inner".into());
        inner.video_tracks[0].clip_mut(0).id = Some("physical".into());
        let mut outer = video_sequence();
        outer.id = Some("outer".into());
        outer.video_tracks[0].items.clear();
        let end = outer.end_ticks();
        let mut first = nest_of(inner.clone(), 0..end, 0);
        first.id = Some("first nest".into());
        let mut second = nest_of(inner, end..2 * end, 0);
        second.id = Some("second nest".into());
        outer.video_tracks[0].nests = vec![first, second];
        outer.timeline_end_ticks = 2 * end;
        let mut native = NativeMediaScope {
            picture_items: BTreeMap::from([
                (
                    "outer".into(),
                    BTreeSet::from(["first nest".into(), "second nest".into()]),
                ),
                ("inner".into(), BTreeSet::from(["physical".into()])),
            ]),
            ..Default::default()
        };
        assert!(native.covers(&outer));
        let mut missing = outer.clone();
        missing.video_tracks[0].nests[0].sequence.video_tracks[0]
            .items
            .clear();
        assert!(!native.covers(&missing));
        // A global set still contains every ID from the second retained copy.
        // It cannot authorize the first copy's unknown source interval.
        missing = outer.clone();
        missing.video_tracks[0].nests.remove(0);
        assert!(!native.covers(&missing));
        native.audio_items.insert("unretained sound".into());
        assert!(!native.covers(&outer));
        native.audio_items.clear();
        native
            .unassessed
            .push("unresolved native track reference".into());
        assert!(!native.covers(&outer));
        native.unassessed.clear();
        native.nested_audio = true;
        assert!(!native.covers(&outer));
    }
}
