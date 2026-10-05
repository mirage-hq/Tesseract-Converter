use crate::{
    format::{inspect_project_with_omissions, FrameRate, MediaId},
    schema::{
        PrKeyframeEasing, PrMediaKind, PrProjectFile, PrTimeRemap, PrTimeRemapKeyframe,
        PrVideoTrack, STILL_INTRINSIC_TICKS, TICKS, TICKS_PER_MILLISECOND,
    },
    tests::support::{project_document_with_media, video_media, video_sequence},
};
use serde_json::json;

const THIRTY_FPS_TICKS: i64 = FrameRate::Fps30.ticks_per_frame();

#[test]
fn source_span_consistency_preserves_saved_native_rate() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-constant-rate-timing.xml");
    let (project, omissions) = PrProjectFile::load(path).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = project
        .sequences()
        .next()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    // Public conversion timing tests assert the exact tick ranges and editable
    // structure; the saved rate and remap flag require private model access.
    assert_eq!(clip.playback_rate, 2.936507936515802);
    assert!(clip.time_remap.is_none());

    // Independent second saved case from the same source hash as the fixture:
    // nested occurrence 1168 / VideoClip 1664. Its 6.4236273259948800-tick
    // residue at near-unit speed rules out a speed-scaled, first-case-only fix.
    // This tests window consistency, not support for that nest's effects.
    assert!(crate::schema::source_span_matches(
        194_940_345_600,
        194_929_623_888,
        0.9999450000029977,
    ));
}

#[test]
fn source_span_consistency_has_nanosecond_not_frame_precision() {
    use crate::schema::source_span_matches;
    for rate in [1.0, -1.0] {
        // One nanosecond is 254.016 ticks: test both sides of the integer edge,
        // with magnitudes small enough that arithmetic error is negligible.
        assert!(source_span_matches(10_000, 10_254, rate));
        assert!(source_span_matches(10_000, 9_746, rate));
        assert!(!source_span_matches(10_000, 10_255, rate));
        assert!(!source_span_matches(10_000, 9_745, rate));
        assert!(!source_span_matches(TICKS, TICKS + THIRTY_FPS_TICKS, rate));
    }
    // Arithmetic precision scales with magnitude, not duration in frames.
    // These i64 operands exceed binary64's exact-integer range; the first
    // discrepancy fits their conservative numeric budget, the second does not.
    let large = (1_i64 << 60) + 1;
    assert!(source_span_matches(large, large + 1024, 1.0));
    assert!(!source_span_matches(large, large + 1_000_000, 1.0));
}

#[test]
fn source_span_consistency_rejects_invalid_and_overflowing_inputs() {
    use crate::schema::source_span_matches;
    for rate in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!source_span_matches(TICKS, TICKS, rate));
    }
    for (timeline, source) in [(0, TICKS), (-TICKS, TICKS), (TICKS, 0), (TICKS, -TICKS)] {
        assert!(!source_span_matches(timeline, source, 1.0));
    }
    // Finite speed must not make an overflowing product/tolerance admit a span.
    assert!(!source_span_matches(i64::MAX, i64::MAX, f64::MAX));
    assert!(!source_span_matches(1, i64::MAX, f64::MAX));
}

#[test]
fn a_still_span_may_differ_from_its_placement_only_at_unit_forward_rate() {
    let still = PrMediaKind::Still { alpha: false };
    let video = PrMediaKind::Video {
        codec: None,
        hdr_profile: None,
    };
    // Premiere keeps a still's 5 s span from its one-hour in-point.
    let source_in = FrameRate::Fps30.generator_in_ticks();
    let span = source_in..source_in + 5 * TICKS;
    let remap = PrTimeRemap {
        keys: [(0, span.start), (10 * TICKS, span.end)]
            .map(|(timeline_ticks, source_ticks)| PrTimeRemapKeyframe {
                timeline_ticks,
                source_ticks,
                easing: PrKeyframeEasing::Linear,
            })
            .to_vec(),
    };
    // (case, media, placement, source range, rate, remap, error)
    #[rustfmt::skip]
    let cases = [
        ("an ordinary still", still, 0..5 * TICKS, span.clone(), 1.0, None, None),
        ("a still lengthened to 10 s", still, 0..10 * TICKS, span.clone(), 1.0, None, None),
        // Native images/nests inner item123 keeps a 5 s source span on a 2 s placement.
        ("a still shortened to 2 s", still, 0..2 * TICKS, span.clone(), 1.0, None, None),
        ("a shortened still at 2x", still, 0..2 * TICKS, span.clone(), 2.0, None, Some("source span")),
        ("a shortened still reversed", still, 0..2 * TICKS, span.clone(), -1.0, None, Some("source span")),
        ("a video lengthened to 10 s", video, 0..10 * TICKS, 0..5 * TICKS, 1.0, None, Some("source span")),
        ("a lengthened still at 2x", still, 0..10 * TICKS, span.clone(), 2.0, None, Some("source span")),
        ("a lengthened still reversed", still, 0..10 * TICKS, span.clone(), -1.0, None, Some("source span")),
        // Its rate, not the lengthening, fills the placement: the rate rule.
        ("a still at 0.5x", still, 0..10 * TICKS, span.clone(), 0.5, None, None),
        ("a lengthened still remapped", still, 0..10 * TICKS, span.clone(), 1.0, Some(remap), Some("TimeRemapping In to Out must match")),
        ("a still lengthened past its clock", still, 0..10 * TICKS, STILL_INTRINSIC_TICKS - 5 * TICKS..STILL_INTRINSIC_TICKS, 1.0, None, Some("frame past")),
        ("a still lengthened past i64", still, 0..i64::MAX, span.clone(), 1.0, None, Some("frame past")),
    ];
    for (case, kind, placement, source, rate, remap, error) in cases {
        let mut sequence = video_sequence();
        let mut media = video_media();
        let clip = sequence.video_tracks[0].clip_mut(0);
        (clip.start_ticks, clip.end_ticks) = (placement.start, placement.end);
        (clip.in_ticks, clip.out_ticks) = (source.start, source.end);
        (clip.playback_rate, clip.time_remap) = (rate, remap);
        let facts = media.get_mut(&clip.media).unwrap().video.as_mut().unwrap();
        facts.kind = kind;
        if kind.is_still() {
            facts.intrinsic_ticks = STILL_INTRINSIC_TICKS;
        }
        sequence.timeline_end_ticks = placement.end;
        match (sequence.validate_timeline(&media), error) {
            (Ok(()), None) => {}
            (Err(actual), Some(expected)) if actual.to_string().contains(expected) => {}
            (result, _) => panic!("{case}: {result:?}"),
        }
    }
}

#[test]
fn ranges_reject_negative_empty_out_of_bounds_and_rate_mismatched_clips() {
    for (ranges, expected) in [
        ((-1, 29, 0, 30, 60), "ranges"),
        ((0, 0, 0, 30, 60), "ranges"),
        ((0, 30, -1, 29, 60), "ranges"),
        ((0, 30, 30, 30, 60), "ranges"),
        ((0, 30, 32, 62, 60), "frame past"),
        ((0, 30, 0, 29, 60), "source span"),
    ] {
        let mut sequence = video_sequence();
        let mut media = video_media();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.start_ticks = ranges.0 * THIRTY_FPS_TICKS;
        clip.end_ticks = ranges.1 * THIRTY_FPS_TICKS;
        clip.in_ticks = ranges.2 * THIRTY_FPS_TICKS;
        clip.out_ticks = ranges.3 * THIRTY_FPS_TICKS;
        let video = media.get_mut(&clip.media).unwrap().video.as_mut().unwrap();
        video.intrinsic_ticks = ranges.4 * THIRTY_FPS_TICKS;
        let error = sequence.validate_timeline(&media).unwrap_err();
        assert!(error.to_string().contains(expected), "{ranges:?}: {error}");
    }

    let mut sequence = video_sequence();
    let mut upper = sequence.video_tracks[0].clip(0).clone();
    upper.out_ticks -= THIRTY_FPS_TICKS;
    sequence.video_tracks.push(PrVideoTrack::media([upper]));
    let error = sequence
        .validate_timeline(&video_media())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("video track 1") && error.contains("source span"),
        "{error}"
    );
}

#[test]
fn final_frame_hold_allows_one_sequence_frame_and_the_rounding_budget() {
    // Documented allowance: three nearest-millisecond errors of 0.5 ms each.
    let budget = 3 * TICKS_PER_MILLISECOND / 2;
    for frame_rate in [FrameRate::Fps30, FrameRate::Fps24000Over1001] {
        let frame = frame_rate.ticks_per_frame();
        // (case, timeline end, source in, source out, media duration, error)
        #[rustfmt::skip]
        let cases = [
            ("one frame sampled before the end", frame, frame - 1, 2 * frame - 1, frame, None),
            ("a second frame past the end", 2 * frame, frame - 1, 3 * frame - 1, frame, Some("frame past")),
            ("30 source frames fill 31", 31 * frame, 0, 31 * frame, 30 * frame, None),
            ("30 source frames cannot fill 32", 32 * frame, 0, 32 * frame, 30 * frame, Some("frame past")),
            ("final sample at the budget", 2 * frame, budget, 2 * frame + budget, frame, None),
            ("final sample past the budget", 2 * frame, budget + 1, 2 * frame + budget + 1, frame, Some("frame past")),
            ("source-in at the media end", frame, frame, 2 * frame, frame, Some("ranges")),
        ];
        for (case, end, source_in, source_out, intrinsic, error) in cases {
            let mut sequence = video_sequence();
            sequence.frame_rate = frame_rate;
            let mut media = video_media();
            let clip = sequence.video_tracks[0].clip_mut(0);
            (clip.end_ticks, clip.in_ticks, clip.out_ticks) = (end, source_in, source_out);
            let video = media.get_mut(&clip.media).unwrap().video.as_mut().unwrap();
            video.intrinsic_ticks = intrinsic;
            sequence.timeline_end_ticks = end;
            match (sequence.validate_timeline(&media), error) {
                (Ok(()), None) => {}
                (Err(actual), Some(expected)) if actual.to_string().contains(expected) => {}
                (result, _) => panic!("{frame_rate}, {case}: {result:?}"),
            }
        }
    }
}

#[test]
fn adjacent_and_gapped_placements_are_valid_but_overlap_and_reverse_order_are_not() {
    for (ranges, accepted) in [
        (vec![(0, 30), (30, 60)], true),
        (vec![(0, 15), (30, 45)], true),
        (vec![(0, 15), (14, 29)], false),
        (vec![(30, 45), (0, 15)], false),
        (vec![(0, 10), (10, 20), (10, 20)], false),
    ] {
        let mut sequence = video_sequence();
        let source = sequence.video_tracks[0].clip(0).clone();
        sequence.video_tracks[0] = PrVideoTrack::media(ranges.iter().map(|&(start, end)| {
            let mut clip = source.clone();
            clip.start_ticks = start * THIRTY_FPS_TICKS;
            clip.end_ticks = end * THIRTY_FPS_TICKS;
            clip.out_ticks = (end - start) * THIRTY_FPS_TICKS;
            clip
        }));
        let result = sequence.validate_timeline(&video_media());
        if accepted {
            result.unwrap();
        } else {
            let error = result.unwrap_err();
            assert!(
                error.to_string().contains("non-overlapping"),
                "{ranges:?}: {error}"
            );
        }
    }
}

#[test]
fn only_timeline_boundaries_require_sequence_frame_alignment() {
    // A fractional sequence rate also proves that the rule uses the sequence grid.
    let frame_rate = FrameRate::Fps24000Over1001;
    let mut sequence = video_sequence();
    let mut media = video_media();
    sequence.frame_rate = frame_rate;
    sequence.timeline_end_ticks = frame_rate.ticks_per_frame();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.end_ticks = frame_rate.ticks_per_frame();
    clip.in_ticks = 1;
    clip.out_ticks = clip.end_ticks + 1;
    let video = media.get_mut(&clip.media).unwrap().video.as_mut().unwrap();
    video.intrinsic_ticks += 1;
    sequence.validate_timeline(&media).unwrap();
    sequence.video_tracks[0].clip_mut(0).start_ticks += 1;
    sequence.video_tracks[0].clip_mut(0).end_ticks += 1;
    let error = sequence.validate_timeline(&media).unwrap_err().to_string();
    assert!(
        error.contains("timeline start") && error.contains("frame boundary"),
        "{error}"
    );
    sequence.video_tracks[0].clip_mut(0).start_ticks -= 1;
    sequence.video_tracks[0].clip_mut(0).out_ticks += 1;
    let error = sequence.validate_timeline(&media).unwrap_err().to_string();
    assert!(
        error.contains("timeline end") && error.contains("frame boundary"),
        "{error}"
    );
}

#[test]
fn timeline_end_may_follow_the_last_occurrence_between_samples_but_not_precede_it() {
    let media = video_media();
    let mut sequence = video_sequence();
    sequence.timeline_end_ticks = 4 * TICKS;
    let error = sequence.validate_timeline(&media).unwrap_err();
    assert!(error.to_string().contains(
        "timeline end 4000 ms precedes the last media occurrence end 5000 ms; trim or delete the occurrences past the timeline end"
    ), "{error}");
    let mut sequence = video_sequence();
    sequence.timeline_end_ticks = 5 * TICKS + 1;
    sequence.validate_timeline(&media).unwrap();
    // A trailing tail is valid; it is the last gap.
    let mut sequence = video_sequence();
    sequence.timeline_end_ticks = 5 * TICKS + 3 * THIRTY_FPS_TICKS;
    sequence.validate_timeline(&media).unwrap();
    assert_eq!(
        sequence.gaps(&media),
        vec![5 * TICKS..5 * TICKS + 3 * THIRTY_FPS_TICKS]
    );
}

#[test]
fn completed_project_validation_and_writer_reject_invalid_timeline() {
    let mut project = PrProjectFile::from_sequences(vec![video_sequence()], video_media());
    project.validate().unwrap();
    project.sequences[0].video_tracks[0].clip_mut(0).start_ticks = -THIRTY_FPS_TICKS;
    let error = project.validate().unwrap_err().to_string();
    assert!(
        error.contains("ranges") && error.contains("sequence"),
        "{error}"
    );
    let error = crate::format::writer::project_xml(&project)
        .unwrap_err()
        .to_string();
    assert!(error.contains("ranges"), "{error}");
}

#[test]
fn completed_project_validation_reports_every_invalid_selected_sequence() {
    let mut first = video_sequence();
    first.id = Some("first-guid".into());
    first.video_tracks[0].clip_mut(0).start_ticks = -THIRTY_FPS_TICKS;
    let mut second = video_sequence();
    second.id = Some("second-guid".into());
    second.name = "Other".into();
    second.video_tracks[0].clip_mut(0).end_ticks = 0;
    let project = PrProjectFile::from_sequences(vec![first, second], video_media());
    let error = project.validate().unwrap_err().to_string();
    assert!(
        error.contains("first-guid") && error.contains("second-guid"),
        "{error}"
    );
    assert!(error.contains("Main") && error.contains("Other"), "{error}");
}

#[test]
fn media_references_must_exist_and_be_used_somewhere_in_the_project() {
    let mut project = PrProjectFile::from_sequences(vec![video_sequence()], video_media());
    project.sequences[0].video_tracks[0].clip_mut(0).media = MediaId("missing".into());
    assert!(project
        .validate_media_references()
        .unwrap_err()
        .to_string()
        .contains("unknown media"));
    project.sequences[0].video_tracks[0].clip_mut(0).media = MediaId("source".into());
    project.media.insert(
        MediaId("unused".into()),
        project.media[&MediaId("source".into())].clone(),
    );
    assert!(project
        .validate_media_references()
        .unwrap_err()
        .to_string()
        .contains("unreferenced media"));
    let other = video_sequence();
    project.sequences.push(other);
    project.sequences[1].video_tracks[0].clip_mut(0).media = MediaId("unused".into());
    project.validate_media_references().unwrap();
}

#[test]
fn video_animation_keys_preserve_long_tracks_and_validate_values() {
    use crate::schema::{
        PrKeyframeEasing::Linear, PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe,
    };
    let scalar = |count: usize, value: f64| -> Vec<_> {
        (0..count)
            .map(|index| PrScalarKeyframe {
                source_ticks: index as i64 * TICKS_PER_MILLISECOND,
                value,
                easing: Linear,
            })
            .collect()
    };
    let count = "animation must have at least one key";
    let values = "animation keys must have finite values and strictly increasing source times";
    let mut repeated = scalar(2, 100.0);
    repeated[1].source_ticks = repeated[0].source_ticks;
    let not_finite = PrPointKeyframe {
        source_ticks: 0,
        value: [f64::NAN, 0.5],
        easing: Linear,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    };
    for (animation, expected) in [
        (PrPropertyAnimation::Opacity(Vec::new()), Some(count)),
        (PrPropertyAnimation::Rotation(scalar(4096, 0.0)), None),
        (PrPropertyAnimation::Rotation(scalar(4097, 0.0)), None),
        (
            PrPropertyAnimation::Position(vec![not_finite]),
            Some(values),
        ),
        (PrPropertyAnimation::UniformScale(repeated), Some(values)),
        // Opacity checks its range after the keys.
        (
            PrPropertyAnimation::Opacity(scalar(1, 150.0)),
            Some("opacity animation values must be between 0 and 100"),
        ),
    ] {
        let mut sequence = video_sequence();
        sequence.video_tracks[0].clip_mut(0).animations = vec![animation];
        let result = sequence.validate_timeline(&video_media());
        match expected {
            None => result.unwrap(),
            Some(message) => {
                let error = result.unwrap_err().to_string();
                assert!(error.ends_with(message), "{error}");
            }
        }
    }
}

#[test]
fn linear_wipe_completion_past_the_former_limit_keeps_its_range_and_order_checks() {
    use crate::schema::{PrKeyframeEasing, PrLinearWipe, PrScalarKeyframe};
    let validate = |completion: Vec<PrScalarKeyframe>| {
        let mut sequence = video_sequence();
        sequence.video_tracks[0].clip_mut(0).linear_wipe = Some(PrLinearWipe {
            initial_completion: 0.0,
            completion,
            angle_degrees: 90,
            feather: 0.0,
        });
        sequence.validate_timeline(&video_media())
    };
    // 5,000 keys, above the 4,096 that this check once allowed, a millisecond
    // apart.
    let keys: Vec<_> = (0..5000)
        .map(|index: i64| PrScalarKeyframe {
            source_ticks: index * TICKS_PER_MILLISECOND,
            value: (index % 101) as f64,
            easing: PrKeyframeEasing::Linear,
        })
        .collect();
    validate(keys.clone()).unwrap();
    let with_last = |change: &dyn Fn(&mut PrScalarKeyframe, i64)| {
        let mut keys = keys.clone();
        let before = keys[4998].source_ticks;
        change(&mut keys[4999], before);
        keys
    };
    validate(Vec::new()).unwrap(); // The initial value is a constant wipe.
    let mut swapped = keys.clone();
    swapped.swap(4998, 4999);
    for (case, keys) in [
        ("above 100", with_last(&|key, _| key.value = 100.5)),
        ("nonfinite", with_last(&|key, _| key.value = f64::NAN)),
        (
            "repeated time",
            with_last(&|key, before| key.source_ticks = before),
        ),
        ("swapped times", swapped),
    ] {
        let error = validate(keys).unwrap_err().to_string();
        assert!(
            error.contains("Linear Wipe completion must have bounded, increasing keys"),
            "{case}: {error}"
        );
    }
}

/// Synthetic forward unit-speed spans on the 30 fps sequence, in 30 fps
/// frames: Start, End, InPoint, OutPoint and the media duration. The saved Out
/// of `LATE_OUT` is one sequence frame past In plus the duration; that of
/// `EARLY_OUT` is one frame short of it.
const LATE_OUT: [i64; 5] = [60, 90, 300, 331, 1800];
const EARLY_OUT: [i64; 5] = [150, 195, 600, 644, 1800];

fn span_ticks(frames: [i64; 5]) -> [i64; 5] {
    frames.map(|frames| frames * THIRTY_FPS_TICKS)
}

/// `one-clip.xml` whose clip has `span` and 29.97 fps media of its duration.
fn unit_span_xml([start, end, source_in, source_out, intrinsic]: [i64; 5]) -> String {
    let source = include_str!("../../../tests/fixtures/one-clip.xml");
    [
        (
            "<TrackItem><End>1270080000000</End></TrackItem>",
            format!("<TrackItem><Start>{start}</Start><End>{end}</End></TrackItem>"),
        ),
        (
            "<InPoint>0</InPoint><OutPoint>1270080000000</OutPoint>",
            format!("<InPoint>{source_in}</InPoint><OutPoint>{source_out}</OutPoint>"),
        ),
        (
            "<OriginalDuration>2540160000000</OriginalDuration>",
            format!("<OriginalDuration>{intrinsic}</OriginalDuration>"),
        ),
        (
            "<Duration>2540160000000</Duration><FrameRate>8467200000</FrameRate>",
            format!("<Duration>{intrinsic}</Duration><FrameRate>8475667200</FrameRate>"),
        ),
    ]
    .into_iter()
    .fold(source.to_owned(), |xml, (from, to)| {
        assert!(xml.contains(from), "{from}");
        xml.replace(from, &to)
    })
}

#[test]
fn unit_speed_clip_plays_from_in_for_its_duration_past_a_one_frame_stale_out() {
    // Synthetic structural check: a saved Out one sequence frame later or
    // earlier than In plus the duration still plays from In for the duration.
    for (item, frames, out_frames, active, source) in [
        (
            "late",
            LATE_OUT,
            1,
            json!({"start":2000,"duration":1000}),
            json!({"start":10000,"duration":1000}),
        ),
        (
            "early",
            EARLY_OUT,
            -1,
            json!({"start":5000,"duration":1500}),
            json!({"start":20000,"duration":1500}),
        ),
    ] {
        let span @ [start, end, source_in, source_out, _] = span_ticks(frames);
        assert_eq!(
            source_out - source_in - (end - start),
            out_frames * THIRTY_FPS_TICKS
        );
        let (project, omissions) =
            inspect_project_with_omissions(&unit_span_xml(span), None).unwrap();
        assert!(omissions.is_empty(), "{item}: {omissions:?}");
        let sequence = project.single_sequence().unwrap();
        let clip = sequence.video_tracks[0].clip(0);
        assert_eq!((clip.start_ticks, clip.end_ticks), (start, end), "{item}");
        assert_eq!(
            (clip.in_ticks, clip.out_ticks),
            (source_in, source_in + end - start),
            "{item}"
        );
        assert_eq!(clip.playback_rate, 1.0, "{item}");
        let document = project_document_with_media(sequence, &project.media);
        let layer = document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        assert_eq!(crate::test_support::layer_range(layer), &active, "{item}");
        assert_eq!(layer["sourceRange"], source, "{item}");
        // Unit speed from In: a linear window-to-source mapping, no remap.
        assert_eq!(
            layer["playback"],
            crate::test_support::linear_playback(active, source),
            "{item}: {layer}"
        );
    }
}

#[test]
fn stale_out_allowance_is_one_frame_of_forward_unit_playback_within_the_media() {
    // The only clip is omitted, so the sequence has nothing to convert.
    let reason = |xml: &str| {
        inspect_project_with_omissions(xml, None)
            .unwrap_err()
            .to_string()
    };
    // An empty saved range is malformed, not stale, even a frame from the
    // played end.
    let empty = unit_span_xml([0, THIRTY_FPS_TICKS, 0, 0, 10 * THIRTY_FPS_TICKS]);
    assert!(
        reason(&empty).contains("invalid timeline/source ranges"),
        "{}",
        reason(&empty)
    );
    let [start, end, source_in, source_out, intrinsic] = span_ticks(LATE_OUT);
    // Two frames off is no stale endpoint; nor is a frame off another rate or
    // direction, whose source span the saved Out still determines.
    let two_frames = unit_span_xml([
        start,
        end,
        source_in,
        source_out + THIRTY_FPS_TICKS,
        intrinsic,
    ]);
    let late = unit_span_xml(span_ticks(LATE_OUT));
    for (case, xml) in [
        ("two frames", two_frames),
        (
            "speed",
            late.replace("<InPoint>", "<PlaybackSpeed>1.5</PlaybackSpeed><InPoint>"),
        ),
        (
            "reverse",
            late.replace("<InPoint>", "<PlayBackwards>true</PlayBackwards><InPoint>"),
        ),
    ] {
        let reason = reason(&xml);
        assert!(
            reason.contains("source span does not match"),
            "{case}: {reason}"
        );
    }
    // The played end, not the saved Out, must lie within the media: the
    // frame-short Out plays one frame past it.
    let [start, end, source_in, source_out, _] = span_ticks(EARLY_OUT);
    let media_end = source_out - THIRTY_FPS_TICKS;
    let past = reason(&unit_span_xml([
        start, end, source_in, source_out, media_end,
    ]));
    assert!(past.contains("frame past the media end"), "{past}");
}
