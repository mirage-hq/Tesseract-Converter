//! Channel 0 of a stereo bus containing exactly one centered, true-mono leaf.
//!
//! This is not a general bus selector: unequal channels, nested leaves,
//! automation and omitted processing cannot establish the invariant.

use super::super::{
    nested::{self, NestSound},
    required,
};
use super::{
    centered_stereo_master, chain_gain, is_muted, read_occurrence, report_pan, AudioItem,
    ChainOwner,
};
use crate::{
    approximate,
    error::{ensure, unsupported, Result},
    format::{graph, Graph, Located},
    schema::{
        native::{
            AudioClip, AudioClipTrack, AudioClipTrackItem, AudioMixTrack, AudioTrackGroup,
            Reference, Sequence, SubClip,
        },
        records, AudioChannels, MediaId, PrAudioOccurrence, PrMedia,
    },
    Omission, OmissionKind,
};
use std::collections::{BTreeMap, BTreeSet};

/// The native route and static faders must be known, not unity fallbacks.
fn centered_static_track_gain(
    graph: &Graph<'_>,
    track: &Located<AudioClipTrack>,
    master_reference: Option<&Reference>,
) -> Result<f64> {
    let master = graph.follow::<AudioMixTrack>(
        required(master_reference, &track.identity, "stereo master")?,
        &track.identity,
    )?;
    let inlet = centered_stereo_master(graph, &master)
        .ok_or_else(|| unsupported("nested mono selection requires the default stereo master"))?;
    let uid = required(
        track.value.object_uid.as_deref(),
        &track.identity,
        "track UID",
    )?;
    let mut notes = Vec::new();
    ensure!(
        report_pan(
            graph,
            &track.value.audio_track.panner,
            &track.identity,
            &mut notes
        ) && track.value.audio_track.assign.is_none()
            && inlet.element().child("Sources").is_some_and(|sources| {
                sources.children().any(|source| {
                    source.tag() == "Source" && source.attribute("ObjectURef") == Some(uid)
                })
            }),
        "{}: nested mono selection requires verified centered stereo routing",
        track.identity
    );
    let mut gain = 1.0;
    for (owner, identity) in [
        (&master.value.audio_track.component_owner, &master.identity),
        (&track.value.audio_track.component_owner, &track.identity),
    ] {
        let chain = chain_gain(
            graph,
            owner.components.as_ref(),
            identity,
            ChainOwner::Other,
            &mut notes,
        )?;
        ensure!(
            !chain.unread && chain.keys.is_empty() && notes.is_empty(),
            "{identity}: nested mono selection requires static faders without omitted processing"
        );
        gain *= chain.gain;
    }
    if is_muted(&master.value.track, &master.identity)?
        || track
            .value
            .clip_track
            .track
            .as_ref()
            .map_or(Ok(false), |native| is_muted(native, &track.identity))?
    {
        gain = 0.0;
    }
    Ok(gain)
}

/// Validates the single-leaf route under the unmeasured assumption that saved
/// inner Solo/MutedBySolo state propagates through the nest.
struct MonoBusLeaf(PrAudioOccurrence);

impl MonoBusLeaf {
    fn read(
        graph: &Graph<'_>,
        guid: &str,
        media: &mut BTreeMap<MediaId, PrMedia>,
        omissions: &mut Vec<Omission>,
    ) -> Result<Self> {
        let sequence = graph.decode::<Sequence>(graph.locate_uid(guid, "nested mono sequence")?)?;
        let groups = required(
            sequence.value.track_groups,
            &sequence.identity,
            "TrackGroups",
        )?;
        let audio_groups: Vec<_> = groups
            .groups
            .iter()
            .map(|group| graph.locate(&group.target, &sequence.identity))
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|group| group.tag() == records::AUDIO_TRACK_GROUP.tag)
            .collect();
        let [group] = audio_groups.as_slice() else {
            return Err(unsupported(
                "nested mono selection requires one audio track group",
            ));
        };
        let group = graph.decode::<AudioTrackGroup>(*group)?;
        ensure!(
            group
                .value
                .num_adaptive_channels
                .as_deref()
                .is_none_or(|value| value == "2"),
            "{}: nested mono selection requires a two-channel bus",
            group.identity
        );
        let tracks = required(
            group.value.track_group.as_ref(),
            &group.identity,
            "TrackGroup",
        )?;
        let mut decoded = Vec::new();
        for reference in tracks.tracks.iter().flat_map(|tracks| &tracks.tracks) {
            let record = graph.locate(reference, &group.identity)?;
            let track = graph.decode_as::<AudioClipTrack>(record, &group.identity)?;
            let flag = |name: &str| -> Result<bool> {
                let audio = required(
                    record.element().child("AudioTrack"),
                    &track.identity,
                    "AudioTrack",
                )?;
                let mut fields = audio.children().filter(|field| field.tag() == name);
                let field = fields.next();
                ensure!(
                    fields.next().is_none(),
                    "{}: duplicate {name}",
                    track.identity
                );
                match field.and_then(graph::Element::text) {
                    None | Some("0") => Ok(false),
                    Some("1") => Ok(true),
                    _ => Err(unsupported(format!("{}: invalid {name}", track.identity))),
                }
            };
            let solo = flag("Solo")?;
            let muted_by_solo = flag("MutedBySolo")?;
            decoded.push((track, solo, muted_by_solo));
        }
        let master = graph.follow::<AudioMixTrack>(
            required(
                group.value.master_track.as_ref(),
                &group.identity,
                "stereo master",
            )?,
            &group.identity,
        )?;
        let inlet = centered_stereo_master(graph, &master).ok_or_else(|| {
            unsupported("nested mono selection requires the default stereo master")
        })?;
        let sources = required(
            inlet.element().child("Sources"),
            &group.identity,
            "master inlet sources",
        )?;
        let saved_sources: Vec<_> = sources
            .children()
            .map(|source| source.attribute("ObjectURef"))
            .collect();
        ensure!(
            saved_sources.len() == decoded.len()
                && sources.children().all(|source| source.tag() == "Source")
                && decoded.iter().filter_map(|(track, _, _)| track.value.object_uid.as_deref()).collect::<BTreeSet<_>>().len() == decoded.len()
                && decoded.iter().all(|(track, _, _)| {
                    track.value.object_uid.as_deref().is_some_and(|uid| {
                        saved_sources.iter().filter(|source| **source == Some(uid)).count() == 1
                    })
                }),
            "{guid}: nested mono selection requires exactly the saved tracks as master inlet sources"
        );
        let has_solo = decoded.iter().any(|(_, solo, _)| *solo);
        let mut leaf = None;
        for (track, solo, muted_by_solo) in decoded {
            ensure!(
                muted_by_solo == (has_solo && !solo),
                "{}: nested mono selection requires consistent saved solo routing",
                track.identity
            );
            let muted = track
                .value
                .clip_track
                .track
                .as_ref()
                .map_or(Ok(false), |native| is_muted(native, &track.identity))?;
            if muted || muted_by_solo {
                continue;
            }
            let items = track
                .value
                .clip_track
                .clip_items
                .as_ref()
                .and_then(|items| items.track_items.as_ref());
            let Some(items) = items.filter(|items| !items.items.is_empty()) else {
                continue;
            };
            ensure!(
                leaf.is_none() && items.items.len() == 1,
                "{guid}: nested mono selection requires exactly one audible direct mono leaf"
            );
            ensure!(
                track.value.clip_track.transition_items.as_ref().and_then(|items| items.track_items.as_ref()).is_none_or(|items| items.items.is_empty())
                    && graph::nested_sequence(graph, graph.locate(&items.items[0], &track.identity)?)?.is_none(),
                "{guid}: nested mono selection does not support leaf transitions or further nesting"
            );
            let native_item =
                graph.follow::<AudioClipTrackItem>(&items.items[0], &track.identity)?;
            let sub = graph.follow::<SubClip>(
                required(
                    native_item.value.clip_track_item.sub_clip.as_ref(),
                    &native_item.identity,
                    "SubClip",
                )?,
                &native_item.identity,
            )?;
            let native_clip = graph.follow::<AudioClip>(&sub.value.clip, &sub.identity)?;
            ensure!(
                AudioChannels::parse(&native_clip.value.audio_channel_layout)?
                    == AudioChannels::Mono,
                "{guid}: nested mono selection of a stereo leaf is not yet mapped (converter follow-up)"
            );
            let gain =
                centered_static_track_gain(graph, &track, group.value.master_track.as_ref())?;
            let mut notes = Vec::new();
            // The preceding check establishes the route; the direct reader
            // folds this leaf's centered factor once, not the parent's factor.
            let AudioItem::Media(clip) = read_occurrence(
                graph,
                &items.items[0],
                &track.identity,
                gain,
                true,
                media,
                &mut notes,
            )?
            else {
                return Err(unsupported(
                    "nested mono selection requires a direct media leaf",
                ));
            };
            ensure!(
                clip.uses_layer_clock(),
                "{guid}: nested mono selection requires a unit-forward pitch-OFF leaf without TimeRemapping"
            );
            ensure!(
                media[&clip.media].audio.as_ref().is_some_and(|stream| stream.channels == AudioChannels::Mono)
                    && clip.source_channel.is_none(),
                "{guid}: nested mono selection of a stereo or source-extraction leaf is not yet mapped (converter follow-up)"
            );
            ensure!(
                !clip.has_volume_animation()
                    && notes
                        .iter()
                        .all(|note| note.kind == OmissionKind::Approximated),
                "{guid}: nested mono selection requires static gain and no omitted processing"
            );
            for note in notes {
                crate::export_loss::OmissionSink::emit(omissions, note);
            }
            leaf = Some(clip);
        }
        leaf.map(Self)
            .ok_or_else(|| unsupported("nested mono selection has no audible mono leaf"))
    }
}

pub(super) fn read(
    graph: &Graph<'_>,
    sound: &NestSound,
    parent_track: &Located<AudioClipTrack>,
    parent_master: Option<&Reference>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    omissions: &mut Vec<Omission>,
) -> Result<PrAudioOccurrence> {
    // Parent gain was already folded by read_occurrence. Requiring the actual
    // route/faders here rules out that reader's diagnosed unity fallbacks.
    centered_static_track_gain(graph, parent_track, parent_master)?;
    let parent = graph.locate_uid(
        required(
            parent_track.value.object_uid.as_deref(),
            &parent_track.identity,
            "track UID",
        )?,
        &parent_track.identity,
    )?;
    let audio = required(
        parent.element().child("AudioTrack"),
        &parent_track.identity,
        "AudioTrack",
    )?;
    ensure!(
        audio
            .children()
            .filter(|field| matches!(field.tag(), "Solo" | "MutedBySolo"))
            .all(|field| field.text() == Some("0")),
        "{}: nested mono selection does not convert parent track solo routing",
        parent_track.identity
    );
    let MonoBusLeaf(mut leaf) = MonoBusLeaf::read(graph, &sound.sequence, media, omissions)?;
    let shift = sound
        .timeline
        .start
        .checked_sub(sound.source.start)
        .ok_or_else(|| unsupported("nested mono selection exceeds Premiere's tick range"))?;
    let (timeline, source) = nested::shown(
        leaf.start_ticks..leaf.end_ticks,
        leaf.in_ticks,
        &sound.source,
        shift,
    )?
    .ok_or_else(|| unsupported("nested mono selection has no leaf inside its source window"))?;
    ensure!(
        leaf.play_part(timeline, source)?.is_empty(),
        "nested mono selection unexpectedly dropped a leaf fade while trimming its source window"
    );
    nested::scale(&mut leaf, if sound.enabled { sound.gain } else { 0.0 })?;
    leaf.validate(
        media[&leaf.media]
            .audio
            .as_ref()
            .expect("validated mono leaf has audio"),
    )?;
    approximate(
        omissions, &sound.id,
        "nested mono channel 0 normalized to its single centered mono leaf; static gains and source window composed into an ordinary editable sound placement; saved inner Solo/MutedBySolo propagation through the nest is an unmeasured assumption",
    );
    Ok(leaf)
}
