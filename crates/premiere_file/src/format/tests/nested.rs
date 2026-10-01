//! Reader rules for nested-sequence placements (JRB-1979).

use crate::{
    format::inspect_project_with_omissions,
    schema::{
        MediaId, PrAnimatedProperty, PrAudioOccurrence, PrBlendMode, PrKeyframeEasing,
        PrNestOccurrence, PrSequence, PrVideoItem, MAX_NEST_DEPTH, MOTION_PARAMS_26_5, TICKS,
    },
    tests::support::{clip_of, nest_of, nested_sequence, project_document_with_media, sequence_of},
};

// Native XML records for these tests and for `format::reader::nested::tests`,
// appended to `one-clip.xml` (sequence `sequence-1`, "Main": one 5 s clip of
// the media source `7`).

pub(in crate::format) const ONE_CLIP_XML: &str =
    include_str!("../../../tests/fixtures/one-clip.xml");

/// One placement: its timeline range and the source time it shows first.
pub(in crate::format) struct Placement {
    pub(in crate::format) start: i64,
    pub(in crate::format) end: i64,
    pub(in crate::format) source_in: i64,
}

/// A 30 fps 1080p sequence with one video track holding `items`.
pub(in crate::format) fn sequence_records(
    guid: &str,
    name: &str,
    group: u32,
    items: &[u32],
) -> String {
    let references: String = items
        .iter()
        .map(|id| format!(r#"<TrackItem ObjectRef="{id}"/>"#))
        .collect();
    let chain = group + 1;
    format!(
        r#"<Sequence ObjectUID="{guid}"><Name>{name}</Name><TrackGroups><TrackGroup><Second ObjectRef="{group}"/></TrackGroup></TrackGroups></Sequence>
<VideoTrackGroup ObjectID="{group}"><TrackGroup><Tracks><Track ObjectURef="{guid}-track"/></Tracks><FrameRate>8467200000</FrameRate></TrackGroup><FrameRect>0,0,1920,1080</FrameRect><ComponentOwner><Components ObjectRef="{chain}"/></ComponentOwner></VideoTrackGroup>
<VideoComponentChain ObjectID="{chain}"><ComponentChain/></VideoComponentChain>
<VideoClipTrack ObjectUID="{guid}-track"><ClipTrack><Track><ID>1</ID></Track><ClipItems><TrackItems>{references}</TrackItems></ClipItems></ClipTrack></VideoClipTrack>
"#
    )
}

pub(in crate::format) fn sequence_source(id: u32, guid: &str) -> String {
    format!(
        r#"<VideoSequenceSource ObjectID="{id}"><SequenceSource><Sequence ObjectURef="{guid}"/></SequenceSource><OriginalDuration>{}</OriginalDuration></VideoSequenceSource>
"#,
        5 * TICKS
    )
}

/// The track item `id` and the three records after it; `source` is a
/// sequence source for a nest or the media source `7` of `one-clip.xml`.
pub(in crate::format) fn placement_records(id: u32, source: u32, placement: &Placement) -> String {
    let out = placement.source_in + placement.end - placement.start;
    let (chain, sub_clip, clip) = (id + 1, id + 2, id + 3);
    format!(
        r#"<VideoClipTrackItem ObjectID="{id}"><ClipTrackItem><ComponentOwner><Components ObjectRef="{chain}"/></ComponentOwner><TrackItem><Start>{}</Start><End>{}</End></TrackItem><SubClip ObjectRef="{sub_clip}"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
<VideoComponentChain ObjectID="{chain}"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>
<SubClip ObjectID="{sub_clip}"><Clip ObjectRef="{clip}"/><Name>Placement</Name></SubClip>
<VideoClip ObjectID="{clip}"><Clip><Source ObjectRef="{source}"/><InPoint>{}</InPoint><OutPoint>{out}</OutPoint></Clip></VideoClip>
"#,
        placement.start, placement.end, placement.source_in
    )
}

pub(in crate::format) fn with_records(xml: &str, records: &str) -> String {
    xml.replace("</PremiereData>", &format!("{records}</PremiereData>"))
}

/// `one-clip.xml` ("Main": one 5 s clip) placed by a top-level "Outer" sequence.
/// Nest track items are 110, 120, ...; the Main sequence source is 102.
fn outer_xml(placements: &[Placement]) -> String {
    let items: Vec<u32> = (0..placements.len() as u32)
        .map(|index| 110 + 10 * index)
        .collect();
    let mut records = sequence_records("outer", "Outer", 100, &items);
    records.push_str(&sequence_source(102, "sequence-1"));
    for (id, placement) in items.iter().zip(placements) {
        records.push_str(&placement_records(*id, 102, placement));
    }
    with_records(ONE_CLIP_XML, &records)
}

fn one_placement() -> String {
    outer_xml(&[Placement {
        start: 0,
        end: 2 * TICKS,
        source_in: TICKS,
    }])
}

/// The reason reported for Outer's only placement; Outer then has no content.
fn rejection(xml: &str) -> String {
    inspect_project_with_omissions(xml, Some("outer"))
        .unwrap_err()
        .to_string()
}

fn nest_depth(sequence: &PrSequence) -> usize {
    sequence
        .nest_occurrences()
        .map(|nest| 1 + nest_depth(&nest.sequence))
        .max()
        .unwrap_or(0)
}

#[test]
fn repeated_placements_read_independent_copies_that_share_media() {
    let xml = outer_xml(&[
        Placement {
            start: 0,
            end: 2 * TICKS,
            source_in: TICKS,
        },
        Placement {
            start: 3 * TICKS,
            end: 8 * TICKS,
            source_in: 0,
        },
    ]);
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.name(), "Outer");
    assert_eq!(outer.video_occurrences().count(), 0);
    assert_eq!(outer.end_ticks(), 8 * TICKS);
    let nests: Vec<&PrNestOccurrence> = outer.nest_occurrences().collect();
    assert_eq!(
        nests
            .iter()
            .map(|nest| (nest.timeline_ticks(), nest.in_ticks..nest.out_ticks))
            .collect::<Vec<_>>(),
        [
            (0..2 * TICKS, TICKS..3 * TICKS),
            (3 * TICKS..8 * TICKS, 0..5 * TICKS)
        ]
    );
    for nest in nests {
        assert_eq!(nest.sequence.id(), Some("sequence-1"));
        assert_eq!(nest.sequence.name(), "Main");
        assert_eq!(
            nest.sequence
                .video_occurrences()
                .map(|clip| (clip.timeline_ticks(), clip.media_id().as_str()))
                .collect::<Vec<_>>(),
            [(0..5 * TICKS, "Media:ObjectUID:media-1")]
        );
    }
    // Both copies refer to the one native media record.
    assert_eq!(
        project
            .media
            .keys()
            .map(MediaId::as_str)
            .collect::<Vec<_>>(),
        ["Media:ObjectUID:media-1"]
    );
}

/// Outer places Main at 0-5 s and a clip at 5-6 s; Main places Outer at 5-6 s.
fn cyclic_xml() -> String {
    let late = Placement {
        start: 5 * TICKS,
        end: 6 * TICKS,
        source_in: 0,
    };
    with_records(
        &outer_xml(&[Placement {
            start: 0,
            end: 5 * TICKS,
            source_in: 0,
        }])
        .replacen(
            r#"<TrackItem ObjectRef="110"/>"#,
            r#"<TrackItem ObjectRef="110"/><TrackItem ObjectRef="150"/>"#,
            1,
        )
        .replacen(
            r#"<TrackItem ObjectRef="3"/>"#,
            r#"<TrackItem ObjectRef="3"/><TrackItem ObjectRef="200"/>"#,
            1,
        ),
        &(placement_records(150, 7, &late)
            + &sequence_source(209, "outer")
            + &placement_records(200, 209, &late)),
    )
}

#[test]
fn a_placement_of_a_sequence_on_a_nesting_cycle_is_omitted() {
    let (project, omissions) =
        inspect_project_with_omissions(&cyclic_xml(), Some("outer")).unwrap();
    assert!(
        omissions.iter().any(|item| item.record == "110"
            && item.reason
                == "unsupported conversion: nested sequence sequence-1 is on a nesting cycle; cyclic nesting is not converted"),
        "{omissions:?}"
    );
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.video_occurrences().count(), 1);
    assert_eq!(outer.nest_occurrences().count(), 0);
}

#[test]
fn a_cycle_the_topology_missed_is_rejected_where_it_closes() {
    let xml = cyclic_xml();
    let mut omissions = Vec::new();
    let outer = crate::format::reader::read_sequence(
        &crate::format::Graph::parse(&xml).unwrap(),
        Some("outer"),
        &std::collections::BTreeSet::new(),
        &mut std::collections::BTreeMap::new(),
        &mut omissions,
    )
    .unwrap();
    assert!(
        omissions.iter().any(|item| item.record == "200"
            && item.reason
                == "unsupported conversion: nested sequence outer contains itself; cyclic nesting is not converted"),
        "{omissions:?}"
    );
    // Main keeps its media clip; only its placement back into Outer is gone.
    let nest = outer.nest_occurrences().next().unwrap();
    assert_eq!(nest.sequence.video_occurrences().count(), 1);
    assert_eq!(nest.sequence.nest_occurrences().count(), 0);
}

/// A sequence `guid` with a clip at 0-1 s and, when `next` is a sequence
/// source, a placement of it at 1-2 s. Its own source is `base + 2`.
fn chain_link(guid: &str, base: u32, next: Option<u32>) -> String {
    let mut items = vec![base + 10];
    let mut records = placement_records(
        base + 10,
        7,
        &Placement {
            start: 0,
            end: TICKS,
            source_in: 0,
        },
    );
    if let Some(source) = next {
        items.push(base + 20);
        records.push_str(&placement_records(
            base + 20,
            source,
            &Placement {
                start: TICKS,
                end: 2 * TICKS,
                source_in: 0,
            },
        ));
    }
    sequence_records(guid, guid, base, &items) + &sequence_source(base + 2, guid) + &records
}

#[test]
fn a_reused_inner_read_never_nests_deeper_than_its_placement_allows() {
    // s places t-1 ... t-7, seven levels below s. w-1 ... w-7 lead to s, so
    // that placement of s is the eighth level and may nest nothing.
    let mut records = chain_link("s", 3000, Some(3102));
    for index in 1..=7 {
        let next = (index < 7).then_some(3000 + 100 * (index + 1) + 2);
        records.push_str(&chain_link(&format!("t-{index}"), 3000 + 100 * index, next));
        let next = if index < 7 {
            4000 + 100 * (index + 1) + 2
        } else {
            3002
        };
        records.push_str(&chain_link(
            &format!("w-{index}"),
            4000 + 100 * index,
            Some(next),
        ));
    }
    // Root places s at 0-1 s and w-1 at 1-2 s; `order` is the read order.
    let root = |order: [u32; 2]| {
        let second = |start: i64| Placement {
            start: start * TICKS,
            end: (start + 1) * TICKS,
            source_in: 0,
        };
        let xml = with_records(
            ONE_CLIP_XML,
            &(sequence_records("root", "Root", 2000, &order)
                + &placement_records(2010, 3002, &second(0))
                + &placement_records(2020, 4102, &second(1))
                + &records),
        );
        let (project, omissions) = inspect_project_with_omissions(&xml, Some("root")).unwrap();
        assert!(
            omissions.iter().any(|item| item.record == "3020"
                && item.reason
                    == "unsupported conversion: nested sequence t-1 is deeper than 8 levels"),
            "{order:?}: {omissions:?}"
        );
        let sequence = project.single_sequence().unwrap();
        sequence
            .nest_occurrences()
            .map(|nest| {
                (
                    nest.sequence.name().to_owned(),
                    1 + nest_depth(&nest.sequence),
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = [("s".to_owned(), 8), ("w-1".to_owned(), 8)];
    assert_eq!(root([2010, 2020]), expected);
    assert_eq!(root([2020, 2010]), expected);
}

#[test]
fn every_nest_copy_is_kept_past_1024_expanded_layers() {
    // Four copies of Busy (255 clips + one group) reach exactly 1024 layers,
    // and a root media clip takes the timeline to 1025. The reader keeps
    // every copy: #4579 removed the expanded-layer limit that omitted the
    // fourth.
    let frame = crate::format::FrameRate::Fps30.ticks_per_frame();
    let items: Vec<u32> = (0..255).map(|index| 6010 + 10 * index).collect();
    let mut records =
        sequence_records("busy", "Busy", 6000, &items) + &sequence_source(6002, "busy");
    for (index, id) in (0_i64..).zip(&items) {
        records.push_str(&placement_records(
            *id,
            7,
            &Placement {
                start: index * frame,
                end: (index + 1) * frame,
                source_in: 0,
            },
        ));
    }
    for (second, id) in [(0, 110), (1, 120), (2, 130), (3, 140)] {
        records.push_str(&placement_records(
            id,
            6002,
            &Placement {
                start: second * TICKS,
                end: (second + 1) * TICKS,
                source_in: 0,
            },
        ));
    }
    let root = sequence_records("outer", "Outer", 100, &[110, 120, 130, 140]);
    let (project, omissions) = inspect_project_with_omissions(
        &with_records(ONE_CLIP_XML, &(records.clone() + &root)),
        Some("outer"),
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.nest_occurrences().count(), 4);
    assert_eq!(outer.expanded_occurrence_count(), 1024);

    records.push_str(&sequence_records(
        "outer",
        "Outer",
        100,
        &[110, 120, 130, 140, 150],
    ));
    records.push_str(&placement_records(
        150,
        7,
        &Placement {
            start: 4 * TICKS,
            end: 4 * TICKS + frame,
            source_in: 0,
        },
    ));
    let (project, omissions) =
        inspect_project_with_omissions(&with_records(ONE_CLIP_XML, &records), Some("outer"))
            .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let outer = project.single_sequence().unwrap();
    assert_eq!(
        outer
            .nest_occurrences()
            .map(PrNestOccurrence::timeline_ticks)
            .collect::<Vec<_>>(),
        [
            0..TICKS,
            TICKS..2 * TICKS,
            2 * TICKS..3 * TICKS,
            3 * TICKS..4 * TICKS
        ]
    );
    assert_eq!(outer.video_occurrences().count(), 1);
    assert_eq!(outer.expanded_occurrence_count(), 1025);
}

/// 255 nests and two media clips are 257 top-level placements: the model
/// validates them and the reader converts them all, since #4579 removed the
/// 256-placement limit.
#[test]
fn top_level_placements_past_256_all_convert() {
    let placements: Vec<_> = (0..255)
        .map(|second| Placement {
            start: second * TICKS,
            end: (second + 1) * TICKS,
            source_in: 0,
        })
        .collect();
    let xml = with_records(
        &outer_xml(&placements).replace(
            "<TrackItem ObjectRef=\"110\"/>",
            "<TrackItem ObjectRef=\"110\"/><TrackItem ObjectRef=\"4000\"/>",
        ),
        &placement_records(
            4000,
            7,
            &Placement {
                start: 255 * TICKS,
                end: 256 * TICKS,
                source_in: 0,
            },
        ),
    );
    let (mut project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let outer = &mut project.sequences[0];
    assert_eq!(outer.video_occurrences().count(), 1);
    assert_eq!(outer.nest_occurrences().count(), 255);
    assert_eq!(outer.expanded_occurrence_count(), 511);
    let mut clip = outer.video_tracks[0].clip(0).clone();
    clip.start_ticks = 256 * TICKS;
    clip.end_ticks = 257 * TICKS;
    outer.video_tracks[0].items.push(PrVideoItem::Media(clip));
    outer.timeline_end_ticks = outer.occurrence_end_ticks();
    outer.validate_timeline(&project.media).unwrap();
    let xml = with_records(
        &xml.replace(
            "<TrackItem ObjectRef=\"4000\"/>",
            "<TrackItem ObjectRef=\"4000\"/><TrackItem ObjectRef=\"4010\"/>",
        ),
        &placement_records(
            4010,
            7,
            &Placement {
                start: 256 * TICKS,
                end: 257 * TICKS,
                source_in: 0,
            },
        ),
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.video_occurrences().count(), 2);
    assert_eq!(outer.nest_occurrences().count(), 255);
}

#[test]
fn a_placement_past_the_end_of_its_nested_sequence_is_omitted() {
    // Main ends at 5 s; this placement shows inner 1-6 s.
    let error = rejection(&outer_xml(&[Placement {
        start: 0,
        end: 5 * TICKS,
        source_in: TICKS,
    }]));
    assert!(
        error.ends_with(&format!(
            "out point {} ticks is past the {} tick end of nested sequence \"Main\"; a placement longer than its nested sequence is not supported",
            6 * TICKS,
            5 * TICKS
        )),
        "{error}"
    );
}

#[test]
fn a_nest_with_another_canvas_is_invalid_in_the_model() {
    // The model rule that the reader and export share: a nest keeps no
    // viewport of its own, so its canvas must be the outer one.
    let (mut outer, media) = nested_sequence();
    let inner = &mut outer.video_tracks[1].nests[0].sequence;
    (inner.width, inner.height) = (1080, 1920);
    let error = outer.validate_timeline(&media).unwrap_err().to_string();
    assert!(
        error.ends_with(
            "nested sequence \"Inner\" canvas 1080x1920 differs from the outer 1920x1080 canvas"
        ),
        "{error}"
    );
}

#[test]
fn nesting_deeper_than_the_limit_is_rejected_at_the_first_excess_level() {
    let levels = MAX_NEST_DEPTH as u32 + 1;
    let mut records = String::new();
    for level in 0..=levels {
        let base = 1000 + 100 * level;
        let items: Vec<u32> = if level < levels {
            vec![base + 10, base + 20]
        } else {
            vec![base + 10]
        };
        records.push_str(&sequence_records(
            &format!("level-{level}"),
            &format!("Level {level}"),
            base,
            &items,
        ));
        records.push_str(&sequence_source(base + 2, &format!("level-{level}")));
        records.push_str(&placement_records(
            base + 10,
            7,
            &Placement {
                start: 0,
                end: TICKS,
                source_in: 0,
            },
        ));
        if level < levels {
            records.push_str(&placement_records(
                base + 20,
                base + 102,
                &Placement {
                    start: TICKS,
                    end: 2 * TICKS,
                    source_in: 0,
                },
            ));
        }
    }
    let xml = with_records(ONE_CLIP_XML, &records);
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("level-0")).unwrap();
    assert!(
        omissions.iter().any(|item| item
            .reason
            .contains(&format!("deeper than {MAX_NEST_DEPTH} levels"))),
        "{omissions:?}"
    );
    assert_eq!(
        nest_depth(project.single_sequence().unwrap()),
        MAX_NEST_DEPTH
    );
}

/// The 0-2 s variable-speed ramp records (TimeRemapping 146 and its Speed
/// parameter 147) of the pinned `feature_time_remap_variable_speed_strict`.
fn variable_speed_ramp() -> String {
    use std::io::Read;

    let mut xml = String::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../../../tests/fixtures/feature_time_remap_variable_speed_strict.prproj")
            [..],
    )
    .read_to_string(&mut xml)
    .unwrap();
    let start = xml.find(r#"<TimeRemapping ObjectID="146""#).unwrap();
    let end = start + xml[start..].find("</TimeComponentParam>").unwrap();
    format!("{}</TimeComponentParam>", &xml[start..end])
}

#[test]
fn a_nest_of_its_custom_canvas_size_reads_at_that_size() {
    // Every frame of both sequences, their placements and the source is portrait.
    let xml = one_placement().replace(
        "<FrameRect>0,0,1920,1080</FrameRect>",
        "<FrameRect>0,0,1080,1920</FrameRect>",
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.dimensions(), [1080, 1920]);
    let nests: Vec<_> = outer.nest_occurrences().collect();
    assert_eq!(nests.len(), 1);
    assert_eq!(nests[0].sequence.dimensions(), [1080, 1920]);
    assert_eq!(nests[0].sequence.video_occurrences().count(), 1);
    let document = project_document_with_media(outer, &project.media);
    assert_eq!(
        document["dimensions"],
        serde_json::json!({"width": 1080, "height": 1920})
    );
    let group = &document["composition"]["layers"][0];
    assert_eq!(group["type"], "Group");
    assert_eq!(group["layers"][0]["type"], "Video");
}

#[test]
fn nests_with_another_canvas_or_an_unsupported_clock_are_rejected() {
    // Main's 1-3 s at normal speed ends at Out 3 s; Out 5 s is twice as long.
    let long_window = one_placement().replace(
        &format!("<OutPoint>{}</OutPoint></Clip></VideoClip>\n", 3 * TICKS),
        &format!("<OutPoint>{}</OutPoint></Clip></VideoClip>\n", 5 * TICKS),
    );
    let speed = |xml: &str, clip: &str| {
        xml.replace(
            r#"<Clip><Source ObjectRef="102"/>"#,
            &format!(r#"<Clip>{clip}<Source ObjectRef="102"/>"#),
        )
    };
    for (xml, reason) in [
        (
            // The inner sequence and its clip on a portrait canvas.
            one_placement()
                .replacen(
                    r#"</TrackGroup><FrameRect>0,0,1920,1080</FrameRect><ComponentOwner><Components ObjectRef="2"/>"#,
                    r#"</TrackGroup><FrameRect>0,0,1080,1920</FrameRect><ComponentOwner><Components ObjectRef="2"/>"#,
                    1,
                )
                .replacen(
                    r#"<SubClip ObjectRef="5"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect>"#,
                    r#"<SubClip ObjectRef="5"/></ClipTrackItem><FrameRect>0,0,1080,1920</FrameRect>"#,
                    1,
                ),
            "nested sequence \"Main\" canvas 1080x1920 differs from the outer 1920x1080 canvas",
        ),
        (
            one_placement().replacen(
                "</Tracks><FrameRate>8467200000</FrameRate>",
                "</Tracks><FrameRate>10160640000</FrameRate>",
                1,
            ),
            "over a window as long as its placement; such a mixed-rate nest is not supported",
        ),
        (
            long_window.clone(),
            "VideoClip:113: In 254016000000 to Out 1270080000000 does not match PlaybackSpeed 1 over the 508032000000-tick placement",
        ),
        (
            speed(&one_placement(), "<PlaybackSpeed>2</PlaybackSpeed>"),
            "VideoClip:113: In 254016000000 to Out 762048000000 does not match PlaybackSpeed 2 over the 508032000000-tick placement",
        ),
        (
            speed(&one_placement(), "<PlayBackwards>true</PlayBackwards>"),
            "VideoClip:113: reverse playback of a nested sequence occurrence is not converted",
        ),
        (
            with_records(
                &one_placement().replace(
                    r#"<Clip><Source ObjectRef="102"/>"#,
                    r#"<Clip><TimeRemapping ObjectRef="146"/><Source ObjectRef="102"/>"#,
                ),
                &variable_speed_ramp(),
            ),
            "VideoClip:113: TimeRemapping on a nested sequence occurrence is not converted",
        ),
    ] {
        let error = rejection(&xml);
        assert!(error.contains(reason), "{reason}: {error}");
    }
    // The long window at the speed that plays it reads unchanged, off the
    // normal-speed path.
    let (project, omissions) = inspect_project_with_omissions(
        &speed(&long_window, "<PlaybackSpeed>2</PlaybackSpeed>"),
        Some("outer"),
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let [nest] = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .collect::<Vec<_>>()[..]
    else {
        panic!("one nest");
    };
    assert_eq!(
        (nest.timeline_ticks(), nest.in_ticks..nest.out_ticks),
        (0..2 * TICKS, TICKS..5 * TICKS)
    );
    assert!(nest.is_retimed());
}

#[test]
fn nest_placement_motion_reads_and_its_effects_are_rejected() {
    let nest_chain = r#"<VideoComponentChain ObjectID="111"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>"#;
    // An intrinsic Motion component with `position` and optional Rotation keys.
    let motion = |position: &str, rotation_keys: &str| {
        let mut params = String::new();
        for (id, name, value, point) in [
            (1, "Position", position, true),
            (2, "Scale", "100.", false),
            (3, "Scale Width", "100.", false),
            (4, " ", "true", false),
            (5, "Rotation", "0.", false),
            (6, "Anchor Point", "0.5:0.5", true),
            (7, "Anti-flicker Filter", "0.", false),
        ] {
            let (tag, initial) = if point {
                (
                    "PointComponentParam",
                    format!("-91445760000000000,{value},0,0,0,0,0,0,5,4,0,0,0,0"),
                )
            } else {
                (
                    "VideoComponentParam",
                    format!("-91445760000000000,{value},0,0,0,0,0,0"),
                )
            };
            let keys = if id == 5 { rotation_keys } else { "" };
            params.push_str(&format!("<{tag} ObjectID=\"{}\"><Name>{name}</Name><ParameterID>{id}</ParameterID><StartKeyframe>{initial}</StartKeyframe>{keys}</{tag}>", 300 + id));
        }
        let references: String = (1..=7)
            .map(|id| format!(r#"<Param Index="{}" ObjectRef="{}"/>"#, id - 1, 300 + id))
            .collect();
        format!(
            r#"<VideoComponentChain ObjectID="111"><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index="0" ObjectRef="300"/></Components></ComponentChain></VideoComponentChain><VideoFilterComponent ObjectID="300"><Component><Params>{references}</Params><DisplayName>Motion</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic></Component><MatchName>AE.ADBE Motion</MatchName></VideoFilterComponent>{params}"#
        )
    };
    let rotation_keys = format!("<Keyframes>0,0.,0,0,0,0,0,0;{TICKS},90.,0,0,0,0,0,0;</Keyframes>");
    let blur = r#"<VideoComponentChain ObjectID="111"><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index="0" ObjectRef="300"/></Components></ComponentChain></VideoComponentChain><VideoFilterComponent ObjectID="300"><Component><DisplayName>Gaussian Blur</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Gaussian Blur 2</MatchName></VideoFilterComponent>"#;
    // Static and keyed Motion read as the placement's, which its group takes.
    for (position, keys) in [("0.5:0.5", rotation_keys.as_str()), ("0.25:0.5", "")] {
        let (project, omissions) = inspect_project_with_omissions(
            &one_placement().replace(nest_chain, &motion(position, keys)),
            Some("outer"),
        )
        .unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let nest = project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .next()
            .unwrap();
        assert_eq!(
            (nest.transform.position, nest.animations.len()),
            if keys.is_empty() {
                ([0.25, 0.5], 0)
            } else {
                ([0.5, 0.5], 1)
            }
        );
    }
    for (xml, reason) in [
        (
            one_placement().replace(nest_chain, blur),
            "VideoClipTrackItem:110: effects on a nested sequence occurrence are not converted",
        ),
        (
            one_placement().replace(
                &format!("<OutPoint>{}</OutPoint></Clip></VideoClip>", 3 * TICKS),
                &format!(
                    "<OutPoint>{}</OutPoint></Clip><ScaleToFramePolicy>1</ScaleToFramePolicy></VideoClip>",
                    3 * TICKS
                ),
            ),
            "VideoClipTrackItem:110: Scale to Frame Size on a nested sequence occurrence is not converted",
        ),
        (
            one_placement().replace(
                nest_chain,
                &format!(
                    r#"<VideoComponentChain ObjectID="111"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index="0" ObjectRef="200"/></Components></ComponentChain></VideoComponentChain>{}{}"#,
                    super::mask::masked_opacity(200, 300),
                    super::mask::mask(300, true)
                ),
            ),
            "VideoClipTrackItem:110: an Opacity mask on a nested sequence occurrence is not converted",
        ),
        (
            one_placement().replace(
                nest_chain,
                &nest_chain.replace(
                    "<DefaultOpacity>true</DefaultOpacity>",
                    "<DefaultOpacity>false</DefaultOpacity>",
                ),
            ),
            "VideoComponentChain:111: nondefault opacity",
        ),
    ] {
        assert_ne!(xml, one_placement());
        let error = rejection(&xml);
        assert!(error.ends_with(reason), "{reason}: {error}");
    }
    // Chain `chain` with an intrinsic Opacity component (records from
    // `component`) with the Normal pair (18, 0), whose keys fade from 100% to 0%
    // over the first second.
    let opacity = |chain: u32, component: u32| {
        let [level, primary, legacy] = [1, 2, 3].map(|offset| component + offset);
        format!(
            r#"<VideoComponentChain ObjectID="{chain}"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index="0" ObjectRef="{component}"/></Components></ComponentChain></VideoComponentChain><VideoFilterComponent ObjectID="{component}"><Component><Params><Param Index="0" ObjectRef="{level}"/><Param Index="1" ObjectRef="{primary}"/><Param Index="2" ObjectRef="{legacy}"/></Params><DisplayName>Opacity</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic></Component><MatchName>AE.ADBE Opacity</MatchName></VideoFilterComponent><VideoComponentParam ObjectID="{level}" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3"><Name>Opacity</Name><ParameterID>1</ParameterID><ParameterControlType>2</ParameterControlType><LowerBound>0</LowerBound><UpperBound>100</UpperBound><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><Keyframes>0,100.,0,0,0,0,0,0;{TICKS},0.,0,0,0,0,0,0;</Keyframes></VideoComponentParam><VideoComponentParam ObjectID="{primary}" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8"><Name>Blend Mode</Name><ParameterID>2</ParameterID><ParameterControlType>10</ParameterControlType><LowerBound>0</LowerBound><UpperBound>26</UpperBound><StartKeyframe>-91445760000000000,18.,0,0,0,0,0,0</StartKeyframe></VideoComponentParam><VideoComponentParam ObjectID="{legacy}" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8"><Name>Blend Mode</Name><ParameterID>3</ParameterID><ParameterControlType>7</ParameterControlType><LowerBound>0</LowerBound><UpperBound>31</UpperBound><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>"#
        )
    };
    // Opacity keys omit only their own placement: a sibling placement of the
    // same inner sequence, whose clip has Opacity keys, still reads.
    let inner_chain = r#"<VideoComponentChain ObjectID="4"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>"#;
    let xml = outer_xml(&[
        Placement {
            start: 0,
            end: 2 * TICKS,
            source_in: TICKS,
        },
        Placement {
            start: 3 * TICKS,
            end: 8 * TICKS,
            source_in: 0,
        },
    ])
    .replace(inner_chain, &opacity(4, 400))
    .replace(nest_chain, &opacity(111, 410));
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    assert!(
        omissions.len() == 1 && omissions[0].record == "110"
            && omissions[0].reason == "unsupported conversion: VideoClipTrackItem:110: Opacity keyframes on a nested sequence occurrence are not converted",
        "{omissions:?}"
    );
    let nests: Vec<&PrNestOccurrence> = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .collect();
    assert_eq!(nests.len(), 1);
    assert_eq!(nests[0].timeline_ticks(), 3 * TICKS..8 * TICKS);
    let clip = nests[0].sequence.video_occurrences().next().unwrap();
    let [animation] = clip.animations.as_slice() else {
        panic!("{:?}", clip.animations);
    };
    assert_eq!(animation.property(), PrAnimatedProperty::Opacity);
    let keys: Vec<_> = animation
        .keys()
        .iter()
        .map(|key| (key.source_ticks, key.value))
        .collect();
    assert_eq!(keys, [(0, 100.0), (TICKS, 0.0)]);
}

#[test]
fn a_nest_placement_keeps_its_blend_pair() {
    // The nest's chain with an intrinsic Opacity at 100 and the pair
    // (`primary`, `legacy`), which the placement keeps whatever the pair.
    let nest_chain = r#"<VideoComponentChain ObjectID="111"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>"#;
    let blended = |primary: u8, legacy: u8| {
        format!(
            r#"<VideoComponentChain ObjectID="111"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index="0" ObjectRef="200"/></Components></ComponentChain></VideoComponentChain><VideoFilterComponent ObjectID="200"><Component><Params><Param Index="0" ObjectRef="201"/><Param Index="1" ObjectRef="202"/><Param Index="2" ObjectRef="203"/></Params><DisplayName>Opacity</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic></Component><MatchName>AE.ADBE Opacity</MatchName></VideoFilterComponent><VideoComponentParam ObjectID="201" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3"><Name>Opacity</Name><ParameterID>1</ParameterID><ParameterControlType>2</ParameterControlType><LowerBound>0</LowerBound><UpperBound>100</UpperBound><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe></VideoComponentParam><VideoComponentParam ObjectID="202" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8"><Name>Blend Mode</Name><ParameterID>2</ParameterID><ParameterControlType>10</ParameterControlType><LowerBound>0</LowerBound><UpperBound>26</UpperBound><StartKeyframe>-91445760000000000,{primary}.,0,0,0,0,0,0</StartKeyframe></VideoComponentParam><VideoComponentParam ObjectID="203" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8"><Name>Blend Mode</Name><ParameterID>3</ParameterID><ParameterControlType>7</ParameterControlType><LowerBound>0</LowerBound><UpperBound>31</UpperBound><StartKeyframe>-91445760000000000,{legacy}.,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>"#
        )
    };
    for (pair, expected) in [
        ((22, 10), PrBlendMode::Screen),
        (
            (6, 1),
            PrBlendMode::Unmeasured {
                primary: 6,
                legacy: 1,
            },
        ),
    ] {
        let xml = one_placement().replace(nest_chain, &blended(pair.0, pair.1));
        assert_ne!(xml, one_placement());
        let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
        assert!(omissions.is_empty(), "{pair:?}: {omissions:?}");
        let nests: Vec<_> = project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .map(|nest| (nest.blend_mode, nest.opacity, nest.timeline_ticks()))
            .collect();
        assert_eq!(nests, [(expected, 100.0, 0..2 * TICKS)], "{pair:?}");
    }
}

#[test]
fn a_disabled_nest_or_one_on_an_output_off_track_reads_as_disabled() {
    // Clip Enable and track output hide a nest as they hide media (JRB-1966).
    let disabled = one_placement().replace(
        r#"<SubClip ObjectRef="112"/></ClipTrackItem>"#,
        r#"<SubClip ObjectRef="112"/><IsMuted>true</IsMuted></ClipTrackItem>"#,
    );
    let muted_track = one_placement().replace(
        r#"<VideoClipTrack ObjectUID="outer-track"><ClipTrack><Track><ID>1</ID></Track>"#,
        r#"<VideoClipTrack ObjectUID="outer-track"><ClipTrack><Track><ID>1</ID><IsMuted>true</IsMuted></Track>"#,
    );
    for (xml, enabled) in [
        (one_placement(), true),
        (disabled, false),
        (muted_track, false),
    ] {
        let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let nest = project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .next()
            .unwrap();
        assert_eq!(
            (nest.enabled, nest.sequence.video_occurrences().count()),
            (enabled, 1)
        );
    }
}

#[test]
fn a_nest_master_clip_must_play_the_same_sequence_source() {
    let with_master = |master: &str| {
        with_records(
            &one_placement().replace(
                r#"<SubClip ObjectID="112"><Clip ObjectRef="113"/>"#,
                r#"<SubClip ObjectID="112"><Clip ObjectRef="113"/><MasterClip ObjectURef="master"/>"#,
            ),
            master,
        )
    };
    // A sequence master clip holds an audio and a video clip of that sequence.
    let sequence_master = r#"<MasterClip ObjectUID="master"><Clips><Clip Index="0" ObjectRef="401"/><Clip Index="1" ObjectRef="402"/></Clips><Name>Main</Name></MasterClip>
<AudioClip ObjectID="401"><Clip><Source ObjectRef="403"/></Clip></AudioClip>
<VideoClip ObjectID="402"><Clip><Source ObjectRef="102"/><InUse>false</InUse></Clip></VideoClip>
<AudioSequenceSource ObjectID="403"><SequenceSource><Sequence ObjectURef="sequence-1"/></SequenceSource><OriginalDuration>1270080000000</OriginalDuration></AudioSequenceSource>
"#;
    let (project, omissions) =
        inspect_project_with_omissions(&with_master(sequence_master), Some("outer")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .count(),
        1
    );
    // Master marks annotate the source, as for media (JRB-1953): the placement
    // keeps its own range. Adobe-saved sequence masters mostly mark only Out.
    let marked = sequence_master.replace(
        r#"<Source ObjectRef="102"/><InUse>"#,
        &format!(
            r#"<Source ObjectRef="102"/><OutPoint>{}</OutPoint><InUse>"#,
            2 * TICKS
        ),
    );
    assert_ne!(marked, sequence_master);
    let (project, omissions) =
        inspect_project_with_omissions(&with_master(&marked), Some("outer")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let nest = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert_eq!((nest.in_ticks, nest.out_ticks), (TICKS, 3 * TICKS));
    // The XML-edited nest of `feature_nested_second_sequence_strict` keeps a
    // media master clip.
    let media_master =
        sequence_master.replace(r#"<Source ObjectRef="102"/>"#, r#"<Source ObjectRef="7"/>"#);
    let error = rejection(&with_master(&media_master));
    assert!(error.contains("source identity mismatch"), "{error}");
}

#[test]
fn a_nest_without_an_audio_item_is_silent_and_keeps_its_picture() {
    use std::io::Read;

    // Structural derivative of the pinned linked-A/V source, not an Adobe-authored nest.
    let mut xml = String::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../../../tests/fixtures/feature_linked_av_strict.prproj")[..],
    )
    .read_to_string(&mut xml)
    .unwrap();
    let mut records = sequence_records("outer", "Outer", 10000, &[10010, 10020]);
    records.push_str(&sequence_source(
        10002,
        "80acdd81-0a96-4677-b17f-b2ffe2dff738",
    ));
    for (id, start) in [(10010, 0), (10020, 5 * TICKS)] {
        records.push_str(&placement_records(
            id,
            10002,
            &Placement {
                start,
                end: start + 5 * TICKS,
                source_in: 0,
            },
        ));
    }
    let (project, omissions) =
        inspect_project_with_omissions(&with_records(&xml, &records), Some("outer")).unwrap();
    let outer = project.single_sequence().unwrap();
    let nests: Vec<_> = outer.nest_occurrences().collect();
    assert_eq!(nests.len(), 2);
    for nest in nests {
        assert_eq!(nest.sequence.video_occurrences().count(), 1);
        assert!(nest.sequence.audio.is_empty());
    }
    // Without an audio item a nest plays none of its inner sound.
    assert!(
        !omissions
            .iter()
            .any(|item| item.record.starts_with("VideoClipTrackItem:100")),
        "{omissions:?}"
    );
    let mut mapped_omissions = Vec::new();
    let document = crate::convert::premiere_to_tesseract(
        outer,
        &project.media,
        &crate::tesseract_output::asset_ids_in_order(outer, &project.media),
        &mut mapped_omissions,
    )
    .unwrap();
    assert!(mapped_omissions.is_empty(), "{mapped_omissions:?}");
    for layer in document.composition().layers() {
        if let fx_schema::LayerData::Group(group) = layer.data() {
            assert!(
                matches!(group.layers.as_slice(), [child] if matches!(child.data(), fx_schema::LayerData::Video(video) if video.volume == Some(fx_schema::LinearGain::ZERO)))
            );
        }
    }
}

#[test]
fn an_inner_sound_is_reported_once_by_its_own_read() {
    let audio = r#"<AudioTrackGroup ObjectID="50"><TrackGroup><Tracks><Track ObjectRef="51"/></Tracks></TrackGroup></AudioTrackGroup>
<AudioClipTrack ObjectID="51"><ClipTrack><ClipItems><TrackItems><TrackItem ObjectRef="52"/></TrackItems></ClipItems></ClipTrack></AudioClipTrack>
<AudioClipTrackItem ObjectID="52"><ClipTrackItem><SubClip ObjectRef="53"/></ClipTrackItem></AudioClipTrackItem>
<SubClip ObjectID="53"><Clip ObjectRef="54"/></SubClip>
<AudioClip ObjectID="54"><Clip><Source ObjectRef="55"/></Clip></AudioClip>
<AudioMediaSource ObjectID="55"/>
"#;
    let placements = [
        Placement {
            start: 0,
            end: 2 * TICKS,
            source_in: TICKS,
        },
        Placement {
            start: 3 * TICKS,
            end: 8 * TICKS,
            source_in: 0,
        },
    ];
    // Main gains an audio track group whose one track holds an item.
    let xml = with_records(
        &outer_xml(&placements).replacen(
            r#"<TrackGroup><Second ObjectRef="1"/></TrackGroup>"#,
            r#"<TrackGroup><Second ObjectRef="1"/></TrackGroup><TrackGroup><Second ObjectRef="50"/></TrackGroup>"#,
            1,
        ),
        audio,
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .count(),
        2
    );
    // The inner read reports its broken sound once; its placements add no report.
    assert_eq!(
        omissions
            .iter()
            .filter(|item| item.record == "AudioClipTrack:51")
            .count(),
        1,
        "{omissions:?}"
    );
    assert!(
        !omissions
            .iter()
            .any(|item| item.record.starts_with("VideoClipTrackItem:1")),
        "{omissions:?}"
    );
}

/// The model counts every inline copy but no longer bounds them: four copies
/// of Busy and one more clip, 1025 expanded layers, validate once the
/// timeline ends after its last clip. The end check still applies: #4579
/// removed only the 1024-layer bound.
#[test]
fn inline_copies_past_1024_expanded_layers_validate() {
    let (_, media) = nested_sequence();
    let busy = sequence_of(
        "Busy",
        vec![crate::schema::PrVideoTrack::media((0..255).map(|frame| {
            let frame_ticks = crate::format::FrameRate::Fps30.ticks_per_frame();
            clip_of(
                "timecoded",
                frame * frame_ticks..(frame + 1) * frame_ticks,
                0,
            )
        }))],
    );
    busy.validate_timeline(&media).unwrap();
    let mut outer = sequence_of(
        "Four copies",
        vec![crate::schema::PrVideoTrack {
            transitions: Vec::new(),
            items: Vec::new(),
            nests: (0..4)
                .map(|second| nest_of(busy.clone(), second * TICKS..(second + 1) * TICKS, 0))
                .collect(),
        }],
    );
    assert_eq!(outer.expanded_occurrence_count(), 1024);
    outer.validate_timeline(&media).unwrap();
    outer.video_tracks[0].items.push(PrVideoItem::Media(clip_of(
        "timecoded",
        4 * TICKS..5 * TICKS,
        0,
    )));
    assert_eq!(outer.expanded_occurrence_count(), 1025);
    let error = outer.validate_timeline(&media).unwrap_err().to_string();
    assert!(
        error.contains("timeline end 4000 ms precedes the last media occurrence end 5000 ms"),
        "{error}"
    );
    outer.timeline_end_ticks = outer.occurrence_end_ticks();
    outer.validate_timeline(&media).unwrap();
}

#[test]
fn nested_placements_share_their_track_order_and_frame_grid() {
    let (outer, media) = nested_sequence();
    outer.validate_timeline(&media).unwrap();
    let mut overlapping = outer.clone();
    let nest = overlapping.video_tracks[1].nests.remove(1);
    overlapping.video_tracks[0].nests.push(nest);
    let error = overlapping
        .validate_timeline(&media)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("must not overlap other placements on the same track"),
        "{error}"
    );
    let mut off_frame = outer;
    off_frame.video_tracks[1].nests[0].in_ticks += 1;
    off_frame.video_tracks[1].nests[0].out_ticks += 1;
    let error = off_frame.validate_timeline(&media).unwrap_err().to_string();
    assert!(
        error.contains("nested sequence in point must align"),
        "{error}"
    );
}

#[test]
fn a_nest_overlapping_media_on_its_track_is_omitted() {
    let xml = with_records(
        &outer_xml(&[Placement {
            start: 0,
            end: 2 * TICKS,
            source_in: 0,
        }])
        .replace(
            r#"<TrackItem ObjectRef="110"/>"#,
            r#"<TrackItem ObjectRef="110"/><TrackItem ObjectRef="150"/>"#,
        ),
        &placement_records(
            150,
            7,
            &Placement {
                start: TICKS,
                end: 3 * TICKS,
                source_in: 0,
            },
        ),
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.video_occurrences().count(), 1);
    assert_eq!(outer.nest_occurrences().count(), 0);
    assert!(
        omissions
            .iter()
            .any(|item| item.reason.contains("overlaps another occurrence")),
        "{omissions:?}"
    );
}

#[test]
fn linked_transitions_on_a_nest_are_reported_without_track_membership() {
    let xml = with_records(
        &one_placement().replace(
            r#"<SubClip ObjectRef="112"/>"#,
            r#"<SubClip ObjectRef="112"/><HeadTransition ObjectRef="300"/>"#,
        ),
        r#"<VideoTransitionTrackItem ObjectID="300"/>"#,
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .count(),
        1
    );
    assert!(
        omissions.iter().any(|item| {
            item.scope == crate::OmissionScope::Feature
                && item.record == "VideoTransitionTrackItem:300"
                && item.reason
                    == "clip-linked video transition is missing from TransitionItems; transition not converted"
        }),
        "{omissions:?}"
    );
}

#[test]
fn writer_rejects_transitions_inside_nested_sequences() {
    let (mut outer, mut media) = nested_sequence();
    outer.video_tracks[1].nests[0].sequence.video_tracks[0]
        .transitions
        .push(crate::schema::PrVideoTransition {
            id: "nested-transition".into(),
            kind: crate::schema::PrVideoTransitionKind::CrossDissolve,
            start_ticks: 0,
            cut_ticks: 0,
            end_ticks: TICKS,
            outgoing_clip: None,
            incoming_clip: Some("9".into()),
        });
    for source in media.values_mut() {
        source.relative_path = Some(format!("./media/{}", source.name));
        source.absolute_paths = vec![(
            crate::schema::records::MediaPathField::FilePath,
            format!("/media/{}", source.name).into(),
        )];
    }
    let project = crate::format::PrProjectFile::from_sequences(vec![outer], media);
    let error = crate::format::PremiereProjectXml::new(&project).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("writer cannot encode native video transitions without flattening them"),
        "{error}"
    );
}

#[test]
fn writer_encodes_graphics_inside_nested_sequences() {
    // No native nested graphic is measured: structural round trip only.
    let (mut outer, mut media) = nested_sequence();
    outer.video_tracks[1].nests[0]
        .sequence
        .video_tracks
        .push(crate::schema::PrVideoTrack {
            items: vec![crate::schema::PrVideoItem::Graphic(
                crate::tests::support::text_graphic(),
            )],
            nests: Vec::new(),
            transitions: Vec::new(),
        });
    for source in media.values_mut() {
        source.relative_path = Some(format!("./media/{}", source.name));
        source.absolute_paths = vec![(
            crate::schema::records::MediaPathField::FilePath,
            format!("/media/{}", source.name).into(),
        )];
        source.video.as_mut().unwrap().kind = crate::schema::PrMediaKind::Video {
            codec: Some(crate::schema::VideoCodec::H264),
            hdr_profile: None,
        };
    }
    let project = crate::format::PrProjectFile::from_sequences(vec![outer], media);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.prproj");
    crate::format::PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reread, _) = crate::format::PrProjectFile::load(&path).unwrap();
    let nest = reread
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .find(|nest| {
            nest.sequence
                .video_items()
                .any(|item| item.graphic().is_some())
        })
        .expect("the nest keeps its graphic");
    assert_eq!(
        nest.sequence
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .count(),
        1
    );
}

/// `xml` with `edit` applied to the text of the record that opens with
/// `open`, up to the first `close` after it.
fn edit_record(mut xml: String, open: &str, close: &str, edit: impl Fn(&str) -> String) -> String {
    let start = xml.find(open).unwrap();
    let end = start + xml[start..].find(close).unwrap() + close.len();
    let record = edit(&xml[start..end]);
    xml.replace_range(start..end, &record);
    xml
}

/// The pinned images/nests fixture's XML, whose nest N (video item 116) has
/// the audio item 115 over the same ranges, with `edit` applied to the text of
/// record 115.
fn images_nests_with_sound(edit: impl Fn(&str) -> String) -> String {
    use std::io::Read;
    let mut xml = String::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../../../tests/fixtures/feature_images_nests_26_5.prproj")[..],
    )
    .read_to_string(&mut xml)
    .unwrap();
    edit_record(xml, SOUND_ITEM, "</AudioClipTrackItem>", edit)
}

const IMAGES_NESTS_SEQUENCE: &str = "f3c651e6-0302-4499-b6f5-814b7b22c207";
const SOUND_ITEM: &str = r#"<AudioClipTrackItem ObjectID="115""#;
const VIDEO_ITEM: &str = r#"<VideoClipTrackItem ObjectID="116""#;
/// Item 115's clip, which holds its In/Out.
const SOUND_CLIP: &str = r#"<AudioClip ObjectID="178""#;
/// The root sequence's second audio track, empty in the fixture.
const SECOND_AUDIO_TRACK: &str =
    r#"<AudioClipTrack ObjectUID="5b8321fd-17ff-40c1-bfdf-cec174272d14""#;

/// A clip track item's record with its clip Enable off.
fn disabled(record: &str) -> String {
    record.replacen(
        "</ClipTrackItem>",
        "<IsMuted>true</IsMuted></ClipTrackItem>",
        1,
    )
}

/// A copy of item 115 under ObjectID 9115, with `edit` applied, as the one
/// item of the root's second audio track, so that two items on two tracks
/// play N's placement. The copy refers to 115's clip and chain records.
fn with_sound_item_on_the_second_track(xml: String, edit: impl Fn(&str) -> String) -> String {
    let xml = edit_record(xml, SOUND_ITEM, "</AudioClipTrackItem>", |record| {
        record.to_owned() + &edit(&record.replacen(r#"ObjectID="115""#, r#"ObjectID="9115""#, 1))
    });
    edit_record(xml, SECOND_AUDIO_TRACK, "</AudioClipTrack>", |track| {
        track.replacen(
            r#"<ClipItems Version="3">"#,
            r#"<ClipItems Version="3"><TrackItems Version="1"><TrackItem Index="0" ObjectRef="9115"/></TrackItems>"#,
            1,
        )
    })
}

/// N's video item 116 off its track; the reader does not read the link 107.
fn without_video_item(xml: String) -> String {
    let item = "<TrackItem Index=\"5\" ObjectRef=\"116\"/>";
    assert_eq!(xml.matches(item).count(), 1);
    xml.replacen(item, "", 1)
}

/// N's audio item 115 off its track, which is left empty as Premiere saves an
/// empty track.
fn without_sound_item(xml: String) -> String {
    let item = "<TrackItems Version=\"1\">\n\t\t\t\t\t<TrackItem Index=\"0\" ObjectRef=\"115\"/>\n\t\t\t\t</TrackItems>";
    assert_eq!(xml.matches(item).count(), 1);
    xml.replacen(item, "", 1)
}

/// Without its video I1 (Inner's V1 item 111), N holds only I4's sound.
fn without_inner_picture(xml: String) -> String {
    let item = "<TrackItems Version=\"1\">\n\t\t\t\t\t<TrackItem Index=\"0\" ObjectRef=\"111\"/>\n\t\t\t\t</TrackItems>";
    assert_eq!(xml.matches(item).count(), 1);
    xml.replacen(item, "", 1)
}

/// N's video item 116 with an explicit Opacity that names two masks, as the
/// Mini Glitch Pack nests 1199 and 1214 do: how Premiere combines them is
/// unverified, so no conversion of N's picture is safe.
fn with_two_masks(xml: String) -> String {
    let chain = r#"<VideoComponentChain ObjectID="137" ClassID="0970e08a-f58f-4108-b29a-1a717b8e12e2" Version="3"><DefaultMotion>true</DefaultMotion><DefaultMotionComponentID>1</DefaultMotionComponentID><ComponentChain Version="3"><Components Version="1"><Component Index="0" ObjectRef="900"/></Components></ComponentChain></VideoComponentChain>"#;
    let opacity = super::mask::masked_opacity(900, 910).replacen(
        r#"<SubComponent Index="0" ObjectRef="910"/>"#,
        r#"<SubComponent Index="0" ObjectRef="910"/><SubComponent Index="1" ObjectRef="930"/>"#,
        1,
    );
    let xml = edit_record(
        xml,
        r#"<VideoComponentChain ObjectID="137""#,
        "</VideoComponentChain>",
        |_| chain.to_owned(),
    );
    with_records(
        &xml,
        &(opacity + &super::mask::mask(910, true) + &super::mask::mask(930, true)),
    )
}

/// Item 115 moved one second earlier, to 9-13 s: same duration and In/Out.
fn moved_sound_item(xml: String) -> String {
    edit_record(xml, SOUND_ITEM, "</AudioClipTrackItem>", |record| {
        record
            .replacen(
                "<Start>2540160000000</Start>",
                "<Start>2286144000000</Start>",
                1,
            )
            .replacen("<End>3556224000000</End>", "<End>3302208000000</End>", 1)
    })
}

/// Item 115 at In 1 s and Start 11 s: Out 4 s and End 14 s stay, so it is not
/// N's range, and it plays Inner 1-4 s at 11-14 s.
fn trimmed_sound_item(xml: String) -> String {
    let xml = edit_record(xml, SOUND_ITEM, "</AudioClipTrackItem>", |record| {
        record.replacen(
            "<Start>2540160000000</Start>",
            "<Start>2794176000000</Start>",
            1,
        )
    });
    edit_record(xml, SOUND_CLIP, "</AudioClip>", |record| {
        record.replacen("<InPoint>0</InPoint>", "<InPoint>254016000000</InPoint>", 1)
    })
}

/// Item 115's audio track muted.
fn with_sound_track_muted(xml: String) -> String {
    let track = r#"<AudioClipTrack ObjectUID="2a9b36be-df83-406c-bb40-7cbf2578cee3""#;
    edit_record(xml, track, "</AudioClipTrack>", |record| {
        record.replacen("</Track>", "<IsMuted>true</IsMuted></Track>", 1)
    })
}

/// Converts `xml`, an edit of the images/nests fixture, and checks N's kept
/// group, as its Enable and the sounds it carries; the sounds that play
/// alone, in order, each as its start and source start in seconds and its
/// volume (I4 plays the linked-A/V source over Inner 0-4 s, so each plays to
/// source 4 s); and the reason that omits N's picture, the only report on
/// N's items.
fn assert_nest_and_alone(
    name: &str,
    xml: &str,
    group: Option<(bool, usize)>,
    alone: &[(i64, i64, f64)],
    picture: Option<&str>,
) {
    let (project, omissions) =
        inspect_project_with_omissions(xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
    let sequence = project.single_sequence().unwrap();
    let groups: Vec<_> = sequence
        .nest_occurrences()
        .map(|nest| {
            // A kept N shows its video I1.
            assert_eq!(nest.sequence.video_occurrences().count(), 1, "{name}");
            (nest.enabled, nest.sequence.audio.len())
        })
        .collect();
    assert_eq!(groups, Vec::from_iter(group), "{name}");
    let sounds: Vec<_> = sequence
        .audio
        .iter()
        .map(|clip| {
            assert!(clip.volume_keys.is_none(), "{name}");
            (
                project.media[&clip.media].name(),
                [
                    clip.start_ticks,
                    clip.end_ticks,
                    clip.in_ticks,
                    clip.out_ticks,
                ],
                clip.volume.as_f64(),
            )
        })
        .collect();
    assert_eq!(sounds.len(), alone.len(), "{name}: {sounds:?}");
    for ((media, ticks, volume), &(start, source, expected_volume)) in sounds.iter().zip(alone) {
        let expected_ticks = [start, start + 4 - source, source, 4].map(|second| second * TICKS);
        assert_eq!(
            (media, ticks),
            (&"feature_linked_av_source.mp4", &expected_ticks),
            "{name}"
        );
        // Premiere stores the Level as 0.089125096797943115, 3e-9 off 10^(-21/20).
        assert!((volume - expected_volume).abs() < 1e-7, "{name}: {volume}");
    }
    // The items convert, and only a picture that cannot convert is reported
    // (the copy 9115 also ends with 115).
    let reported: Vec<_> = omissions
        .iter()
        .filter(|item| {
            item.scope == crate::OmissionScope::Occurrence
                && ["115", "116"].iter().any(|id| item.record.ends_with(id))
        })
        .map(|item| item.reason.as_str())
        .collect();
    assert_eq!(
        reported,
        Vec::from_iter(picture.map(|reason| format!("unsupported conversion: {reason}"))),
        "{name}"
    );
}

const TWO_MASKS: &str =
    "VideoFilterComponent:900: 2 masks on Opacity are not converted; how Premiere combines them is unverified";

/// Each row edits the fixture around nest N (video item 116 and audio item
/// 115 over 10-14 s from Inner 0 s, where Inner's I4 plays the linked-A/V
/// sound at 0-4 s: G6/G7) and expects N's group, as its Enable and the
/// sounds it carries, the sound that plays alone, as its start and source
/// start in seconds and its volume, and the reason that omits N's picture.
/// I4 plays exactly once: in N's group, or alone over the item's own ranges,
/// at the item's gain unless its Enable or its track mutes it.
#[test]
fn an_audio_item_that_no_group_carries_plays_its_sound_alone() {
    type Edit = fn(String) -> String;
    // N's kept group, as its Enable and the sounds it carries.
    type Group = Option<(bool, usize)>;
    // The sound that plays alone, as its start and source start in seconds
    // and its volume.
    type Alone = Option<(i64, i64, f64)>;
    let i4 = 10f64.powf(-6.0 / 20.0);
    let two_masks = TWO_MASKS;
    let rows: [(&str, Edit, Group, Alone, Option<&str>); 10] = [
        (
            "combined picture and sound",
            |xml| xml,
            Some((true, 1)),
            None,
            None,
        ),
        (
            "mask-unsafe picture",
            with_two_masks,
            None,
            Some((10, 0, i4)),
            Some(two_masks),
        ),
        (
            "mask-unsafe picture, item disabled",
            |xml| {
                with_two_masks(edit_record(
                    xml,
                    SOUND_ITEM,
                    "</AudioClipTrackItem>",
                    disabled,
                ))
            },
            None,
            Some((10, 0, 0.0)),
            Some(two_masks),
        ),
        (
            "mask-unsafe picture, track muted",
            |xml| with_sound_track_muted(with_two_masks(xml)),
            None,
            Some((10, 0, 0.0)),
            Some(two_masks),
        ),
        (
            "hidden picture",
            |xml| edit_record(xml, VIDEO_ITEM, "</VideoClipTrackItem>", disabled),
            Some((false, 0)),
            Some((10, 0, i4)),
            None,
        ),
        (
            "disabled sound",
            |xml| edit_record(xml, SOUND_ITEM, "</AudioClipTrackItem>", disabled),
            Some((true, 0)),
            Some((10, 0, 0.0)),
            None,
        ),
        (
            "another range",
            moved_sound_item,
            Some((true, 0)),
            Some((9, 0, i4)),
            None,
        ),
        (
            "trimmed item",
            trimmed_sound_item,
            Some((true, 0)),
            Some((11, 1, i4)),
            None,
        ),
        (
            "no video item",
            without_video_item,
            None,
            Some((10, 0, i4)),
            None,
        ),
        (
            "hidden picture of sound alone",
            // N shows nothing, so it is dropped unreported: its sound plays.
            |xml| {
                edit_record(
                    without_inner_picture(xml),
                    VIDEO_ITEM,
                    "</VideoClipTrackItem>",
                    disabled,
                )
            },
            None,
            Some((10, 0, i4)),
            None,
        ),
    ];
    for (name, edit, nest, alone, picture) in rows {
        let xml = edit(images_nests_with_sound(str::to_owned));
        assert_nest_and_alone(name, &xml, nest, alone.as_slice(), picture);
    }
}

/// Item 115 and a copy of it on the root's second audio track both play N's
/// placement. Each plays alone, over its own ranges and at its own gain,
/// Enable and track mute, and N keeps at most its picture, silent: the
/// sound is heard once per item, as Premiere plays each audio clip of a
/// timeline (inferred: no Premiere save has two). The copy's Volume is I4's
/// -6 dB (chain 121), so it plays at a quarter.
#[test]
fn each_audio_item_of_a_nest_with_several_plays_alone() {
    type Edit = fn(String) -> String;
    fn at_i4(record: &str) -> String {
        record.replacen(
            r#"<Components ObjectRef="135"/>"#,
            r#"<Components ObjectRef="121"/>"#,
            1,
        )
    }
    fn second_at_i4(xml: String) -> String {
        with_sound_item_on_the_second_track(xml, at_i4)
    }
    // N's kept group, as its Enable and the sounds it carries.
    type Group = Option<(bool, usize)>;
    // The volumes of the two sounds that play alone, 115's and then 9115's,
    // each at 10-14 s from source 0-4 s.
    type Volumes = [f64; 2];
    let i4 = 10f64.powf(-6.0 / 20.0);
    let rows: [(&str, Edit, Group, Volumes, Option<&str>); 6] = [
        (
            "picture kept",
            second_at_i4,
            Some((true, 0)),
            [i4, i4 * i4],
            None,
        ),
        (
            "picture kept, second item disabled",
            |xml| with_sound_item_on_the_second_track(xml, disabled),
            Some((true, 0)),
            [i4, 0.0],
            None,
        ),
        (
            "picture kept, second track muted",
            |xml| {
                edit_record(
                    second_at_i4(xml),
                    SECOND_AUDIO_TRACK,
                    "</AudioClipTrack>",
                    |record| record.replacen("</Track>", "<IsMuted>true</IsMuted></Track>", 1),
                )
            },
            Some((true, 0)),
            [i4, 0.0],
            None,
        ),
        (
            "picture rejected",
            |xml| second_at_i4(with_two_masks(xml)),
            None,
            [i4, i4 * i4],
            Some(TWO_MASKS),
        ),
        (
            "picture rejected, second item disabled",
            |xml| with_sound_item_on_the_second_track(with_two_masks(xml), disabled),
            None,
            [i4, 0.0],
            Some(TWO_MASKS),
        ),
        (
            "nest of sound alone",
            // N shows nothing, so it is dropped unreported: both items play.
            |xml| second_at_i4(without_inner_picture(xml)),
            None,
            [i4, i4 * i4],
            None,
        ),
    ];
    for (name, edit, nest, [first, second], picture) in rows {
        let xml = edit(images_nests_with_sound(str::to_owned));
        assert_nest_and_alone(
            name,
            &xml,
            nest,
            &[(10, 0, first), (10, 0, second)],
            picture,
        );
    }
}

/// Without its video item, N's audio item 115 and 250 copies of it, 4 s
/// apart after it, each play Inner alone. With the root's six video items
/// that is 257 timed items; no count limit omits any of them, so each item's
/// sound is kept, in start order.
#[test]
fn every_sound_that_plays_alone_is_kept() {
    let xml = without_video_item(images_nests_with_sound(str::to_owned));
    let start = xml.find(SOUND_ITEM).unwrap();
    let end = start + xml[start..].find("</AudioClipTrackItem>").unwrap();
    let item = &xml[start..end + "</AudioClipTrackItem>".len()];
    let (mut copies, mut references) = (String::new(), String::new());
    for copy in 1..=250_i64 {
        let [from, to] = [10 + 4 * copy, 14 + 4 * copy].map(|second| second * TICKS);
        copies.push_str(
            &item
                .replacen(
                    r#"ObjectID="115""#,
                    &format!(r#"ObjectID="{}""#, 9000 + copy),
                    1,
                )
                .replacen(
                    "<Start>2540160000000</Start>",
                    &format!("<Start>{from}</Start>"),
                    1,
                )
                .replacen("<End>3556224000000</End>", &format!("<End>{to}</End>"), 1),
        );
        references.push_str(&format!(
            r#"<TrackItem Index="{copy}" ObjectRef="{}"/>"#,
            9000 + copy
        ));
    }
    let listed = "<TrackItem Index=\"0\" ObjectRef=\"115\"/>\n\t\t\t\t</TrackItems>";
    assert_eq!(xml.matches(listed).count(), 1);
    let xml = with_records(
        &xml.replacen(
            listed,
            &format!("<TrackItem Index=\"0\" ObjectRef=\"115\"/>{references}</TrackItems>"),
            1,
        ),
        &copies,
    );
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_items().count(), 6);
    let starts: Vec<_> = sequence.audio.iter().map(|clip| clip.start_ticks).collect();
    let expected: Vec<_> = (0..=250_i64).map(|copy| (10 + 4 * copy) * TICKS).collect();
    assert_eq!(starts, expected);
    assert!(
        !omissions
            .iter()
            .any(|item| item.reason.starts_with("nested sequence audio item")),
        "{omissions:?}"
    );
}

/// Without its video I1, N holds only I4's sound: it is kept with that sound
/// at 0.501187 (a group of it on import), and without an audio item that
/// plays it, nothing is left and N is omitted with the reason.
#[test]
fn a_nest_of_only_sound_plays_it_through_its_item_or_is_omitted_with_its_reason() {
    let xml = without_inner_picture(images_nests_with_sound(str::to_owned));
    let (project, _) = inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
    let sequence = project.single_sequence().unwrap();
    let nests: Vec<_> = sequence.nest_occurrences().collect();
    assert_eq!(nests.len(), 1);
    assert_eq!(nests[0].sequence.video_items().count(), 0);
    let [sound] = nests[0].sequence.audio.as_slice() else {
        panic!("{:?}", nests[0].sequence.audio);
    };
    assert!((sound.volume.as_f64() - 0.501187).abs() < 1e-6, "{sound:?}");
    let xml = without_sound_item(without_inner_picture(images_nests_with_sound(
        str::to_owned,
    )));
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "VideoClipTrackItem:116"
                && item.reason == "nested sequence has only sound that no audio item of it plays"),
        "{omissions:?}"
    );
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.nest_occurrences().count(), 0);
    assert!(sequence.audio.is_empty());
}

#[test]
fn a_nested_mono_source_is_normalized_once_before_the_stereo_nest_gain() {
    let mut xml = images_nests_with_sound(|record| {
        record.replacen(
            r#"<Components ObjectRef="135"/>"#,
            r#"<Components ObjectRef="121"/>"#,
            1,
        )
    });
    // A structural derivative: Inner's direct linked-media sound becomes
    // mono; the outer AudioSequenceSource still carries a stereo mix.
    for (id, tag) in [
        ("101", "AudioStream"),
        ("48", "AudioClip"),
        ("161", "AudioClip"),
    ] {
        xml = edit_record(
            xml,
            &format!("<{tag} ObjectID=\"{id}\""),
            &format!("</{tag}>"),
            |record| {
                let record = record.replace(
                    r#"[{"channellabel":100},{"channellabel":101}]"#,
                    r#"[{"channellabel":0}]"#,
                );
                if tag == "AudioClip" {
                    let parsed = roxmltree::Document::parse(&record).unwrap();
                    let second = parsed
                        .descendants()
                        .find(|node| {
                            node.has_tag_name("SecondaryContentItem")
                                && node.attribute("Index") == Some("1")
                        })
                        .unwrap();
                    let mut result = record.clone();
                    result.replace_range(second.range(), "");
                    result
                } else {
                    record
                }
            },
        );
    }
    let expected = 10_f64.powf(-12.0 / 20.0) * std::f64::consts::FRAC_1_SQRT_2;
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
    assert!(
        !omissions.iter().any(|o| o
            .reason
            .contains("mono centered-stereo gain not normalized")),
        "{omissions:#?}"
    );
    let nested = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert!((nested.sequence.audio[0].volume.as_f64() - expected).abs() < 1e-6);
    let alone = without_video_item(xml);
    let (project, _) = inspect_project_with_omissions(&alone, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
    let sound = &project.single_sequence().unwrap().audio[0];
    assert!((sound.volume.as_f64() - expected).abs() < 1e-6);
}

/// Nest N of the images/nests fixture is kept with its picture and without
/// any sound in its subtree.
fn assert_silent_nest(project: &crate::format::PrProjectFile, name: &str) {
    let nests: Vec<_> = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .collect();
    assert_eq!(nests.len(), 1, "{name}");
    assert!(!nests[0].sequence.has_sound(), "{name}");
}

#[test]
fn a_nest_sound_with_unreadable_volume_keyed_mute_or_one_channel_is_not_read() {
    // A Volume that the shared reader cannot read (Bezier keys, a missing
    // parameter record, parameters of no known layout) would play the nest's
    // sound at unity, and a keyed Mute at its static value; a mono item would
    // fold the stereo mix. None is read, so N plays no inner sound. The
    // edited items share I4's Volume 159 (chain 121, Mute 210, Level 211),
    // with `from` replaced by `to`. Linear and Hold Level keys are read
    // (`an_audio_item_with_level_keys_plays_alone_with_them_on_each_sounds_clock`).
    let volume = |from: &str, to: &str| {
        let xml = images_nests_with_sound(|record| {
            record.replacen(
                r#"<Components ObjectRef="135"/>"#,
                r#"<Components ObjectRef="121"/>"#,
                1,
            )
        });
        assert_eq!(xml.matches(from).count(), 1, "{from}");
        xml.replacen(from, to, 1)
    };
    let keyed = |name: &str, keys: &str| {
        volume(
            &format!("<IsTimeVarying>false</IsTimeVarying>\n\t\t<Name>{name}</Name>"),
            &format!("<Keyframes>{keys}</Keyframes>\n\t\t<Name>{name}</Name>"),
        )
    };
    let unconverted =
        "the audio item of a nested sequence with an unreadable Volume or a keyed Mute is not converted";
    let cases = [
        (
            "Bezier Level",
            keyed("Level", "914457600000000,0.089125096798,5,0,0,0.16666666666666666,0,0.16666666666666666;914711616000000,0.177827941,5,0,0,0.16666666666666666,0,0.16666666666666666;"),
            unconverted,
            Some(("AudioFilterComponent:159", "Bezier Volume keyframes are not converted")),
        ),
        (
            "keyed Mute",
            keyed("Mute", "914457600000000,0,4,0,0,0,0,0;914711616000000,1,4,0,0,0,0,0;"),
            unconverted,
            Some(("AudioComponentParam:210", "Mute automation not converted; static value used")),
        ),
        (
            "missing parameter",
            volume(r#"ObjectRef="211""#, r#"ObjectRef="9211""#),
            unconverted,
            Some(("AudioFilterComponent:159", "missing reference at Param")),
        ),
        (
            "unknown layout",
            volume(
                "<Param Index=\"0\" ObjectRef=\"210\"/>\n\t\t\t\t\t<Param Index=\"1\" ObjectRef=\"211\"/>",
                "<Param Index=\"0\" ObjectRef=\"211\"/>\n\t\t\t\t\t<Param Index=\"1\" ObjectRef=\"210\"/>",
            ),
            unconverted,
            Some(("AudioFilterComponent:159", "unknown clip Volume parameters")),
        ),
        (
            "mono",
            images_nests_with_sound(str::to_owned).replacen(
                r#"<SecondaryContentItem Index="1" ObjectRef="259"/>
		</SecondaryContents>
		<AudioChannelLayout>[{"channellabel":100},{"channellabel":101}]</AudioChannelLayout>"#,
                r#"</SecondaryContents>
		<AudioChannelLayout>[{"channellabel":0}]</AudioChannelLayout>"#,
                1,
            ),
            "only a stereo audio item of a nested sequence is supported",
            None,
        ),
    ];
    for (name, xml, reason, volume_report) in cases {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
        assert!(
            omissions
                .iter()
                .any(|item| item.record == "115" && item.reason.ends_with(reason)),
            "{name}: {omissions:?}"
        );
        // The shared reader reports what of the Volume it did not read.
        if let Some((record, report)) = volume_report {
            assert!(
                omissions
                    .iter()
                    .any(|item| item.scope == crate::OmissionScope::Feature
                        && item.record == record
                        && item.reason.ends_with(report)),
                "{name}: {omissions:?}"
            );
        }
        assert_silent_nest(&project, name);
    }
}

/// Item 115 with a clip Volume of its own in the keyed form that Premiere
/// 26.5.1 saves a clip Level (`feature_audio_volume_keys_strict`, param
/// 142): chain 9135, Volume 9159, Mute 9210 and Level 9211, whose native
/// Keyframes `keys` lie on the item's source clock, Inner's, as Premiere
/// 26.5.1 saves a nest audio item's (`premiere_isolated_nest_audio_outer_keys_26_5`,
/// param 174). An XML edit of this fixture.
fn with_item_level_keys(xml: String, keys: &str) -> String {
    let xml = edit_record(xml, SOUND_ITEM, "</AudioClipTrackItem>", |record| {
        record.replacen(
            r#"<Components ObjectRef="135"/>"#,
            r#"<Components ObjectRef="9135"/>"#,
            1,
        )
    });
    let layout =
        r#"<AudioChannelLayout>[{"channellabel":100},{"channellabel":101}]</AudioChannelLayout>"#;
    let config = r#"<ChannelConfigData>{"in":[{"layout":[100,101],"name":"Stereo In","type":0}],"out":[{"layout":[100,101],"name":"Stereo Out","type":0}]}</ChannelConfigData>"#;
    with_records(
        &xml,
        &format!(
            r#"<AudioComponentChain ObjectID="9135" ClassID="3cb131d1-d3c0-47ae-a19a-bdf75ea11674" Version="4"><ComponentChain Version="3"><Components Version="1"><Component Index="0" ObjectRef="9159"/></Components></ComponentChain>{layout}<ChannelType>1</ChannelType></AudioComponentChain>
<AudioFilterComponent ObjectID="9159" ClassID="d77a90a0-6c9e-44bf-9b20-de8c21168fe1" Version="4"><AudioComponent Version="3"><Component Version="7"><Params Version="1"><Param Index="0" ObjectRef="9210"/><Param Index="1" ObjectRef="9211"/></Params><ID>1</ID><Intrinsic>true</Intrinsic></Component>{layout}<AudioComponentType>0</AudioComponentType><FrameRate>5292000</FrameRate><ChannelType>1</ChannelType></AudioComponent><FilterPreset>0</FilterPreset>{config}<FilterMatchName>Internal Volume Stereo</FilterMatchName><FilterIndex>-1</FilterIndex></AudioFilterComponent>
<AudioComponentParam ObjectID="9210" ClassID="32657501-3aa4-445f-a49b-d09ecb9fa1ae" Version="10"><IsTimeVarying>false</IsTimeVarying><Name>Mute</Name><RangeLocked>false</RangeLocked></AudioComponentParam>
<AudioComponentParam ObjectID="9211" ClassID="a714635e-a628-4b27-9d59-77eba47dbc1a" Version="10"><StartKeyframe>-91445760000000000,0.177827939391,0,0,0,0,0,0</StartKeyframe><CurrentValue>0.17782793939113617</CurrentValue><Keyframes>{keys}</Keyframes><Name>Level</Name><UnitsString>dB</UnitsString></AudioComponentParam>
"#
        ),
    )
}

/// Item 115's Level keys on Inner's clock, the clock of its In/Out: 0 dB at
/// 0.5 s, Linear to -12 dB at 2 s, held until it falls silent at 2.5 s, held
/// silent until 0 dB at 3 s, then Linear to -6 dB at 5 s, after its Out.
const ITEM_LEVEL_KEYS: &str = "127008000000,0.177827939391,0,0,0,0,0,0;508032000000,0.044668357819,4,0,0,0,0,0;635040000000,0.,4,0,0,0,0,0;762048000000,0.177827939391,0,0,0,0,0,0;1270080000000,0.089125096798,0,0,0,0,0,0;";

/// Item 115's Level keys on Inner's clock that only step: 0 dB at 0.5 s,
/// then Holds to -12 dB at 1.5 s, to silence at 2.5 s, to 0 dB at 3 s and to
/// -6 dB at 5 s, after its Out.
const ITEM_STEP_KEYS: &str = "127008000000,0.177827939391,4,0,0,0,0,0;381024000000,0.044668357819,4,0,0,0,0,0;635040000000,0.,4,0,0,0,0,0;762048000000,0.177827939391,4,0,0,0,0,0;1270080000000,0.089125096798,4,0,0,0,0,0;";

/// I4's Level keys on its source clock that only step: -6 dB at 0.25 s,
/// before its In; a Hold to -20 dB at 2 s, where the item's -12 dB step
/// lands once I4 plays from 0.5 s; Linear to the same -20 dB at 2.5 s; and
/// Holds to 0 dB at 3.25 s, to -3 dB at 4.5 s, its Out, and to -40 dB at 6 s.
const INNER_STEP_KEYS: &str = "63504000000,0.089125096798,4,0,0,0,0,0;508032000000,0.017782794312,0,0,0,0,0,0;635040000000,0.017782794312,4,0,0,0,0,0;825552000000,0.177827939391,4,0,0,0,0,0;1143072000000,0.125892541179,4,0,0,0,0,0;1524096000000,0.00177827939391,0,0,0,0,0,0;";

/// Inner's audio I4 (item 112, clip 161) playing the linked-A/V source from
/// 0.5 s to 4.5 s at Inner 0-4 s, so that its source clock runs 0.5 s ahead
/// of Inner's.
fn with_inner_sound_from_half_a_second(xml: String) -> String {
    edit_record(
        xml,
        r#"<AudioClip ObjectID="161""#,
        "</AudioClip>",
        |record| {
            record
                .replacen("<InPoint>0</InPoint>", "<InPoint>127008000000</InPoint>", 1)
                .replacen(
                    "<OutPoint>1016064000000</OutPoint>",
                    "<OutPoint>1143072000000</OutPoint>",
                    1,
                )
        },
    )
}

/// I4's Level 211 with the native Keyframes `keys` on its source clock.
fn with_inner_level_keys(xml: String, keys: &str) -> String {
    let level = "<IsTimeVarying>false</IsTimeVarying>\n\t\t<Name>Level</Name>";
    assert_eq!(xml.matches(level).count(), 1);
    xml.replacen(
        level,
        &format!("<Keyframes>{keys}</Keyframes>\n\t\t<Name>Level</Name>"),
        1,
    )
}

/// A copy of I4 (item 112) under ObjectID 9112 at default Volume (chain
/// 135), as the one item of Inner's second audio track: a sibling of I4
/// that plays the same source over the same ranges.
fn with_static_inner_sound(xml: String) -> String {
    let xml = edit_record(
        xml,
        r#"<AudioClipTrackItem ObjectID="112""#,
        "</AudioClipTrackItem>",
        |record| {
            record.to_owned()
                + &record
                    .replacen(r#"ObjectID="112""#, r#"ObjectID="9112""#, 1)
                    .replacen(
                        r#"<Components ObjectRef="121"/>"#,
                        r#"<Components ObjectRef="135"/>"#,
                        1,
                    )
        },
    );
    let track = r#"<AudioClipTrack ObjectUID="f1f32858-b51c-4131-8fe2-367659b28c9a""#;
    edit_record(xml, track, "</AudioClipTrack>", |record| {
        record.replacen(
            r#"<ClipItems Version="3">"#,
            r#"<ClipItems Version="3"><TrackItems Version="1"><TrackItem Index="0" ObjectRef="9112"/></TrackItems>"#,
            1,
        )
    })
}

/// A sound's Volume keys as (source ticks, Level gain, easing into the key).
type LevelKeys = Vec<(i64, f64, PrKeyframeEasing)>;
/// A sound's expected Volume keys with their gain, or `None` for none.
type KeysAndGain = Option<(LevelKeys, f64)>;

/// Keys at `seconds` on a source clock, as (seconds, Level gain, easing).
fn level_keys(keys: &[(f64, f64, PrKeyframeEasing)]) -> LevelKeys {
    keys.iter()
        .map(|&(seconds, gain, easing)| ((seconds * TICKS as f64) as i64, gain, easing))
        .collect()
}

/// [`ITEM_LEVEL_KEYS`] on I4's source clock once it plays from 0.5 s: each
/// key 0.5 s later.
fn item_keys_on_i4() -> LevelKeys {
    use PrKeyframeEasing::{Hold, Linear};
    level_keys(&[
        (1.0, 1.0, Linear),
        (2.5, 10f64.powf(-12.0 / 20.0), Linear),
        (3.0, 0.0, Hold),
        (3.5, 1.0, Hold),
        (5.5, 10f64.powf(-6.0 / 20.0), Linear),
    ])
}

/// [`ITEM_STEP_KEYS`] on I4's source clock once it plays from 0.5 s.
fn item_steps_on_i4() -> LevelKeys {
    use PrKeyframeEasing::{Hold, Linear};
    level_keys(&[
        (1.0, 1.0, Linear),
        (2.0, 10f64.powf(-12.0 / 20.0), Hold),
        (3.0, 0.0, Hold),
        (3.5, 1.0, Hold),
        (5.5, 10f64.powf(-6.0 / 20.0), Hold),
    ])
}

/// The product of [`item_steps_on_i4`] and [`INNER_STEP_KEYS`]: a Hold key
/// at every time either has a key, both at 2 s, valued at the product of
/// both Levels there, from -6 dB (both first Levels) to -46 dB.
fn step_product_on_i4() -> LevelKeys {
    use PrKeyframeEasing::{Hold, Linear};
    let db = |db: f64| 10f64.powf(db / 20.0);
    level_keys(&[
        (0.25, db(-6.0), Linear),
        (1.0, db(-6.0), Hold),
        (2.0, db(-32.0), Hold),
        (2.5, db(-32.0), Hold),
        (3.0, 0.0, Hold),
        (3.25, 0.0, Hold),
        (3.5, 1.0, Hold),
        (4.5, db(-3.0), Hold),
        (5.5, db(-9.0), Hold),
        (6.0, db(-46.0), Hold),
    ])
}

/// Checks one sound: its media, [start, end, in, out] in seconds, volume,
/// and its Volume keys with their gain, or no keys.
fn assert_sound(
    name: &str,
    project: &crate::format::PrProjectFile,
    sound: &PrAudioOccurrence,
    ranges: [f64; 4],
    volume: f64,
    keys: KeysAndGain,
) {
    assert_eq!(
        project.media[&sound.media].name(),
        "feature_linked_av_source.mp4",
        "{name}"
    );
    assert_eq!(
        [
            sound.start_ticks,
            sound.end_ticks,
            sound.in_ticks,
            sound.out_ticks
        ],
        ranges.map(|seconds| (seconds * TICKS as f64) as i64),
        "{name}"
    );
    // Premiere stores Levels to about 1e-11 of their dB values.
    let near = |actual: f64, expected: f64| (actual - expected).abs() < 1e-7;
    assert!(near(sound.volume.as_f64(), volume), "{name}: {sound:?}");
    match (&sound.volume_keys, keys) {
        (None, None) => {}
        (Some(actual), Some((expected, gain))) => {
            assert!(near(actual.gain, gain), "{name}: {actual:?}");
            assert_eq!(actual.keys.len(), expected.len(), "{name}: {actual:?}");
            for (key, (ticks, value, easing)) in actual.keys.iter().zip(expected) {
                assert_eq!((key.source_ticks, key.easing), (ticks, easing), "{name}");
                assert!(near(key.value, value), "{name}: {key:?}");
            }
        }
        (actual, expected) => panic!("{name}: {actual:?}, expected {expected:?}"),
    }
}

/// Each row gives item 115 [`ITEM_LEVEL_KEYS`] and I4 its 0.5 s source
/// offset, edits the fixture as a row of
/// `an_audio_item_that_no_group_carries_plays_its_sound_alone` does, and
/// expects N's group, as its Enable and the sounds it carries; I4's one
/// sound, as its [start, end, in] in seconds, its volume and the gain of its
/// keys; and the reason that omits N's picture, the only report on N's
/// items. The keyed item plays alone whatever its picture (`pair_sounds`),
/// so I4 plays once. Its sound takes the item's keys on its own source
/// clock, those before In and after Out too, times its static -6 dB and the
/// item's other stages; a disabled item or a muted track or Volume keeps
/// them at zero gain.
#[test]
fn an_audio_item_with_level_keys_plays_alone_with_them_on_each_sounds_clock() {
    type Edit = fn(String) -> String;
    type Group = Option<(bool, usize)>;
    type Alone = ([f64; 3], f64, f64);
    let i4 = 10f64.powf(-6.0 / 20.0);
    let at_10 = [10.0, 14.0, 0.5];
    let rows: [(&str, Edit, Group, Alone, Option<&str>); 10] = [
        (
            "picture kept",
            |xml| xml,
            Some((true, 0)),
            (at_10, i4, i4),
            None,
        ),
        (
            "hidden picture",
            |xml| edit_record(xml, VIDEO_ITEM, "</VideoClipTrackItem>", disabled),
            Some((false, 0)),
            (at_10, i4, i4),
            None,
        ),
        (
            "mask-unsafe picture",
            with_two_masks,
            None,
            (at_10, i4, i4),
            Some(TWO_MASKS),
        ),
        (
            "no video item",
            without_video_item,
            None,
            (at_10, i4, i4),
            None,
        ),
        (
            "another range",
            moved_sound_item,
            Some((true, 0)),
            ([9.0, 13.0, 0.5], i4, i4),
            None,
        ),
        (
            "trimmed item",
            trimmed_sound_item,
            Some((true, 0)),
            ([11.0, 14.0, 1.5], i4, i4),
            None,
        ),
        (
            "Clip Gain",
            |xml| {
                edit_record(xml, SOUND_CLIP, "</AudioClip>", |record| {
                    record.replacen("</AudioClip>", "<Gain>2</Gain></AudioClip>", 1)
                })
            },
            Some((true, 0)),
            (at_10, 2.0 * i4, 2.0 * i4),
            None,
        ),
        (
            "disabled item",
            |xml| edit_record(xml, SOUND_ITEM, "</AudioClipTrackItem>", disabled),
            Some((true, 0)),
            (at_10, 0.0, 0.0),
            None,
        ),
        (
            "track muted",
            with_sound_track_muted,
            Some((true, 0)),
            (at_10, 0.0, 0.0),
            None,
        ),
        (
            "Volume muted",
            |xml| {
                let mute = "<IsTimeVarying>false</IsTimeVarying><Name>Mute</Name>";
                xml.replacen(mute, &format!("<CurrentValue>1</CurrentValue>{mute}"), 1)
            },
            Some((true, 0)),
            (at_10, 0.0, 0.0),
            None,
        ),
    ];
    for (name, edit, group, ([start, end, source_in], volume, keys_gain), picture) in rows {
        let xml = edit(with_item_level_keys(
            with_inner_sound_from_half_a_second(images_nests_with_sound(str::to_owned)),
            ITEM_LEVEL_KEYS,
        ));
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
        let sequence = project.single_sequence().unwrap();
        let groups: Vec<_> = sequence
            .nest_occurrences()
            .map(|nest| {
                assert_eq!(nest.sequence.video_occurrences().count(), 1, "{name}");
                (nest.enabled, nest.sequence.audio.len())
            })
            .collect();
        assert_eq!(groups, Vec::from_iter(group), "{name}");
        let [sound] = sequence.audio.as_slice() else {
            panic!("{name}: {:?}", sequence.audio);
        };
        assert_sound(
            name,
            &project,
            sound,
            [start, end, source_in, 4.5],
            volume,
            Some((item_keys_on_i4(), keys_gain)),
        );
        let reported: Vec<_> = omissions
            .iter()
            .filter(|item| {
                item.scope == crate::OmissionScope::Occurrence
                    && ["115", "116"].iter().any(|id| item.record.ends_with(id))
            })
            .map(|item| item.reason.as_str())
            .collect();
        assert_eq!(
            reported,
            Vec::from_iter(picture.map(|reason| format!("unsupported conversion: {reason}"))),
            "{name}"
        );
    }
}

/// One key track holds the product of two changing Volume Levels where one
/// of them holds over the sound, that Level folded into the gain of the
/// other's keys, or where both only step: Hold keys at every time either
/// has a key then carry it exactly. Each row keys item 115's Level and I4's
/// (Level 211). Where both change over I4 and one of them along a Linear
/// segment (I4's keys at 1 s and 3 s of its source), I4 is reported and not
/// converted, while its static sibling 9112 still takes the item's keys; a
/// disabled item keeps both silent, with nothing lost. Stepping Levels
/// multiply on I4 over the item's range or a trimmed one, and a disabled
/// item keeps that product at zero gain.
#[test]
fn item_and_inner_level_keys_combine_where_one_holds_or_both_step_over_the_sound() {
    use PrKeyframeEasing::Linear;
    fn item_keyed(xml: String) -> String {
        with_item_level_keys(xml, ITEM_LEVEL_KEYS)
    }
    // I4's Level: -20 dB at 3600 s and 0 dB at 3601 s of its source, so it
    // holds -20 dB over I4, apart from its static -6 dB.
    fn far_i4(xml: String) -> String {
        with_inner_level_keys(
            xml,
            "914457600000000,0.017782794312,0,0,0,0,0,0;914711616000000,0.177827939391,0,0,0,0,0,0;",
        )
    }
    // I4's Level changes over I4 (0 dB at 1 s, Linear to -20 dB at 3 s of
    // its source), as item 115's does; the static 9112 plays beside I4.
    fn both_change(xml: String) -> String {
        let xml = with_inner_level_keys(
            xml,
            "254016000000,0.177827939391,0,0,0,0,0,0;762048000000,0.017782794312,0,0,0,0,0,0;",
        );
        with_static_inner_sound(item_keyed(xml))
    }
    // Both Levels only step; the static 9112 plays beside I4.
    fn both_step(xml: String) -> String {
        with_static_inner_sound(with_item_level_keys(
            with_inner_level_keys(xml, INNER_STEP_KEYS),
            ITEM_STEP_KEYS,
        ))
    }
    let from_half = |edit: fn(String) -> String| {
        edit(with_inner_sound_from_half_a_second(
            images_nests_with_sound(str::to_owned),
        ))
    };
    let i4 = 10f64.powf(-6.0 / 20.0);
    let i4_keys = level_keys(&[(1.0, 1.0, Linear), (3.0, 0.1, Linear)]);
    let product = "nested sound AudioClipTrackItem:112 not converted: its Volume and the Level keys of this audio item both change over it, and one key track cannot hold their product because one of them changes its Level along a Linear segment";
    // Each sound as [start, end, in, out] in seconds, volume and keys.
    type Sound = ([f64; 4], f64, KeysAndGain);
    let rows: [(&str, String, Vec<Sound>, Option<&str>); 7] = [
        (
            // Item 115 shares I4's Volume, whose keys at 3600 s and 3601 s
            // lie past both ranges: the item's Level holds -6 dB over I4.
            "keys past both ranges",
            with_inner_level_keys(
                images_nests_with_sound(|record| {
                    record.replacen(
                        r#"<Components ObjectRef="135"/>"#,
                        r#"<Components ObjectRef="121"/>"#,
                        1,
                    )
                }),
                "914457600000000,0.089125096798,0,0,0,0,0,0;914711616000000,0.177827941,0,0,0,0,0,0;",
            ),
            vec![(
                [10.0, 14.0, 0.0, 4.0],
                i4 * i4,
                Some((
                    level_keys(&[(3600.0, i4, Linear), (3601.0, 1.0, Linear)]),
                    i4,
                )),
            )],
            None,
        ),
        (
            "I4's Level holds",
            from_half(|xml| item_keyed(far_i4(xml))),
            vec![([10.0, 14.0, 0.5, 4.5], 0.1, Some((item_keys_on_i4(), 0.1)))],
            None,
        ),
        (
            "both change",
            from_half(both_change),
            vec![([10.0, 14.0, 0.5, 4.5], 1.0, Some((item_keys_on_i4(), 1.0)))],
            Some(product),
        ),
        (
            "both change, item disabled",
            from_half(|xml| {
                edit_record(
                    both_change(xml),
                    SOUND_ITEM,
                    "</AudioClipTrackItem>",
                    disabled,
                )
            }),
            vec![
                ([10.0, 14.0, 0.5, 4.5], 0.0, Some((i4_keys.clone(), 0.0))),
                ([10.0, 14.0, 0.5, 4.5], 0.0, Some((item_keys_on_i4(), 0.0))),
            ],
            None,
        ),
        (
            "both step",
            from_half(both_step),
            vec![
                ([10.0, 14.0, 0.5, 4.5], i4, Some((step_product_on_i4(), 1.0))),
                ([10.0, 14.0, 0.5, 4.5], 1.0, Some((item_steps_on_i4(), 1.0))),
            ],
            None,
        ),
        (
            "both step, item trimmed",
            from_half(|xml| trimmed_sound_item(both_step(xml))),
            vec![
                ([11.0, 14.0, 1.5, 4.5], i4, Some((step_product_on_i4(), 1.0))),
                ([11.0, 14.0, 1.5, 4.5], 1.0, Some((item_steps_on_i4(), 1.0))),
            ],
            None,
        ),
        (
            "both step, item disabled",
            from_half(|xml| {
                edit_record(
                    both_step(xml),
                    SOUND_ITEM,
                    "</AudioClipTrackItem>",
                    disabled,
                )
            }),
            vec![
                ([10.0, 14.0, 0.5, 4.5], 0.0, Some((step_product_on_i4(), 0.0))),
                ([10.0, 14.0, 0.5, 4.5], 0.0, Some((item_steps_on_i4(), 0.0))),
            ],
            None,
        ),
    ];
    for (name, xml, expected, reason) in rows {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(
            sequence.audio.len(),
            expected.len(),
            "{name}: {:?}",
            sequence.audio
        );
        for (sound, (ranges, volume, keys)) in sequence.audio.iter().zip(expected) {
            assert_sound(name, &project, sound, ranges, volume, keys);
        }
        let reported: Vec<_> = omissions
            .iter()
            .filter(|item| item.record.ends_with("115"))
            .map(|item| (item.scope, item.reason.as_str()))
            .collect();
        assert_eq!(
            reported,
            Vec::from_iter(reason.map(|reason| (crate::OmissionScope::Occurrence, reason))),
            "{name}"
        );
    }
}

/// Item 115's one key at tick 9223372036854775000 cannot move 0.5 s later
/// to I4's source clock within Premiere's tick range: I4 is omitted with
/// that reason, reported on the item, and N keeps its picture without
/// sound.
#[test]
fn item_level_keys_past_the_tick_range_on_a_sounds_clock_omit_that_sound() {
    let xml = with_item_level_keys(
        with_inner_sound_from_half_a_second(images_nests_with_sound(str::to_owned)),
        "9223372036854775000,0.177827939391,0,0,0,0,0,0;",
    );
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
    assert!(
        omissions.iter().any(|item| item.record == "AudioClipTrackItem:115"
            && item.reason
                == "nested sound AudioClipTrackItem:112 not converted: nested sequence audio item exceeds Premiere's tick range"),
        "{omissions:?}"
    );
    assert!(project.single_sequence().unwrap().audio.is_empty());
    assert_silent_nest(&project, "tick range");
}

/// Item 115 at +6 dB, sharing I4's Volume with Level 211 edited, and a Clip
/// Gain of 1e308: the item's gain, which every sound that it plays shares,
/// overflows. The item is omitted as it is read, with that reason, whether
/// N's group would carry its sound or it would play alone without a video
/// item; N keeps its picture without sound.
#[test]
fn an_audio_item_whose_gain_overflows_is_omitted_as_it_is_read() {
    let overflowing = |xml: String| {
        let xml = edit_record(xml, SOUND_CLIP, "</AudioClip>", |record| {
            record.replacen(
                "</AudioChannelLayout>",
                "</AudioChannelLayout><Gain>1e308</Gain>",
                1,
            )
        });
        let level = "-91445760000000000,0.089125096798,0,0,0,0,0,0";
        assert_eq!(xml.matches(level).count(), 1);
        xml.replacen(level, "-91445760000000000,0.354813396931,0,0,0,0,0,0", 1)
    };
    let shared_volume = |record: &str| {
        record.replacen(
            r#"<Components ObjectRef="135"/>"#,
            r#"<Components ObjectRef="121"/>"#,
            1,
        )
    };
    for (name, xml, nests) in [
        (
            "carried",
            overflowing(images_nests_with_sound(shared_volume)),
            1,
        ),
        (
            "alone",
            without_video_item(overflowing(images_nests_with_sound(shared_volume))),
            0,
        ),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
        let reported: Vec<_> = omissions
            .iter()
            .filter(|item| item.record.ends_with("115"))
            .map(|item| (item.scope, item.reason.as_str()))
            .collect();
        assert_eq!(
            reported,
            [(
                crate::OmissionScope::Occurrence,
                "unsupported conversion: audio gain overflows"
            )],
            "{name}"
        );
        let sequence = project.single_sequence().unwrap();
        assert!(sequence.audio.is_empty(), "{name}");
        assert_eq!(sequence.nest_occurrences().count(), nests, "{name}");
        if nests == 1 {
            assert_silent_nest(&project, name);
        }
    }
}

/// Item 115 with [`ITEM_LEVEL_KEYS`] and a copy of it on the root's second
/// audio track both play N's placement, each alone with its own Volume: a
/// copy at I4's static -6 dB (chain 121) plays I4 at a quarter without keys,
/// and a copy that shares 115's keyed Volume plays I4 again with its own
/// copy of the keys. N keeps only its picture, and I4 plays once per item.
#[test]
fn each_audio_item_of_a_nest_plays_alone_with_its_own_level_keys() {
    let i4 = 10f64.powf(-6.0 / 20.0);
    let keyed = || {
        with_item_level_keys(
            with_inner_sound_from_half_a_second(images_nests_with_sound(str::to_owned)),
            ITEM_LEVEL_KEYS,
        )
    };
    // The volume and keys of 115's sound, then of 9115's.
    type Sounds = [(f64, KeysAndGain); 2];
    let rows: [(&str, String, Sounds); 2] = [
        (
            "copy at -6 dB",
            with_sound_item_on_the_second_track(keyed(), |record| {
                record.replacen(
                    r#"<Components ObjectRef="9135"/>"#,
                    r#"<Components ObjectRef="121"/>"#,
                    1,
                )
            }),
            [(i4, Some((item_keys_on_i4(), i4))), (i4 * i4, None)],
        ),
        (
            "copy with the same keys",
            with_sound_item_on_the_second_track(keyed(), str::to_owned),
            [
                (i4, Some((item_keys_on_i4(), i4))),
                (i4, Some((item_keys_on_i4(), i4))),
            ],
        ),
    ];
    for (name, xml, expected) in rows {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
        let sequence = project.single_sequence().unwrap();
        let groups: Vec<_> = sequence
            .nest_occurrences()
            .map(|nest| (nest.enabled, nest.sequence.audio.len()))
            .collect();
        assert_eq!(groups, [(true, 0)], "{name}");
        assert_eq!(sequence.audio.len(), 2, "{name}: {:?}", sequence.audio);
        for (sound, (volume, keys)) in sequence.audio.iter().zip(expected) {
            assert_sound(name, &project, sound, [10.0, 14.0, 0.5, 4.5], volume, keys);
        }
        assert!(
            !omissions
                .iter()
                .any(|item| item.scope == crate::OmissionScope::Occurrence
                    && item.record.ends_with("115")),
            "{name}: {omissions:?}"
        );
    }
}

/// A synthetic intrinsic Motion component `id` in the 26.5 layout, its records
/// `id + 1` to `id + 11` built from [`MOTION_PARAMS_26_5`]: a time-varying
/// Position without keys at the centre, a Scale keyed from 120 at source frame
/// 5 to 150 at frame 20 (30 fps, eased), and defaults for the rest, crop
/// included.
pub(in crate::format) fn keyed_motion(id: u32) -> String {
    const FRAME: i64 = 8_467_200_000;
    let ease = "5,0,0,0.33333333333333331,0,0.33333333333333331";
    let scale = format!("{},120.,{ease};{},150.,{ease};", 5 * FRAME, 20 * FRAME);
    motion_component(id, |name| match name {
        "Position" => (None, Some(String::new())),
        "Scale" => (None, Some(scale.clone())),
        _ => (None, None),
    })
}

/// The records of [`keyed_motion`] with each parameter as `edit` gives it by
/// name: a StartKeyframe value in place of the default, and the key list,
/// possibly empty, of a time-varying parameter.
fn motion_component(
    id: u32,
    edit: impl Fn(&str) -> (Option<&'static str>, Option<String>),
) -> String {
    let mut references = String::new();
    let mut records = String::new();
    for (object, spec) in (id + 1..).zip(&MOTION_PARAMS_26_5) {
        let tag = spec.record.tag;
        let (value, keys) = edit(spec.name);
        let value = value.unwrap_or(spec.initial);
        let initial = if spec.is_point() {
            format!("-91445760000000000,{value},0,0,0,0,0,0,5,4,0,0,0,0")
        } else {
            format!("-91445760000000000,{value},0,0,0,0,0,0")
        };
        let control = spec.control.map_or(String::new(), |control| {
            format!("<ParameterControlType>{control}</ParameterControlType>")
        });
        let bounds = spec.bounds.map_or(String::new(), |(lower, upper, ui)| {
            let ui = ui.map_or(String::new(), |ui| {
                format!("<UpperUIBound>{ui}</UpperUIBound>")
            });
            format!("<LowerBound>{lower}</LowerBound><UpperBound>{upper}</UpperBound>{ui}")
        });
        let animation = keys.map_or(String::new(), |keys| {
            let keys = if keys.is_empty() {
                keys
            } else {
                format!("<Keyframes>{keys}</Keyframes>")
            };
            format!("<IsTimeVarying>true</IsTimeVarying>{keys}")
        });
        references.push_str(&format!(
            r#"<Param Index="{}" ObjectRef="{object}"/>"#,
            spec.id - 1
        ));
        records.push_str(&format!(
            "<{tag} ObjectID=\"{object}\" ClassID=\"{}\" Version=\"{}\"><Name>{}</Name><ParameterID>{}</ParameterID>{animation}{control}<StartKeyframe>{initial}</StartKeyframe>{bounds}</{tag}>",
            spec.record.class_id, spec.record.version, spec.name, spec.id
        ));
    }
    format!(
        r#"<VideoFilterComponent ObjectID="{id}"><Component><Params>{references}</Params><DisplayName>Motion</DisplayName><Intrinsic>true</Intrinsic></Component><MatchName>AE.ADBE Motion</MatchName></VideoFilterComponent>{records}"#
    )
}

/// Outer's one placement of Main at 2-3 s from its start, whose chain holds
/// [`keyed_motion`], with those records edited by `edit`.
fn moved_nest(edit: impl Fn(&str) -> String) -> String {
    let chain = r#"<VideoComponentChain ObjectID="111"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>"#;
    let moved = r#"<VideoComponentChain ObjectID="111"><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index="0" ObjectRef="500"/></Components></ComponentChain></VideoComponentChain>"#;
    let xml = outer_xml(&[Placement {
        start: 2 * TICKS,
        end: 3 * TICKS,
        source_in: 0,
    }]);
    assert_eq!(xml.matches(chain).count(), 1);
    with_records(&xml.replace(chain, moved), &edit(&keyed_motion(500)))
}

#[test]
fn nest_motion_imports_as_its_group_transform_and_scale_keys() {
    let (project, omissions) =
        inspect_project_with_omissions(&moved_nest(str::to_owned), Some("outer")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let outer = project.single_sequence().unwrap();
    let [nest] = outer.nest_occurrences().collect::<Vec<_>>()[..] else {
        panic!("one nest");
    };
    // Position is time-varying without keys: its StartKeyframe, the centre.
    // The static Scale is the first key's, not the StartKeyframe 100.
    assert_eq!(nest.transform.position, [0.5, 0.5]);
    assert_eq!(nest.transform.scale, [120.0; 2]);
    assert_eq!(nest.opacity, 100.0);
    let [animation] = nest.animations.as_slice() else {
        panic!("one animation: {:?}", nest.animations);
    };
    assert_eq!(animation.property(), PrAnimatedProperty::UniformScale);
    let keys: Vec<_> = animation
        .keys()
        .iter()
        .map(|key| (key.source_ticks, key.value))
        .collect();
    assert_eq!(keys, [(42_336_000_000, 120.0), (169_344_000_000, 150.0)]);
    assert!(matches!(
        animation.keys()[1].easing,
        PrKeyframeEasing::CubicBezier { .. }
    ));

    let document = project_document_with_media(outer, &project.media);
    let group = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    // At normal speed the group clock starts at zero at the placement start.
    assert_eq!(
        group["playback"],
        serde_json::json!({
            "type": "windowed",
            "inputRange": {"start": 2000, "duration": 1000},
            "mapping": {
                "type": "linear",
                "input": {"start": 2000, "duration": 1000},
                "output": {"start": 0, "duration": 1000}
            },
            "inputOffsetMs": 0
        })
    );
    // The canvas-sized picture scales about its centre.
    assert_eq!(
        group["transform"]["anchorPoint"],
        serde_json::json!([960.0, 540.0])
    );
    assert_eq!(
        group["transform"]["position"],
        serde_json::json!([960.0, 540.0])
    );
    // Premiere draws the nested sequence's frame only: the group is clipped
    // to it by an Add mask whose guide is the frame, a child at the identity.
    let [mask] = group["masks"].as_array().unwrap().as_slice() else {
        panic!("one mask: {group}");
    };
    assert_eq!(mask["mode"], "add");
    let guide = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == mask["layer"])
        .unwrap();
    assert_eq!(guide["type"], "Rect");
    assert_eq!(guide["rect"]["size"], serde_json::json!([1920.0, 1080.0]));
    assert_eq!(
        guide["activeRange"],
        serde_json::json!({"start": 0, "duration": 1000})
    );
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    for axis in ["scaleX", "scaleY"] {
        let entry = entries
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == group["id"] && entry["target"]["propertyType"] == axis
            })
            .unwrap_or_else(|| panic!("{axis}: {entries:#?}"));
        let keys: Vec<_> = entry["animator"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| {
                (
                    key["layerTime"].as_i64().unwrap(),
                    key["value"]["value"].as_f64().unwrap(),
                )
            })
            .collect();
        // Frames 5 and 20 round to whole milliseconds.
        assert_eq!(keys, [(167, 120.0), (667, 150.0)], "{axis}");
    }
}

#[test]
fn nest_anchor_point_and_scale_width_keys_import_onto_the_group_from_their_first_values() {
    // Synthetic structural check. Outer places Main at 2-3 s from In 1 s.
    // Its Motion has Uniform Scale off, a static Scale Height of 80, and
    // Linear Scale Width and Anchor Point keys 6 and 24 frames after In,
    // whose first values are not their StartKeyframes.
    let at = |frames: i64| TICKS + frames * TICKS / 30;
    let point = |ticks: i64, value: &str| {
        format!("{ticks},{value},0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;")
    };
    let motion = motion_component(500, |name| match name {
        "Scale" => (Some("80."), None),
        " " => (Some("false"), None),
        "Scale Width" => (
            None,
            Some(format!(
                "{},110.,0,0,0,0,0,0;{},140.,0,0,0,0,0,0;",
                at(6),
                at(24)
            )),
        ),
        "Anchor Point" => (
            None,
            Some(point(at(6), "0.25:0.25") + &point(at(24), "0.75:0.5")),
        ),
        _ => (None, None),
    });
    let window = |clip: String| {
        let from = format!(
            r#"<Clip><Source ObjectRef="102"/><InPoint>0</InPoint><OutPoint>{TICKS}</OutPoint>"#
        );
        let xml = moved_nest(|_| motion.clone());
        assert_eq!(xml.matches(&from).count(), 1);
        xml.replace(&from, &clip)
    };
    let import = |xml: &str| {
        let (project, omissions) = inspect_project_with_omissions(xml, Some("outer")).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let outer = project.single_sequence().unwrap();
        let [nest] = outer.nest_occurrences().collect::<Vec<_>>()[..] else {
            panic!("one nest");
        };
        let read = (
            nest.in_ticks,
            nest.transform,
            nest.animations
                .iter()
                .map(|animation| animation.property())
                .collect::<Vec<_>>(),
        );
        let ids = crate::tesseract_output::asset_ids_in_order(outer, &project.media);
        let mut converted = Vec::new();
        let document =
            crate::convert::premiere_to_tesseract(outer, &project.media, &ids, &mut converted)
                .unwrap()
                .to_json_value()
                .unwrap();
        let group = document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Group")
            .unwrap()
            .clone();
        // The group's key tracks: per property, (layer ms, value) pairs.
        let tracks: std::collections::BTreeMap<_, _> = document["composition"]["dynamics"]
            ["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["target"]["layerId"] == group["id"])
            .map(|entry| {
                let keys: Vec<_> = entry["animator"]["keyframes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|key| {
                        (
                            key["layerTime"].as_i64().unwrap(),
                            key["value"]["value"].as_f64().unwrap(),
                        )
                    })
                    .collect();
                (
                    entry["target"]["propertyType"].as_str().unwrap().to_owned(),
                    keys,
                )
            })
            .collect();
        (read, group, tracks, converted)
    };
    let video = |group: &serde_json::Value| {
        let video = group["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        (video["playback"].clone(), video["sourceRange"].clone())
    };
    let static_transform = |group: &serde_json::Value| {
        assert_eq!(
            group["transform"]["anchorPoint"],
            serde_json::json!([480.0, 270.0])
        );
        assert_eq!(
            group["transform"]["position"],
            serde_json::json!([960.0, 540.0])
        );
        assert_eq!(
            group["transform"]["scale"],
            serde_json::json!([110.0, 80.0])
        );
    };

    let ((source_in, transform, properties), group, tracks, converted) = import(&window(format!(
        r#"<Clip><Source ObjectRef="102"/><InPoint>{TICKS}</InPoint><OutPoint>{}</OutPoint>"#,
        2 * TICKS
    )));
    assert_eq!(source_in, TICKS);
    assert_eq!(
        properties,
        [
            PrAnimatedProperty::ScaleWidth,
            PrAnimatedProperty::AnchorPoint
        ]
    );
    // Before a later first key the static value is that key's; Scale Height
    // keeps its own.
    assert_eq!(transform.anchor_point, [0.25, 0.25]);
    assert_eq!(transform.scale, [110.0, 80.0]);
    assert!(converted.is_empty(), "{converted:?}");
    static_transform(&group);
    // Keys count from In. Anchor Point keys are source pixels, a nest's
    // source being the canvas; Scale Width keys move the X axis alone.
    assert_eq!(
        tracks,
        std::collections::BTreeMap::from([
            ("anchorPointX".to_owned(), vec![(200, 480.0), (800, 1440.0)]),
            ("anchorPointY".to_owned(), vec![(200, 270.0), (800, 540.0)]),
            ("scaleX".to_owned(), vec![(200, 110.0), (800, 140.0)]),
        ])
    );
    // The keys change no clock: the group and its picture play as the same
    // placement without Motion does.
    let range =
        |start: u64, duration: u64| serde_json::json!({"start": start, "duration": duration});
    assert_eq!(
        group["playback"],
        serde_json::json!({
            "type": "windowed",
            "inputRange": range(2_000, 1_000),
            "mapping": {
                "type": "linear",
                "input": range(2_000, 1_000),
                "output": range(0, 1_000)
            },
            "inputOffsetMs": 0
        })
    );
    assert_eq!(video(&group).1, range(1_000, 1_000));
    let (_, plain, plain_tracks, _) = import(&outer_xml(&[Placement {
        start: 2 * TICKS,
        end: 3 * TICKS,
        source_in: TICKS,
    }]));
    assert!(plain_tracks.is_empty(), "{plain_tracks:?}");
    assert_eq!(group["playback"], plain["playback"]);
    assert_eq!(video(&group), video(&plain));

    // At twice the speed the keys are omitted, and the same first-key values
    // stay static.
    let ((_, transform, _), group, tracks, converted) = import(&window(format!(
        r#"<Clip><PlaybackSpeed>2</PlaybackSpeed><Source ObjectRef="102"/><InPoint>{TICKS}</InPoint><OutPoint>{}</OutPoint>"#,
        3 * TICKS
    )));
    assert_eq!(transform.anchor_point, [0.25, 0.25]);
    assert_eq!(transform.scale, [110.0, 80.0]);
    let reasons: Vec<_> = converted
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        ["ScaleWidth", "AnchorPoint"].map(|property| format!(
            "{property} animation was not imported: keys on a retimed nested sequence occurrence are not converted; static values were kept"
        ))
    );
    static_transform(&group);
    assert!(tracks.is_empty(), "{tracks:?}");
}

#[test]
fn nest_window_trims_its_slow_inner_clip_in_proportion() {
    // Main's clip plays 0-1.5 s at half speed from source 10 s; the
    // placement's 0-1 s window shows only that slow clip.
    let xml = [
        (
            "<TrackItem><End>1270080000000</End></TrackItem>".to_owned(),
            format!("<TrackItem><End>{}</End></TrackItem>", 3 * TICKS / 2),
        ),
        (
            "<InPoint>0</InPoint><OutPoint>1270080000000</OutPoint>".to_owned(),
            format!(
                "<InPoint>{}</InPoint><OutPoint>{}</OutPoint><PlaybackSpeed>0.5</PlaybackSpeed>",
                10 * TICKS,
                10 * TICKS + 3 * TICKS / 4
            ),
        ),
        (
            "<OriginalDuration>2540160000000</OriginalDuration>".to_owned(),
            format!("<OriginalDuration>{}</OriginalDuration>", 60 * TICKS),
        ),
        (
            "<Duration>2540160000000</Duration>".to_owned(),
            format!("<Duration>{}</Duration>", 60 * TICKS),
        ),
    ]
    .into_iter()
    .fold(moved_nest(str::to_owned), |xml, (from, to)| {
        assert_eq!(xml.matches(&from).count(), 1, "{from}");
        xml.replace(&from, &to)
    });
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let outer = project.single_sequence().unwrap();
    let document = project_document_with_media(outer, &project.media);
    let group = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    let video = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    // The window keeps 1 s of 1.5 s, 500 of 750 ms of source, at the clip's
    // own rate.
    assert_eq!(
        video["playback"]["inputRange"],
        serde_json::json!({"start": 0, "duration": 1000})
    );
    assert_eq!(
        video["sourceRange"],
        serde_json::json!({"start": 10000, "duration": 500})
    );
    assert_eq!(video["playback"]["mapping"]["type"], "timeRemap");
    let keys: Vec<_> = video["playback"]["mapping"]["property"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["time"].as_i64().unwrap(),
                key["value"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(keys, [(0, 10000), (1000, 10500)]);
}

#[test]
fn moved_nests_keep_motion_crop_and_disabled_time_varying_rejections() {
    let crop = moved_nest(|records| {
        records.replace(
            "<Name>Crop Left</Name><ParameterID>8</ParameterID><StartKeyframe>-91445760000000000,0.,",
            "<Name>Crop Left</Name><ParameterID>8</ParameterID><StartKeyframe>-91445760000000000,10.,",
        )
    });
    assert_ne!(crop, moved_nest(str::to_owned));
    assert!(
        rejection(&crop).contains("Motion Crop Left"),
        "{}",
        rejection(&crop)
    );
    // Keys with IsTimeVarying false still conflict, on a nest as on a clip.
    let disabled = moved_nest(|records| {
        records.replace(
            "<Name>Scale</Name><ParameterID>2</ParameterID><IsTimeVarying>true</IsTimeVarying>",
            "<Name>Scale</Name><ParameterID>2</ParameterID><IsTimeVarying>false</IsTimeVarying>",
        )
    });
    assert_ne!(disabled, moved_nest(str::to_owned));
    assert!(
        rejection(&disabled).contains("keyframes conflict with disabled IsTimeVarying"),
        "{}",
        rejection(&disabled)
    );
}

/// Synthetic mixed-rate retimed nest: Outer (30 fps) places Main, at 29.97
/// fps, over 2 s plus 18 frames with In and Out on neither frame grid, at the
/// speed that its saved window confirms, with Optical Flow. Main's clip
/// plays 30 inner frames from source 5 s.
fn retimed_nest() -> String {
    const INNER_FRAME: i64 = 8_475_667_200;
    let placement = Placement {
        start: 2 * TICKS,
        end: 2 * TICKS + 18 * 8_467_200_000,
        source_in: 25_000_000_000,
    };
    let unit_out = placement.source_in + placement.end - placement.start;
    [
        (
            format!("<InPoint>25000000000</InPoint><OutPoint>{unit_out}</OutPoint></Clip>"),
            // Out - In is half the placement's span: 500/1001 inner frames
            // per outer frame.
            "<InPoint>25000000000</InPoint><OutPoint>101204800000</OutPoint><PlaybackSpeed>0.4995004995004995</PlaybackSpeed></Clip><TimeInterpolationType>2</TimeInterpolationType>".to_owned(),
        ),
        (
            format!(
                "<OriginalDuration>{}</OriginalDuration></VideoSequenceSource>",
                5 * TICKS
            ),
            format!(
                "<OriginalDuration>{}</OriginalDuration></VideoSequenceSource>",
                30 * INNER_FRAME
            ),
        ),
        (
            r#"<FrameRate>8467200000</FrameRate></TrackGroup><FrameRect>0,0,1920,1080</FrameRect><ComponentOwner><Components ObjectRef="2"/>"#
                .to_owned(),
            format!(
                r#"<FrameRate>{INNER_FRAME}</FrameRate></TrackGroup><FrameRect>0,0,1920,1080</FrameRect><ComponentOwner><Components ObjectRef="2"/>"#
            ),
        ),
        (
            "<TrackItem><End>1270080000000</End></TrackItem>".to_owned(),
            format!("<TrackItem><End>{}</End></TrackItem>", 30 * INNER_FRAME),
        ),
        (
            "<InPoint>0</InPoint><OutPoint>1270080000000</OutPoint>".to_owned(),
            format!(
                "<InPoint>{}</InPoint><OutPoint>{}</OutPoint>",
                5 * TICKS,
                5 * TICKS + 30 * INNER_FRAME
            ),
        ),
        (
            "<OriginalDuration>2540160000000</OriginalDuration>".to_owned(),
            format!("<OriginalDuration>{}</OriginalDuration>", 60 * TICKS),
        ),
        (
            "<Duration>2540160000000</Duration><FrameRate>8467200000</FrameRate>".to_owned(),
            format!(
                "<Duration>{}</Duration><FrameRate>{INNER_FRAME}</FrameRate>",
                60 * TICKS
            ),
        ),
    ]
    .into_iter()
    .fold(outer_xml(&[placement]), |xml, (from, to)| {
        assert_eq!(xml.matches(&from).count(), 1, "{from}");
        xml.replace(&from, &to)
    })
}

#[test]
fn mixed_rate_retimed_nest_maps_its_placement_onto_its_inner_window() {
    let (project, omissions) =
        inspect_project_with_omissions(&retimed_nest(), Some("outer")).unwrap();
    // Only the nest's Optical Flow is lost; its picture imports.
    let [omission] = omissions.as_slice() else {
        panic!("one omission: {omissions:?}");
    };
    assert_eq!(
        (omission.scope, omission.record.as_str()),
        (crate::OmissionScope::Feature, "VideoClip:113")
    );
    assert!(
        omission.reason.starts_with("Optical Flow time interpolation of a retimed nested sequence occurrence is not converted"),
        "{omission:?}"
    );
    let outer = project.single_sequence().unwrap();
    let [nest] = outer.nest_occurrences().collect::<Vec<_>>()[..] else {
        panic!("one nest");
    };
    // The saved window stays exact, off both frame grids.
    assert_eq!(nest.timeline_ticks(), 508_032_000_000..660_441_600_000);
    assert_eq!(
        nest.in_ticks..nest.out_ticks,
        25_000_000_000..101_204_800_000
    );
    assert_eq!(
        nest.sequence.frame_rate,
        crate::format::FrameRate::Fps30000Over1001
    );
    assert_eq!(nest.sequence.video_occurrences().count(), 1);

    let ids = crate::tesseract_output::asset_ids_in_order(outer, &project.media);
    let mut converted_omissions = Vec::new();
    let document = crate::convert::premiere_to_tesseract(
        outer,
        &project.media,
        &ids,
        &mut converted_omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(converted_omissions.is_empty(), "{converted_omissions:?}");
    let group = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    // 2-2.6 s plays inner 98.42-398.42 ms once: the group maps its window
    // onto the inner window, each end rounded once to whole milliseconds.
    let range =
        |start: u64, duration: u64| serde_json::json!({"start": start, "duration": duration});
    assert_eq!(
        group["playback"],
        serde_json::json!({
            "type": "windowed",
            "inputRange": range(2_000, 600),
            "mapping": {
                "type": "linear",
                "input": range(2_000, 600),
                "output": range(98, 300)
            },
            "inputOffsetMs": 0
        })
    );
    assert!(
        group["masks"].as_array().is_none_or(Vec::is_empty),
        "{group}"
    );
    // The picture keeps its authored place on the inner clock, untrimmed and
    // unshifted by In: inner 98 ms shows source 5.098 s once.
    let [video] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("one child: {group}");
    };
    assert_eq!(video["type"], "Video");
    assert_eq!(
        video["playback"],
        serde_json::json!({
            "type": "windowed",
            "inputRange": range(0, 1001),
            "mapping": {
                "type": "linear",
                "input": range(0, 1001),
                "output": range(5_000, 1001)
            },
            "inputOffsetMs": 0
        })
    );
    assert_eq!(video["sourceRange"], range(5_000, 1001));
}
