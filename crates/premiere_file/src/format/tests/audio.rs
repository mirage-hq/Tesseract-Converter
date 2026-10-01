use crate::{
    format::{
        inspect_project, inspect_project_with_omissions, writer::project_xml, MediaId, PrMedia,
        PrProjectFile, PrSequence,
    },
    schema::{
        records::MediaPathField, AudioChannels, PrAudioOccurrence, PrAudioStream, PrKeyframeEasing,
        PrScalarKeyframe, PrVolumeKeys, TICKS, TICKS_PER_MILLISECOND,
    },
    tests::support::{premiere_linear_gain, video_media, video_sequence},
    Omission, OmissionKind, OmissionScope,
};
use fx_schema::LinearGain;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The pinned Adobe-derived case `premiere_isolated_audio_clips`.
fn audio_clips() -> PathBuf {
    fixture("feature_audio_clips_strict.prproj")
}

fn fixture_xml(name: &str) -> String {
    let mut xml = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(fixture(name)).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

fn audio_clips_xml() -> String {
    fixture_xml("feature_audio_clips_strict.prproj")
}

/// The pinned Adobe-derived linked picture-and-sound case.
fn linked_av_xml() -> String {
    fixture_xml("feature_linked_av_strict.prproj")
}

fn db(gain: f64) -> f64 {
    20.0 * gain.log10()
}

/// A key as (source seconds, Level dB, easing).
type DbKey = (f64, f64, PrKeyframeEasing);

/// A placement's keys, with the gain of its other stages.
fn keys_in_db(clip: &PrAudioOccurrence) -> Option<(Vec<DbKey>, f64)> {
    clip.volume_keys.as_ref().map(|keys| {
        (
            keys.keys
                .iter()
                .map(|key| {
                    let seconds = key.source_ticks as f64 / TICKS as f64;
                    (
                        (seconds * 1000.0).round() / 1000.0,
                        db(key.value),
                        key.easing,
                    )
                })
                .collect(),
            keys.gain,
        )
    })
}

fn assert_db(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-3 || actual == expected,
        "{actual} dB, expected {expected} dB"
    );
}

/// Inserts child elements at the start of one native record.
fn with_children(xml: &str, record: &str, children: &str) -> String {
    let start = xml.find(record).unwrap();
    let end = start + xml[start..].find('>').unwrap() + 1;
    format!("{}{children}{}", &xml[..end], &xml[end..])
}

fn volumes(xml: &str) -> Vec<f64> {
    let sequence = inspect_project(xml, None).unwrap();
    sequence
        .audio
        .iter()
        .map(|clip| clip.volume.as_f64())
        .collect()
}

#[test]
fn adobe_audio_placements_keep_ranges_layout_and_static_levels() {
    let (project, omissions) = PrProjectFile::load(audio_clips()).unwrap();
    // The base project reports only picture color settings, never audio content.
    assert!(
        omissions
            .iter()
            .all(|item| item.record == "VideoTrackGroup:80"),
        "{omissions:?}"
    );
    let sequence = &project.sequences[0];
    assert_eq!(sequence.video_occurrences().count(), 0);
    let placements: Vec<_> = sequence
        .audio
        .iter()
        .map(|clip| {
            (
                clip.start_ticks..clip.end_ticks,
                clip.in_ticks..clip.out_ticks,
                clip.volume.as_f64(),
            )
        })
        .collect();
    let quarter = TICKS / 4;
    assert_eq!(
        placements,
        [
            (0..5 * TICKS, 0..5 * TICKS, 1.0),
            (TICKS..5 * TICKS, quarter..4 * TICKS + quarter, 0.5),
        ]
    );
    for clip in &sequence.audio {
        let stream = project.media[&clip.media].audio.as_ref().unwrap();
        assert_eq!(
            (stream.channels, stream.sample_rate, stream.intrinsic_ticks),
            (AudioChannels::Stereo, 48_000, 5 * TICKS)
        );
    }

    // The master fader multiplies every track fader; its mute silences both.
    let master = with_children(
        &audio_clips_xml(),
        r#"<AudioComponentParam ObjectID="135""#,
        "<StartKeyframe>-91445760000000000,0.5,0,0,0,0,0,0</StartKeyframe>",
    );
    assert_eq!(volumes(&master), [0.5, 0.25]);
    let muted = with_children(
        &master,
        r#"<AudioComponentParam ObjectID="136""#,
        "<CurrentValue>1</CurrentValue>",
    );
    assert_eq!(volumes(&muted), [0.0, 0.0]);

    // The timeline mute of one track silences its placements; the master's silences all.
    let track_muted = audio_clips_xml().replacen(
        "<ID>3</ID>\n\t\t\t\t<MediaType>80b8e3d5-6dca-4195-aefb-cb5f407ab009</MediaType>",
        "<ID>3</ID><IsMuted>true</IsMuted><MediaType>80b8e3d5-6dca-4195-aefb-cb5f407ab009</MediaType>",
        1,
    );
    assert_ne!(track_muted, audio_clips_xml());
    assert_eq!(volumes(&track_muted), [1.0, 0.0]);
    let master_muted = audio_clips_xml().replacen(
        "<ID>1</ID>\n\t\t\t<MediaType>80b8e3d5-6dca-4195-aefb-cb5f407ab009</MediaType>",
        "<ID>1</ID><IsMuted>true</IsMuted><MediaType>80b8e3d5-6dca-4195-aefb-cb5f407ab009</MediaType>",
        1,
    );
    assert_ne!(master_muted, audio_clips_xml());
    assert_eq!(volumes(&master_muted), [0.0, 0.0]);
}

#[test]
fn audio_subclip_time_offset_omits_the_placement() {
    // Synthetic offset on the pinned audio case, not Adobe audio-subclip proof.
    let xml = audio_clips_xml();
    let subclip = r#"<SubClip ObjectRef="112"/>"#;
    assert_eq!(xml.matches(subclip).count(), 1);
    let changed = xml.replacen(
        subclip,
        &format!("{subclip}<OriginalSubClipTimeOffset>1</OriginalSubClipTimeOffset>"),
        1,
    );
    let (project, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
    let audio = &project.sequences[0].audio;
    assert_eq!(audio.len(), 1);
    assert_eq!(audio[0].in_ticks..audio[0].out_ticks, 0..5 * TICKS);
    assert!(
        omissions.iter().any(|item| {
            item.scope == crate::OmissionScope::Occurrence
                && item.record == "96"
                && item.reason.contains("nonzero OriginalSubClipTimeOffset")
        }),
        "{omissions:?}"
    );
}

#[test]
fn adobe_audio_omits_retimed_and_reversed_placements() {
    let xml = audio_clips_xml();
    // A2's placed clip. Premiere writes speed and direction inside its native Clip.
    let clip = r#"<AudioClip ObjectID="128""#;
    let start = xml.find(clip).unwrap();
    for field in [
        "<PlayBackwards>true</PlayBackwards>",
        "<PlaybackSpeed>2</PlaybackSpeed>",
        // Any existing record: the reader must not follow the remap.
        r#"<TimeRemapping ObjectRef="128"/>"#,
    ] {
        let retimed = format!(
            "{}{}",
            &xml[..start],
            xml[start..].replacen("<InPoint>", &format!("{field}<InPoint>"), 1)
        );
        let (project, omissions) = inspect_project_with_omissions(&retimed, None).unwrap();
        assert_eq!(project.sequences[0].audio.len(), 1, "{field}");
        assert!(
            omissions
                .iter()
                .any(|item| item.reason.contains("only unit, forward audio playback")),
            "{field}: {omissions:?}"
        );
    }
}

#[test]
fn adobe_audio_reports_automation_and_pan_but_omits_remapped_channels() {
    let xml = audio_clips_xml();
    let automated = with_children(
        &xml,
        r#"<AudioComponentParam ObjectID="126""#,
        "<Keyframes>0,1;1,0</Keyframes>",
    );
    let (project, omissions) = inspect_project_with_omissions(&automated, None).unwrap();
    assert_eq!(project.sequences[0].audio.len(), 2);
    assert!(omissions
        .iter()
        .any(|item| item.reason.contains("Volume automation")));

    let pan = r#"<AudioComponentParam ObjectID="110""#;
    let start = xml.find(pan).unwrap();
    let panned = format!(
        "{}{}",
        &xml[..start],
        xml[start..].replacen("0.5,0,0,0,0,0,0", "0.2,0,0,0,0,0,0", 1)
    );
    let (_, omissions) = inspect_project_with_omissions(&panned, None).unwrap();
    assert!(omissions.iter().any(|item| item.reason.contains("pan")));

    // Solo changes the mix of the other tracks; the placements stay at their levels.
    let soloed = xml.replacen(
        "<NextPannerID>4294967279</NextPannerID>\n\t\t</AudioTrack>",
        "<NextPannerID>4294967279</NextPannerID><Solo>1</Solo></AudioTrack>",
        1,
    );
    let (project, omissions) = inspect_project_with_omissions(&soloed, None).unwrap();
    assert_eq!(volumes(&soloed), [1.0, 0.5]);
    assert_eq!(project.sequences[0].audio.len(), 2);
    assert!(omissions
        .iter()
        .any(|item| item.reason == "audio solo not converted"));

    // A1 plays source channel 1 on its first clip channel.
    let channel = r#"<SecondaryContent ObjectID="124""#;
    let start = xml.find(channel).unwrap();
    let remapped = format!(
        "{}{}",
        &xml[..start],
        xml[start..].replacen(
            "<ChannelIndex>0</ChannelIndex>",
            "<ChannelIndex>1</ChannelIndex>",
            1
        )
    );
    let (project, omissions) = inspect_project_with_omissions(&remapped, None).unwrap();
    assert_eq!(project.sequences[0].audio.len(), 1);
    assert!(omissions
        .iter()
        .any(|item| item.reason.contains("channel remapping")));
}

/// The pinned audio case's timeline. A broken member link leaves nesting
/// unproven, so automatic selection would decline to guess.
const AUDIO_SEQUENCE: &str = "093e7f82-8e3b-4fd4-9a66-1234f931ffd8";
/// A1 of the pinned audio case places occurrence 87; A2 places 96; A3 places
/// nothing.
const A1: &str = "AudioClipTrack:9093e0e5-852c-4367-a90e-7199fbdd6c4a";
const A3: &str = "AudioClipTrack:7a7ec1a3-efa9-4be4-a105-f12aeb7bcc01";
const A1_PLACEMENT: &str = r#"<TrackItem Index="0" ObjectRef="87"/>"#;

/// Lists the transition `reference` on the pinned case's audio track `track`,
/// where Premiere lists them: first in the track's `TransitionItems`.
fn with_audio_transition(xml: &str, track: &str, reference: &str) -> String {
    let (_, uid) = track.split_once(':').unwrap();
    let start = xml.find(&format!(r#"ObjectUID="{uid}""#)).unwrap();
    format!(
        "{}{}",
        &xml[..start],
        xml[start..].replacen(
            r#"<TransitionItems Version="3">"#,
            &format!(
                r#"<TransitionItems Version="3"><TrackItems Version="1"><TrackItem Index="0" ObjectRef="{reference}"/></TrackItems>"#
            ),
            1,
        )
    )
}

/// The Constant Power crossfade that Premiere 25 saved as
/// `AudioTransitionTrackItem:1078` in the pinned authentic
/// `adobe_native_variable_speed_ramp_ppro25.prproj`.
fn adobe_audio_transition() -> String {
    let xml = fixture_xml("adobe_native_variable_speed_ramp_ppro25.prproj");
    let start = xml
        .find(r#"<AudioTransitionTrackItem ObjectID="1078""#)
        .unwrap();
    let end = "</AudioTransitionTrackItem>";
    let length = xml[start..].find(end).unwrap() + end.len();
    xml[start..start + length].to_owned()
}

/// The sound placements of an edited pinned audio case, selected explicitly,
/// and its omissions beyond the base project's picture color reports.
fn audio_placements_and_omissions(xml: &str) -> (Vec<String>, Vec<Omission>) {
    let (project, omissions) = inspect_project_with_omissions(xml, Some(AUDIO_SEQUENCE)).unwrap();
    let placements = project.sequences[0]
        .audio
        .iter()
        .map(|clip| clip.id.clone().unwrap_or_default())
        .collect();
    let omissions = omissions
        .into_iter()
        .filter(|item| item.record != "VideoTrackGroup:80")
        .collect();
    (placements, omissions)
}

fn omitted(scope: OmissionScope, record: impl Into<String>, reason: impl Into<String>) -> Omission {
    Omission {
        scope,
        kind: OmissionKind::Omitted,
        record: record.into(),
        reason: reason.into(),
    }
}

#[test]
fn broken_audio_membership_links_do_not_discard_their_track_or_group() {
    // A broken or ambiguous placement link omits only its placement, and a
    // broken track link only its track. Both forms of the ambiguous link name
    // existing records, so only the ambiguity fails.
    let ambiguous = r#"<TrackItem Index="1" ObjectRef="93" ObjectURef="a93a2a56-139c-4766-9d3c-9490881259a2"/>"#;
    let last_track = r#"<Track Index="3" ObjectURef="59171a02-3893-4107-a2d7-c330edbeb587"/>"#;
    let xml = audio_clips_xml();
    for (edited, expected) in [
        (
            xml.replacen(
                A1_PLACEMENT,
                &format!(r#"{A1_PLACEMENT}<TrackItem Index="1" ObjectRef="999"/>"#),
                1,
            ),
            omitted(
                OmissionScope::Occurrence,
                "999",
                format!("invalid Premiere project: missing reference at {A1}"),
            ),
        ),
        (
            xml.replacen(A1_PLACEMENT, &format!("{A1_PLACEMENT}{ambiguous}"), 1),
            omitted(
                OmissionScope::Occurrence,
                "93",
                format!("invalid Premiere project: expected exactly one reference at {A1}"),
            ),
        ),
        (
            xml.replacen(
                last_track,
                &format!(r#"{last_track}<Track Index="4" ObjectURef="missing-audio-track"/>"#),
                1,
            ),
            omitted(
                OmissionScope::Track,
                "AudioTrackGroup:81",
                "invalid Premiere project: missing reference at AudioTrackGroup:81",
            ),
        ),
    ] {
        let (placements, omissions) = audio_placements_and_omissions(&edited);
        assert_eq!(
            placements,
            ["AudioClipTrackItem:87", "AudioClipTrackItem:96"],
            "{expected:?}"
        );
        assert_eq!(omissions, [expected]);
    }
}

#[test]
fn audio_links_outside_member_lists_still_reject_their_record() {
    // A1's panner and transition items are not its clip items, and the reader
    // never resolves audio transitions. A broken link there still omits A1,
    // but not A2's placement.
    let xml = audio_clips_xml();
    let transition = with_audio_transition(&xml, A1, "999");
    let panner = xml.replacen(
        r#"<Panner ObjectRef="95"/>"#,
        r#"<Panner ObjectRef="999"/>"#,
        1,
    );
    for (edited, link) in [(transition, "TrackItem"), (panner, "Panner")] {
        let (placements, omissions) = audio_placements_and_omissions(&edited);
        assert_eq!(placements, ["AudioClipTrackItem:96"], "{link}");
        assert_eq!(
            omissions,
            [omitted(
                OmissionScope::Track,
                A1,
                format!("invalid Premiere project: missing reference at {link}"),
            )]
        );
    }

    // Nor is the master track a member: a broken link still omits the whole
    // group, and the linked case keeps only its picture.
    let linked = linked_av_xml().replacen(
        r#"<MasterTrack ObjectRef="124"/>"#,
        r#"<MasterTrack ObjectRef="999"/>"#,
        1,
    );
    let (project, omissions) =
        inspect_project_with_omissions(&linked, Some("80acdd81-0a96-4677-b17f-b2ffe2dff738"))
            .unwrap();
    let sequence = &project.sequences[0];
    assert_eq!(
        (sequence.video_occurrences().count(), sequence.audio.len()),
        (1, 0)
    );
    assert!(
        omissions.contains(&omitted(
            OmissionScope::Feature,
            "AudioTrackGroup:115",
            "audio not converted: invalid Premiere project: missing reference at MasterTrack",
        )),
        "{omissions:?}"
    );
}

/// `xml` with each `(from, to)` edit made at its one occurrence and `records`
/// appended.
fn edited(xml: &str, edits: &[(&str, &str)], records: &str) -> String {
    let mut xml = xml.to_owned();
    for (from, to) in edits {
        assert_eq!(xml.matches(from).count(), 1, "{from}");
        xml = xml.replacen(from, to, 1);
    }
    xml.replacen("</PremiereData>", &format!("{records}</PremiereData>"), 1)
}

/// Master clip records of a real Premiere 26.3 project (SHA-256
/// 3f47fe84ac2c285da95da40aa53189400f963a5fd87e68be57e26ece47d93174): its
/// TranscriptClip, without the data source and transcript document that the
/// reader never follows, and the Source Monitor properties of its music master
/// clip, whitespace removed.
const REAL_TRANSCRIPT_CLIP: &str = r#"<TranscriptClip ObjectID="208" ClassID="9e0179bb-153c-4884-b34b-eb7082f34384" Version="2"><DataClip Version="1"><Clip Version="18"/></DataClip></TranscriptClip>"#;
const REAL_MUSIC_MONITOR: &str = concat!(
    r#"<Properties Version="1">"#,
    "<AMM.CurrentSolo>[]</AMM.CurrentSolo>",
    "<monitor.edit.time>34224744153600</monitor.edit.time>",
    "<monitor.zoom.in.time>0</monitor.zoom.in.time>",
    "<monitor.zoom.out.time>56720396880000</monitor.zoom.out.time>",
    "<monitor.take.video>false</monitor.take.video>",
    "<monitor.take.audio>true</monitor.take.audio>",
    "<monitor.show.audio.waveform>true</monitor.show.audio.waveform>",
    "</Properties>",
);
const LINKED_SEQUENCE: &str = "80acdd81-0a96-4677-b17f-b2ffe2dff738";
/// The linked case's master clip lists `record` (ObjectID 208) after its
/// VideoClip and AudioClip, where the real project's video masters list their
/// TranscriptClip.
fn linked_master_with_third_clip(record: &str) -> String {
    edited(
        &linked_av_xml(),
        &[(
            r#"<Clip Index="1" ObjectRef="59"/>"#,
            r#"<Clip Index="1" ObjectRef="59"/><Clip Index="2" ObjectRef="208"/>"#,
        )],
        record,
    )
}

#[test]
fn transcribed_master_clips_keep_their_picture_and_sound() {
    // The linked case's master clip transcribed as the real video masters are;
    // the audio case's first master clip transcribed, with the Source Monitor
    // state, as the real music master clip is.
    let music_master = edited(
        &audio_clips_xml(),
        &[
            (
                "<ID>1000001</ID>",
                &format!("{REAL_MUSIC_MONITOR}<ID>1000001</ID>"),
            ),
            (
                r#"<Clip Index="0" ObjectRef="50"/>"#,
                r#"<Clip Index="0" ObjectRef="50"/><Clip Index="1" ObjectRef="208"/>"#,
            ),
        ],
        REAL_TRANSCRIPT_CLIP,
    );
    for (xml, selection, source) in [
        (
            linked_master_with_third_clip(REAL_TRANSCRIPT_CLIP),
            LINKED_SEQUENCE,
            linked_av_xml(),
        ),
        (music_master, AUDIO_SEQUENCE, audio_clips_xml()),
    ] {
        let (expected, expected_omissions) =
            inspect_project_with_omissions(&source, Some(selection)).unwrap();
        let (project, omissions) = inspect_project_with_omissions(&xml, Some(selection)).unwrap();
        assert_eq!(
            format!("{project:?}"),
            format!("{expected:?}"),
            "{selection}"
        );
        assert_eq!(omissions, expected_omissions, "{selection}");
    }

    // A second VideoClip is an ambiguous picture source, and any other record
    // may draw: either omits the picture placement, not its sound.
    for (record, reason) in [
        (
            r#"<VideoClip ObjectID="208"><Clip><Source ObjectRef="84"/></Clip></VideoClip>"#,
            "multiple source clips unsupported",
        ),
        (
            r#"<RemixClip ObjectID="208"/>"#,
            "RemixClip source clip unsupported",
        ),
    ] {
        let (project, omissions) = inspect_project_with_omissions(
            &linked_master_with_third_clip(record),
            Some(LINKED_SEQUENCE),
        )
        .unwrap();
        let sequence = &project.sequences[0];
        assert_eq!(
            (sequence.video_occurrences().count(), sequence.audio.len()),
            (0, 1),
            "{reason}"
        );
        assert!(
            omissions.contains(&omitted(
                OmissionScope::Occurrence,
                "121",
                format!(
                    "unsupported conversion: MasterClip:6fed5564-291d-4b94-8c5a-dbecb76dbaa4: {reason}"
                ),
            )),
            "{omissions:?}"
        );
    }
}

#[test]
fn audio_transitions_are_reported_without_losing_placements() {
    // The crossfade record is Adobe-saved; listing it on the pinned case's
    // tracks is synthetic. No audio transition converts, whether or not its
    // track places sound, and the placements keep their static levels.
    let crossfade = adobe_audio_transition();
    for track in [A1, A3] {
        let xml = with_audio_transition(&audio_clips_xml(), track, "1078").replacen(
            "</PremiereData>",
            &format!("{crossfade}</PremiereData>"),
            1,
        );
        let (placements, omissions) = audio_placements_and_omissions(&xml);
        assert_eq!(
            placements,
            ["AudioClipTrackItem:87", "AudioClipTrackItem:96"],
            "{track}"
        );
        assert_eq!(volumes(&xml), [1.0, 0.5], "{track}");
        assert_eq!(
            omissions,
            [omitted(
                OmissionScope::Feature,
                track,
                "audio transitions not converted",
            )]
        );
    }
}

fn audio_project(occurrences: Vec<(&str, AudioChannels, f64)>) -> PrProjectFile {
    let mut sequence = video_sequence();
    let mut media = video_media();
    for (name, channels, gain) in occurrences {
        let id = MediaId(name.to_owned());
        let path = std::path::PathBuf::from("/tmp/media").join(name);
        media.entry(id.clone()).or_insert_with(|| PrMedia {
            name: name.to_owned(),
            relative_path: Some(format!("./media/{name}")),
            relative_paths: vec![format!("./media/{name}")],
            absolute_paths: vec![(MediaPathField::FilePath, path)],
            video: None,
            audio: Some(PrAudioStream {
                intrinsic_ticks: 8 * TICKS,
                channels,
                sample_rate: 44_100,
            }),
        });
        sequence.audio.push(PrAudioOccurrence {
            id: None,
            media: id,
            start_ticks: TICKS,
            end_ticks: 4 * TICKS,
            in_ticks: 2 * TICKS,
            out_ticks: 5 * TICKS,
            volume: LinearGain::new(gain).unwrap(),
            volume_keys: None,
        });
    }
    let video = media.get_mut(&MediaId("source".into())).unwrap();
    video.video.as_mut().unwrap().kind = crate::schema::PrMediaKind::Video {
        codec: Some(crate::schema::VideoCodec::H264),
        hdr_profile: None,
    };
    video.relative_path = Some("./media/source.mp4".into());
    video.relative_paths = vec!["./media/source.mp4".into()];
    video.absolute_paths = vec![(MediaPathField::FilePath, "/tmp/media/source.mp4".into())];
    PrProjectFile::from_sequences(vec![sequence], media)
}

#[test]
fn written_audio_placements_read_back_with_levels_and_layouts() {
    let project = audio_project(vec![
        ("music.mp3", AudioChannels::Stereo, 0.5),
        ("voice.wav", AudioChannels::Mono, 1.0),
        ("music.mp3", AudioChannels::Stereo, 2.0),
        ("voice.wav", AudioChannels::Mono, 0.0),
        ("voice.wav", AudioChannels::Mono, 1.0),
    ]);
    let xml = project_xml(&project).unwrap();
    // Five overlapping placements need five tracks, one more than the scaffold's four.
    assert_eq!(xml.matches("<AudioClipTrack ").count(), 5);
    assert_eq!(xml.matches("<AudioClipTrackItem ").count(), 5);
    assert!(xml.contains(r#"<AudioChannelLayout>[{"channellabel":0}]</AudioChannelLayout>"#));

    let (loaded, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let PrSequence { audio, .. } = &loaded.sequences[0];
    let expected = &project.sequences[0].audio;
    assert_eq!(audio.len(), expected.len());
    for (actual, expected) in audio.iter().zip(expected) {
        assert_eq!(
            (
                actual.start_ticks,
                actual.end_ticks,
                actual.in_ticks,
                actual.out_ticks
            ),
            (
                expected.start_ticks,
                expected.end_ticks,
                expected.in_ticks,
                expected.out_ticks
            )
        );
        assert_eq!(actual.volume, expected.volume);
        assert_eq!(
            loaded.media[&actual.media].audio,
            project.media[&expected.media].audio
        );
    }
    assert_eq!(loaded.sequences[0].video_occurrences().count(), 1);
}

#[test]
fn writer_admits_sound_containers_only_for_sound_only_media() {
    let name = "voice.wav";
    project_xml(&audio_project(vec![(name, AudioChannels::Stereo, 1.0)])).unwrap();
    // The same container cannot hold a video record.
    let mut project = audio_project(Vec::new());
    let video = project.media.get_mut(&MediaId("source".into())).unwrap();
    video.name = name.to_owned();
    video.relative_path = Some(format!("./media/{name}"));
    video.relative_paths = vec![format!("./media/{name}")];
    video.absolute_paths = vec![(MediaPathField::FilePath, Path::new("/tmp/media").join(name))];
    let error = project_xml(&project).unwrap_err().to_string();
    assert!(
        error.contains("writer supports MP4/MOV video and WAV/MP3/M4A audio"),
        "{error}"
    );
}

#[test]
fn adobe_clip_volume_levels_fold_with_clip_gain_and_faders() {
    // Premiere 26.5.1 writes each changed clip Volume as `[Mute, Level]` with
    // 0 dB at 10^(-15/20) and +15 dB as a Level without a value; Clip Gain and
    // the A2 fader multiply it (AME render of this fixture). Its mono chains
    // carry no AudioChannelLayout.
    let (project, omissions) =
        PrProjectFile::load(fixture("feature_audio_volume_levels_strict.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let expected = [
        0.0,
        f64::NEG_INFINITY,
        -10.0,
        6.0,
        15.0,
        8.9794,
        // Level +6 dB with Clip Gain -8 dB, then Clip Gain +6 dB alone.
        -2.0,
        6.0,
        // Effective mono gain includes the measured centered-stereo mix.
        -3.010_299_956_639_812,
        2.989_700_043_360_188,
        // A2: Level -10 dB under a 0.5 track fader.
        -16.0206,
    ];
    let audio = &project.sequences[0].audio;
    assert_eq!(audio.len(), expected.len());
    for (clip, expected) in audio.iter().zip(expected) {
        assert_db(db(clip.volume.as_f64()), expected);
        assert!(clip.volume_keys.is_none());
    }

    // A master fader multiplies every clip Volume too (synthetic 0.5).
    let master = with_children(
        &fixture_xml("feature_audio_volume_levels_strict.prproj"),
        r#"<AudioComponentParam ObjectID="171""#,
        "<StartKeyframe>-91445760000000000,0.5,0,0,0,0,0,0</StartKeyframe>",
    );
    for (volume, expected) in volumes(&master).into_iter().zip(expected) {
        assert_db(db(volume), expected + db(0.5));
    }
}

/// Edit one native record without changing matching sibling parameters.
fn edit_audio_record(xml: &str, id: &str, edit: impl FnOnce(&str) -> String) -> String {
    let document = roxmltree::Document::parse(xml).unwrap();
    let record = document
        .root_element()
        .children()
        .find(|node| node.attribute("ObjectID") == Some(id))
        .unwrap();
    let before = &xml[record.range()];
    let after = edit(before);
    assert_ne!(before, after, "record {id}");
    let mut result = xml.to_owned();
    result.replace_range(record.range(), &after);
    result
}

#[test]
fn mono_mix_normalization_requires_the_measured_static_center_and_route() {
    let source = fixture_xml("feature_audio_volume_levels_strict.prproj");
    let changes = [
        (
            "90",
            "unknown panner",
            "StereoToStereoPanProcessor",
            "OwnPanProcessor",
        ),
        (
            "123",
            "noncenter",
            "-91445760000000000,0.5,",
            "-91445760000000000,0.25,",
        ),
        (
            "123",
            "automated",
            "</AudioComponentParam>",
            "<Keyframes>0,0.5,0,0,0,0,0,0;</Keyframes></AudioComponentParam>",
        ),
        (
            "123",
            "time varying",
            "</AudioComponentParam>",
            "<IsTimeVarying>true</IsTimeVarying></AudioComponentParam>",
        ),
        (
            "90",
            "channel type",
            "<ChannelType>1</ChannelType>",
            "<ChannelType>0</ChannelType>",
        ),
        (
            "99",
            "master route",
            "<DefaultPannerOutputChannelType>1</DefaultPannerOutputChannelType>",
            "<DefaultPannerOutputChannelType>0</DefaultPannerOutputChannelType>",
        ),
        (
            "100",
            "inlet layout",
            r#"[{"channellabel":100},{"channellabel":101}]"#,
            r#"[{"channellabel":0}]"#,
        ),
    ];
    let expected = volumes(&source);
    for (id, case, before, after) in changes {
        let xml = edit_audio_record(&source, id, |record| record.replace(before, after));
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let audio = &project.sequences[0].audio;
        assert_eq!(audio.len(), expected.len(), "{case}");
        for (index, clip) in audio.iter().enumerate() {
            let gain = if matches!(index, 8 | 9) {
                expected[index] / std::f64::consts::FRAC_1_SQRT_2
            } else {
                expected[index]
            };
            assert!(
                (clip.volume.as_f64() - gain).abs() < 1e-12,
                "{case}: {index}"
            );
        }
        assert!(
            omissions.iter().any(|o| o
                .reason
                .contains("mono centered-stereo gain not normalized")),
            "{case}: {omissions:#?}"
        );
    }
}

#[test]
fn mono_mix_requires_a_track_uid_in_the_stereo_inlet() {
    let source = fixture_xml("feature_audio_volume_levels_strict.prproj");
    let uid = "bffa7acc-d6aa-49db-9b09-a340ba5515c5";
    let xml = source
        .replace(&format!("ObjectUID=\"{uid}\""), "ObjectID=\"900\"")
        .replace(&format!("ObjectURef=\"{uid}\""), "ObjectRef=\"900\"");
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert_db(db(project.sequences[0].audio[8].volume.as_f64()), 0.0);
    assert_db(db(project.sequences[0].audio[9].volume.as_f64()), 6.0);
    assert!(
        omissions.iter().any(|o| o
            .reason
            .contains("mono centered-stereo gain not normalized")),
        "{omissions:#?}"
    );
}

#[test]
fn mono_static_and_keyed_effective_gain_write_reciprocal_native_levels() {
    let factor = std::f64::consts::FRAC_1_SQRT_2;
    for (channels, effective, native) in [
        (AudioChannels::Mono, factor, 1.0),
        (AudioChannels::Mono, 1.0, 1.0 / factor),
        (AudioChannels::Stereo, 1.0, 1.0),
    ] {
        let mut project = audio_project(vec![("voice.wav", channels, effective)]);
        let xml = project_xml(&project).unwrap();
        let loaded = inspect_project(&xml, None).unwrap();
        assert!((loaded.audio[0].volume.as_f64() - effective).abs() < 1e-12);
        assert_eq!(
            xml.contains("<FilterMatchName>Internal Volume Mono</FilterMatchName>"),
            channels == AudioChannels::Mono && effective != factor
        );
        // Read through an unmeasured noncenter route to expose the serialized
        // native clip gain independently of the reader's mix normalization.
        let xml = xml.replace(
            "<Name>Balance</Name>",
            "<Name>Balance</Name><Keyframes>0,0.5,0,0,0,0,0,0;</Keyframes>",
        );
        assert!((volumes(&xml)[0] - native).abs() < 1e-12);
        project.sequences[0].audio[0].volume_keys = Some(PrVolumeKeys {
            gain: 2.0,
            keys: [(TICKS, 0.1), (2 * TICKS, 4.0)]
                .into_iter()
                .map(|(source_ticks, value)| PrScalarKeyframe {
                    source_ticks,
                    value,
                    easing: PrKeyframeEasing::Linear,
                })
                .collect(),
        });
        let xml = project_xml(&project).unwrap();
        let loaded = inspect_project(&xml, None).unwrap();
        for (actual, expected) in effective_keys(&loaded.audio[0])
            .iter()
            .zip(effective_keys(&project.sequences[0].audio[0]))
        {
            assert_eq!((actual.0, actual.2), (expected.0, expected.2));
            assert!((actual.1 - expected.1).abs() < 1e-12);
        }
    }
}

#[test]
fn adobe_clip_volume_keys_stay_on_the_source_clock() {
    // Linear and Hold Level keys as Premiere 26.5.1 saved them: the sound of a
    // linked A/V clip, two overlapping beds, and a placement with In 1.0 s
    // whose keys lie before its In and after its Out.
    use PrKeyframeEasing::{Hold, Linear};
    let (project, omissions) =
        PrProjectFile::load(fixture("feature_audio_volume_keys_strict.prproj")).unwrap();
    assert!(
        omissions
            .iter()
            .all(|item| !item.reason.contains("audio") && !item.reason.contains("Volume")),
        "{omissions:?}"
    );
    let expected = [
        vec![(1.75, 0.0, Linear), (2.75, -12.0, Linear)],
        vec![
            (0.75, 0.0, Linear),
            (1.75, -20.0, Linear),
            (2.25, -20.0, Linear),
            (2.33, 6.0, Linear),
            (3.2, 6.0, Linear),
            (3.3, -10.0, Hold),
        ],
        vec![(0.5, 0.0, Linear), (3.5, -20.0, Linear)],
        vec![(0.5, f64::NEG_INFINITY, Linear), (4.0, 6.0, Linear)],
    ];
    let audio = &project.sequences[0].audio;
    assert_eq!(audio.len(), expected.len());
    for (clip, expected) in audio.iter().zip(expected) {
        // The value before the keys, 0 dB, stays the static volume.
        assert_eq!(clip.volume, LinearGain::UNITY);
        let (keys, gain) = keys_in_db(clip).unwrap();
        assert_eq!(gain, 1.0);
        assert_eq!(keys.len(), expected.len());
        for ((time, level, easing), (expected_time, expected_level, expected_easing)) in
            keys.into_iter().zip(expected)
        {
            assert_eq!((time, easing), (expected_time, expected_easing));
            assert_db(level, expected_level);
        }
    }
    assert_eq!(audio[3].in_ticks, TICKS);
}

#[test]
fn legacy_clip_volume_reads_with_its_own_scale() {
    // An XML derivative of a Premiere 10.4 save (project version 31): its
    // `[Bypass, Level]` Volume plays 0 dB at 0.5 and a Level without a value
    // at +6.02 dB (AME render of this fixture), and the project wraps every
    // AudioChannelLayout array.
    use PrKeyframeEasing::Linear;
    let (project, omissions) =
        PrProjectFile::load(fixture("feature_audio_volume_legacy_strict.prproj")).unwrap();
    let expected = [
        0.0,
        0.0,
        f64::NEG_INFINITY,
        -30.0,
        -20.0,
        -6.0206,
        3.8017,
        6.0206,
        6.0206,
        // Level 0 dB; its Channel Volume Right 0.25 is omitted.
        0.0,
        0.0,
    ];
    let audio = &project.sequences[0].audio;
    assert_eq!(audio.len(), expected.len());
    for (clip, expected) in audio.iter().zip(expected) {
        assert_db(db(clip.volume.as_f64()), expected);
        let stream = project.media[&clip.media].audio.as_ref().unwrap();
        assert_eq!(stream.channels, AudioChannels::Stereo);
    }
    let (keys, gain) = keys_in_db(&audio[10]).unwrap();
    assert_eq!(gain, 1.0);
    assert_eq!(audio[10].in_ticks, 507_269_952_000);
    assert_eq!(
        keys.iter()
            .map(|(time, _, easing)| (*time, *easing))
            .collect::<Vec<_>>(),
        [(2.75, Linear), (3.75, Linear)]
    );
    assert_db(keys[0].1, 0.0);
    assert_db(keys[1].1, -20.0);
    let audio_omissions: Vec<_> = omissions
        .iter()
        .filter(|item| item.record.starts_with("Audio"))
        .map(|item| (item.scope, item.record.as_str(), item.reason.as_str()))
        .collect();
    assert_eq!(
        audio_omissions,
        [(
            OmissionScope::Feature,
            "AudioFilterComponent:1583",
            "clip Channel Volume not converted"
        )]
    );
}

#[test]
fn unsupported_clip_volume_variants_fail_closed() {
    // The pinned Premiere 26.5.1 case `premiere_isolated_audio_volume_keys`.
    let xml = fixture_xml("feature_audio_volume_keys_strict.prproj");
    let placement = |xml: &str| {
        let (project, omissions) = inspect_project_with_omissions(xml, None).unwrap();
        (project.sequences[0].audio[0].clone(), omissions)
    };
    // The linked A/V sound: Volume 118 (Mute 141, Level 142) and Channel Volume 119.
    let first_key = "444528000000,0.177827939391,0,";
    assert_eq!(xml.matches(first_key).count(), 1);

    // Bezier Level keys play as main does: no Volume and no partial keys. So
    // does a Volume whose Level record is missing, still named the clip Volume.
    for (edited, report) in [
        (
            xml.replacen(first_key, "444528000000,0.177827939391,5,", 1),
            "Bezier Volume keyframes are not converted",
        ),
        (
            xml.replacen(r#"ObjectRef="142""#, r#"ObjectRef="9142""#, 1),
            "missing reference at Param",
        ),
    ] {
        let (clip, omissions) = placement(&edited);
        assert_eq!(clip.volume, LinearGain::UNITY);
        assert!(clip.volume_keys.is_none());
        assert!(
            omissions
                .iter()
                .any(|item| item.scope == OmissionScope::Feature
                    && item.record == "AudioFilterComponent:118"
                    && item.reason.starts_with("clip Volume not converted: ")
                    && item.reason.contains(report)),
            "{omissions:?}"
        );
    }

    // Keys that a disabled IsTimeVarying hides are not activated.
    let disabled = with_children(
        &xml,
        r#"<AudioComponentParam ObjectID="142""#,
        "<IsTimeVarying>false</IsTimeVarying>",
    );
    let (clip, omissions) = placement(&disabled);
    assert!(clip.volume_keys.is_none());
    assert!(omissions.iter().any(|item| item
        .reason
        .contains("Level keyframes and IsTimeVarying disagree")));

    // Keyed Mute keeps its static value; the Level keys still convert.
    let muted = with_children(
        &xml,
        r#"<AudioComponentParam ObjectID="141""#,
        "<Keyframes>0,1,4,0,0,0,0,0;</Keyframes>",
    );
    let (clip, omissions) = placement(&muted);
    assert_eq!(clip.volume_keys.unwrap().keys.len(), 2);
    assert!(omissions
        .iter()
        .any(|item| item.reason == "Mute automation not converted; static value used"));

    // A Channel Volume away from unity is named; the Level keys still convert.
    let left = r#"<AudioComponentParam ObjectID="144""#;
    let start = xml.find(left).unwrap();
    let panned = format!(
        "{}{}",
        &xml[..start],
        xml[start..].replacen("0.177827939391,0,0", "0.0889,0,0", 1)
    );
    let (clip, omissions) = placement(&panned);
    assert_eq!(clip.volume_keys.unwrap().keys.len(), 2);
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "AudioFilterComponent:119"
                && item.reason == "clip Channel Volume not converted"),
        "{omissions:?}"
    );
}

#[test]
fn written_clip_volume_reads_back_with_levels_keys_and_clip_gain() {
    use PrKeyframeEasing::{Hold, Linear};
    let mut project = audio_project(vec![
        ("music.mp3", AudioChannels::Stereo, 1.0),
        ("voice.wav", AudioChannels::Mono, 1.0),
        ("music.mp3", AudioChannels::Stereo, 10.0),
        ("voice.wav", AudioChannels::Mono, 0.5),
    ]);
    // Keys before the In point and past +15 dB share one Clip Gain.
    let key = |seconds: f64, value: f64, easing| PrScalarKeyframe {
        source_ticks: (seconds * TICKS as f64) as i64,
        value,
        easing,
    };
    project.sequences[0].audio[0].volume_keys = Some(PrVolumeKeys {
        keys: vec![
            key(1.5, 1.0, Linear),
            key(2.5, 0.1, Linear),
            key(3.5, 8.0, Linear),
            key(4.5, 0.25, Hold),
        ],
        gain: 1.0,
    });
    let xml = project_xml(&project).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    // Every fader stays at unity: levels live in the clip Volume.
    let params: Vec<_> = document
        .descendants()
        .filter(|node| node.has_tag_name("AudioComponentParam"))
        .collect();
    let named = |name: &str| {
        params
            .iter()
            .filter(|node| {
                node.children()
                    .any(|child| child.has_tag_name("Name") && child.text() == Some(name))
            })
            .collect::<Vec<_>>()
    };
    let faders = named("Volume");
    assert_eq!(faders.len(), 5);
    assert!(faders.iter().all(|param| !param
        .children()
        .any(|child| child.has_tag_name("StartKeyframe"))));
    // Native centered unity keeps the default chain; FX mono unity needs a Level.
    // Stereo Volumes also add a Channel Volume.
    assert_eq!(named("Level").len(), 4);
    assert_eq!(xml.matches("<FilterMatchName>").count(), 6);
    assert_eq!(named("Left").len(), 2);
    assert_eq!(xml.matches("<Gain>").count(), 2);

    let (loaded, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let expected = &project.sequences[0].audio;
    let actual = &loaded.sequences[0].audio;
    assert_eq!(actual.len(), expected.len());
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs().max(1.0);
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(close(actual.volume.as_f64(), expected.volume.as_f64()));
        let (actual, expected) = (effective_keys(actual), effective_keys(expected));
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert_eq!((actual.0, actual.2), (expected.0, expected.2));
            assert!(close(actual.1, expected.1), "{actual:?} {expected:?}");
        }
    }

    // A clip Volume track beyond the former count cap survives reimport.
    let keys = &mut project.sequences[0].audio[0]
        .volume_keys
        .as_mut()
        .unwrap()
        .keys;
    let last = keys[3].clone();
    keys.extend((1..=4097 - 4).map(|index| PrScalarKeyframe {
        source_ticks: last.source_ticks + index as i64 * TICKS_PER_MILLISECOND,
        ..last.clone()
    }));
    let xml = project_xml(&project).unwrap();
    let loaded = inspect_project(&xml, None).unwrap();
    assert_eq!(
        loaded.audio[0].volume_keys.as_ref().unwrap().keys.len(),
        4097
    );
}

/// Keys as (source ticks, gain of every stage, easing).
fn effective_keys(clip: &PrAudioOccurrence) -> Vec<(i64, f64, PrKeyframeEasing)> {
    clip.volume_keys
        .iter()
        .flat_map(|keys| {
            keys.keys
                .iter()
                .map(|key| (key.source_ticks, key.value * keys.gain, key.easing))
        })
        .collect()
}

/// Writes one keyed segment as export does (its gains in the Level, no other
/// stage) and reads it back. Returns the largest dB difference between the
/// curve Premiere plays for the written Level and Clip Gain and the native
/// segment (Level gains `levels` under Clip Gain `clip_gain`), with the Level
/// keys and Clip Gain that were written.
fn exported_segment_error(levels: [f64; 2], clip_gain: f64) -> (f64, [f64; 2], f64) {
    let mut project = audio_project(vec![("music.mp3", AudioChannels::Stereo, 1.0)]);
    let clip = &mut project.sequences[0].audio[0];
    clip.volume = LinearGain::new(levels[0] * clip_gain).unwrap();
    clip.volume_keys = Some(PrVolumeKeys {
        keys: [(2, levels[0]), (4, levels[1])]
            .into_iter()
            .map(|(seconds, level)| PrScalarKeyframe {
                source_ticks: seconds * TICKS,
                value: level * clip_gain,
                easing: PrKeyframeEasing::Linear,
            })
            .collect(),
        gain: 1.0,
    });
    let xml = project_xml(&project).unwrap();
    let loaded = inspect_project(&xml, None).unwrap();
    let keys = loaded.audio[0].volume_keys.clone().unwrap();
    let written = [keys.keys[0].value, keys.keys[1].value];
    let error = (1..1000)
        .map(|index| f64::from(index) / 1000.0)
        .filter_map(|t| {
            let native = clip_gain * premiere_linear_gain(levels[0], levels[1], t);
            let played = keys.gain * premiere_linear_gain(written[0], written[1], t);
            (native >= 1e-3).then(|| db(played / native).abs())
        })
        .fold(0.0, f64::max);
    (error, written, keys.gain)
}

#[test]
fn keyed_export_moves_a_boost_into_clip_gain_without_changing_the_curve() {
    let gain = |db: f64| 10_f64.powf(db / 20.0);
    // 0 to -20 dB under +20 dB Clip Gain: the native split comes back exactly.
    let (error, written, clip_gain) = exported_segment_error([1.0, gain(-20.0)], gain(20.0));
    assert!(error < 1e-9, "{error}");
    assert!((written[0] - 1.0).abs() < 1e-12 && (written[1] - gain(-20.0)).abs() < 1e-12);
    assert!((clip_gain - gain(20.0)).abs() < 1e-9);
    // -6 to -20 dB under +20 dB: the peak moves into Clip Gain, so Premiere
    // shows another split, but every Level stays at or below 0 dB and the
    // curve is unchanged.
    let (error, written, clip_gain) = exported_segment_error([gain(-6.0), gain(-20.0)], gain(20.0));
    assert!(error < 1e-9, "{error}");
    assert!((written[0] - 1.0).abs() < 1e-12);
    assert!((db(clip_gain) - 14.0).abs() < 1e-9);
    // A native key above 0 dB: 0 to +6 dB under -8 dB writes Levels -8 to
    // -2 dB, on the other branch of the fader curve. The segment plays
    // another curve, so export does not reuse its imported fit and divides
    // it into Linear pieces instead (`convert::audio`).
    let (error, _, clip_gain) = exported_segment_error([1.0, gain(6.0)], gain(-8.0));
    assert_eq!(clip_gain, 1.0);
    assert!(error > 0.1, "{error}");
}
