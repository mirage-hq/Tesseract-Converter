//! Reader and writer rules for the Track Matte Key.

use super::{
    animation::animation_fixture::{track_matte_key_xml, with_matte_track, MATTE_TRACK, SOURCE},
    effects::{blur, invert, tint, top_crop, track_matte_key, with_second_clip},
    nested::{
        corpus_motion_component, placement_chain, placement_records, read_motion, sequence_records,
        sequence_source, with_records, Placement, MOTION,
    },
};
use crate::{
    format::{inspect_project_with_omissions, writer::project_xml, Graph},
    schema::{
        check_track_matte, native::VideoComponentParam, MediaId, PrKeyframeEasing, PrMatteChannel,
        PrNestOccurrence, PrPropertyAnimation, PrScalarKeyframe, PrStaticTransform, PrTrackMatte,
        PrVideoOccurrence, PrVideoTrack, TICKS,
    },
    tests::support::{clip_of, nest_of, sequence_of, video_media},
    OmissionScope,
};

#[test]
fn non_canvas_transform_matte_keeps_the_existing_source_size_rejection() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_transform_track_matte_26_5_strict.prproj");
    let original = crate::format::read_xml(&path).unwrap();
    // Both saved orders remain outside the Track Matte source-size boundary,
    // even when their matte carries an effect without a mapping.
    for chain in ["122", "124"] {
        let mut xml = original.clone();
        for id in ["77", chain] {
            let parsed = roxmltree::Document::parse(&xml).unwrap();
            let record = parsed
                .root_element()
                .children()
                .find(|node| node.attribute("ObjectID") == Some(id))
                .unwrap();
            let before = &xml[record.range()];
            let after = if id == "77" {
                before.replace("0,0,1920,1080", "0,0,1280,720")
            } else {
                before.replace("</ComponentChain>", "<Components Version=\"1\"><Component Index=\"0\" ObjectRef=\"900\"/></Components></ComponentChain>")
            };
            assert_ne!(before, after);
            let range = record.range();
            xml.replace_range(range, &after);
        }
        let end = xml.rfind("</").unwrap();
        xml.insert_str(end, "<VideoFilterComponent ObjectID=\"900\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><ID>99</ID><DisplayName>Own unsupported probe</DisplayName><Bypass>false</Bypass></Component><VideoFilterType>2</VideoFilterType><MatchName>Own.Unsupported.Probe</MatchName></VideoFilterComponent>");
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("18832324-570e-4e73-8460-84b8c8150813"))
                .unwrap();
        assert_eq!(
            omissions
                .iter()
                .filter(|omission| omission
                    .reason
                    .contains("Track Matte Key on media that is not sequence-sized"))
                .count(),
            2,
            "{chain}: {omissions:#?}"
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains("Own.Unsupported.Probe")),
            "{omissions:#?}"
        );
        assert!(
            !omissions
                .iter()
                .any(|omission| omission.reason.contains("measured A4")),
            "{omissions:#?}"
        );
        assert!(!project.sequences[0]
            .video_tracks
            .iter()
            .flat_map(|track| &track.items)
            .filter_map(crate::schema::PrVideoItem::media)
            .any(|clip| clip.track_matte.is_some()));
    }
}

/// `food_promo` `VideoClipTrackItem:86` (Premiere 14.4): its Opacity
/// (`VideoFilterComponent:126`, 23 %, Normal) and Track Matte Key
/// (`:127`, Matte 3 = the track ID of `Index` 2, Matte Alpha), verbatim.
const FOOD_PROMO_OPACITY: &str = "<VideoFilterComponent ObjectID=\"126\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"8\"><Component Version=\"6\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"148\"/><Param Index=\"1\" ObjectRef=\"149\"/><Param Index=\"2\" ObjectRef=\"150\"/></Params><ID>2</ID><DisplayName>Opacity</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>AE.ADBE Opacity</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"148\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>Opacity</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,23.,0,0,0,0,0,0</StartKeyframe><CurrentValue>23</CurrentValue><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"149\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Blend Mode</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>10</ParameterControlType><StartKeyframe>-91445760000000000,18,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>26</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"150\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Blend Mode</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>31</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>";
const FOOD_PROMO_TRACK_MATTE_KEY: &str = "<VideoFilterComponent ObjectID=\"127\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"8\"><Component Version=\"6\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"151\"/><Param Index=\"1\" ObjectRef=\"152\"/><Param Index=\"2\" ObjectRef=\"153\"/></Params><ID>3</ID><DisplayName>Track Matte Key</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>AE.ADBE Legacy Key Track Matte</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"151\" ClassID=\"2f2eb0a3-318c-4a93-99fc-f1d319edc864\" Version=\"9\"><Name>Matte:</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>13</ParameterControlType><StartKeyframe>-91445760000000000,3,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4294967295</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"152\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Composite Using:</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>1</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"153\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\"><Name>Reverse</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>";

/// `corporate_slideshow` `VideoClipTrackItem:1176` (Premiere 12.x): its
/// Track Matte Key `VideoFilterComponent:1799`, `Version` 7 with `Component`
/// 5 and a `Node`, Matte 13 (the track ID of `Index` 12), Matte Luma, verbatim.
const CORPORATE_TRACK_MATTE_KEY: &str = "<VideoFilterComponent ObjectID=\"1799\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><Node Version=\"1\"><Properties Version=\"1\"><ECP.Filter.Expanded>false</ECP.Filter.Expanded></Properties></Node><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"2486\"/><Param Index=\"1\" ObjectRef=\"2487\"/><Param Index=\"2\" ObjectRef=\"2488\"/></Params><ID>3</ID><DisplayName>Track Matte Key</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Legacy Key Track Matte</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"2486\" ClassID=\"2f2eb0a3-318c-4a93-99fc-f1d319edc864\" Version=\"9\"><Name>Matte:</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>13</ParameterControlType><StartKeyframe>-91445760000000000,13,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4294967295</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"2487\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Composite Using:</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,1,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>1</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"2488\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\"><Name>Reverse</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>";

#[test]
fn track_matte_keys_read_their_matte_track_by_id_channel_and_chain_position() {
    // The matte track carries `Track/ID` 7 at `Index` 1, so the key's Matte 7
    // resolves to track 1, not to a seventh track.
    // Each corpus record names its own matte track ID; the matte track takes it.
    for (case, matte_track_id, components, channel, opacity, effects, above) in [
        // `horror_title` `VideoClipTrackItem:89`: Invert at Index 0 and the
        // key at Index 1, which applies first; the Invert follows it.
        (
            "horror_title, Invert after the key",
            7,
            vec![(20, invert(20)), (30, track_matte_key(30))],
            PrMatteChannel::Alpha,
            100.0,
            1,
            0,
        ),
        (
            "food_promo, Opacity 23 and the key",
            3,
            vec![
                (126, FOOD_PROMO_OPACITY.to_owned()),
                (127, FOOD_PROMO_TRACK_MATTE_KEY.to_owned()),
            ],
            PrMatteChannel::Alpha,
            23.0,
            0,
            0,
        ),
        (
            "corporate_slideshow 12.x record, Matte Luma",
            13,
            vec![(1799, CORPORATE_TRACK_MATTE_KEY.to_owned())],
            PrMatteChannel::Luma,
            100.0,
            0,
            0,
        ),
        // The blur at Index 1 applies before the key at Index 0.
        (
            "Gaussian Blur before the key",
            7,
            vec![(20, track_matte_key(20)), (30, blur(30))],
            PrMatteChannel::Alpha,
            100.0,
            1,
            1,
        ),
        // Fixture clip D: Reverse with Matte Alpha is one minus the alpha (G3a).
        (
            "Reverse with Matte Alpha",
            7,
            vec![(20, track_matte_key_xml(20, 7, 0, true))],
            PrMatteChannel::AlphaInverted,
            100.0,
            0,
            0,
        ),
    ] {
        let xml = with_matte_track(SOURCE, &components).replace(
            "<ID>7</ID><Index>1</Index>",
            &format!("<ID>{matte_track_id}</ID><Index>1</Index>"),
        );
        // An explicit Opacity component replaces the chain's default flag.
        let xml = if opacity == 100.0 {
            xml
        } else {
            xml.replacen("<DefaultOpacity>true</DefaultOpacity>", "", 1)
        };
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        assert!(
            omissions
                .iter()
                .all(|omission| omission.scope != OmissionScope::Occurrence),
            "{case}: {omissions:?}"
        );
        let tracks = &project.sequences[0].video_tracks;
        let clip = tracks[0].clip(0);
        assert_eq!(
            clip.track_matte,
            Some(PrTrackMatte {
                track_index: 1,
                channel
            }),
            "{case}"
        );
        assert_eq!(
            (clip.effects.len(), clip.effects_above_mask, clip.opacity),
            (effects, above, opacity),
            "{case}"
        );
        let matte = tracks[1].clip(0);
        assert_eq!(matte.id.as_deref(), Some("VideoClipTrackItem:93"), "{case}");
        assert!(matte.enabled && matte.track_matte.is_none(), "{case}");
    }
}

#[test]
fn an_omitted_track_below_the_matte_track_does_not_shift_the_matte() {
    // Native tracks 0-3: the keyed clip, a track whose record is missing
    // (omitted whole), the matte track (`Track/ID` 7, `Index` 2, item 93)
    // and an unrelated track (ID 8, `Index` 3, item 95 over the same range).
    // The kept tracks are 0, 2 and 3, so Matte 7 is kept track 1: item 93,
    // not item 95 at native `Index` 2's position.
    let unrelated = Placement {
        start: 0,
        end: 5 * TICKS,
        source_in: 0,
    };
    let xml = with_matte_track(SOURCE, &[(20, track_matte_key(20))])
        .replace(
            "<Track Index=\"1\" ObjectURef=\"track-2\"/></Tracks>",
            "<Track Index=\"1\" ObjectURef=\"track-missing\"/><Track Index=\"2\" ObjectURef=\"track-2\"/><Track Index=\"3\" ObjectURef=\"track-4\"/></Tracks>",
        )
        .replace(
            "<Track><ID>7</ID><Index>1</Index></Track><ClipItems><Index>1</Index>",
            "<Track><ID>7</ID><Index>2</Index></Track><ClipItems><Index>2</Index>",
        )
        .replace(
            "</PremiereData>",
            &format!(
                "<VideoClipTrack ObjectUID=\"track-4\"><ClipTrack><Track><ID>8</ID><Index>3</Index></Track><ClipItems><Index>3</Index><TrackItems><TrackItem ObjectRef=\"95\"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>{}</PremiereData>",
                placement_records(95, 7, &unrelated)
            ),
        );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert_eq!(
        omissions
            .iter()
            .map(|omission| omission.scope)
            .collect::<Vec<_>>(),
        [OmissionScope::Track],
        "{omissions:?}"
    );
    let tracks = &project.sequences[0].video_tracks;
    let fill = tracks[0].clip(0);
    let matte = fill.track_matte.expect("the key converts");
    assert_eq!(matte.track_index, 1);
    assert_eq!(
        tracks[matte.track_index].clip(0).id.as_deref(),
        Some("VideoClipTrackItem:93")
    );
    assert_eq!(tracks.len(), 3);
    assert_eq!(
        tracks[2].clip(0).id.as_deref(),
        Some("VideoClipTrackItem:95")
    );
}

/// "Outer" places `one-clip.xml`'s sequence on V1 (item 110) with the key
/// (130), after `motion` in its chain when there is one, and its media source
/// on V2 (item 120), both over 0-2 s; the key names V2 by its `Track/ID` 2. A
/// clip at 3-4 s on V2 (item 125) keeps Outer convertible without them.
fn keyed_nest_xml(motion: Option<&str>) -> String {
    let placement = Placement {
        start: 0,
        end: 2 * TICKS,
        source_in: 0,
    };
    let key = track_matte_key_xml(130, 2, 1, false);
    let chain = match motion {
        Some(motion) => placement_chain(
            111,
            "<DefaultOpacity>true</DefaultOpacity>",
            &[(300, motion.to_owned()), (130, key)],
        ),
        None => placement_chain(
            111,
            "<DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity>",
            &[(130, key)],
        ),
    };
    let mut records = sequence_records("outer", "Outer", 100, &[110]).replace(
        "<Track ObjectURef=\"outer-track\"/></Tracks>",
        "<Track ObjectURef=\"outer-track\"/><Track Index=\"1\" ObjectURef=\"outer-track-2\"/></Tracks>",
    );
    records.push_str(&sequence_source(102, "sequence-1"));
    records.push_str(&placement_records(110, 102, &placement).replace(
        "<VideoComponentChain ObjectID=\"111\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
        &chain,
    ));
    records.push_str("<VideoClipTrack ObjectUID=\"outer-track-2\"><ClipTrack><Track><ID>2</ID><Index>1</Index></Track><ClipItems><Index>1</Index><TrackItems><TrackItem ObjectRef=\"120\"/><TrackItem ObjectRef=\"125\"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>");
    records.push_str(&placement_records(120, 7, &placement));
    records.push_str(&placement_records(
        125,
        7,
        &Placement {
            start: 3 * TICKS,
            end: 4 * TICKS,
            source_in: 0,
        },
    ));
    with_records(SOURCE, &records)
}

#[test]
fn a_nested_placement_keeps_its_track_matte_key() {
    let xml = keyed_nest_xml(None);
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let outer = &project.sequences[0];
    let nest = outer.nest_occurrences().next().expect("the nest");
    assert_eq!(
        (nest.id.as_deref(), nest.track_matte),
        (
            Some("VideoClipTrackItem:110"),
            Some(PrTrackMatte {
                track_index: 1,
                channel: PrMatteChannel::Luma,
            })
        )
    );
    assert_eq!(
        outer.video_tracks[1].clip(0).id.as_deref(),
        Some("VideoClipTrackItem:120")
    );

    // Its Opacity fades the keyed picture: Opacity and the key both scale its
    // alpha, so in either order, and the nest keeps both.
    let chain = super::nested::opacity_chain(111, 410, 60.0, "").replace(
        "<Component Index=\"0\" ObjectRef=\"410\"/>",
        "<Component Index=\"0\" ObjectRef=\"410\"/><Component Index=\"1\" ObjectRef=\"130\"/>",
    );
    let start = xml.find(r#"<VideoComponentChain ObjectID="111""#).unwrap();
    let end = start
        + xml[start..].find("</VideoComponentChain>").unwrap()
        + "</VideoComponentChain>".len();
    let mut faded = xml.clone();
    faded.replace_range(start..end, &chain);
    assert_ne!(faded, xml);
    let (project, omissions) = inspect_project_with_omissions(&faded, Some("outer")).unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let faded_sequence = &project.sequences[0];
    let nest = faded_sequence.nest_occurrences().next().expect("the nest");
    assert_eq!((nest.opacity, nest.track_matte.is_some()), (60.0, true));

    // A keyed nest moves only without a Track Matte Key: with one, the order
    // of the key against its Motion is unmeasured, so the nest is omitted.
    // Its matte clip then draws nothing either; the clip at 3-4 s converts.
    let motion = super::nested::keyed_motion(300);
    let (project, omissions) =
        inspect_project_with_omissions(&keyed_nest_xml(Some(&motion)), Some("outer")).unwrap();
    assert!(
        omissions.iter().any(|omission| omission.record == "110"
            && omission.reason.ends_with(
                "VideoClipTrackItem:110: Motion with a Track Matte Key on a nested sequence occurrence is not converted"
            )),
        "{omissions:?}"
    );
    let outer = &project.sequences[0];
    assert_eq!(outer.nest_occurrences().count(), 0);
    let kept: Vec<_> = outer
        .video_items()
        .filter_map(crate::schema::PrVideoItem::id)
        .collect();
    assert_eq!(kept, ["VideoClipTrackItem:125"]);

    // So is a nest of another canvas at default Motion, which places that
    // canvas in the outer one and so would move the keyed picture.
    let sized = xml
        .replacen(
            r#"</TrackGroup><FrameRect>0,0,1920,1080</FrameRect><ComponentOwner><Components ObjectRef="2"/>"#,
            r#"</TrackGroup><FrameRect>0,0,1080,1920</FrameRect><ComponentOwner><Components ObjectRef="2"/>"#,
            1,
        )
        .replacen(
            r#"<SubClip ObjectRef="5"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect>"#,
            r#"<SubClip ObjectRef="5"/></ClipTrackItem><FrameRect>0,0,1080,1920</FrameRect>"#,
            1,
        );
    assert_ne!(sized, xml);
    let mut omissions = Vec::new();
    let read = crate::format::reader::read_sequence(
        &Graph::parse(&sized).unwrap(),
        Some("outer"),
        &std::collections::BTreeSet::new(),
        &mut std::collections::BTreeMap::new(),
        &mut omissions,
    );
    let sequence = read.unwrap();
    assert_eq!(sequence.nest_occurrences().count(), 0);
    assert_eq!(sequence.video_occurrences().count(), 1);
    assert!(
        omissions.iter().any(|omission| omission.record == "110"
            && omission.reason.ends_with(
                "VideoClipTrackItem:110: a Track Matte Key on a nested sequence occurrence of another canvas is not converted"
            )),
        "{omissions:?}"
    );
}

/// A keyed nest moves only without a Track Matte Key, whose order against its
/// Motion is unmeasured: a keyed nest with static Motion is omitted, and so is
/// its matte clip, which Premiere does not draw while the key names its track.
/// The clip after it converts.
#[test]
fn a_keyed_nest_with_static_motion_is_omitted_with_its_matte_clip() {
    let motion = corpus_motion_component(300, MOTION, &[]);
    let (project, omissions) =
        inspect_project_with_omissions(&keyed_nest_xml(Some(&motion)), Some("outer")).unwrap();
    let reported: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
        .collect();
    assert_eq!(
        reported,
        [
            (
                "110",
                "unsupported conversion: VideoClipTrackItem:110: Motion with a Track Matte Key on a nested sequence occurrence is not converted"
            ),
            (
                "VideoClipTrackItem:120",
                "matte source of the omitted clip VideoClipTrackItem:110 was not converted: Premiere does not draw a track-matte source"
            ),
        ]
    );
    let outer = &project.sequences[0];
    assert_eq!(outer.nest_occurrences().count(), 0);
    let kept: Vec<_> = outer
        .video_items()
        .filter_map(crate::schema::PrVideoItem::id)
        .collect();
    assert_eq!(kept, ["VideoClipTrackItem:125"]);
}

/// The matte names the rendered nested picture, including its Motion. Its
/// editable group clips to the source canvas before applying that Motion.
#[test]
fn a_moved_nest_keeps_its_track_matte_source_identity() {
    let keyed = PrVideoOccurrence {
        track_matte: Some(PrTrackMatte {
            track_index: 1,
            channel: PrMatteChannel::Alpha,
        }),
        ..clip_of("source", 0..2 * TICKS, 0)
    };
    // Rotation keys from the default 0 degrees: the static Motion stays the
    // default, but the keys move the nest.
    let rotation = PrPropertyAnimation::Rotation(
        [(0, 0.0), (TICKS, 90.0)]
            .map(|(source_ticks, value)| PrScalarKeyframe {
                source_ticks,
                value,
                easing: PrKeyframeEasing::Linear,
            })
            .to_vec(),
    );
    for (transform, animations) in [
        (PrStaticTransform::default(), Vec::new()),
        (read_motion(), Vec::new()),
        (PrStaticTransform::default(), vec![rotation]),
    ] {
        let keys = animations.len();
        let matte = PrNestOccurrence {
            transform,
            animations,
            ..nest_of(sequence_of("Titles", Vec::new()), 0..2 * TICKS, 0)
        };
        let tracks = [
            PrVideoTrack::media([keyed.clone()]),
            PrVideoTrack {
                transitions: Vec::new(),
                items: Vec::new(),
                nests: vec![matte],
            },
        ];
        assert_eq!(
            check_track_matte(&tracks, 0, 0..2 * TICKS, keyed.track_matte.unwrap()),
            Ok(()),
            "{transform:?}, {keys} keyed properties"
        );
    }
}

/// The matte track of [`MATTE_TRACK`] holding `items` instead: each an item
/// `id` over `start..end` ticks playing source 7 from 0, with the records of
/// `placement_records`.
fn matte_track_of(items: &[(u32, i64, i64)]) -> String {
    let references: String = items
        .iter()
        .map(|(id, _, _)| format!("<TrackItem ObjectRef=\"{id}\"/>"))
        .collect();
    let mut records = format!(
        "<VideoClipTrack ObjectUID=\"track-2\"><ClipTrack><Track><ID>7</ID><Index>1</Index></Track><ClipItems><Index>1</Index><TrackItems>{references}</TrackItems></ClipItems></ClipTrack></VideoClipTrack>"
    );
    for &(id, start, end) in items {
        records.push_str(&placement_records(
            id,
            7,
            &Placement {
                start,
                end,
                source_in: 0,
            },
        ));
    }
    records
}

#[test]
fn track_matte_keys_outside_the_supported_form_omit_the_occurrence() {
    let key = |id| track_matte_key(id);
    let keyed_matte = key(20).replace(
        "<IsTimeVarying>false</IsTimeVarying><ParameterControlType>13</ParameterControlType><StartKeyframe>-91445760000000000,7,0,0,0,0,0,0</StartKeyframe>",
        "<IsTimeVarying>true</IsTimeVarying><ParameterControlType>13</ParameterControlType><StartKeyframe>-91445760000000000,7,0,0,0,0,0,0</StartKeyframe><Keyframes>0,7,0,0,0,0,0,0;254016000000,1,0,0,0,0,0,0;</Keyframes>",
    );
    assert_ne!(keyed_matte, key(20));
    let with_private_data = key(20).replace(
        "</Component><MatchName>",
        "</Component><PremiereFilterPrivateData Encoding=\"base64\">AAAA</PremiereFilterPrivateData><MatchName>",
    );
    // A second clip (`VideoClipTrackItem:9`, 5-10 s) keeps the sequence
    // convertible when the keyed clip and its matte are both omitted.
    let source = with_second_clip(SOURCE);
    let base = with_matte_track(&source, &[(20, key(20))]);
    let portrait_source = source.replace(
        "<FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
        "<FrameRect>0,0,1080,1920</FrameRect></VideoStream>",
    );
    assert_ne!(portrait_source, source);
    for (case, xml, reason) in [
        (
            "keyed Matte",
            with_matte_track(&source, &[(20, keyed_matte)]),
            "VideoFilterComponent:20: keyframed Matte is not supported; only static values convert",
        ),
        // Fixture clip E: Premiere shows the clip outside the matte item, where
        // the matte has no luma (G3b).
        (
            "Reverse with Matte Luma",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 7, 1, true))]),
            "VideoFilterComponent:20: Reverse with Matte Luma is not converted; Premiere gives the matte clip's zero-luma exterior full coverage where FX's inverted luma matte gives none",
        ),
        (
            "unknown Composite Using",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 7, 2, false))]),
            "VideoFilterComponent:20: Composite Using \"2\" is neither Matte Alpha (0) nor Matte Luma (1)",
        ),
        (
            "Matte 0",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 0, 0, false))]),
            "VideoFilterComponent:20: Matte \"0\" names no video track",
        ),
        // Premiere 26.5.1 saves a fresh key's Matte None as 4294967295 (G6).
        (
            "Matte None",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 4294967295, 0, false))]),
            "VideoFilterComponent:20: Matte None selects no matte track; a key without a matte is not converted",
        ),
        (
            "Matte names no track",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 9, 0, false))]),
            "VideoClipTrackItem:3: Track Matte Key Matte 9 names no video track of the sequence",
        ),
        (
            "Matte names the clip's own track",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 1, 0, false))]),
            "Track Matte Key names video track 0, which is not above the clip's track 0",
        ),
        (
            "private data",
            with_matte_track(&source, &[(20, with_private_data)]),
            "VideoFilterComponent:20: PremiereFilterPrivateData is not supported",
        ),
        (
            "two keys",
            with_matte_track(&source, &[(20, key(20)), (30, key(30))]),
            "VideoComponentChain:4: duplicate Track Matte Key component",
        ),
        (
            "key with a Crop",
            with_matte_track(&source, &[(20, key(20)), (30, top_crop(30))]),
            "a Track Matte Key with a Crop, Linear Wipe or Opacity mask on one clip is not converted",
        ),
        (
            "portrait fill source",
            with_matte_track(&portrait_source, &[(20, key(20))]),
            "Track Matte Key on media that is not sequence-sized is not converted",
        ),
        (
            "matte item over part of the range",
            base.replace(MATTE_TRACK, &matte_track_of(&[(40, 0, 2 * TICKS)])),
            "the matte clip spans 0..508032000000 ticks, not the clip's 0..1270080000000; only a matte clip spanning exactly the clip's range converts",
        ),
        (
            "two matte items over the range",
            base.replace(
                MATTE_TRACK,
                &matte_track_of(&[(40, 0, 2 * TICKS), (50, 2 * TICKS, 5 * TICKS)]),
            ),
            "the matte track 1 holds more than one clip over the clip's range; only one matte clip spanning exactly that range converts",
        ),
        (
            "disabled matte item",
            base.replace(
                "<VideoClipTrackItem ObjectID=\"93\"><ClipTrackItem>",
                "<VideoClipTrackItem ObjectID=\"93\"><ClipTrackItem><IsMuted>true</IsMuted>",
            ),
            "the matte clip is disabled or on a muted track; FX drops a hidden matte source and would show the clip whole",
        ),
        (
            "muted matte track",
            base.replace(
                "<Track><ID>7</ID><Index>1</Index></Track>",
                "<Track><ID>7</ID><Index>1</Index><IsMuted>true</IsMuted></Track>",
            ),
            "the matte clip is disabled or on a muted track",
        ),
        // FX would draw a hidden clip's matte; Premiere's is unmeasured.
        (
            "disabled keyed clip",
            base.replace(
                "<VideoClipTrackItem ObjectID=\"3\"><ClipTrackItem>",
                "<VideoClipTrackItem ObjectID=\"3\"><ClipTrackItem><IsMuted>true</IsMuted>",
            ),
            "a Track Matte Key on a disabled clip or on a muted track is not converted: whether Premiere draws its matte clip is unmeasured, and FX draws a hidden clip's matte source as content",
        ),
        (
            "matte item with a Crop",
            base.replace(
                "<VideoComponentChain ObjectID=\"94\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
                &format!("<VideoComponentChain ObjectID=\"94\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"40\"/></Components></ComponentChain></VideoComponentChain>{}", top_crop(40)),
            ),
            "the matte clip has its own Crop, Linear Wipe, Opacity mask or Track Matte Key",
        ),
        // A matte item that its own rules omit leaves the matte track empty.
        (
            "omitted matte item",
            base.replace(
                "<VideoClipTrackItem ObjectID=\"93\"><ClipTrackItem><ComponentOwner><Components ObjectRef=\"94\"/></ComponentOwner>",
                "<VideoClipTrackItem ObjectID=\"93\"><ClipTrackItem><ComponentOwner><Components ObjectRef=\"94\"/></ComponentOwner><OriginalSubClipTimeOffset>1</OriginalSubClipTimeOffset>",
            ),
            "the matte track 1 holds no clip over the clip's range",
        ),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let kept: Vec<_> = project.sequences[0].video_tracks[0]
            .items
            .iter()
            .filter_map(crate::schema::PrVideoItem::id)
            .collect();
        assert_eq!(kept, ["VideoClipTrackItem:9"], "{case}");
        let omission = omissions
            .iter()
            .find(|omission| {
                omission.scope == OmissionScope::Occurrence
                    && (omission.record == "3" || omission.record == "VideoClipTrackItem:3")
            })
            .unwrap_or_else(|| panic!("{case}: {omissions:?}"));
        assert!(omission.reason.contains(reason), "{case}: {}", omission.reason);
    }
}

#[test]
fn an_omitted_keyed_clip_consumes_its_matte_clips() {
    // Premiere does not draw a matte clip while an active key names its
    // track, whether or not the keyed clip converts (fixture G1b: clip D's
    // matte hid while its Reverse was unconverted), so the matte clips over
    // an omitted clip's range go too, wherever the clip was omitted: in the
    // key's own read (E), before it (a keyer beside the key) or after every
    // item is read (two matte items over the range; a disabled keyed clip).
    // A key whose Composite Using is keyed still names its matte by
    // its static Matte. A second clip (`VideoClipTrackItem:9`, 5-10 s) keeps
    // the sequence convertible.
    let source = with_second_clip(SOURCE);
    let reverse_luma = |id| track_matte_key_xml(id, 7, 1, true);
    let keyed_composite = track_matte_key(20).replace(
        "<IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>",
        "<IsTimeVarying>true</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><Keyframes>0,0,0,0,0,0,0,0;254016000000,1,0,0,0,0,0,0;</Keyframes>",
    );
    assert_ne!(keyed_composite, track_matte_key(20));
    let ultra_key = tint(30)
        .replace(
            "<DisplayName>Tint</DisplayName>",
            "<DisplayName>Ultra Key</DisplayName>",
        )
        .replace("AE.ADBE Tint", "AE.ADBE Ultra Key");
    let consumed = "matte source of the omitted clip VideoClipTrackItem:3 was not converted: Premiere does not draw a track-matte source";
    for (case, xml, omitted_mattes) in [
        (
            "Reverse with Matte Luma",
            with_matte_track(&source, &[(20, reverse_luma(20))]),
            vec!["VideoClipTrackItem:93"],
        ),
        (
            "a keyer beside the key",
            with_matte_track(&source, &[(20, track_matte_key(20)), (30, ultra_key)]),
            vec!["VideoClipTrackItem:93"],
        ),
        (
            "keyed Composite Using",
            with_matte_track(&source, &[(20, keyed_composite)]),
            vec!["VideoClipTrackItem:93"],
        ),
        (
            "disabled keyed clip",
            with_matte_track(&source, &[(20, track_matte_key(20))]).replace(
                "<VideoClipTrackItem ObjectID=\"3\"><ClipTrackItem>",
                "<VideoClipTrackItem ObjectID=\"3\"><ClipTrackItem><IsMuted>true</IsMuted>",
            ),
            vec!["VideoClipTrackItem:93"],
        ),
        (
            "two matte items over the range",
            with_matte_track(&source, &[(20, track_matte_key(20))]).replace(
                MATTE_TRACK,
                &matte_track_of(&[(40, 0, 2 * TICKS), (50, 2 * TICKS, 5 * TICKS)]),
            ),
            vec!["VideoClipTrackItem:40", "VideoClipTrackItem:50"],
        ),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let tracks = &project.sequences[0].video_tracks;
        let kept: Vec<_> = tracks
            .iter()
            .flat_map(|track| {
                track
                    .items
                    .iter()
                    .filter_map(crate::schema::PrVideoItem::id)
            })
            .collect();
        assert_eq!(kept, ["VideoClipTrackItem:9"], "{case}");
        let consumed_records: Vec<_> = omissions
            .iter()
            .filter(|omission| {
                omission.scope == OmissionScope::Occurrence && omission.reason == consumed
            })
            .map(|omission| omission.record.as_str())
            .collect();
        assert_eq!(consumed_records, omitted_mattes, "{case}: {omissions:?}");
    }
    // A matte that a kept clip keys stays: track 1's clip 95 keys the matte
    // (track 2, `Track/ID` 7) by Matte Alpha over the same range as the
    // omitted clip 3.
    let kept_consumer = Placement {
        start: 0,
        end: 5 * TICKS,
        source_in: 0,
    };
    let xml = with_matte_track(&source, &[(20, reverse_luma(20))])
        .replace(
            "<Track Index=\"1\" ObjectURef=\"track-2\"/></Tracks>",
            "<Track Index=\"1\" ObjectURef=\"track-mid\"/><Track Index=\"2\" ObjectURef=\"track-2\"/></Tracks>",
        )
        .replace(
            "<Track><ID>7</ID><Index>1</Index></Track><ClipItems><Index>1</Index>",
            "<Track><ID>7</ID><Index>2</Index></Track><ClipItems><Index>2</Index>",
        )
        .replace(
            "</PremiereData>",
            &format!(
                "<VideoClipTrack ObjectUID=\"track-mid\"><ClipTrack><Track><ID>8</ID><Index>1</Index></Track><ClipItems><Index>1</Index><TrackItems><TrackItem ObjectRef=\"95\"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>{}{}</PremiereData>",
                placement_records(95, 7, &kept_consumer).replace(
                    "<VideoComponentChain ObjectID=\"96\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
                    "<VideoComponentChain ObjectID=\"96\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"60\"/></Components></ComponentChain></VideoComponentChain>",
                ),
                track_matte_key_xml(60, 7, 0, false)
            ),
        );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    let tracks = &project.sequences[0].video_tracks;
    assert_eq!(
        tracks[1].clip(0).track_matte,
        Some(PrTrackMatte {
            track_index: 2,
            channel: PrMatteChannel::Alpha,
        })
    );
    assert_eq!(
        tracks[2].clip(0).id.as_deref(),
        Some("VideoClipTrackItem:93"),
        "{omissions:?}"
    );
    assert!(
        omissions.iter().all(|omission| omission.reason != consumed),
        "{omissions:?}"
    );
}

/// Premiere 26.5.1's Track Matte Key records, verbatim from the
/// second save of `feature_track_matte_key_26_5_strict.prproj`:
/// `VideoFilterComponent` 9 / `Component` 7 with `VideoFilterType` before
/// `MatchName`, no `Bypass`, `Intrinsic` or `ArchivedType`, and three v10
/// parameters without `ParameterControlType` or `IsTimeVarying`; only
/// Composite Using carries bounds and `DiscontinuousInterpolate`. Clip A
/// (`:220`): Matte 3 (the V3 track's `Track/ID`), Matte Alpha, Reverse false.
const PREMIERE_26_5_KEY_A: &str = r#"<VideoFilterComponent ObjectID="220" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="259"/>
				<Param Index="1" ObjectRef="260"/>
				<Param Index="2" ObjectRef="261"/>
			</Params>
			<ID>3</ID>
			<DisplayName>Track Matte Key</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Legacy Key Track Matte</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="259" ClassID="2f2eb0a3-318c-4a93-99fc-f1d319edc864" Version="10">
		<Name>Matte:</Name>
		<ParameterID>1</ParameterID>
		<StartKeyframe>-91445760000000000,3,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoComponentParam ObjectID="260" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Composite Using:</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>1</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="261" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name>Reverse</Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>"#;
/// Clip D (`:233`): Matte 3, Matte Alpha, Reverse true.
const PREMIERE_26_5_KEY_D: &str = r#"<VideoFilterComponent ObjectID="233" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="288"/>
				<Param Index="1" ObjectRef="289"/>
				<Param Index="2" ObjectRef="290"/>
			</Params>
			<ID>3</ID>
			<DisplayName>Track Matte Key</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Legacy Key Track Matte</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="288" ClassID="2f2eb0a3-318c-4a93-99fc-f1d319edc864" Version="10">
		<Name>Matte:</Name>
		<ParameterID>1</ParameterID>
		<StartKeyframe>-91445760000000000,3,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoComponentParam ObjectID="289" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Composite Using:</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>1</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="290" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name>Reverse</Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>"#;
/// Clip E (`:238`): Matte 3, Matte Luma, Reverse true.
const PREMIERE_26_5_KEY_E: &str = r#"<VideoFilterComponent ObjectID="238" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="297"/>
				<Param Index="1" ObjectRef="298"/>
				<Param Index="2" ObjectRef="299"/>
			</Params>
			<ID>3</ID>
			<DisplayName>Track Matte Key</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Legacy Key Track Matte</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="297" ClassID="2f2eb0a3-318c-4a93-99fc-f1d319edc864" Version="10">
		<Name>Matte:</Name>
		<ParameterID>1</ParameterID>
		<StartKeyframe>-91445760000000000,3,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoComponentParam ObjectID="298" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Composite Using:</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,1,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>1</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="299" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name>Reverse</Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>"#;
/// Clip D's key in the first save, before any matte was selected: Matte None
/// is 4294967295.
const PREMIERE_26_5_KEY_NONE: &str = r#"<VideoFilterComponent ObjectID="233" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="288"/>
				<Param Index="1" ObjectRef="289"/>
				<Param Index="2" ObjectRef="290"/>
			</Params>
			<ID>3</ID>
			<DisplayName>Track Matte Key</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Legacy Key Track Matte</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="288" ClassID="2f2eb0a3-318c-4a93-99fc-f1d319edc864" Version="10">
		<Name>Matte:</Name>
		<ParameterID>1</ParameterID>
		<StartKeyframe>-91445760000000000,4294967295,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoComponentParam ObjectID="289" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Composite Using:</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>1</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="290" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name>Reverse</Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>"#;
/// Clip F (`VideoClipTrackItem:136`): its chain `:178` holds the key `:235`
/// at `Index` 0 (`ID` 4) above the Gaussian Blur (Legacy) `:236` at `Index`
/// 1 (`ID` 3), Blurriness 25, so the blur applies before the key (G7).
const PREMIERE_26_5_CHAIN_F: &str = r#"<VideoComponentChain ObjectID="178" ClassID="0970e08a-f58f-4108-b29a-1a717b8e12e2" Version="3">
		<DefaultMotion>true</DefaultMotion>
		<DefaultOpacity>true</DefaultOpacity>
		<DefaultMotionComponentID>1</DefaultMotionComponentID>
		<DefaultOpacityComponentID>2</DefaultOpacityComponentID>
		<ComponentChain Version="3">
			<Node Version="1">
				<Properties Version="1">
					<MZ.ComponentChain.ActiveComponentID>2</MZ.ComponentChain.ActiveComponentID>
					<MZ.ComponentChain.ActiveComponentParamIndex>4294967295</MZ.ComponentChain.ActiveComponentParamIndex>
				</Properties>
			</Node>
			<Components Version="1">
				<Component Index="0" ObjectRef="235"/>
				<Component Index="1" ObjectRef="236"/>
			</Components>
		</ComponentChain>
	</VideoComponentChain>
<VideoFilterComponent ObjectID="235" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="291"/>
				<Param Index="1" ObjectRef="292"/>
				<Param Index="2" ObjectRef="293"/>
			</Params>
			<ID>4</ID>
			<DisplayName>Track Matte Key</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Legacy Key Track Matte</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="291" ClassID="2f2eb0a3-318c-4a93-99fc-f1d319edc864" Version="10">
		<Name>Matte:</Name>
		<ParameterID>1</ParameterID>
		<StartKeyframe>-91445760000000000,3,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoComponentParam ObjectID="292" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Composite Using:</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>1</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="293" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name>Reverse</Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoFilterComponent ObjectID="236" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="294"/>
				<Param Index="1" ObjectRef="295"/>
				<Param Index="2" ObjectRef="296"/>
			</Params>
			<ID>3</ID>
			<DisplayName>Gaussian Blur (Legacy)</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Gaussian Blur 2</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="294" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="10">
		<Name>Blurriness</Name>
		<ParameterID>1</ParameterID>
		<UpperUIBound>50</UpperUIBound>
		<StartKeyframe>-91445760000000000,25.,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>30000</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="295" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Blur Dimensions</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>2</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="296" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name> </Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>"#;

#[test]
fn premiere_26_5_track_matte_keys_read_as_saved() {
    // The matte track takes the fixture's V3 `Track/ID` 3; the records are
    // otherwise as saved.
    let fixture =
        |xml: String| xml.replace("<ID>7</ID><Index>1</Index>", "<ID>3</ID><Index>1</Index>");
    for (case, records, id, channel) in [
        (
            "A: Matte Alpha",
            PREMIERE_26_5_KEY_A,
            220,
            PrMatteChannel::Alpha,
        ),
        (
            "D: Matte Alpha, Reverse",
            PREMIERE_26_5_KEY_D,
            233,
            PrMatteChannel::AlphaInverted,
        ),
    ] {
        let xml = fixture(with_matte_track(SOURCE, &[(id, records.to_owned())]));
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        assert!(omissions.is_empty(), "{case}: {omissions:?}");
        let clip = project.sequences[0].video_tracks[0].clip(0);
        assert_eq!(
            clip.track_matte,
            Some(PrTrackMatte {
                track_index: 1,
                channel
            }),
            "{case}"
        );
        assert!(clip.effects.is_empty(), "{case}");
    }
    // F: the chain as saved, the key at Index 0 above the blur at Index 1.
    let xml = fixture(
        with_matte_track(SOURCE, &[])
            .replace(
                "<Components ObjectRef=\"4\"/>",
                "<Components ObjectRef=\"178\"/>",
            )
            .replace(
                "</PremiereData>",
                &format!("{PREMIERE_26_5_CHAIN_F}</PremiereData>"),
            ),
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = project.sequences[0].video_tracks[0].clip(0);
    assert_eq!(
        clip.track_matte,
        Some(PrTrackMatte {
            track_index: 1,
            channel: PrMatteChannel::Alpha,
        })
    );
    assert_eq!((clip.effects.len(), clip.effects_above_mask), (1, 1));
    assert!(
        matches!(
            &clip.effects[0].params,
            crate::schema::PrEffectParams::GaussianBlur(blur) if blur.blurriness == 25.0
        ),
        "{:?}",
        clip.effects[0].params
    );
    // E and the unselected Matte fail closed by name; a second clip keeps the
    // sequence convertible. E's key consumes the matte clip; a key without a
    // matte names no track, so track 1's clip is ordinary content.
    for (case, records, id, reason, kept) in [
        (
            "E: Matte Luma, Reverse",
            PREMIERE_26_5_KEY_E,
            238,
            "VideoFilterComponent:238: Reverse with Matte Luma is not converted",
            vec!["VideoClipTrackItem:9"],
        ),
        (
            "Matte None",
            PREMIERE_26_5_KEY_NONE,
            233,
            "VideoFilterComponent:233: Matte None selects no matte track",
            vec!["VideoClipTrackItem:9", "VideoClipTrackItem:93"],
        ),
    ] {
        let xml = fixture(with_matte_track(
            &with_second_clip(SOURCE),
            &[(id, records.to_owned())],
        ));
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let items: Vec<_> = project.sequences[0]
            .video_tracks
            .iter()
            .flat_map(|track| {
                track
                    .items
                    .iter()
                    .filter_map(crate::schema::PrVideoItem::id)
            })
            .collect();
        assert_eq!(items, kept, "{case}: {omissions:?}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.record == "3" && omission.reason.contains(reason)),
            "{case}: {omissions:?}"
        );
    }
}

#[test]
fn a_graphic_matte_clip_with_a_clip_opacity_mask_is_a_masked_matte() {
    // A graphic matte keys its clip, but one with a clip Opacity mask imports
    // as a group and the mask's guide beside it, which a stage group that
    // takes its matte would leave behind: the key does not convert, as for a
    // media matte with its own Opacity mask.
    let matte = PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    };
    let mut graphic = crate::tests::support::text_graphic();
    (graphic.start_ticks, graphic.end_ticks) = (0, 5 * TICKS);
    let mut sequence = keyed_sequence(PrMatteChannel::Alpha);
    sequence.video_tracks[1].items = vec![crate::schema::PrVideoItem::Graphic(graphic.clone())];
    assert_eq!(
        crate::schema::check_track_matte(&sequence.video_tracks, 0, 0..5 * TICKS, matte),
        Ok(())
    );
    graphic.opacity_mask = Some(crate::tests::support::opacity_mask());
    sequence.video_tracks[1].items = vec![crate::schema::PrVideoItem::Graphic(graphic)];
    assert_eq!(
        crate::schema::check_track_matte(&sequence.video_tracks, 0, 0..5 * TICKS, matte),
        Err(
            "the matte clip has its own Crop, Linear Wipe, Opacity mask or Track Matte Key"
                .to_owned()
        )
    );
}

/// A 30 fps 1080p sequence: the 0-5 s source on track 0 keyed by `channel`
/// from the same source on track 1.
fn keyed_sequence(channel: PrMatteChannel) -> crate::schema::PrSequence {
    let mut fill = clip_of("source", 0..5 * TICKS, 0);
    fill.track_matte = Some(PrTrackMatte {
        track_index: 1,
        channel,
    });
    sequence_of(
        "Main",
        vec![
            PrVideoTrack::media([fill]),
            PrVideoTrack::media([clip_of("source", 0..5 * TICKS, 0)]),
        ],
    )
}

#[test]
fn written_track_matte_keys_name_the_matte_track_id_and_read_back() {
    let mut media = video_media();
    let source = media.get_mut(&MediaId("source".into())).unwrap();
    source.video.as_mut().unwrap().kind = crate::schema::PrMediaKind::Video {
        codec: Some(crate::schema::VideoCodec::H264),
        hdr_profile: None,
    };
    source.name = "source.mp4".into();
    source.relative_path = Some("./media/source.mp4".into());
    source.relative_paths = vec!["./media/source.mp4".into()];
    source.absolute_paths = vec![(
        crate::schema::records::MediaPathField::FilePath,
        "/media/source.mp4".into(),
    )];
    for (channel, composite, reverse) in [
        (PrMatteChannel::Alpha, "0", "false"),
        (PrMatteChannel::AlphaInverted, "0", "true"),
        (PrMatteChannel::Luma, "1", "false"),
    ] {
        let project = crate::schema::PrProjectFile::from_sequences(
            vec![keyed_sequence(channel)],
            media.clone(),
        );
        let xml = project_xml(&project).unwrap();
        let graph = Graph::parse(&xml).unwrap();
        let key = graph
            .records()
            .find(|record| {
                record
                    .element()
                    .child("MatchName")
                    .and_then(crate::format::graph::Element::text)
                    == Some("AE.ADBE Legacy Key Track Matte")
            })
            .expect("a Track Matte Key component");
        // The corpus 12.x form: the written track at index 1 has ID 2.
        assert_eq!(key.element().attribute("Version"), Some("7"));
        let body = key.element().child("Component").expect("Component");
        assert_eq!(body.attribute("Version"), Some("5"));
        let values: Vec<_> = body
            .child("Params")
            .expect("Params")
            .children()
            .map(|param| {
                let record = graph.locate(&param.reference(), "test").unwrap();
                let param = graph.decode::<VideoComponentParam>(record).unwrap();
                (
                    param.value.name.unwrap(),
                    param.value.parameter_control_type.unwrap(),
                    param.value.start_keyframe,
                )
            })
            .collect();
        assert_eq!(
            values,
            [
                (
                    "Matte:".to_owned(),
                    "13".to_owned(),
                    "-91445760000000000,2,0,0,0,0,0,0".to_owned()
                ),
                (
                    "Composite Using:".to_owned(),
                    "7".to_owned(),
                    format!("-91445760000000000,{composite},0,0,0,0,0,0")
                ),
                (
                    "Reverse".to_owned(),
                    "4".to_owned(),
                    format!("-91445760000000000,{reverse},0,0,0,0,0,0")
                ),
            ],
            "{channel:?}"
        );
        let (reread, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let tracks = &reread.sequences[0].video_tracks;
        assert_eq!(
            tracks[0].clip(0).track_matte,
            Some(PrTrackMatte {
                track_index: 1,
                channel
            })
        );
        assert_eq!(tracks[1].clip(0).track_matte, None);
    }
    // The model rejects a key whose matte the sequence lacks, so no clip is
    // written without its matte.
    let mut sequence = keyed_sequence(PrMatteChannel::Alpha);
    sequence.video_tracks.pop();
    sequence.timeline_end_ticks = sequence.occurrence_end_ticks();
    let project = crate::schema::PrProjectFile::from_sequences(vec![sequence], media);
    let error = project_xml(&project).unwrap_err().to_string();
    assert!(
        error.contains("Track Matte Key names video track 1, which the sequence does not have"),
        "{error}"
    );
}
