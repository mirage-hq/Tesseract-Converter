//! Discover timeline roots from actual clip placements, not project-panel source records.

use super::{Element, Graph, Record};
use crate::format::{invalid, Result};
use crate::schema::{native::Reference, records};
use std::collections::{BTreeMap, BTreeSet};

// Element paths read from unselected records. Shared native records are decoded
// only for the selected sequence, so topology never depends on their strictness.
const TRACK_GROUPS: &str = "TrackGroups";
const SECOND: &str = "Second";
const CLIP_TRACK_ITEM: &str = "ClipTrackItem";
const CLIP: &str = "Clip";
const SOURCE: &str = "Source";
const SEQUENCE_SOURCE: &str = "SequenceSource";

/// Placements that can nest a sequence.
const CLIP_TRACK_ITEM_TAGS: [&str; 2] = [
    records::VIDEO_CLIP_TRACK_ITEM.tag,
    records::AUDIO_CLIP_TRACK_ITEM.tag,
];
const SEQUENCE_SOURCE_TAGS: [&str; 2] = [
    records::VIDEO_SEQUENCE_SOURCE.tag,
    records::AUDIO_SEQUENCE_SOURCE.tag,
];

/// A native timeline and the timelines placed inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SequenceInfo {
    pub(crate) guid: String,
    pub(crate) name: String,
    /// None when a broken nesting link could point to this sequence.
    pub(crate) top_level: Option<bool>,
    pub(crate) nested_sequences: Vec<String>,
}

/// List timelines in stable GUID order.
///
/// Cyclic nesting is reported, but explicit selection can still convert a
/// sequence's supported occurrences.
///
/// Every sequence has project-panel source records, including ordinary roots.
/// Only sources reached through timeline track items establish a nesting edge.
pub(crate) fn sequences(
    graph: &Graph<'_>,
    omissions: &mut Vec<crate::Omission>,
) -> Result<Vec<SequenceInfo>> {
    let mut result = BTreeMap::new();
    let mut unresolved_nesting = false;
    for sequence in graph
        .records()
        .filter(|record| record.tag() == records::SEQUENCE.tag)
    {
        let identity = sequence.identity();
        let element = sequence.element();
        let Some(guid) = element.attribute(records::OBJECT_UID) else {
            crate::omit(
                omissions,
                crate::OmissionScope::Sequence,
                &identity,
                "missing GUID; timeline not converted",
            );
            unresolved_nesting = true;
            continue;
        };
        let guid = guid.to_owned();
        let name = element
            .child(records::NAME)
            .and_then(Element::text)
            .unwrap_or_default()
            .to_owned();
        if name.is_empty() {
            crate::omit(
                omissions,
                crate::OmissionScope::Sequence,
                guid,
                "missing name; timeline not converted",
            );
            unresolved_nesting = true;
            continue;
        }

        let mut nested = BTreeSet::new();
        for group_link in element
            .child(TRACK_GROUPS)
            .into_iter()
            .flat_map(Element::children)
        {
            let Some(second) = nesting_link(
                require(group_link.child(SECOND), &identity, SECOND),
                &identity,
                omissions,
                &mut unresolved_nesting,
            ) else {
                continue;
            };
            let Some(group) = nesting_link(
                graph.locate(&second.reference(), &identity),
                &identity,
                omissions,
                &mut unresolved_nesting,
            ) else {
                continue;
            };
            let Some(tracks) = nesting_link(
                group.track_references(),
                &identity,
                omissions,
                &mut unresolved_nesting,
            ) else {
                continue;
            };
            for track_reference in tracks {
                let Some(track) = nesting_link(
                    graph.locate(&track_reference, &group.identity()),
                    &identity,
                    omissions,
                    &mut unresolved_nesting,
                ) else {
                    continue;
                };
                for item_reference in track.track_item_references() {
                    let Some(item) = nesting_link(
                        graph.locate(&item_reference, &track.identity()),
                        &identity,
                        omissions,
                        &mut unresolved_nesting,
                    ) else {
                        continue;
                    };
                    if !CLIP_TRACK_ITEM_TAGS.contains(&item.tag()) {
                        continue;
                    }
                    if let Some(Some(child)) = nesting_link(
                        nested_sequence(graph, item),
                        &identity,
                        omissions,
                        &mut unresolved_nesting,
                    ) {
                        nested.insert(child);
                    }
                }
            }
        }
        result.insert(
            guid.clone(),
            SequenceInfo {
                guid,
                name,
                top_level: None,
                nested_sequences: nested.into_iter().collect(),
            },
        );
    }
    crate::format::ensure_valid!(!result.is_empty(), "project has no timelines");

    // Kahn's algorithm also finds a disconnected cycle beside valid roots.
    let mut incoming: BTreeMap<_, usize> = result.keys().map(|id| (id.clone(), 0)).collect();
    for sequence in result.values() {
        for child in &sequence.nested_sequences {
            if let Some(count) = incoming.get_mut(child) {
                *count += 1;
            } else {
                crate::omit(
                    omissions,
                    crate::OmissionScope::Sequence,
                    child,
                    "nested timeline is unavailable",
                );
                unresolved_nesting = true;
            }
        }
    }
    let mut ready = Vec::new();
    for (id, count) in &incoming {
        result
            .get_mut(id)
            .ok_or_else(|| invalid("sequence index missing"))?
            .top_level = if *count == 0 && unresolved_nesting {
            None
        } else {
            Some(*count == 0)
        };
        if *count == 0 {
            ready.push(id.clone());
        }
    }
    let mut visited = 0;
    while let Some(id) = ready.pop() {
        visited += 1;
        for child in &result[&id].nested_sequences {
            if let Some(count) = incoming.get_mut(child) {
                *count -= 1;
                if *count == 0 {
                    ready.push(child.clone());
                }
            }
        }
    }
    if visited != result.len() {
        for (guid, count) in incoming {
            if count > 0 {
                crate::omit(
                    omissions,
                    crate::OmissionScope::Feature,
                    guid,
                    "cyclic sequence nesting not converted",
                );
            }
        }
    }
    Ok(result.into_values().collect())
}

/// GUIDs of the timelines on a nesting cycle: each places itself, directly or
/// through other timelines.
///
/// [`sequences`] reports every timeline that its topological order cannot
/// place, which includes timelines merely placed by a cycle. Only the members
/// of a cycle are returned here, from Tarjan's strongly connected components.
/// The walk keeps its own stack, so a long nesting chain cannot overflow the
/// thread's stack.
pub(crate) fn cyclic_sequences(sequences: &[SequenceInfo]) -> BTreeSet<String> {
    const UNVISITED: usize = usize::MAX;
    let positions: BTreeMap<&str, usize> = sequences
        .iter()
        .enumerate()
        .map(|(position, sequence)| (sequence.guid.as_str(), position))
        .collect();
    let edges: Vec<Vec<usize>> = sequences
        .iter()
        .map(|sequence| {
            sequence
                .nested_sequences
                .iter()
                .filter_map(|child| positions.get(child.as_str()).copied())
                .collect()
        })
        .collect();
    let mut order = vec![UNVISITED; sequences.len()];
    let mut low = vec![UNVISITED; sequences.len()];
    let mut on_stack = vec![false; sequences.len()];
    let mut stack = Vec::new();
    let mut visited = 0;
    let mut cyclic = BTreeSet::new();
    for root in 0..sequences.len() {
        if order[root] != UNVISITED {
            continue;
        }
        // Each entry is a timeline and the index of its next nesting edge.
        let mut walk = vec![(root, 0)];
        (order[root], low[root]) = (visited, visited);
        visited += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some((node, edge)) = walk.pop() {
            if let Some(&child) = edges[node].get(edge) {
                walk.push((node, edge + 1));
                if order[child] == UNVISITED {
                    (order[child], low[child]) = (visited, visited);
                    visited += 1;
                    stack.push(child);
                    on_stack[child] = true;
                    walk.push((child, 0));
                } else if on_stack[child] {
                    low[node] = low[node].min(order[child]);
                }
                continue;
            }
            if let Some(&(parent, _)) = walk.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] == order[node] {
                let mut component = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack[member] = false;
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                if component.len() > 1 || edges[node].contains(&node) {
                    cyclic.extend(
                        component
                            .into_iter()
                            .map(|member| sequences[member].guid.clone()),
                    );
                }
            }
        }
    }
    cyclic
}

/// Record a broken nesting link instead of treating its target as absent.
fn nesting_link<T>(
    result: Result<T>,
    owner: &str,
    omissions: &mut Vec<crate::Omission>,
    unresolved: &mut bool,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            *unresolved = true;
            crate::omit(
                omissions,
                crate::OmissionScope::Feature,
                owner,
                format!("sequence nesting cannot be determined: {error}"),
            );
            None
        }
    }
}

/// The GUID of the sequence a clip placement plays, if its source is a sequence.
///
/// Media sources end the walk without a result.
pub(crate) fn nested_sequence(graph: &Graph<'_>, item: Record<'_>) -> Result<Option<String>> {
    let sub_clip = records::SUB_CLIP.tag;
    let sub = graph.locate_as(
        &reference_at(item, &[CLIP_TRACK_ITEM, sub_clip])?,
        sub_clip,
        &item.identity(),
    )?;
    let clip = graph.locate(&reference_at(sub, &[CLIP])?, &sub.identity())?;
    let source = graph.locate(&reference_at(clip, &[CLIP, SOURCE])?, &clip.identity())?;
    if !SEQUENCE_SOURCE_TAGS.contains(&source.tag()) {
        return Ok(None);
    }
    let sequence = records::SEQUENCE.tag;
    let child = graph.locate_as(
        &reference_at(source, &[SEQUENCE_SOURCE, sequence])?,
        sequence,
        &source.identity(),
    )?;
    let guid = require(
        child.element().attribute(records::OBJECT_UID),
        &child.identity(),
        "GUID",
    )?;
    Ok(Some(guid.to_owned()))
}

/// The reference carried by the element at `path` below a root record.
fn reference_at(record: Record<'_>, path: &[&str]) -> Result<Reference> {
    path.iter()
        .try_fold(record.element(), |element, tag| element.child(tag))
        .map(Element::reference)
        .ok_or_else(|| invalid(format!("{}: missing {}", record.identity(), path.join("/"))))
}

fn require<T>(value: Option<T>, identity: &str, field: &str) -> Result<T> {
    value.ok_or_else(|| invalid(format!("{identity}: missing {field}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequences(xml: &str) -> Result<Vec<SequenceInfo>> {
        super::sequences(&Graph::parse(xml)?, &mut Vec::new())
    }

    fn nested_xml() -> String {
        r#"<PremiereData>
        <Sequence ObjectUID="root"><Name>Main</Name><TrackGroups><TrackGroup><Second ObjectRef="1"/></TrackGroup></TrackGroups></Sequence>
        <Sequence ObjectUID="child"><Name>Nested</Name></Sequence>
        <Sequence ObjectUID="other"><Name>Main</Name></Sequence>
        <VideoTrackGroup ObjectID="1"><TrackGroup><Tracks><Track ObjectRef="2"/></Tracks></TrackGroup></VideoTrackGroup>
        <VideoClipTrack ObjectID="2"><ClipTrack><ClipItems><TrackItems><TrackItem ObjectRef="3"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>
        <VideoClipTrackItem ObjectID="3"><ClipTrackItem><SubClip ObjectRef="4"/></ClipTrackItem></VideoClipTrackItem>
        <SubClip ObjectID="4"><Clip ObjectRef="5"/></SubClip>
        <VideoClip ObjectID="5"><Clip><Source ObjectRef="6"/></Clip></VideoClip>
        <VideoSequenceSource ObjectID="6"><SequenceSource><Sequence ObjectURef="child"/></SequenceSource></VideoSequenceSource>
        <VideoSequenceSource ObjectID="7"><SequenceSource><Sequence ObjectURef="root"/></SequenceSource></VideoSequenceSource>
        </PremiereData>"#
            .to_owned()
    }

    #[test]
    fn classifies_placements_not_unused_project_panel_sources() {
        let items = sequences(&nested_xml()).unwrap();
        assert_eq!(
            items
                .iter()
                .filter(|sequence| sequence.top_level == Some(true))
                .map(|sequence| sequence.guid.as_str())
                .collect::<Vec<_>>(),
            ["other", "root"]
        );
        assert_eq!(items[2].nested_sequences, ["child"]);

        let nested_items = nested_xml()
            .replace("<TrackItems>", "<Extra><TrackItems>")
            .replace("</TrackItems>", "</TrackItems></Extra>");
        assert_eq!(sequences(&nested_items).unwrap(), items);
    }

    #[test]
    fn cycle_does_not_hide_an_unrelated_root() {
        let xml = nested_xml().replace("ObjectURef=\"child\"", "ObjectURef=\"root\"");
        let mut omissions = Vec::new();
        let items = super::sequences(&Graph::parse(&xml).unwrap(), &mut omissions).unwrap();
        assert!(items
            .iter()
            .any(|item| item.guid == "other" && item.top_level == Some(true)));
        assert!(omissions
            .iter()
            .any(|item| item.record == "root" && item.reason.contains("cyclic")));
        // A cycle does not prove the timeline is a root, but an explicit
        // selection can still convert its non-nested clips.
        assert_eq!(
            items
                .iter()
                .find(|item| item.guid == "root")
                .unwrap()
                .top_level,
            Some(false)
        );
    }

    #[test]
    fn only_the_members_of_a_nesting_cycle_are_cyclic() {
        let info = |guid: &str, nested: &[&str]| SequenceInfo {
            guid: guid.to_owned(),
            name: guid.to_owned(),
            top_level: None,
            nested_sequences: nested.iter().map(|child| (*child).to_owned()).collect(),
        };
        // root -> a -> b -> a is a cycle that also places c; d places itself;
        // e is placed by the cycle and by root but reaches no cycle.
        let sequences = [
            info("root", &["a", "e"]),
            info("a", &["b"]),
            info("b", &["a", "c", "missing"]),
            info("c", &["e"]),
            info("d", &["d"]),
            info("e", &[]),
        ];
        assert_eq!(
            super::cyclic_sequences(&sequences)
                .into_iter()
                .collect::<Vec<_>>(),
            ["a", "b", "d"]
        );
        // A chain far longer than any nesting limit is walked without recursion.
        let chain: Vec<_> = (0..20_000)
            .map(|index| SequenceInfo {
                guid: format!("{index:05}"),
                name: "Chain".into(),
                top_level: None,
                nested_sequences: vec![format!("{:05}", (index + 1) % 20_000)],
            })
            .collect();
        assert_eq!(super::cyclic_sequences(&chain).len(), 20_000);
    }

    #[test]
    fn broken_links_never_prove_a_sequence_is_top_level() {
        assert!(sequences("<PremiereData/>").is_err());
        for xml in [
            nested_xml().replace("ObjectURef=\"child\"", "ObjectURef=\"missing\""),
            nested_xml().replace(
                "TrackItem ObjectRef=\"3\"",
                "TrackItem ObjectRef=\"missing\"",
            ),
        ] {
            let mut omissions = Vec::new();
            let items = super::sequences(&Graph::parse(&xml).unwrap(), &mut omissions).unwrap();
            assert!(items.iter().all(|item| item.top_level != Some(true)));
            assert!(omissions
                .iter()
                .any(|item| item.reason.contains("nesting cannot be determined")));
        }
    }
}
