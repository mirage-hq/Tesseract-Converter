//! Full-frame adjustment layers through the public conversion API: an
//! adjustment clip imports as an FX adjustment layer over the layers below it
//! and exports as a flagged clip of Black Video generator media.
//!
//! Two sources. The synthetic one is the one-second `one-clip.xml` (V1:
//! `source.mp4` 0–1 s) plus a second track that places an adjustment layer
//! 0.2–0.8 s in the corpus record form (`transition_countdown` ObjectIDs
//! 61/64/92 and its `vhs_slideshow` siblings): a `MasterClip` with
//! `IsAdjustmentLayer`, a placed `VideoClip` with `AdjustmentLayer`, Black
//! Video media and a Gaussian Blur (Repeat Edge Pixels on) whose records copy
//! `transition_countdown`; it is structural, not Adobe-rendered evidence. The
//! native one is the Premiere 26.5.1 fixture
//! `feature_adjustment_layer_26_5_strict.prproj` (manifest case
//! `premiere_isolated_adjustment_layer_26_5`, `tests/README.md`).

use super::support::*;
use premiere_file::{OmissionScope, PrProjectFile, PrSequence, PrVideoItem};
use serde_json::{json, Value};
use std::path::Path;
use tesseract_file::TesseractFile;

#[test]
fn native_adjustment_motion_changes_effect_coverage_without_moving_picture() {
    use sha2::{Digest, Sha256};
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (name, digest) in [
        (
            "feature_adjustment_motion_wipe_26_5_strict.prproj",
            "9c9fa7f466400f8977da6f224ab172430cb6b6a9ad8db22ddf2fee237e0f8dcb",
        ),
        (
            "a3_base_grid.png",
            "51f375ccf517621a9dab75e67bc0e5d5bb37cb2111201b4ee53cbfe02744df83",
        ),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(fixtures.join(name)).unwrap())
            ),
            digest
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("adjustment-motion");
    let omissions = premiere_to_tesseract(
        fixtures.join("feature_adjustment_motion_wipe_26_5_strict.prproj"),
        &output,
        Some("b5dcf675-953c-4fa3-b318-3924d9e4f9c7"),
        false,
    )
    .unwrap();
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let mut adjustments = layers
        .iter()
        .filter(|layer| layer["type"] == "Adjustment")
        .collect::<Vec<_>>();
    adjustments.sort_by_key(|layer| {
        (*crate::test_support::layer_range(layer))["start"]
            .as_i64()
            .unwrap()
    });
    assert_eq!(adjustments.len(), 4, "{omissions:?}");
    let ids = layers
        .iter()
        .map(|layer| layer["id"].as_u64().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), layers.len());
    for field in ["effects", "masks"] {
        let ids = layers
            .iter()
            .flat_map(|layer| {
                layer[field]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|item| item["id"].as_u64().unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ids.iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            ids.len()
        );
    }
    for (index, adjustment) in adjustments.iter().enumerate() {
        assert_eq!(
            (*crate::test_support::layer_range(adjustment)),
            json!({"start": index * 1500, "duration": 1500})
        );
        assert_eq!(adjustment["transform"]["position"], json!([0.0, 0.0]));
        assert_eq!(adjustment["transform"]["scale"], json!([100.0, 100.0]));
        if index != 2 {
            let effects = adjustment["effects"].as_array().unwrap();
            assert_eq!(effects.len(), 1);
            assert_eq!(effects[0]["enabled"], true);
            assert_eq!(
                effects[0]["effect"],
                json!({"type": "levels", "inputBlack": 0.0, "inputWhite": 255.0, "gamma": 1.0, "outputBlack": 255.0, "outputWhite": 0.0})
            );
        }
        if index == 0 {
            assert!(adjustment["masks"].is_null());
            continue;
        }
        let masks = adjustment["masks"].as_array().unwrap();
        assert_eq!(masks.len(), 1);
        assert_eq!(masks[0]["mode"], "add");
        assert_eq!(masks[0]["feather"], json!([0.0, 0.0]));
        let guide = layers
            .iter()
            .find(|layer| layer["id"] == masks[0]["layer"])
            .unwrap();
        assert_eq!(guide["type"], "Rect");
        assert_eq!(
            (*crate::test_support::layer_range(guide)),
            (*crate::test_support::layer_range(adjustment))
        );
        assert_eq!(guide["parent"], adjustment["parent"]);
        assert_eq!(guide["rect"]["size"], json!([1920.0, 1080.0]));
        assert_eq!(guide["rect"]["fillEnabled"], false);
        assert_eq!(guide["rect"]["strokeEnabled"], false);
        assert_eq!(guide["transform"]["anchorPoint"], json!([960.0, 540.0]));
        assert_eq!(
            guide["transform"]["scale"],
            if index < 3 {
                json!([50.0, 50.0])
            } else {
                json!([100.0, 100.0])
            }
        );
        assert_eq!(
            guide["transform"]["position"],
            if index < 3 {
                json!([960.0, 540.0])
            } else {
                json!([1152.0, 594.0])
            }
        );
    }
    assert!(
        adjustments[2]["effects"].is_null(),
        "Scale-only J3 must leave the dry picture unchanged"
    );
    assert_eq!(file.metadata().assets.len(), 1);
    let pictures = layers
        .iter()
        .filter(|layer| layer["type"] == "Image")
        .collect::<Vec<_>>();
    assert_eq!(pictures.len(), 2);
    assert!(pictures
        .iter()
        .all(|layer| layer["transform"]["scale"] == json!([100.0, 100.0])
            && layer["transform"]["position"] == json!([960.0, 540.0])));
    // The independently rendered J5 wipe remains outside this bounded mapping.
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence
                && (omission.reason.contains("Linear Wipe")
                    || omission.reason.contains("Transition Completion"))),
        "{omissions:?}"
    );
}

/// The 30 fps generator in-point, one hour into the synthetic clock.
const IN_TICKS: i64 = 3600 * TICKS;
const MILLI: i64 = TICKS / 1000;

const ADJUSTMENT_TRACK: &str = r#"<VideoClipTrack ObjectUID="track-2"><ClipTrack><Track><ID>2</ID><Index>1</Index></Track><ClipItems><TrackItems><TrackItem ObjectRef="40"/></TrackItems><Index>1</Index></ClipItems></ClipTrack></VideoClipTrack>
<VideoClipTrackItem ObjectID="40"><ClipTrackItem><ComponentOwner><Components ObjectRef="41"/></ComponentOwner><TrackItem><Start>50803200000</Start><End>203212800000</End></TrackItem><SubClip ObjectRef="42"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
<VideoComponentChain ObjectID="41"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index="0" ObjectRef="60"/></Components></ComponentChain></VideoComponentChain>
<SubClip ObjectID="42"><Clip ObjectRef="43"/><MasterClip ObjectURef="adjustment-master"/><Name>Adjustment Layer</Name></SubClip>
<VideoClip ObjectID="43"><Clip><Node Version="1"><Properties Version="1"><BE.Prefs.SyntheticMedia.DefaultIsDropFrame>false</BE.Prefs.SyntheticMedia.DefaultIsDropFrame></Properties></Node><Source ObjectRef="44"/><ClipID>placed-adjustment</ClipID><InPoint>914457600000000</InPoint><OutPoint>914610009600000</OutPoint></Clip><AdjustmentLayer>true</AdjustmentLayer></VideoClip>
<VideoMediaSource ObjectID="44"><MediaSource Version="4"><Content Version="10"></Content><Media ObjectURef="adjustment-media"/></MediaSource><OriginalDuration>10973491200000000</OriginalDuration></VideoMediaSource>
<Media ObjectUID="adjustment-media"><VideoStream ObjectRef="45"/><FilePath>1112293707</FilePath><Infinite>true</Infinite><ImplementationID>42008e7a-de6f-4270-96de-7e287abb9b4b</ImplementationID><Title>Black Video</Title><ActualMediaFilePath>1112293707</ActualMediaFilePath><ContentAndMetadataState>00000000-0000-0000-0000-000000000000</ContentAndMetadataState><ConformedAudioRate>9223372036854775807</ConformedAudioRate></Media>
<VideoStream ObjectID="45"><IsStill>true</IsStill><IsContinuousTime>true</IsContinuousTime><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect><Duration>10973491200000000</Duration><CodecType>1380013856</CodecType><FieldTypeIsUncertain>true</FieldTypeIsUncertain></VideoStream>
<MasterClip ObjectUID="adjustment-master"><Clips Version="1"><Clip Index="0" ObjectRef="46"/></Clips><Name>Adjustment Layer</Name><IsAdjustmentLayer>true</IsAdjustmentLayer><MasterClipChangeVersion>4</MasterClipChangeVersion></MasterClip>
<VideoClip ObjectID="46"><Clip><Source ObjectRef="44"/><ClipID>template-adjustment</ClipID><InPoint>0</InPoint><OutPoint>1270080000000</OutPoint><InUse>false</InUse></Clip></VideoClip>
<VideoFilterComponent ObjectID="60" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="8"><Component Version="6"><Params Version="1"><Param Index="0" ObjectRef="61"/><Param Index="1" ObjectRef="62"/><Param Index="2" ObjectRef="63"/></Params><ID>4</ID><DisplayName>Gaussian Blur</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>AE.ADBE Gaussian Blur 2</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>
<VideoComponentParam ObjectID="61" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="9"><Name>Blurriness</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,25.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>30000</UpperBound><ParameterID>1</ParameterID><UpperUIBound>50</UpperUIBound></VideoComponentParam>
<VideoComponentParam ObjectID="62" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="9"><Name>Blur Dimensions</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>2</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="63" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="9"><Name> </Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>
"#;

/// `(track, start ms, end ms, media name)` for every occurrence.
fn placements(project: &PrProjectFile, sequence: &PrSequence) -> Vec<(usize, i64, i64, String)> {
    sequence
        .video_tracks()
        .enumerate()
        .flat_map(|(track, clips)| {
            clips.iter().map(move |item| {
                let PrVideoItem::Media(clip) = item else {
                    panic!("the adjustment source contains only media occurrences");
                };
                let range = clip.timeline_ticks();
                (
                    track,
                    range.start / MILLI,
                    range.end / MILLI,
                    project.media(clip).unwrap().name().to_owned(),
                )
            })
        })
        .collect()
}

#[test]
fn synthetic_adjustment_layer_round_trips_as_an_fx_adjustment_layer() {
    let dir = tempfile::tempdir().unwrap();
    let xml = one_second()
        .replace(
            r#"<Track ObjectURef="track-1"/>"#,
            r#"<Track ObjectURef="track-1"/><Track ObjectURef="track-2" Index="1"/>"#,
        )
        .replace(
            "</PremiereData>",
            &format!("{ADJUSTMENT_TRACK}</PremiereData>"),
        );
    let source = fixture(&dir.path().join("native"), &xml);
    let native = PrProjectFile::load(&source).unwrap().0;
    let sequence = native.sequences().next().unwrap();
    let expected = vec![
        (0, 0, 1000, "source.mp4".to_owned()),
        (1, 200, 800, "Adjustment Layer".to_owned()),
    ];
    assert_eq!(placements(&native, sequence), expected);
    let adjustment = sequence.video_occurrences().nth(1).unwrap();
    assert_eq!(adjustment.source_ticks(), IN_TICKS..IN_TICKS + 600 * MILLI);

    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &output, Some("sequence-1"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let archive = first_project(&output);
    let file = TesseractFile::open(&archive).unwrap();
    // The adjustment is generated, so only the video is packaged.
    assert_eq!(file.metadata().assets.len(), 1);
    let document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let names: Vec<_> = layers
        .iter()
        .map(|layer| {
            (
                layer["type"].as_str().unwrap(),
                layer["name"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            ("Adjustment", "Premiere adjustment 2"),
            ("Video", "Premiere video 1"),
            ("Rect", "Premiere black canvas"),
        ]
    );
    let layer = &layers[0];
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 200, "duration": 600})
    );
    assert_eq!(layer["transform"]["opacity"], 100.0);
    assert_eq!(
        layer["effects"],
        json!([{"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 25.0, "repeatEdgePixels": true}}])
    );

    let package = dir.path().join("premiere");
    let omissions = tesseract_to_premiere(&archive, &package, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let rebuilt = PrProjectFile::load(package.join("project.prproj"))
        .unwrap()
        .0;
    let rebuilt_sequence = rebuilt.sequences().next().unwrap();
    assert_eq!(placements(&rebuilt, rebuilt_sequence), expected);
    let rebuilt_adjustment = rebuilt_sequence.video_occurrences().nth(1).unwrap();
    assert_eq!(rebuilt_adjustment.source_ticks(), adjustment.source_ticks());
    let xml = read_xml(&package.join("project.prproj"));
    // One flagged project item and one flagged placement, on Black Video media.
    assert_eq!(
        xml.matches("<IsAdjustmentLayer>true</IsAdjustmentLayer>")
            .count(),
        1
    );
    assert_eq!(
        xml.matches("<AdjustmentLayer>true</AdjustmentLayer>")
            .count(),
        1
    );
    assert_eq!(xml.matches("<FilePath>1112293707</FilePath>").count(), 1);
    assert!(xml.contains("<Title>Black Video</Title>"));
    assert_eq!(
        xml.matches("<IsContinuousTime>true</IsContinuousTime>")
            .count(),
        1
    );
    // The blur rides on the placed adjustment clip, Repeat Edge Pixels kept (Edge Behavior 1).
    assert_eq!(
        xml.matches("<MatchName>AE.Impact_Blur_FX</MatchName>")
            .count(),
        1
    );
    assert!(xml.contains("<StartKeyframe>-91445760000000000,1,0,0,0,0,0,0</StartKeyframe>"));
}

/// `(type, name, start ms, duration ms, effects)` of every layer.
fn layer_rows(document: &Value) -> Vec<(String, String, i64, i64, Value)> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| {
            (
                layer["type"].as_str().unwrap().to_owned(),
                layer["name"].as_str().unwrap().to_owned(),
                (*crate::test_support::layer_range(layer))["start"]
                    .as_i64()
                    .unwrap(),
                (*crate::test_support::layer_range(layer))["duration"]
                    .as_i64()
                    .unwrap(),
                layer["effects"].clone(),
            )
        })
        .collect()
}

/// `premiere_isolated_adjustment_layer_26_5`, Premiere 26.5.1's save of the
/// Oracle's sequence: V1 S1/S2 (linked A/V, 0–5/5–10 s), V2 P (timecoded at
/// Scale 50, 4–8 s), V3 adjustments A Levels 0–3 s, B Gaussian Blur repeat
/// edge 3–5 s, C the same without repeat edge 5–7 s, D Levels output white
/// 180 with Opacity keys 100 → 20 at 7.5/8.75 s, then held, 7–9 s, F Crop
/// Left 40 9–10 s, and V4 E Levels Gamma 1.6 1–2.5 s. Every adjustment but F
/// converts; F is omitted by name (Premiere renders its cropped region black,
/// so that second differs). The adjustments then export as five flagged
/// placements of one flagged item and reimport unchanged.
#[test]
fn native_adjustment_layers_import_over_their_lower_tracks_and_round_trip() {
    const SEQUENCE: &str = "b500144a-c795-484c-9717-e8e826f53f95";
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_adjustment_layer_26_5_strict.prproj");
    let converted = dir.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &converted, Some(SEQUENCE), false).unwrap();
    let occurrences: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
        .collect();
    assert_eq!(
        occurrences,
        [(
            "VideoClipTrackItem:102",
            "track 2, range 2286144000000..2540160000000 ticks: nondefault Crop on an adjustment layer is unsupported; occurrence omitted"
        )]
    );
    // The native metadata reports are the only other omissions: no clip, and
    // in particular neither S1 nor S2 under their ID-only master clip Node.
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope == OmissionScope::Occurrence
                || omission.reason.contains("ClipTrackItem/TrackItem/Node")
                || omission.reason.contains("DefMappingID")),
        "{omissions:?}"
    );
    let archive = first_project(&converted);
    let document = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();
    // Effect ids run through the document, top layer first.
    let levels = |id: u32, black: f64, white: f64, gamma: f64, out_white: f64| json!([{"id": id, "enabled": true, "effect": {"type": "levels", "inputBlack": black, "inputWhite": white, "gamma": gamma, "outputBlack": 0.0, "outputWhite": out_white}}]);
    let adjustment = |index: u32, start: i64, duration: i64, effects: Value| {
        (
            "Adjustment".to_owned(),
            format!("Premiere adjustment {index}"),
            start,
            duration,
            effects,
        )
    };
    let media = |kind: &str, index: u32, start: i64, duration: i64| {
        (
            kind.to_owned(),
            format!("Premiere {} {index}", kind.to_ascii_lowercase()),
            start,
            duration,
            Value::Null,
        )
    };
    let expected = vec![
        adjustment(8, 1000, 1500, levels(1, 0.0, 255.0, 1.6, 255.0)),
        adjustment(4, 0, 3000, levels(2, 40.0, 220.0, 1.0, 255.0)),
        adjustment(
            5,
            3000,
            2000,
            json!([{"id": 3, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 25.0, "repeatEdgePixels": true}}]),
        ),
        adjustment(
            6,
            5000,
            2000,
            json!([{"id": 4, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 25.0}}]),
        ),
        adjustment(7, 7000, 2000, levels(5, 0.0, 255.0, 1.0, 180.0)),
        media("Video", 3, 4000, 4000),
        media("Video", 1, 0, 5000),
        media("Video", 2, 5000, 5000),
        media("Audio", 1, 0, 5000),
        media("Audio", 2, 5000, 5000),
        (
            "Rect".to_owned(),
            "Premiere black canvas".to_owned(),
            0,
            10000,
            Value::Null,
        ),
    ];
    assert_eq!(layer_rows(&document), expected);
    let layers = document["composition"]["layers"].as_array().unwrap();
    // P keeps its Motion; the adjustments sit at the identity.
    assert_eq!(layers[5]["transform"]["scale"], json!([50.0, 50.0]));
    assert!(layers[..5]
        .iter()
        .all(|layer| layer["transform"]["scale"] == json!([100.0, 100.0])
            && layer["transform"]["position"] == json!([0.0, 0.0])
            && layer["transform"]["opacity"] == 100.0));
    // D's Opacity keys on its own clock: 7.5 s → 0.5 s, 8.75 s → 1.75 s. The
    // native Hold is the second key's outgoing mode, which only governs the
    // time after it; the interval between the keys is the first key's Linear
    // (the render ramps 100 → 20 between them, then holds).
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["target"],
        json!({"kind": "layer", "layerId": layers[4]["id"], "propertyType": "opacity"})
    );
    let keys: Vec<_> = entries[0]["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].as_f64().unwrap(),
                key["easing"]["type"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        keys,
        [
            (500, 100.0, "linear".to_owned()),
            (1750, 20.0, "linear".to_owned())
        ]
    );

    let package = dir.path().join("premiere");
    let omissions = tesseract_to_premiere(&archive, &package, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let xml = read_xml(&package.join("project.prproj"));
    assert_eq!(
        xml.matches("<IsAdjustmentLayer>true</IsAdjustmentLayer>")
            .count(),
        1
    );
    assert_eq!(
        xml.matches("<AdjustmentLayer>true</AdjustmentLayer>")
            .count(),
        5
    );
    premiere_to_tesseract(
        package.join("project.prproj"),
        dir.path().join("again"),
        None,
        false,
    )
    .unwrap();
    let again = TesseractFile::open(first_project(&dir.path().join("again")))
        .unwrap()
        .project_json()
        .unwrap();
    // Export packs the adjustments onto two tracks (A alone, E/B/C/D above
    // it), which keeps every instant's stack order but renumbers the layers.
    let settings = |rows: Vec<(String, String, i64, i64, Value)>| {
        let mut rows: Vec<_> = rows
            .into_iter()
            .map(|(kind, _, start, duration, mut effects)| {
                for effect in effects.as_array_mut().into_iter().flatten() {
                    effect.as_object_mut().unwrap().remove("id");
                }
                (kind, start, duration, effects.to_string())
            })
            .collect();
        rows.sort();
        rows
    };
    assert_eq!(settings(layer_rows(&again)), settings(expected));
    let opacity_keys = |document: &Value| {
        document["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| (key["layerTime"].clone(), key["value"].clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(opacity_keys(&again), opacity_keys(&document));
}
