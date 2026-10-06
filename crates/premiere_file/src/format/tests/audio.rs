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

/// A fader Mute on, as Premiere saves it: the values of the muted A1 and A2
/// faders of a real Premiere 26.5.1 project (SHA-256
/// ace57eb53250a0c41b6aeff89474753fc5c68fe4c41157e4daf89a703b7bbe55,
/// AudioComponentParam 3892 and 3981; neither track has `Track/IsMuted`) and
/// of five muted multicam track faders of the pinned authentic
/// `adobe_native_variable_speed_ramp_ppro25.prproj`.
const NATIVE_MUTE_ON: &str = "<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>\n\t\t<CurrentValue>true</CurrentValue>";

/// A2 of the pinned audio case places occurrence 96.
const A2: &str = "AudioClipTrack:5664f3dd-3a10-4e58-848d-ea7bb402f5d9";

/// The report of an unreadable value `name` of a fader parameter on `track`.
fn invalid_fader_value(track: &str, param: &str, name: &str) -> Omission {
    omitted(
        OmissionScope::Feature,
        track,
        format!(
            "audio level not converted: unsupported conversion: AudioComponentParam:{param}: invalid {name} value"
        ),
    )
}

#[test]
fn audio_source_proxies_native_multichannel_primary_is_not_replaced() {
    // Unchanged Premiere 25.6.6 save: the primary's Canon Log picture profile
    // rejects before audio layout inspection. Its four-channel/proxy facts are
    // checked separately below; this is not evidence that the layout guard ran.
    let xml = fixture_xml("adobe_native_variable_speed_ramp_ppro25.prproj");
    let (project, notes) =
        inspect_project_with_omissions(&xml, Some("dd04c039-a6ca-4253-9a81-8da3f58840f1")).unwrap();
    let sound = &project.sequences[0].audio;
    assert_eq!(sound.len(), 1);
    assert_eq!(sound[0].id.as_deref(), Some("AudioClipTrackItem:1263"));
    assert_eq!(sound[0].volume, LinearGain::ZERO);
    assert_eq!(
        sound[0].media.0,
        "Media:ObjectUID:82b53cc7-8483-4e16-9f9f-95769ed0f48b"
    );
    for item in ["1265", "1266", "1267", "1268"] {
        let failures: Vec<_> = notes
            .iter()
            .filter(|note| note.scope == OmissionScope::Occurrence && note.record == item)
            .collect();
        assert_eq!(failures.len(), 1, "{notes:?}");
        assert_eq!(
            failures[0].reason,
            "unsupported conversion: VideoStream:902: unsupported OriginalColorSpace profile for its native role"
        );
    }
    let wire = roxmltree::Document::parse(&xml).unwrap();
    let record = |id: &str| {
        wire.root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap()
    };
    let primary_uid = "1dd864a8-10b6-4f2c-a3c9-e41ab651aefd";
    assert_eq!(
        record("517")
            .descendants()
            .find(|node| node.has_tag_name("Media"))
            .unwrap()
            .attribute("ObjectURef"),
        Some(primary_uid)
    );
    let primary = wire
        .root_element()
        .children()
        .find(|node| node.attribute("ObjectUID") == Some(primary_uid))
        .unwrap();
    for (tag, id) in [("AudioStream", "901"), ("VideoStream", "902")] {
        assert_eq!(
            primary
                .children()
                .find(|node| node.has_tag_name(tag))
                .unwrap()
                .attribute("ObjectRef"),
            Some(id)
        );
    }
    assert_eq!(
        record("901")
            .children()
            .find(|node| node.has_tag_name("AudioChannelLayout"))
            .unwrap()
            .text(),
        Some(r#"[{"channellabel":0},{"channellabel":0},{"channellabel":0},{"channellabel":0}]"#)
    );
    let profile: serde_json::Value = serde_json::from_str(
        record("902")
            .children()
            .find(|node| node.has_tag_name("OriginalColorSpace"))
            .unwrap()
            .text()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        profile["baseColorProfile"]["colorProfileName"],
        "Canon Log2/Cinema Gamut"
    );
    let mut proxy_media = std::collections::BTreeSet::new();
    let proxies: Vec<_> = record("517")
        .descendants()
        .filter(|node| node.has_tag_name("AudioProxyItem"))
        .collect();
    assert_eq!(proxies.len(), 4);
    for (index, reference) in proxies.iter().enumerate() {
        let proxy = record(reference.attribute("ObjectRef").unwrap());
        assert_eq!(
            proxy
                .children()
                .find(|node| node.has_tag_name("ProxyStreamIndex"))
                .unwrap()
                .text(),
            Some("0")
        );
        assert_eq!(
            proxy
                .children()
                .find(|node| node.has_tag_name("OriginalSecondaryIndex"))
                .unwrap()
                .text(),
            Some(index.to_string().as_str())
        );
        let uid = proxy
            .children()
            .find(|node| node.has_tag_name("ProxyMedia"))
            .unwrap()
            .attribute("ObjectURef")
            .unwrap();
        assert!(proxy_media.insert(uid));
        assert!(!project
            .media
            .contains_key(&MediaId(format!("Media:ObjectUID:{uid}"))));
        let media = wire
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectUID") == Some(uid))
            .unwrap();
        let stream = media
            .children()
            .find(|node| node.has_tag_name("AudioStream"))
            .unwrap()
            .attribute("ObjectRef")
            .unwrap();
        assert_eq!(
            record(stream)
                .children()
                .find(|node| node.has_tag_name("AudioChannelLayout"))
                .unwrap()
                .text(),
            Some(r#"[{"channellabel":0}]"#)
        );
    }
}

#[test]
fn adobe_fader_mute_reads_premiere_true_and_false() {
    // The five track faders that Premiere 25.6.6 saved muted in the authentic
    // multicam sequence read. Its ordinary stereo source with AudioProxies
    // now imports, but the saved track mute must still silence it.
    let ramp = fixture_xml("adobe_native_variable_speed_ramp_ppro25.prproj");
    assert_eq!(ramp.matches(NATIVE_MUTE_ON).count(), 5);
    let (project, omissions) =
        inspect_project_with_omissions(&ramp, Some("dd04c039-a6ca-4253-9a81-8da3f58840f1"))
            .unwrap();
    let sounds = &project.sequences[0].audio;
    assert_eq!(sounds.len(), 1);
    assert_eq!(sounds[0].id.as_deref(), Some("AudioClipTrackItem:1263"));
    assert_eq!(sounds[0].volume, LinearGain::ZERO);
    assert_eq!(
        sounds[0].media.0,
        "Media:ObjectUID:82b53cc7-8483-4e16-9f9f-95769ed0f48b"
    );
    let unread: Vec<_> = omissions
        .iter()
        .filter(|item| item.reason.starts_with("audio level not converted"))
        .collect();
    assert!(unread.is_empty(), "{unread:?}");

    // A1's fader (Volume 126, Mute 127) plays its placement at 1.0, A2's
    // (Volume 129 at 0.5, Mute 130) its placement at 0.5. Each row inserts
    // values at the start of fader parameters, as `(ObjectID, children)`, and
    // expects both volumes and the unread audio level, if any.
    let rows = [
        // A1's Mute on as Premiere saves it; A2's off in its StartKeyframe,
        // which its CurrentValue does not override.
        (
            vec![
                ("127", NATIVE_MUTE_ON),
                ("130", "<StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><CurrentValue>true</CurrentValue>"),
            ],
            [0.0, 0.5],
            None,
        ),
        // A Volume stays a number, even beside a Mute that is on: it is
        // reported, and its chain plays at unity.
        (
            vec![("126", NATIVE_MUTE_ON), ("127", NATIVE_MUTE_ON)],
            [1.0, 0.5],
            Some(invalid_fader_value(A1, "126", "Volume")),
        ),
        // A Mute's text is exact: it is reported, and its chain, the 0.5
        // Volume included, plays at unity.
        (
            vec![("130", "<CurrentValue>True</CurrentValue>")],
            [1.0, 1.0],
            Some(invalid_fader_value(A2, "130", "Mute")),
        ),
    ];
    for (edits, expected, report) in rows {
        let xml = edits
            .iter()
            .fold(audio_clips_xml(), |xml, (param, children)| {
                with_children(
                    &xml,
                    &format!(r#"<AudioComponentParam ObjectID="{param}""#),
                    children,
                )
            });
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let read: Vec<_> = project.sequences[0]
            .audio
            .iter()
            .map(|clip| clip.volume.as_f64())
            .collect();
        assert_eq!(read, expected, "{edits:?}");
        let unread: Vec<_> = omissions
            .into_iter()
            .filter(|item| item.reason.starts_with("audio level not converted"))
            .collect();
        assert_eq!(unread, Vec::from_iter(report), "{edits:?}");
    }
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
fn audio_clock_reads_constant_speed_reverse_and_rejects_variable_remapping() {
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
        let retimed = if field.contains("PlaybackSpeed") {
            let xml = edit_audio_record(&retimed, "128", |record| {
                record.replace(
                    "<OutPoint>1079568000000</OutPoint>",
                    "<OutPoint>2095632000000</OutPoint>",
                )
            });
            xml.replace(
                "<Duration>1270080000000</Duration>",
                "<Duration>2540160000000</Duration>",
            )
        } else {
            retimed
        };
        let (project, omissions) = inspect_project_with_omissions(&retimed, None).unwrap();
        if field.contains("TimeRemapping") {
            assert_eq!(project.sequences[0].audio.len(), 1, "{field}");
            assert!(
                omissions
                    .iter()
                    .any(|item| item.reason.contains("audio TimeRemapping")),
                "{omissions:?}"
            );
        } else {
            assert_eq!(
                project.sequences[0].audio.len(),
                2,
                "{field}: {omissions:?}"
            );
            let sound = project.sequences[0]
                .audio
                .iter()
                .find(|sound| sound.record() == "AudioClipTrackItem:96")
                .unwrap();
            assert_eq!(
                sound.playback_rate,
                if field.contains("PlayBackwards") {
                    -1.0
                } else {
                    2.0
                }
            );
        }
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
    // tracks is synthetic. Its 5.1 layout does not convert, whether or not its
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
                "1078",
                "unsupported conversion: AudioTransitionTrackItem:1078: unsupported audio transition layout",
            )],
            "{track}"
        );
    }
}

/// Crossfade G3 of the pinned transitions case (`AudioTransitionTrackItem:94`,
/// 11.5-13.5 s, cut at 12.5 s) plays placement 71 (10-12.5 s, AudioClip 180,
/// source 0-2.5 s) 1 s past its Out and 72 (12.5-14.5 s, AudioClip 181,
/// source 1-3 s) from 1 s before its In; the 5 s source leaves both handles.
/// Without the incoming handle (72 from source 0.5 s) or the outgoing one (71
/// to source 4.5 s), the crossfade is reported, and both clips keep their
/// cuts, source ranges and levels without fades.
#[test]
fn an_audio_crossfade_without_a_source_handle_keeps_both_cuts() {
    let xml = fixture_xml("feature_audio_transitions_strict.prproj");
    let with_clip_source = |id: &str, source: [i64; 2]| {
        let start = xml.find(&format!(r#"<AudioClip ObjectID="{id}""#)).unwrap();
        let end = start + xml[start..].find("</AudioClip>").unwrap();
        let mut record = xml[start..end].to_owned();
        for (tag, ticks) in [("InPoint", source[0]), ("OutPoint", source[1])] {
            let open = record.find(&format!("<{tag}>")).unwrap() + tag.len() + 2;
            let close = open + record[open..].find('<').unwrap();
            record.replace_range(open..close, &ticks.to_string());
        }
        format!("{}{record}{}", &xml[..start], &xml[end..])
    };
    // Each placement as its id, [start, end, in, out] and static volume.
    let placements = |project: &PrProjectFile| -> Vec<(String, [i64; 4], f64)> {
        project.sequences[0]
            .audio
            .iter()
            .filter(|clip| clip.start_ticks >= 10 * TICKS && clip.start_ticks <= 25 * TICKS / 2)
            .map(|clip| {
                assert!(
                    clip.fade_in.is_none() && clip.fade_out.is_none(),
                    "{clip:?}"
                );
                (
                    clip.record().to_owned(),
                    [
                        clip.start_ticks,
                        clip.end_ticks,
                        clip.in_ticks,
                        clip.out_ticks,
                    ],
                    clip.volume.as_f64(),
                )
            })
            .collect()
    };
    let (base, _) = inspect_project_with_omissions(&xml, None).unwrap();
    let level = |id: &str| {
        base.sequences[0]
            .audio
            .iter()
            .find(|clip| clip.record() == id)
            .unwrap()
            .volume
            .as_f64()
    };
    for (edited, reason, sources) in [
        (
            with_clip_source("181", [TICKS / 2, 5 * TICKS / 2]),
            "incoming clip lacks the required source handle",
            [[0, 5 * TICKS / 2], [TICKS / 2, 5 * TICKS / 2]],
        ),
        (
            with_clip_source("180", [2 * TICKS, 9 * TICKS / 2]),
            "outgoing clip lacks the required source handle",
            [[2 * TICKS, 9 * TICKS / 2], [TICKS, 3 * TICKS]],
        ),
    ] {
        let (project, omissions) = inspect_project_with_omissions(&edited, None).unwrap();
        let reported: Vec<_> = omissions
            .iter()
            .filter(|item| item.record == "94")
            .map(|item| (item.scope, item.reason.as_str()))
            .collect();
        assert_eq!(
            reported,
            [(
                OmissionScope::Feature,
                format!("unsupported conversion: AudioTransitionTrackItem:94: {reason}").as_str()
            )]
        );
        assert_eq!(
            placements(&project),
            [
                (
                    "AudioClipTrackItem:71".to_owned(),
                    [10 * TICKS, 25 * TICKS / 2, sources[0][0], sources[0][1]],
                    level("AudioClipTrackItem:71"),
                ),
                (
                    "AudioClipTrackItem:72".to_owned(),
                    [25 * TICKS / 2, 29 * TICKS / 2, sources[1][0], sources[1][1]],
                    level("AudioClipTrackItem:72"),
                ),
            ],
            "{reason}"
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
                prepared_clock: None,
                intrinsic_ticks: 8 * TICKS,
                channels,
                sample_rate: 44_100,
            }),
        });
        sequence.audio.push(PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: None,
            media: id,
            start_ticks: TICKS,
            end_ticks: 4 * TICKS,
            in_ticks: 2 * TICKS,
            out_ticks: 5 * TICKS,
            volume: LinearGain::new(gain).unwrap(),
            volume_keys: None,
            fade_in: None,
            fade_out: None,
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
fn audio_clock_import_keeps_source_gain_clocks_and_retimed_fade_endpoints() {
    for rate in [2.0, -2.0] {
        let mut project = audio_project(vec![("clock.wav", AudioChannels::Stereo, 1.0)]);
        let sound = &mut project.sequences[0].audio[0];
        sound.playback_rate = rate;
        sound.end_ticks = 2 * TICKS;
        sound.out_ticks = 4 * TICKS;
        sound.volume_keys = Some(PrVolumeKeys {
            gain: 1.0,
            keys: vec![
                PrScalarKeyframe {
                    source_ticks: 5 * TICKS / 2,
                    value: 0.25,
                    easing: PrKeyframeEasing::Linear,
                },
                PrScalarKeyframe {
                    source_ticks: 7 * TICKS / 2,
                    value: 1.0,
                    easing: PrKeyframeEasing::Linear,
                },
            ],
        });
        sound.fade_out = Some(crate::schema::PrAudioFade {
            id: None,
            curve: crate::schema::PrFadeCurve::ConstantGain,
            duration_ticks: TICKS / 4,
        });
        let wire = crate::tests::support::project_document_with_media(
            &project.sequences[0],
            &project.media,
        );
        let layer = wire["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Audio")
            .unwrap();
        let property = &layer["playback"]["mapping"]["property"];
        let expected = if rate > 0.0 {
            [2000, 4000]
        } else {
            [6000, 4000]
        };
        assert_eq!(property["keyframes"][0]["value"], expected[0]);
        assert_eq!(property["keyframes"][1]["value"], expected[1]);
        let entry = wire["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["target"]["layerId"] == layer["id"])
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        let gain_time = if rate > 0.0 { 2500 } else { 5500 };
        assert!(
            keys.iter()
                .any(|key| key["layerTime"] == gain_time && key["value"]["value"] == 0.25),
            "{keys:?}"
        );
        assert!(
            keys.iter()
                .any(|key| key["layerTime"] == 4000 && key["value"]["value"] == 0.0),
            "{keys:?}"
        );
    }
}

#[test]
fn audio_clock_crossfade_handles_scale_with_saved_source_endpoints() {
    let xml = fixture_xml("feature_audio_transitions_strict.prproj");
    let fast = edit_audio_record(&xml, "180", |record| {
        record
            .replace("<InPoint>", "<PlaybackSpeed>2</PlaybackSpeed><InPoint>")
            .replace(
                "<OutPoint>635040000000</OutPoint>",
                "<OutPoint>1270080000000</OutPoint>",
            )
    });
    let (project, omissions) = inspect_project_with_omissions(&fast, None).unwrap();
    let sound = project.sequences[0]
        .audio
        .iter()
        .find(|sound| sound.record() == "AudioClipTrackItem:71")
        .unwrap();
    assert_eq!(sound.playback_rate, 2.0);
    assert!(sound.fade_out.is_none());
    assert!(
        omissions.iter().any(|item| item.record == "94"
            && item
                .reason
                .contains("outgoing clip lacks the required source handle")),
        "{omissions:?}"
    );
    assert_eq!(sound.end_ticks, 25 * TICKS / 2);
    assert_eq!(sound.out_ticks, 5 * TICKS);
    for backwards in [false, true] {
        let slow = edit_audio_record(&xml, "180", |record| {
            record
                .replace(
                    "<InPoint>",
                    &format!(
                        "<PlaybackSpeed>0.5</PlaybackSpeed>{}<InPoint>",
                        if backwards {
                            "<PlayBackwards>true</PlayBackwards>"
                        } else {
                            ""
                        }
                    ),
                )
                .replace(
                    "<OutPoint>635040000000</OutPoint>",
                    "<OutPoint>317520000000</OutPoint>",
                )
        });
        let (project, omissions) = inspect_project_with_omissions(&slow, None).unwrap();
        let sound = project.sequences[0]
            .audio
            .iter()
            .find(|sound| sound.record() == "AudioClipTrackItem:71")
            .unwrap();
        assert_eq!(sound.playback_rate, if backwards { -0.5 } else { 0.5 });
        assert_eq!(sound.end_ticks, 27 * TICKS / 2);
        assert_eq!(sound.out_ticks, 7 * TICKS / 4);
        assert_eq!(sound.fade_out.as_ref().unwrap().duration_ticks, 2 * TICKS);
        assert!(
            !omissions.iter().any(|item| item.record == "94"),
            "{omissions:?}"
        );
    }
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
        .find(|node| {
            node.attribute("ObjectID") == Some(id) || node.attribute("ObjectUID") == Some(id)
        })
        .unwrap();
    let before = &xml[record.range()];
    let after = edit(before);
    assert_ne!(before, after, "record {id}");
    let mut result = xml.to_owned();
    result.replace_range(record.range(), &after);
    result
}

#[test]
fn custom_shape_is_bounded() {
    for value in [-24, 30, i64::MIN, i64::MAX] {
        assert!(crate::schema::CustomFadeShape::new(value).is_none());
    }
    for value in [-23, 0, 29] {
        assert_eq!(
            i64::from(crate::schema::CustomFadeShape::new(value).unwrap().value()),
            value
        );
    }
}

#[test]
fn unproven_custom_crossfade_preserves_sibling_sound() {
    let source = fixture_xml("feature_audio_custom_fades_strict.prproj");
    let changed = edit_audio_record(&source, "73", |record| {
        record.replace(
            "<HasOutgoingClip>false</HasOutgoingClip>",
            "<HasOutgoingClip>true</HasOutgoingClip>",
        )
    });
    let (project, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
    let clips = &project.sequences[0].audio;
    assert_eq!(clips.len(), 4);
    assert!(clips[0].fade_in.is_none() && clips[0].fade_out.is_none());
    assert!(clips[1..].iter().all(|clip| clip.fade_in.is_some()));
    assert_eq!(
        (clips[0].start_ticks, clips[0].end_ticks),
        (0, 5 * TICKS / 2)
    );
    assert!(omissions.iter().any(|note| note.record == "73"
        && note.kind == OmissionKind::Omitted
        && note.reason.contains("no accepted native render evidence")));
}

#[test]
fn unmeasured_custom_shapes_preserve_sibling_fades_and_cuts() {
    let source = fixture_xml("feature_audio_custom_fades_strict.prproj");
    for replacement in [
        "<FadeShapeType>0</FadeShapeType><FadeShapeValue>-23</FadeShapeValue>",
        "<FadeShapeType>1</FadeShapeType><FadeShapeValue>-23</FadeShapeValue>",
        "<FadeShapeValue>-24</FadeShapeValue>",
        "<FadeShapeValue>30</FadeShapeValue>",
        "<FadeShapeValue>0.5</FadeShapeValue>",
        "<CrossfadeSymmetry>1</CrossfadeSymmetry><FadeShapeValue>-23</FadeShapeValue>",
        "",
    ] {
        let changed = edit_audio_record(&source, "73", |record| {
            record.replace("<FadeShapeValue>-23</FadeShapeValue>", replacement)
        });
        let (project, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
        let clips = &project.sequences[0].audio;
        assert_eq!(clips.len(), 4);
        assert!(clips[0].fade_in.is_none());
        assert!(clips[1..].iter().all(|clip| clip.fade_in.is_some()));
        assert_eq!(
            (
                clips[0].start_ticks,
                clips[0].end_ticks,
                clips[0].in_ticks,
                clips[0].out_ticks
            ),
            (0, 5 * TICKS / 2, 0, 5 * TICKS / 2)
        );
        assert!(
            omissions
                .iter()
                .any(|note| note.record == "73" && note.kind == OmissionKind::Omitted),
            "{replacement}: {omissions:?}"
        );
    }
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
fn saved_ducking_unequal_channels_fail_closed_unless_bypassed() {
    let xml = fixture_xml("feature_saved_ducking.prproj");
    // Mutate only Right's plateau. Never choose one channel for the stereo mix.
    let unequal = edit_audio_record(&xml, "1152", |record| {
        record.replace("0.541666686535", "0.6")
    });
    let (project, omissions) = inspect_project_with_omissions(&unequal, None).unwrap();
    let clips = &project.sequences[0].audio;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].volume.as_f64(), 0.0);
    assert!(clips[0].volume_keys.is_none());
    assert_eq!(clips[1].volume, LinearGain::UNITY);
    assert!(omissions.iter().any(|item| item
        .reason
        .contains("unequal Amplify channel gains or keys")));

    let bypassed = with_children(
        &unequal,
        r#"<AudioComponentParam ObjectID="1148""#,
        "<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>",
    );
    let (project, omissions) = inspect_project_with_omissions(&bypassed, None).unwrap();
    let music = &project.sequences[0].audio[0];
    assert_eq!(music.volume, LinearGain::UNITY);
    assert!(music.volume_keys.is_none());
    assert!(!omissions.iter().any(|item| item.reason.contains("Amplify")));
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
    // does a Volume whose Level record is missing, still named the clip Volume,
    // and a Level saved as a switch's text, which is no number.
    let level = r#"<AudioComponentParam ObjectID="142""#;
    let start = xml.find(level).unwrap();
    let switch_text_level = format!(
        "{}{}",
        &xml[..start],
        xml[start..].replacen(
            "<StartKeyframe>-91445760000000000,0.177827939391,0,0,0,0,0,0</StartKeyframe>\n\t\t<CurrentValue>0.17782793939113617</CurrentValue>",
            NATIVE_MUTE_ON,
            1,
        )
    );
    assert_ne!(switch_text_level, xml);
    for (edited, report) in [
        (
            xml.replacen(first_key, "444528000000,0.177827939391,5,", 1),
            "Bezier Volume keyframes are not converted",
        ),
        (
            xml.replacen(r#"ObjectRef="142""#, r#"ObjectRef="9142""#, 1),
            "missing reference at Param",
        ),
        (
            switch_text_level,
            "AudioComponentParam:142: invalid Level value",
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

#[test]
fn nonfinite_mono_writer_gain_is_rejected() {
    let project = audio_project(vec![("voice.wav", AudioChannels::Mono, f64::MAX)]);
    assert!(project_xml(&project).is_err());
    let mut keyed = audio_project(vec![("voice.wav", AudioChannels::Mono, 1.0)]);
    keyed.sequences[0].audio[0].volume_keys = Some(PrVolumeKeys {
        gain: 1.0,
        keys: vec![PrScalarKeyframe {
            source_ticks: TICKS,
            value: f64::MAX,
            easing: PrKeyframeEasing::Linear,
        }],
    });
    assert!(project_xml(&keyed).is_err());
    let representable = audio_project(vec![("voice.wav", AudioChannels::Mono, 1e100)]);
    assert!(project_xml(&representable).is_ok());
}

#[test]
fn invalid_audio_item_mute_preserves_siblings() {
    let project = audio_project(vec![
        ("voice.wav", AudioChannels::Mono, 1.0),
        ("music.mp3", AudioChannels::Stereo, 0.5),
    ]);
    let xml = project_xml(&project).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let item = document
        .descendants()
        .find(|node| node.has_tag_name("AudioClipTrackItem"))
        .unwrap();
    let id = item.attribute("ObjectID").unwrap();
    let xml = edit_audio_record(&xml, id, |record| {
        record.replace("</ClipTrackItem>", "<IsMuted>yes</IsMuted></ClipTrackItem>")
    });
    let (loaded, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert_eq!(loaded.sequences[0].audio.len(), 1);
    assert_eq!(loaded.sequences[0].video_occurrences().count(), 1);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("IsMuted")),
        "{omissions:?}"
    );
}

#[test]
fn invalid_audio_track_and_master_mute_do_not_play() {
    for tag in ["AudioClipTrack", "AudioMixTrack"] {
        let project = audio_project(vec![
            ("voice.wav", AudioChannels::Mono, 1.0),
            ("music.mp3", AudioChannels::Stereo, 0.5),
        ]);
        let xml = project_xml(&project).unwrap();
        let document = roxmltree::Document::parse(&xml).unwrap();
        let track = document
            .descendants()
            .find(|node| node.has_tag_name(tag))
            .unwrap();
        let id = track
            .attribute("ObjectID")
            .or_else(|| track.attribute("ObjectUID"))
            .unwrap();
        let xml = edit_audio_record(&xml, id, |record| {
            record.replace("</Track>", "<IsMuted>yes</IsMuted></Track>")
        });
        let (loaded, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert_eq!(
            loaded.sequences[0].audio.len(),
            if tag == "AudioMixTrack" { 0 } else { 1 }
        );
        assert_eq!(loaded.sequences[0].video_occurrences().count(), 1);
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains("IsMuted")),
            "{omissions:?}"
        );
    }
}

#[test]
fn interpreted_picture_rejections_preserve_independently_placed_sound() {
    let xml = linked_av_xml();
    let control = inspect_project(&xml, None).unwrap();
    assert!(!control.audio.is_empty());
    for fields in [
        "<IsFrameRateOverridden>invalid</IsFrameRateOverridden>",
        "<IsFrameRateOverridden>true</IsFrameRateOverridden>",
        "<IsFrameRateOverridden>true</IsFrameRateOverridden><OveriddenFrameRate>0</OveriddenFrameRate>",
        "<IsFrameRateOverridden>true</IsFrameRateOverridden><OveriddenFrameRate>-1</OveriddenFrameRate>",
        // Deliberately unverified alternate intrinsic OriginalDuration.
        "<IsFrameRateOverridden>true</IsFrameRateOverridden><OveriddenFrameRate>16934400000</OveriddenFrameRate>",
    ] {
        let changed = with_children(&xml, "<VideoStream ObjectID=\"112\"", fields);
        let (project, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.video_occurrences().count(), 0, "{fields}");
        assert_eq!(format!("{:?}", sequence.audio), format!("{:?}", control.audio));
        assert!(omissions.iter().any(|item| item.reason.contains("interpretation")
            || item.reason.contains("OveriddenFrameRate")), "{omissions:?}");
    }
    // A saved inactive value is not an active interpretation.
    let changed = with_children(&xml, "<VideoStream ObjectID=\"112\"",
        "<IsFrameRateOverridden>false</IsFrameRateOverridden><OveriddenFrameRate>16934400000</OveriddenFrameRate>");
    let sequence = inspect_project(&changed, None).unwrap();
    assert_eq!(
        sequence.video_occurrences().count(),
        control.video_occurrences().count()
    );
    assert_eq!(
        format!("{:?}", sequence.audio),
        format!("{:?}", control.audio)
    );
}

#[test]
fn clip_volume_keeps_static_zero_when_current_mute_cannot_convert() {
    // Only Mute175 is changed from the pinned native static-zero placement.
    let source = fixture_xml("feature_audio_volume_levels_strict.prproj");
    let (expected, _) = inspect_project_with_omissions(&source, None).unwrap();
    assert_eq!(
        expected.sequences[0].id.as_deref(),
        Some("0a58340d-11c1-4813-8344-4eaf7b7d1bf7")
    );
    let zero = expected.sequences[0]
        .audio
        .iter()
        .find(|clip| clip.id.as_deref() == Some("AudioClipTrackItem:80"))
        .unwrap();
    assert_eq!(zero.volume, LinearGain::ZERO);
    assert_eq!(zero.start_ticks..zero.end_ticks, TICKS..2 * TICKS);
    let changed = edit_audio_record(&source, "175", |record| {
        record.replace(
            "</AudioComponentParam>",
            "<CurrentValue>unreadable</CurrentValue></AudioComponentParam>",
        )
    });
    let (actual, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
    assert_eq!(
        actual.sequences[0].audio.len(),
        expected.sequences[0].audio.len()
    );
    for (actual, expected) in actual.sequences[0]
        .audio
        .iter()
        .zip(&expected.sequences[0].audio)
    {
        assert_eq!(
            (
                &actual.id,
                &actual.media,
                actual.start_ticks,
                actual.end_ticks,
                actual.in_ticks,
                actual.out_ticks
            ),
            (
                &expected.id,
                &expected.media,
                expected.start_ticks,
                expected.end_ticks,
                expected.in_ticks,
                expected.out_ticks
            )
        );
        assert_eq!(actual.volume, expected.volume);
        assert!(actual.volume_keys.is_none());
    }
    assert!(
        omissions
            .iter()
            .any(|item| item.record.contains("138") && item.reason.contains("Mute")),
        "{omissions:?}"
    );
}

#[test]
fn clip_volume_keeps_known_mute_when_level_cannot_convert() {
    // Mutations of the existing current-layout fixture, not new native Mute evidence.
    let source = fixture_xml("feature_audio_volume_keys_strict.prproj");
    let (original, original_omissions) = inspect_project_with_omissions(&source, None).unwrap();
    let original = &original.sequences[0];
    for (before, after, reason) in [
        (
            "444528000000,0.177827939391,0,",
            "444528000000,0.177827939391,5,",
            "Bezier Volume keyframes are not converted",
        ),
        (
            "-91445760000000000,0.177827939391,0,",
            "-91445760000000000,invalid,0,",
            "invalid Level value",
        ),
        (
            "<Name>Level</Name>",
            "<IsTimeVarying>false</IsTimeVarying><Name>Level</Name>",
            "Level keyframes and IsTimeVarying disagree",
        ),
    ] {
        let unread = edit_audio_record(&source, "142", |record| record.replace(before, after));
        for muted in [false, true] {
            let xml = edit_audio_record(&unread, "141", |record| {
                record.replace(
                    "<Name>Mute</Name>",
                    &format!(
                        "<CurrentValue>{}</CurrentValue><Name>Mute</Name>",
                        u8::from(muted)
                    ),
                )
            });
            let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
            let audio = &project.sequences[0].audio;
            assert_eq!(audio.len(), original.audio.len());
            let clip = &audio[0];
            assert_eq!(
                clip.volume.as_f64(),
                if muted { 0.0 } else { 1.0 },
                "{reason}"
            );
            assert!(clip.volume_keys.is_none());
            for (index, (clip, original)) in audio.iter().zip(&original.audio).enumerate() {
                assert_eq!(clip.id, original.id);
                assert_eq!(clip.media, original.media);
                assert_eq!(
                    (
                        clip.start_ticks,
                        clip.end_ticks,
                        clip.in_ticks,
                        clip.out_ticks
                    ),
                    (
                        original.start_ticks,
                        original.end_ticks,
                        original.in_ticks,
                        original.out_ticks
                    ),
                );
                if index != 0 {
                    assert_eq!(clip.volume, original.volume);
                    assert_eq!(
                        clip.volume_keys
                            .as_ref()
                            .map(|keys| (&keys.keys, keys.gain)),
                        original
                            .volume_keys
                            .as_ref()
                            .map(|keys| (&keys.keys, keys.gain)),
                    );
                }
            }
            assert_eq!(
                omissions.len(),
                original_omissions.len() + 1,
                "{omissions:?}"
            );
            assert_eq!(&omissions[1..], original_omissions.as_slice());
            assert_eq!(omissions[0].scope, OmissionScope::Feature);
            assert_eq!(omissions[0].record, "AudioFilterComponent:118");
            assert!(omissions[0]
                .reason
                .starts_with("clip Volume not converted: "));
            assert!(omissions[0].reason.ends_with(reason), "{omissions:?}");
        }
    }
}

#[test]
fn unread_clip_mute_does_not_admit_other_levels_or_layouts() {
    let source = fixture_xml("feature_audio_volume_levels_strict.prproj");
    let rising = edit_audio_record(&source, "176", |record| {
        record.replace("<IsTimeVarying>false</IsTimeVarying>", "<IsTimeVarying>true</IsTimeVarying>").replace("</AudioComponentParam>",
            "<Keyframes>0,0,0,0,0,0,0,0;254016000000,0.177827939391,0,0,0,0,0,0;</Keyframes></AudioComponentParam>")
    });
    let (keyed, reports) = inspect_project_with_omissions(&rising, None).unwrap();
    assert!(reports.is_empty(), "{reports:?}");
    let keys = keyed.sequences[0]
        .audio
        .iter()
        .find(|clip| clip.id.as_deref() == Some("AudioClipTrackItem:80"))
        .unwrap()
        .volume_keys
        .as_ref()
        .unwrap();
    assert_eq!(keys.keys.len(), 2);
    assert_eq!(keys.keys[0].value, 0.0);
    assert!(keys.keys[1].value > 0.0);

    let unread = edit_audio_record(&source, "175", |record| {
        record.replace(
            "</AudioComponentParam>",
            "<CurrentValue>unreadable</CurrentValue></AudioComponentParam>",
        )
    });
    for changed in [
        edit_audio_record(&unread, "176", |record| {
            record
                .replace(
                    ",0.,0,0,0,0,0,0</StartKeyframe>",
                    ",0.1,0,0,0,0,0,0</StartKeyframe>",
                )
                .replace(
                    "<CurrentValue>0</CurrentValue>",
                    "<CurrentValue>0.1</CurrentValue>",
                )
        }),
        edit_audio_record(&rising, "175", |record| {
            record.replace(
                "</AudioComponentParam>",
                "<CurrentValue>unreadable</CurrentValue></AudioComponentParam>",
            )
        }),
        edit_audio_record(&unread, "175", |record| {
            record.replace("<Name>Mute</Name>", "<Name>Bypass</Name>")
        }),
        edit_audio_record(&unread, "175", |record| {
            record.replace("<Name>Mute</Name>", "<Name>Unknown</Name>")
        }),
        edit_audio_record(&unread, "176", |record| {
            record.replace(
                "<IsTimeVarying>false</IsTimeVarying>",
                "<IsTimeVarying>true</IsTimeVarying>",
            )
        }),
    ] {
        let (project, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
        let clip = project.sequences[0]
            .audio
            .iter()
            .find(|clip| clip.id.as_deref() == Some("AudioClipTrackItem:80"))
            .unwrap();
        assert_eq!(clip.volume.as_f64(), 1.0);
        assert!(clip.volume_keys.is_none());
        assert!(
            omissions.iter().any(|item| item.record.contains("138")
                && item.reason.contains("clip Volume not converted")),
            "{omissions:?}"
        );
    }
}

#[test]
fn audio_filters_fill_right_reads_native_mono_disconnection_and_static_bypass() {
    let excerpts = include_str!("../../../tests/fixtures/feature_audio_filters_native.xml");
    let document = roxmltree::Document::parse(excerpts).unwrap();
    let records: String = document
        .root_element()
        .children()
        .filter(|node| node.is_element())
        .map(|node| &excerpts[node.range()])
        .collect();
    let xml = audio_clips_xml().replace("</PremiereData>", &format!("{records}</PremiereData>"));
    let xml = edit_audio_record(&xml, "92", |record| {
        record.replace("</ComponentChain>",
        r#"<Components Version="1"><Component Index="0" ObjectRef="1111"/></Components></ComponentChain>"#)
    });
    let xml = edit_audio_record(&xml, "84", |record| {
        record.replace(AudioChannels::Stereo.layout(), AudioChannels::Mono.layout())
    });
    // Native saved SecondaryContent form: left channel 0, disconnected right.
    let xml = edit_audio_record(&xml, "125", |record| {
        record.replace(r#"<Content ObjectRef="64"/>"#, "").replace(
            "<ChannelIndex>1</ChannelIndex>",
            "<ChannelIndex>18446744073709551615</ChannelIndex>",
        )
    });
    let xml = edit_audio_record(&xml, "107", |record| {
        record.replace("</AudioClip>", "<Gain>0.5</Gain></AudioClip>")
    });
    for (component, param, audible, invalid) in [
        ("false", "false", true, false),
        ("true", "false", false, false),
        ("invalid", "false", false, true),
        ("false", "true", false, false),
        ("false", "invalid", false, true),
    ] {
        let changed = edit_audio_record(&xml, "1111", |record| {
            record.replace(
                "</Component>",
                &format!("<Bypass>{component}</Bypass></Component>"),
            )
        });
        let changed = edit_audio_record(&changed, "1558", |record| {
            record.replace(
                "</AudioComponentParam>",
                &format!("<CurrentValue>{param}</CurrentValue></AudioComponentParam>"),
            )
        });
        let (project, omissions) =
            inspect_project_with_omissions(&changed, Some(AUDIO_SEQUENCE)).unwrap();
        let sounds = &project.sequences[0].audio;
        assert_eq!(
            sounds.len(),
            if audible { 2 } else { 1 },
            "{component}/{param}: {omissions:?}"
        );
        assert_eq!(sounds.last().unwrap().volume.as_f64(), 0.5);
        if audible {
            assert_eq!(sounds[0].volume.as_f64(), 0.5);
            assert_eq!(sounds[0].source_channel, None);
            assert_eq!(sounds[0].in_ticks..sounds[0].out_ticks, 0..5 * TICKS);
        }
        let reports: Vec<_> = omissions
            .iter()
            .filter(|item| item.record == "AudioFilterComponent:1111")
            .collect();
        assert_eq!(reports.len(), usize::from(invalid), "{omissions:?}");
        if invalid {
            assert!(reports[0].reason.contains("Internal Audio Fill Right"));
        }
    }
    // Conflicting bypass fields and a malformed disconnected input must not
    // establish the measured route by taking just the first matching field.
    for (id, close, extra) in [
        (
            "1111",
            "</Component>",
            "<Bypass>true</Bypass><Bypass>false</Bypass>",
        ),
        (
            "125",
            "</SecondaryContent>",
            "<ChannelIndex>0</ChannelIndex>",
        ),
    ] {
        let changed = edit_audio_record(&xml, id, |record| {
            record.replace(close, &format!("{extra}{close}"))
        });
        let (project, omissions) =
            inspect_project_with_omissions(&changed, Some(AUDIO_SEQUENCE)).unwrap();
        assert_eq!(project.sequences[0].audio.len(), 1, "{omissions:?}");
        assert!(
            omissions
                .iter()
                .any(|item| item.reason.contains("Fill Right")),
            "{omissions:?}"
        );
    }
}

#[test]
fn audio_clock_sub_millisecond_window_omits_only_its_sound() {
    let xml = edit_audio_record(&audio_clips_xml(), "96", |record| {
        record.replace("<End>1270080000000</End>", "<End>254016000001</End>")
    });
    let xml = edit_audio_record(&xml, "128", |record| {
        record
            .replace("<InPoint>", "<PlaybackSpeed>2</PlaybackSpeed><InPoint>")
            .replace(
                "<OutPoint>1079568000000</OutPoint>",
                "<OutPoint>63504000002</OutPoint>",
            )
    });
    let (project, _) = inspect_project_with_omissions(&xml, Some(AUDIO_SEQUENCE)).unwrap();
    assert_eq!(project.sequences[0].audio.len(), 2);
    let ids = crate::tesseract_output::asset_ids_in_order(&project.sequences[0], &project.media);
    let mut omissions = Vec::new();
    let doc = crate::convert::premiere_to_tesseract(
        &project.sequences[0],
        &project.media,
        &ids,
        &mut omissions,
    )
    .unwrap();
    let wire = doc.to_json_value().unwrap();
    assert_eq!(
        wire["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Audio")
            .count(),
        1
    );
    assert!(
        omissions
            .iter()
            .any(|item| item.scope == OmissionScope::Occurrence
                && item.record == "AudioClipTrackItem:96"
                && item
                    .reason
                    .contains("collapses on the editable millisecond grid")),
        "{omissions:?}"
    );
}

#[test]
fn audio_pitch_unverified_saved_forms_preserve_siblings_and_report_raw_scaler() {
    for (flag, scaler, expected, unverified) in [
        ("", "", false, false),
        (
            "<MaintainAudioPitch>false</MaintainAudioPitch>",
            "",
            false,
            true,
        ),
        (
            "<MaintainAudioPitch>true</MaintainAudioPitch>",
            "",
            true,
            true,
        ),
        (
            "<MaintainAudioPitch>true</MaintainAudioPitch>",
            "<AudioTimeScalerSettings>{\"unknown\":7}</AudioTimeScalerSettings>",
            true,
            true,
        ),
        (
            "",
            "<AudioTimeScalerSettings>{\"unknown\":7}</AudioTimeScalerSettings>",
            false,
            true,
        ),
    ] {
        let xml = if flag.is_empty() && scaler.is_empty() {
            audio_clips_xml()
        } else {
            edit_audio_record(&audio_clips_xml(), "128", |record| {
                record
                    .replace("<InPoint>", &format!("{flag}<InPoint>"))
                    .replace("</AudioClip>", &format!("{scaler}</AudioClip>"))
            })
        };
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some(AUDIO_SEQUENCE)).unwrap();
        assert_eq!(project.sequences[0].audio.len(), 2, "{omissions:?}");
        let audio = project.sequences[0]
            .audio
            .iter()
            .find(|clip| clip.record() == "AudioClipTrackItem:96")
            .unwrap();
        assert_eq!(audio.preserve_audio_pitch, expected);
        assert_eq!(
            omissions
                .iter()
                .any(|item| item.record == "AudioClip:128" && item.reason.contains("unverified")),
            unverified,
            "{omissions:?}"
        );
        if scaler.contains("unknown") {
            assert!(
                omissions.iter().any(|item| item.reason.contains("unknown")
                    && item.reason.contains("not interpreted or replayed")),
                "{omissions:?}"
            );
        }
        if expected {
            let wire = crate::tests::support::project_document_with_media(
                &project.sequences[0],
                &project.media,
            );
            let sound = wire["composition"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|layer| {
                    layer["type"] == "Audio" && layer["playback"]["inputRange"]["start"] == 1000
                })
                .unwrap();
            assert_eq!(sound["preserveAudioPitch"], true);
            assert_eq!(sound["playback"]["mapping"]["type"], "timeRemap");
            assert_eq!(
                sound["playback"]["mapping"]["property"]["keyframes"][0]["value"],
                250
            );
        }
    }
}

#[test]
fn audio_clock_gross_saved_speed_disagreement_omits_only_malformed_sound() {
    let xml = edit_audio_record(&audio_clips_xml(), "128", |record| {
        record.replace("<InPoint>", "<PlaybackSpeed>2</PlaybackSpeed><InPoint>")
    });
    let (project, notes) = inspect_project_with_omissions(&xml, Some(AUDIO_SEQUENCE)).unwrap();
    assert_eq!(project.sequences[0].audio.len(), 1);
    assert!(
        notes.iter().any(|note| note.record == "96"
            && note
                .reason
                .contains("saved speed and source/timeline spans disagree")),
        "{notes:?}"
    );
}

#[test]
fn audio_clock_custom_short_outgoing_level_edge_maps_at_each_rate() {
    use crate::schema::{PrAudioFade, PrFadeCurve};
    for rate in [2.0_f64, 0.5, -2.0] {
        for nearby in [false, true] {
            let mut project = audio_project(vec![("clock.wav", AudioChannels::Stereo, 0.5)]);
            let clip = &mut project.sequences[0].audio[0];
            clip.start_ticks = 0;
            clip.end_ticks = TICKS;
            clip.in_ticks = TICKS / 2;
            clip.out_ticks = clip.in_ticks + (rate.abs() * TICKS as f64) as i64;
            clip.playback_rate = rate;
            clip.fade_in = None;
            clip.fade_out = Some(PrAudioFade {
                id: None,
                curve: PrFadeCurve::ConstantPower,
                duration_ticks: 16 * TICKS_PER_MILLISECOND,
            });
            let inward = 28 * TICKS / 30;
            let key_time = if nearby {
                TICKS - 22 * TICKS_PER_MILLISECOND
            } else {
                inward
            };
            let edge_key = clip.source_at(key_time).unwrap();
            clip.volume_keys = Some(PrVolumeKeys {
                gain: 0.5,
                keys: vec![PrScalarKeyframe {
                    source_ticks: edge_key,
                    value: 1.0,
                    easing: PrKeyframeEasing::Linear,
                }],
            });
            let ranges = (
                clip.start_ticks,
                clip.end_ticks,
                clip.in_ticks,
                clip.out_ticks,
            );
            let xml = project_xml(&project)
                .unwrap()
                .replace(
                    "<MatchName>Constant Power</MatchName>",
                    "<MatchName>Custom Fade</MatchName>",
                )
                .replace(
                    "</AudioTransitionTrackItem>",
                    "<FadeShapeValue>29</FadeShapeValue></AudioTransitionTrackItem>",
                );
            let (loaded, notes) = inspect_project_with_omissions(&xml, None).unwrap();
            let clip = &loaded.sequences[0].audio[0];
            assert_eq!(
                (
                    clip.start_ticks,
                    clip.end_ticks,
                    clip.in_ticks,
                    clip.out_ticks
                ),
                ranges
            );
            assert_eq!(
                clip.fade_out.as_ref().unwrap().duration_ticks,
                if nearby {
                    16 * TICKS_PER_MILLISECOND
                } else {
                    TICKS - inward
                },
                "{rate}/{nearby}: {notes:?}"
            );
            let mut reports = Vec::new();
            let mut sequence = loaded.sequences[0].clone();
            sequence.video_tracks.clear();
            sequence.audio = vec![clip.clone()];
            let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &loaded.media);
            let wire =
                crate::convert::premiere_to_tesseract(&sequence, &loaded.media, &ids, &mut reports)
                    .unwrap()
                    .to_json_value()
                    .unwrap();
            let keys = wire["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
                .as_array()
                .unwrap();
            let millis = keys
                .iter()
                .map(|key| key["layerTime"].as_i64().unwrap())
                .collect::<Vec<_>>();
            let edge = if rate < 0.0 {
                loaded.media[&clip.media]
                    .audio
                    .as_ref()
                    .unwrap()
                    .intrinsic_ticks
                    - edge_key
            } else {
                edge_key
            };
            let edge = (edge + TICKS_PER_MILLISECOND / 2) / TICKS_PER_MILLISECOND;
            assert!(millis.contains(&edge), "{rate}/{nearby}: {millis:?}");
            assert_eq!(
                reports
                    .iter()
                    .filter(|note| note.reason.contains("clocks are unverified"))
                    .count(),
                1
            );
        }
    }
}

#[test]
fn audio_clock_custom_short_incoming_reverse_level_key_blocks_enlargement() {
    use crate::schema::{PrAudioFade, PrFadeCurve};
    let mut project = audio_project(vec![("clock.wav", AudioChannels::Stereo, 0.5)]);
    let clip = &mut project.sequences[0].audio[0];
    clip.start_ticks = 0;
    clip.end_ticks = TICKS;
    clip.in_ticks = TICKS / 2;
    clip.out_ticks = clip.in_ticks + 2 * TICKS;
    clip.playback_rate = -2.0;
    clip.fade_out = None;
    clip.fade_in = Some(PrAudioFade {
        id: None,
        curve: PrFadeCurve::ConstantPower,
        duration_ticks: 16 * TICKS_PER_MILLISECOND,
    });
    // The 40ms key lies inside the proposed 66.667ms clamp on the 2x
    // source clock, but beyond the incorrect unit-clock clamp span.
    let key_source = clip.source_at(40 * TICKS_PER_MILLISECOND).unwrap();
    clip.volume_keys = Some(PrVolumeKeys {
        gain: 0.5,
        keys: vec![PrScalarKeyframe {
            source_ticks: key_source,
            value: 1.0,
            easing: PrKeyframeEasing::Linear,
        }],
    });
    let ranges = (
        clip.start_ticks,
        clip.end_ticks,
        clip.in_ticks,
        clip.out_ticks,
    );
    let xml = project_xml(&project)
        .unwrap()
        .replace(
            "<MatchName>Constant Power</MatchName>",
            "<MatchName>Custom Fade</MatchName>",
        )
        .replace(
            "</AudioTransitionTrackItem>",
            "<FadeShapeValue>-19</FadeShapeValue></AudioTransitionTrackItem>",
        );
    let (loaded, notes) = inspect_project_with_omissions(&xml, None).unwrap();
    let clip = &loaded.sequences[0].audio[0];
    assert_eq!(
        (
            clip.start_ticks,
            clip.end_ticks,
            clip.in_ticks,
            clip.out_ticks
        ),
        ranges
    );
    let fade = clip.fade_in.as_ref().unwrap();
    assert_eq!(fade.duration_ticks, 16 * TICKS_PER_MILLISECOND, "{notes:?}");
    assert_eq!(
        fade.curve,
        PrFadeCurve::Custom(crate::schema::CustomFadeShape::new(-19).unwrap())
    );
    let mut reports = Vec::new();
    let mut sequence = loaded.sequences[0].clone();
    sequence.video_tracks.clear();
    sequence.audio = vec![clip.clone()];
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &loaded.media);
    let wire = crate::convert::premiere_to_tesseract(&sequence, &loaded.media, &ids, &mut reports)
        .unwrap()
        .to_json_value()
        .unwrap();
    let keys = wire["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
        .as_array()
        .unwrap();
    let intrinsic = loaded.media[&clip.media]
        .audio
        .as_ref()
        .unwrap()
        .intrinsic_ticks;
    for source in [
        clip.in_ticks,
        clip.source_at(fade.duration_ticks).unwrap(),
        key_source,
    ] {
        let millis = (intrinsic - source + TICKS_PER_MILLISECOND / 2) / TICKS_PER_MILLISECOND;
        assert!(
            keys.iter().any(|key| key["layerTime"] == millis),
            "{millis}: {keys:?}"
        );
    }
    assert_eq!(
        reports
            .iter()
            .filter(|note| note.reason.contains("clocks are unverified"))
            .count(),
        1
    );
}
