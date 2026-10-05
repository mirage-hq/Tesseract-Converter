//! Selected native source reachability, independent of editable placement support.
//!
//! Each reached track item is resolved to the media or nested sequence that it
//! plays through identity links alone, each the only one of its kind, before
//! any other field is read. So a placement that conversion omits still counts
//! as a use of its media, and the loaded model can stand for one media's whole
//! use while it omits another's.

#[cfg(test)]
use super::read_xml;
use super::video;
use crate::{
    error::{ensure, unsupported, Result},
    format::{graph::Element, Graph, PrProjectFile, Record},
    schema::{native::Reference, records, MediaId, PrMedia, PrSequence},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::Path,
};

/// What one native track item plays.
#[derive(Debug)]
enum Source {
    Media(MediaId),
    /// A nested sequence, by GUID.
    Sequence(String),
}

/// One native picture or sound track item.
#[derive(Debug)]
struct Placement {
    /// The item's identity, which its loaded occurrence keeps as its ID.
    item: String,
    picture: bool,
    source: Source,
}

#[derive(Default)]
pub(crate) struct NativeMediaScope {
    /// The placed media whose native records read.
    pub(crate) media: BTreeMap<MediaId, PrMedia>,
    /// Why a media record or a link was not read.
    pub(crate) unassessed: Vec<String>,
    /// The placements of each reached native sequence, by GUID.
    placements: BTreeMap<String, Vec<Placement>>,
    /// Media that a placement can play through a proxy or a channel source of
    /// another record, as a merged clip does. No loaded occurrence keeps it.
    alternates: BTreeSet<MediaId>,
    /// Whether a reached link did not resolve or was not the only one of its
    /// kind, which could hide a use of any media.
    unresolved: bool,
    /// Clock-only proofs, independent of appearance/effect admission. A failed
    /// proof never stands in for a missing loaded placement.
    direct_clocks: BTreeMap<String, Result<Range<i64>>>,
}

impl NativeMediaScope {
    /// Whether the loaded `sequence` keeps every native use of `media`, so its
    /// placements there are the media's whole selected use. Each loaded copy
    /// of a reused sequence must keep its own: a retained copy cannot cover
    /// an omitted one.
    pub(crate) fn covers(&self, sequence: &PrSequence, media: &MediaId) -> bool {
        !self.unresolved && !self.alternates.contains(media) && self.retains(sequence, media)
    }

    /// Bounded fallback for direct physical uses only. A reached nest still
    /// needs the retained-model proof; neither missing copies nor ambiguous
    /// native topology can acquire clocks through this fallback.
    pub(crate) fn direct_video_use(
        &self,
        target: &str,
        media: &MediaId,
    ) -> Option<crate::media::VideoUse> {
        if self.unresolved || self.alternates.contains(media) {
            return None;
        }
        let source = self.media.get(media)?;
        let video = source.video.as_ref()?;
        if !matches!(video.kind, crate::schema::PrMediaKind::Video { .. }) {
            return None;
        }
        let mut ranges = Vec::new();
        let mut uses_audio = false;
        for placement in self.placements.get(target)? {
            match &placement.source {
                Source::Sequence(nested) if self.reaches(nested, media, !placement.picture) => {
                    return None
                }
                Source::Media(id) if id == media => {
                    let range = self.direct_clocks.get(&placement.item)?.as_ref().ok()?;
                    let end = if placement.picture {
                        video.intrinsic_ticks
                    } else {
                        source.audio.as_ref()?.intrinsic_ticks
                    };
                    if range.end > end {
                        return None;
                    }
                    if placement.picture {
                        ranges.push(range.clone());
                    } else {
                        uses_audio = true;
                    }
                }
                _ => {}
            }
        }
        (!ranges.is_empty()).then_some(crate::media::VideoUse { ranges, uses_audio })
    }

    /// Whether the loaded copy `sequence` keeps, with `media`, each native
    /// placement of its sequence that uses it, directly or in a nest.
    fn retains(&self, sequence: &PrSequence, media: &MediaId) -> bool {
        let Some(placements) = sequence.id.as_ref().and_then(|id| self.placements.get(id)) else {
            return false;
        };
        placements.iter().all(|placement| {
            let item = Some(placement.item.as_str());
            match (&placement.source, placement.picture) {
                (Source::Media(played), _) if played != media => true,
                (Source::Media(_), true) => sequence
                    .video_occurrences()
                    .any(|clip| clip.id.as_deref() == item && &clip.media == media),
                (Source::Media(_), false) => sequence
                    .audio
                    .iter()
                    .any(|clip| clip.id.as_deref() == item && &clip.media == media),
                (Source::Sequence(nested), true) => {
                    !self.reaches(nested, media, false)
                        || sequence.nest_occurrences().any(|nest| {
                            nest.id.as_deref() == item
                                && nest.sequence.id.as_ref() == Some(nested)
                                && self.retains(&nest.sequence, media)
                        })
                }
                // A loaded nested sound no longer keeps the audio item that plays it.
                (Source::Sequence(nested), false) => !self.reaches(nested, media, true),
            }
        })
    }

    /// Whether the native sequence `guid` or a sequence nested in it places
    /// `media`. A sequence's audio mix plays only its sound placements, so
    /// with `sound` only those count.
    fn reaches(&self, guid: &str, media: &MediaId, sound: bool) -> bool {
        let mut pending = vec![guid];
        let mut visited = BTreeSet::new();
        while let Some(guid) = pending.pop() {
            if !visited.insert(guid) {
                continue;
            }
            // Every reached sequence is inventoried unless a link is unresolved.
            let Some(placements) = self.placements.get(guid) else {
                return true;
            };
            for placement in placements
                .iter()
                .filter(|placement| !sound || !placement.picture)
            {
                match &placement.source {
                    Source::Media(played) if played == media => return true,
                    Source::Media(_) => {}
                    Source::Sequence(nested) => pending.push(nested),
                }
            }
        }
        false
    }

    /// Record a link that did not resolve: it could hide a use of any media.
    fn mark_unresolved(&mut self, reason: String) {
        self.unresolved = true;
        self.unassessed.push(reason);
    }

    /// The item records on the tracks of `sequence`.
    fn items<'a>(&mut self, graph: &'a Graph<'_>, sequence: Record<'a>) -> Vec<Record<'a>> {
        let mut items = Vec::new();
        let groups = match exactly_one(sequence.element(), &["TrackGroups"], &sequence.identity()) {
            Ok(groups) => groups,
            Err(error) => {
                self.mark_unresolved(error.to_string());
                return items;
            }
        };
        for entry in groups.children() {
            let (group, tracks) = match track_group(graph, sequence, entry) {
                Ok(group) => group,
                Err(error) => {
                    self.mark_unresolved(error.to_string());
                    continue;
                }
            };
            for reference in tracks {
                let track = match graph.locate(&reference, &group.identity()) {
                    Ok(track) => track,
                    Err(error) => {
                        self.mark_unresolved(error.to_string());
                        continue;
                    }
                };
                for reference in track.track_item_references() {
                    match graph.locate(&reference, &track.identity()) {
                        Ok(item) => items.push(item),
                        Err(error) => self.mark_unresolved(error.to_string()),
                    }
                }
            }
        }
        items
    }

    /// What the track item `item` plays, or `None` for an item that places no
    /// media. Its media record is read once that identity is known.
    fn placement(&mut self, graph: &Graph<'_>, item: Record<'_>) -> Result<Option<Placement>> {
        let picture = match item.tag() {
            tag if tag == records::VIDEO_CLIP_TRACK_ITEM.tag => true,
            tag if tag == records::AUDIO_CLIP_TRACK_ITEM.tag => false,
            // Transitions blend the placements beside them; captions are data.
            tag if tag == records::VIDEO_TRANSITION_TRACK_ITEM.tag
                || tag == "AudioTransitionTrackItem"
                || tag == crate::schema::caption::CAPTION_DATA_CLIP_TRACK_ITEM.tag =>
            {
                return Ok(None);
            }
            tag => {
                return Err(unsupported(format!(
                    "{}: native track item {tag} is not assessed",
                    item.identity()
                )))
            }
        };
        let sub = graph.locate_as(
            &link(item, &["ClipTrackItem", "SubClip"])?,
            records::SUB_CLIP.tag,
            &item.identity(),
        )?;
        let clip = graph.locate(&link(sub, &["Clip"])?, &sub.identity())?;
        let played = graph.locate(&link(clip, &["Clip", "Source"])?, &clip.identity())?;
        let (source, media) = self.source(graph, played)?;
        // A clip channel can play another source record, as in a merged clip.
        for entry in at_most_one(clip.element(), &["SecondaryContents"], &clip.identity())?
            .into_iter()
            .flat_map(Element::children)
        {
            let secondary = graph.locate_as(
                &entry.reference(),
                records::SECONDARY_CONTENT.tag,
                &clip.identity(),
            )?;
            let channel = graph.locate(&link(secondary, &["Content"])?, &secondary.identity())?;
            if channel == played {
                continue;
            }
            match self.source(graph, channel)?.0 {
                Source::Media(id) => {
                    if !matches!(&source, Source::Media(primary) if *primary == id) {
                        self.alternates.insert(id);
                    }
                }
                Source::Sequence(_) => {
                    return Err(unsupported(format!(
                        "{}: native channel source {} is a sequence",
                        clip.identity(),
                        channel.identity()
                    )))
                }
            }
        }
        if matches!(source, Source::Media(_)) {
            if let Err(error) =
                read_placed_media(graph, item, &media, &played.identity(), &mut self.media)
            {
                self.unassessed.push(error.to_string());
            }
        }
        self.direct_clocks.insert(
            item.identity(),
            direct_unit_clock(graph, item, clip, picture),
        );
        Ok(Some(Placement {
            item: item.identity(),
            picture,
            source,
        }))
    }

    /// What the source record `source` plays, with the link that names it.
    /// The media that its proxies can play instead join `alternates`. Any
    /// other link could select media that this inventory cannot see.
    fn source(&mut self, graph: &Graph<'_>, source: Record<'_>) -> Result<(Source, Reference)> {
        let from = source.identity();
        let (primary, kind) = match source.tag() {
            tag if tag == records::VIDEO_MEDIA_SOURCE.tag
                || tag == records::AUDIO_MEDIA_SOURCE.tag =>
            {
                ("MediaSource/Media", records::MEDIA.tag)
            }
            tag if tag == records::VIDEO_SEQUENCE_SOURCE.tag
                || tag == records::AUDIO_SEQUENCE_SOURCE.tag =>
            {
                ("SequenceSource/Sequence", records::SEQUENCE.tag)
            }
            tag => {
                return Err(unsupported(format!(
                    "{from}: native source {tag} is not assessed"
                )))
            }
        };
        let mut played = None;
        for (path, reference) in links(source.element()) {
            if path == primary {
                if played.is_some() {
                    return Err(unsupported(format!("{from}: native {path} is duplicated")));
                }
                played = Some((graph.locate_as(&reference, kind, &from)?, reference));
                continue;
            }
            let proxy = match path.as_str() {
                "MediaSource/Content/ProxyMedia" => {
                    graph.locate_as(&reference, records::MEDIA.tag, &from)?
                }
                "MediaSource/Content/AudioProxies/AudioProxyItem" => {
                    let proxy = graph.locate_as(&reference, "AudioProxy", &from)?;
                    graph.locate_as(
                        &link(proxy, &["ProxyMedia"])?,
                        records::MEDIA.tag,
                        &proxy.identity(),
                    )?
                }
                _ => {
                    return Err(unsupported(format!(
                        "{from}: native source link {path} is not assessed"
                    )))
                }
            };
            self.alternates.insert(video::media_id(proxy)?);
        }
        let (record, reference) =
            played.ok_or_else(|| unsupported(format!("{from}: missing native {primary}")))?;
        let source = if kind == records::MEDIA.tag {
            Source::Media(video::media_id(record)?)
        } else {
            let guid = record
                .element()
                .attribute(records::OBJECT_UID)
                .ok_or_else(|| {
                    unsupported(format!(
                        "{}: native sequence identity is unavailable",
                        record.identity()
                    ))
                })?;
            Source::Sequence(guid.to_owned())
        };
        Ok((source, reference))
    }
}

/// Read only temporal fields using the same typed records and scalar readers
/// as placement conversion. Appearance, effects and sound gain are irrelevant
/// to this proof. Unknown remaps/holds and every nonunit clock remain excluded.
fn direct_unit_clock(
    graph: &Graph<'_>,
    item: Record<'_>,
    clip: Record<'_>,
    picture: bool,
) -> Result<Range<i64>> {
    use crate::schema::native::{AudioClip, AudioClipTrackItem, VideoClip, VideoClipTrackItem};
    let from = item.identity();
    let (body, clock) = if picture {
        let item = graph.decode::<VideoClipTrackItem>(item)?;
        let clip = graph.decode::<VideoClip>(clip)?;
        ensure!(
            !clip.value.declares_frame_hold(),
            "{from}: frame hold has no unit clock proof"
        );
        (
            super::required(item.value.clip_track_item, &from, "ClipTrackItem")?,
            super::required(clip.value.clip, &from, "Clip")?,
        )
    } else {
        (
            graph
                .decode::<AudioClipTrackItem>(item)?
                .value
                .clip_track_item,
            graph.decode::<AudioClip>(clip)?.value.clip,
        )
    };
    super::require_zero_subclip_time_offset(&body, &from)?;
    ensure!(
        video::playback_rate(&clock, &from)? == 1.0
            && clock.time_remapping.is_none()
            && !clock.is_multicam.unwrap_or(false)
            && clock.selected_track_index.is_none(),
        "{from}: source clock is not proved unit forward playback"
    );
    let range = super::required(body.track_item, &from, "TrackItem")?;
    let start = range
        .start
        .as_deref()
        .map(|s| super::integer(s, &from))
        .transpose()?
        .unwrap_or(0);
    let end = super::integer(&range.end, &from)?;
    let source_in = super::required_integer(clock.in_point.as_deref(), &from, "InPoint")?;
    let source_out = super::required_integer(clock.out_point.as_deref(), &from, "OutPoint")?;
    ensure!(
        start >= 0
            && end > start
            && source_in >= 0
            && source_out > source_in
            && end - start == source_out - source_in,
        "{from}: invalid or nonunit native source range"
    );
    Ok(source_in..source_out)
}

impl PrProjectFile {
    /// Inventory the native placements that the selected sequence reaches,
    /// whatever conversion keeps of them, and read the media that they place.
    pub(crate) fn native_media_scope(path: &Path, target: &str) -> Result<NativeMediaScope> {
        Self::native_media_scope_with_media_relink(path, target, None)
    }

    pub(crate) fn native_media_scope_with_media_relink(
        path: &Path,
        target: &str,
        relink: Option<&crate::ValidatedMediaRelink>,
    ) -> Result<NativeMediaScope> {
        if let Some(relink) = relink {
            relink.validate_for(path, target)?;
        }
        let xml = super::read_xml_with_media_relink(path, relink)?;
        let graph = Graph::parse(&xml)?.with_media_relink(relink)?;
        graph.locate_uid(target, "media inspection target")?;
        let mut scope = NativeMediaScope::default();
        let mut pending = vec![target.to_owned()];
        while let Some(guid) = pending.pop() {
            if scope.placements.contains_key(&guid) {
                continue;
            }
            // A nested GUID was read from the sequence record that it names.
            let sequence = graph.locate_uid(&guid, "native media inventory")?;
            let mut placements = Vec::new();
            for item in scope.items(&graph, sequence) {
                match scope.placement(&graph, item) {
                    Ok(Some(placement)) => {
                        if let Source::Sequence(nested) = &placement.source {
                            pending.push(nested.clone());
                        }
                        placements.push(placement);
                    }
                    Ok(None) => {}
                    Err(error) => scope.mark_unresolved(error.to_string()),
                }
            }
            scope.placements.insert(guid, placements);
        }
        Ok(scope)
    }
}

/// The track group of one `TrackGroups` entry of `sequence` and its track references.
fn track_group<'a>(
    graph: &'a Graph<'_>,
    sequence: Record<'a>,
    entry: Element<'a>,
) -> Result<(Record<'a>, Vec<Reference>)> {
    let from = sequence.identity();
    let group = graph.locate(&exactly_one(entry, &["Second"], &from)?.reference(), &from)?;
    // `track_references` reads the first list; a second could hold other tracks.
    at_most_one(
        group.element(),
        &["TrackGroup", "Tracks"],
        &group.identity(),
    )?;
    Ok((group, group.track_references()?))
}

/// Read the media that the picture or sound `item` places through the link
/// `media` of the source record `from`, as the readers read it. Synthetic
/// graphic media has no file to inspect.
fn read_placed_media(
    graph: &Graph<'_>,
    item: Record<'_>,
    media: &Reference,
    from: &str,
    table: &mut BTreeMap<MediaId, PrMedia>,
) -> Result<()> {
    // Generator and adjustment records keep their existing admission policy.
    let adjustment = if item.tag() == records::VIDEO_CLIP_TRACK_ITEM.tag {
        let decoded = graph.decode(item)?;
        let adjustment = super::adjustment::is_flagged(graph, &decoded);
        if !adjustment && super::graphic::graphic_clip(graph, &decoded).is_some() {
            return Ok(());
        }
        adjustment
    } else {
        false
    };
    video::read_source_media(graph, media, from, table, adjustment, &mut Vec::new())?;
    Ok(())
}

/// The reference that the element at `path` below `record` carries.
fn link(record: Record<'_>, path: &[&str]) -> Result<Reference> {
    Ok(exactly_one(record.element(), path, &record.identity())?.reference())
}

/// Like [`at_most_one`], but the element must be there.
fn exactly_one<'a>(element: Element<'a>, path: &[&str], from: &str) -> Result<Element<'a>> {
    at_most_one(element, path, from)?
        .ok_or_else(|| unsupported(format!("{from}: missing native {}", path.join("/"))))
}

/// The element at `path` below `element`, if each step is there. Each must be
/// the only child of its tag: a second could own a use that the first does not.
fn at_most_one<'a>(element: Element<'a>, path: &[&str], from: &str) -> Result<Option<Element<'a>>> {
    let mut found = element;
    for (step, tag) in path.iter().enumerate() {
        let mut children = found.children().filter(|child| child.tag() == *tag);
        let Some(child) = children.next() else {
            return Ok(None);
        };
        if children.next().is_some() {
            return Err(unsupported(format!(
                "{from}: native {} is duplicated",
                path[..=step].join("/")
            )));
        }
        found = child;
    }
    Ok(Some(found))
}

/// Every reference below `element`, with the path of tags that leads to it.
fn links(element: Element<'_>) -> Vec<(String, Reference)> {
    fn walk(element: Element<'_>, path: &str, links: &mut Vec<(String, Reference)>) {
        for child in element.children() {
            let path = if path.is_empty() {
                child.tag().to_owned()
            } else {
                format!("{path}/{}", child.tag())
            };
            if child.attribute(records::OBJECT_REF).is_some()
                || child.attribute(records::OBJECT_UREF).is_some()
            {
                links.push((path.clone(), child.reference()));
            }
            walk(child, &path, links);
        }
    }
    let mut links = Vec::new();
    walk(element, "", &mut links);
    links
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        schema::PrAudioOccurrence,
        tests::support::{nest_of, video_sequence},
    };

    fn placement(item: &str, picture: bool, source: Source) -> Placement {
        Placement {
            item: item.into(),
            picture,
            source,
        }
    }

    fn media(id: &str) -> Source {
        Source::Media(MediaId(id.into()))
    }

    fn nested(guid: &str) -> Source {
        Source::Sequence(guid.into())
    }

    fn sound(id: &str, media: &MediaId) -> PrAudioOccurrence {
        PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: Some(id.into()),
            media: media.clone(),
            start_ticks: 0,
            end_ticks: crate::schema::TICKS,
            in_ticks: 0,
            out_ticks: crate::schema::TICKS,
            volume: fx_schema::LinearGain::UNITY,
            volume_keys: None,
            fade_in: None,
            fade_out: None,
        }
    }

    #[test]
    fn every_reused_nest_instance_must_cover_its_native_placements() {
        let source = MediaId("source".into());
        let other = MediaId("other".into());
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
            placements: BTreeMap::from([
                (
                    "outer".into(),
                    vec![
                        placement("first nest", true, nested("inner")),
                        placement("second nest", true, nested("inner")),
                        // Omitted: it places only other media.
                        placement("other nest", true, nested("unrelated")),
                    ],
                ),
                (
                    "inner".into(),
                    vec![placement("physical", true, media("source"))],
                ),
                (
                    "unrelated".into(),
                    vec![
                        placement("other picture", true, media("other")),
                        placement("other sound", false, media("other")),
                    ],
                ),
            ]),
            ..Default::default()
        };
        assert!(native.covers(&outer, &source));
        assert!(!native.covers(&outer, &other));
        // A hidden use is still a use; it must survive like any other.
        let mut hidden = outer.clone();
        hidden.video_tracks[0].nests[1].enabled = false;
        hidden.video_tracks[0].nests[1].sequence.video_tracks[0]
            .clip_mut(0)
            .enabled = false;
        assert!(native.covers(&hidden, &source));
        hidden.video_tracks[0].nests[1].sequence.video_tracks[0]
            .items
            .clear();
        assert!(!native.covers(&hidden, &source));
        let mut missing = outer.clone();
        missing.video_tracks[0].nests[0].sequence.video_tracks[0]
            .items
            .clear();
        assert!(!native.covers(&missing, &source));
        // A global set still contains every ID from the second retained copy.
        // It cannot authorize the first copy's unknown source interval.
        missing = outer.clone();
        missing.video_tracks[0].nests.remove(0);
        assert!(!native.covers(&missing, &source));
        // The retained item must play the same media in a copy of the same sequence.
        missing = outer.clone();
        missing.video_tracks[0].nests[0].sequence.video_tracks[0]
            .clip_mut(0)
            .media = other.clone();
        assert!(!native.covers(&missing, &source));
        missing = outer.clone();
        missing.video_tracks[0].nests[0].sequence.id = Some("unrelated".into());
        assert!(!native.covers(&missing, &source));

        // A sound placement of the media must survive as its sound, in each copy.
        native.placements.get_mut("inner").unwrap().push(placement(
            "sound",
            false,
            media("source"),
        ));
        assert!(!native.covers(&outer, &source));
        let mut sounding = outer.clone();
        for nest in &mut sounding.video_tracks[0].nests {
            nest.sequence.audio.push(sound("sound", &source));
        }
        assert!(native.covers(&sounding, &source));
        sounding.video_tracks[0].nests[1].sequence.audio[0].media = other.clone();
        assert!(!native.covers(&sounding, &source));
        sounding.video_tracks[0].nests[1].sequence.audio[0].media = source.clone();
        // The audio item of a nest plays its sound flattened, without its native
        // owner: it withholds the media that the nest's sound places, only.
        native.placements.get_mut("outer").unwrap().push(placement(
            "unrelated sound",
            false,
            nested("unrelated"),
        ));
        assert!(native.covers(&sounding, &source));
        native.placements.get_mut("outer").unwrap().push(placement(
            "nest sound",
            false,
            nested("inner"),
        ));
        assert!(!native.covers(&sounding, &source));
        // The mix of a nest carries no picture.
        native.placements.get_mut("inner").unwrap().pop();
        assert!(native.covers(&outer, &source));

        // A cycle that reaches the media is a placement that no copy keeps.
        native
            .placements
            .get_mut("inner")
            .unwrap()
            .push(placement("cycle", true, nested("outer")));
        assert!(!native.covers(&outer, &source));
        native.placements.get_mut("inner").unwrap().pop();

        // Media played through another record's proxy or channel source has
        // no loaded occurrence; an unresolved link could hide any media.
        native.alternates.insert(source.clone());
        assert!(!native.covers(&outer, &source));
        native.alternates.clear();
        assert!(native.covers(&outer, &source));
        native.unresolved = true;
        assert!(!native.covers(&outer, &source));
    }

    /// `xml` with the first `from` after `anchor` replaced by `to`.
    fn edit(xml: &str, anchor: &str, from: &str, to: &str) -> String {
        let start = xml.find(anchor).expect("anchor");
        let offset = start + xml[start..].find(from).expect("edited text");
        let mut edited = xml.to_owned();
        edited.replace_range(offset..offset + from.len(), to);
        edited
    }

    /// The native inventory of `xml` and its loaded `target` sequence.
    fn inventory(xml: &str, target: &str) -> (NativeMediaScope, PrSequence) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("project.prproj");
        crate::test_support::write_prproj(&path, xml);
        let scope = PrProjectFile::native_media_scope(&path, target).unwrap();
        let (project, _) = PrProjectFile::load_import(&path, Some(target)).unwrap();
        let (mut sequences, _) = project.into_parts();
        (scope, sequences.pop().unwrap())
    }

    fn fixture(name: &str) -> String {
        read_xml(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )
        .unwrap()
    }

    /// The `feature_nested_sequence_strict` timeline places the red media once
    /// and its Inner sequence twice; Inner places the timecoded media twice.
    #[test]
    fn native_placements_prove_selected_use_for_each_media() {
        const OUTER: &str = "dab91e14-ca76-47e7-93fc-99bf6bcc94be";
        let red = MediaId("Media:ObjectUID:93318ede-9c1e-4e0a-94ea-da42c59425c6".into());
        let timecoded = MediaId("Media:ObjectUID:bb3ebba2-51a3-44a7-b1bb-4d97fcee09b1".into());
        let covers = |xml: &str| {
            let (scope, sequence) = inventory(xml, OUTER);
            [&red, &timecoded].map(|media| scope.covers(&sequence, media))
        };
        let xml = fixture("feature_nested_sequence_strict.prproj");
        assert_eq!(covers(&xml), [true, true]);
        let inner_clip = r#"<VideoClipTrackItem ObjectID="149""#;
        let invalid_end = |xml: &str, item: &str| edit(xml, item, "<End>", "<End>invalid");
        // An omitted placement withholds only its own media, in every copy.
        assert_eq!(covers(&invalid_end(&xml, inner_clip)), [true, false]);
        // An omitted copy of a reused sequence is not covered by the other copy.
        let second_nest = r#"<VideoClipTrackItem ObjectID="153""#;
        assert_eq!(covers(&invalid_end(&xml, second_nest)), [true, false]);
        // A hidden placement is a use, retained or omitted.
        let hidden = edit(
            &xml,
            inner_clip,
            r#"<ClipTrackItem Version="8">"#,
            r#"<ClipTrackItem Version="8"><IsMuted>true</IsMuted>"#,
        );
        assert_eq!(covers(&hidden), [true, true]);
        assert_eq!(covers(&invalid_end(&hidden, inner_clip)), [true, false]);
        // A link that does not resolve could hide a use of any media.
        let dangling = edit(
            &xml,
            r#"<VideoClipTrackItem ObjectID="85""#,
            r#"<SubClip ObjectRef="83"/>"#,
            r#"<SubClip ObjectRef="missing"/>"#,
        );
        assert_eq!(covers(&dangling), [false, false]);
        // A proxy of the timecoded source can play red instead, so red's
        // retained placement is not its whole use. An unknown source link
        // could select any media.
        for (link, expected) in [
            ("ProxyMedia", [false, true]),
            ("UnknownMedia", [false, false]),
        ] {
            let alternate = edit(
                &xml,
                r#"<VideoMediaSource ObjectID="19""#,
                r#"<Content Version="10">"#,
                &format!(
                    r#"<Content Version="10"><{link} ObjectURef="93318ede-9c1e-4e0a-94ea-da42c59425c6"/>"#
                ),
            );
            assert_eq!(covers(&alternate), expected, "{link}");
        }
    }

    /// In `feature_images_nests_26_5`, Inner places the linked A/V media as
    /// picture and sound, and the outer sequence plays Inner's mix through an
    /// audio item. The timecoded media is placed only outside Inner.
    #[test]
    fn nested_sound_withholds_only_the_media_that_it_plays() {
        const OUTER: &str = "f3c651e6-0302-4499-b6f5-814b7b22c207";
        let linked = MediaId("Media:ObjectUID:0fd5ec5c-c507-4605-8729-cb060deca048".into());
        let timecoded = MediaId("Media:ObjectUID:812ab15c-c2fb-4640-8b61-751c0c873d4f".into());
        let (scope, sequence) = inventory(&fixture("feature_images_nests_26_5.prproj"), OUTER);
        assert!(sequence
            .video_occurrences()
            .any(|clip| clip.media == linked));
        assert!(!scope.covers(&sequence, &linked));
        assert!(scope.covers(&sequence, &timecoded));
    }

    /// Two owners where the inventory follows one could each place other
    /// media, in either order, so no media's use is proved.
    #[test]
    fn ambiguous_native_owners_withhold_every_media() {
        const OUTER: &str = "dab91e14-ca76-47e7-93fc-99bf6bcc94be";
        const MAIN: &str = "093e7f82-8e3b-4fd4-9a66-1234f931ffd8";
        let covers = |xml: &str, target: &str, media: [&MediaId; 2]| {
            let (scope, sequence) = inventory(xml, target);
            media.map(|media| scope.covers(&sequence, media))
        };
        // Outer item 85 plays red through SubClip 83, VideoClip 82 and source
        // 25; Inner item 145 plays timecoded through SubClip 143, VideoClip
        // 142 and source 19. Outer's video group 42 holds 85's track 232c2776;
        // Inner's group 102 holds Inner's tracks.
        let nested = fixture("feature_nested_sequence_strict.prproj");
        let red = MediaId("Media:ObjectUID:93318ede-9c1e-4e0a-94ea-da42c59425c6".into());
        let timecoded = MediaId("Media:ObjectUID:bb3ebba2-51a3-44a7-b1bb-4d97fcee09b1".into());
        // Items 87 and 96 play A and B through AudioClips 107 and 128, whose
        // channels 124 and 137 name their sources 64 and 69.
        let audio = fixture("feature_audio_clips_strict.prproj");
        let a = MediaId("Media:ObjectUID:bac3dbbe-fc6a-4a1b-bbb1-f89742f2d15c".into());
        let b = MediaId("Media:ObjectUID:4b917883-9aa7-4908-aa73-b9e87a6113c9".into());
        // Source 64 can also play A through the audio proxy 900.
        let proxied = edit(
            &edit(
                &audio,
                r#"<AudioMediaSource ObjectID="64""#,
                r#"<Content Version="10">"#,
                r#"<Content Version="10"><AudioProxies><AudioProxyItem ObjectRef="900"/></AudioProxies>"#,
            ),
            "</PremiereData>",
            "</PremiereData>",
            r#"<AudioProxy ObjectID="900"><ProxyMedia ObjectURef="bac3dbbe-fc6a-4a1b-bbb1-f89742f2d15c"/></AudioProxy></PremiereData>"#,
        );
        assert_eq!(covers(&nested, OUTER, [&red, &timecoded]), [true, true]);
        assert_eq!(covers(&audio, MAIN, [&a, &b]), [true, true]);
        assert_eq!(covers(&proxied, MAIN, [&a, &b]), [false, true]);
        let item = r#"<VideoClipTrackItem ObjectID="85""#;
        let clip = r#"<VideoClip ObjectID="82""#;
        let inner = r#"<Sequence ObjectUID="07beb511-806b-48cb-b0f5-92293e47d8f4""#;
        let group = r#"<VideoTrackGroup ObjectID="102""#;
        // (owner path in its record, record, the owner's opening and closing
        // text, a second owner)
        let picture = [
            (
                "ClipTrackItem",
                item,
                r#"<ClipTrackItem Version="8">"#,
                "</ClipTrackItem>",
                r#"<ClipTrackItem Version="8"><SubClip ObjectRef="143"/></ClipTrackItem>"#,
            ),
            (
                "ClipTrackItem/SubClip",
                item,
                r#"<SubClip ObjectRef="83"/>"#,
                r#"<SubClip ObjectRef="83"/>"#,
                r#"<SubClip ObjectRef="143"/>"#,
            ),
            (
                "Clip",
                r#"<SubClip ObjectID="83""#,
                r#"<Clip ObjectRef="82"/>"#,
                r#"<Clip ObjectRef="82"/>"#,
                r#"<Clip ObjectRef="142"/>"#,
            ),
            (
                "Clip",
                clip,
                r#"<Clip Version="18">"#,
                "</Clip>",
                r#"<Clip Version="18"><Source ObjectRef="19"/></Clip>"#,
            ),
            (
                "Clip/Source",
                clip,
                r#"<Source ObjectRef="25"/>"#,
                r#"<Source ObjectRef="25"/>"#,
                r#"<Source ObjectRef="19"/>"#,
            ),
            (
                "MediaSource/Media",
                r#"<VideoMediaSource ObjectID="25""#,
                r#"<Media ObjectURef="93318ede-9c1e-4e0a-94ea-da42c59425c6"/>"#,
                r#"<Media ObjectURef="93318ede-9c1e-4e0a-94ea-da42c59425c6"/>"#,
                r#"<Media ObjectURef="bb3ebba2-51a3-44a7-b1bb-4d97fcee09b1"/>"#,
            ),
            (
                "TrackGroups",
                inner,
                r#"<TrackGroups Version="1">"#,
                "</TrackGroups>",
                r#"<TrackGroups Version="1"><TrackGroup Version="1" Index="0"><Second ObjectRef="42"/></TrackGroup></TrackGroups>"#,
            ),
            (
                "Second",
                inner,
                r#"<Second ObjectRef="102"/>"#,
                r#"<Second ObjectRef="102"/>"#,
                r#"<Second ObjectRef="42"/>"#,
            ),
            (
                "TrackGroup",
                group,
                r#"<TrackGroup Version="1">"#,
                "</TrackGroup>",
                r#"<TrackGroup Version="1"><Tracks Version="1"><Track Index="0" ObjectURef="232c2776-d3d2-43e9-85d5-3e51471ddaa5"/></Tracks></TrackGroup>"#,
            ),
            (
                "TrackGroup/Tracks",
                group,
                r#"<Tracks Version="1">"#,
                "</Tracks>",
                r#"<Tracks Version="1"><Track Index="0" ObjectURef="232c2776-d3d2-43e9-85d5-3e51471ddaa5"/></Tracks>"#,
            ),
        ];
        let sound = [
            (
                "SecondaryContents",
                r#"<AudioClip ObjectID="107""#,
                r#"<SecondaryContents Version="1">"#,
                "</SecondaryContents>",
                r#"<SecondaryContents Version="1"><SecondaryContentItem Index="0" ObjectRef="137"/></SecondaryContents>"#,
            ),
            (
                "Content",
                r#"<SecondaryContent ObjectID="124""#,
                r#"<Content ObjectRef="64"/>"#,
                r#"<Content ObjectRef="64"/>"#,
                r#"<Content ObjectRef="69"/>"#,
            ),
        ];
        let proxy = (
            "ProxyMedia",
            r#"<AudioProxy ObjectID="900""#,
            r#"<ProxyMedia ObjectURef="bac3dbbe-fc6a-4a1b-bbb1-f89742f2d15c"/>"#,
            r#"<ProxyMedia ObjectURef="bac3dbbe-fc6a-4a1b-bbb1-f89742f2d15c"/>"#,
            r#"<ProxyMedia ObjectURef="4b917883-9aa7-4908-aa73-b9e87a6113c9"/>"#,
        );
        let cases = picture
            .into_iter()
            .map(|owner| (nested.as_str(), OUTER, [&red, &timecoded], owner))
            .chain(
                sound
                    .into_iter()
                    .map(|owner| (audio.as_str(), MAIN, [&a, &b], owner)),
            )
            .chain([(proxied.as_str(), MAIN, [&a, &b], proxy)]);
        let mut open = Vec::new();
        for (xml, target, media, (owner, record, opening, closing, second)) in cases {
            let duplicated = format!("native {owner} is duplicated");
            for (order, xml) in [
                (
                    "second last",
                    edit(xml, record, closing, &format!("{closing}{second}")),
                ),
                (
                    "second first",
                    edit(xml, record, opening, &format!("{second}{opening}")),
                ),
            ] {
                let (scope, sequence) = inventory(&xml, target);
                let covered = media.map(|media| scope.covers(&sequence, media));
                if covered != [false, false]
                    || !scope
                        .unassessed
                        .iter()
                        .any(|reason| reason.ends_with(&duplicated))
                {
                    open.push(format!(
                        "{record} {owner}, {order}: {covered:?} {:?}",
                        scope.unassessed
                    ));
                }
            }
        }
        assert!(open.is_empty(), "{open:#?}");
    }

    #[test]
    fn direct_native_clock_proof_rejects_unknown_nonunit_and_out_of_bounds_uses() {
        let inventory = |xml: &str, target: &str| {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("project.prproj");
            crate::test_support::write_prproj(&path, xml);
            PrProjectFile::native_media_scope(&path, target).unwrap()
        };
        let xml = include_str!("../../../tests/fixtures/one-clip.xml");
        let media = MediaId("Media:ObjectUID:media-1".into());
        let scope = inventory(xml, "sequence-1");
        assert_eq!(
            scope.direct_video_use("sequence-1", &media).unwrap().ranges,
            std::iter::once(0..5 * crate::schema::TICKS).collect::<Vec<_>>()
        );
        for change in [
            "<PlaybackSpeed>2</PlaybackSpeed>",
            "<PlayBackwards>true</PlayBackwards>",
            "<PlaybackSpeed>NaN</PlaybackSpeed>",
            "<TimeRemapping ObjectRef=\"missing\"/>",
            "<IsMulticam>true</IsMulticam>",
            "<SelectedTrackIndex>1</SelectedTrackIndex>",
            "<UnknownClock>1</UnknownClock>",
            "<InPoint>0</InPoint>",
        ] {
            let changed = xml.replace(
                "<InPoint>0</InPoint>",
                &format!("<InPoint>0</InPoint>{change}"),
            );
            let scope = inventory(&changed, "sequence-1");
            assert!(
                scope.direct_video_use("sequence-1", &media).is_none(),
                "{change}"
            );
        }
        for changed in [
            xml.replace("<InPoint>0</InPoint>", "<InPoint>-1</InPoint>"),
            xml.replace("1270080000000", "2794176000000"),
            xml.replace("<End>1270080000000</End>", "<End>invalid</End>"),
            xml.replace(
                "<ClipTrackItem>",
                "<ClipTrackItem><OriginalSubClipTimeOffset>1</OriginalSubClipTimeOffset>",
            ),
            xml.replace(
                "<VideoClip ObjectID=\"6\">",
                "<VideoClip ObjectID=\"6\"><FrameHold>1</FrameHold>",
            ),
            xml.replace(
                "<Source ObjectRef=\"7\"/>",
                "<Source ObjectRef=\"7\"/><Source ObjectRef=\"7\"/>",
            ),
        ] {
            let scope = inventory(&changed, "sequence-1");
            assert!(
                scope.direct_video_use("sequence-1", &media).is_none(),
                "{changed}"
            );
        }
        let scope = inventory(
            &fixture("feature_nested_sequence_strict.prproj"),
            "dab91e14-ca76-47e7-93fc-99bf6bcc94be",
        );
        let nested = MediaId("Media:ObjectUID:bb3ebba2-51a3-44a7-b1bb-4d97fcee09b1".into());
        assert!(scope
            .direct_video_use("dab91e14-ca76-47e7-93fc-99bf6bcc94be", &nested)
            .is_none());
    }

    #[test]
    fn direct_native_sound_clock_reuses_unit_and_unknown_remap_guards() {
        let xml = fixture("feature_audio_clips_strict.prproj");
        for (xml, valid) in [
            (xml.clone(), true),
            (
                edit(
                    &xml,
                    "<AudioClip ObjectID=\"107\"",
                    "</Clip>",
                    "<TimeRemapping ObjectRef=\"unknown\"/></Clip>",
                ),
                false,
            ),
            (
                edit(
                    &xml,
                    "<AudioClip ObjectID=\"107\"",
                    "</Clip>",
                    "<PlaybackSpeed>2</PlaybackSpeed></Clip>",
                ),
                false,
            ),
        ] {
            let (scope, _) = inventory(&xml, "093e7f82-8e3b-4fd4-9a66-1234f931ffd8");
            assert_eq!(scope.direct_clocks["AudioClipTrackItem:87"].is_ok(), valid);
        }
    }
}
