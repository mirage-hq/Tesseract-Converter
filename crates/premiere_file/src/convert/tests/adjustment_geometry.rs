use crate::{
    format::{FrameRate, PrProjectFile, PremiereProjectXml},
    media::{MediaFacts, VideoMedia},
    schema::{
        PrEffectParams, PrSequence, PrStaticTransform, PrVideoTrack, VideoCodec, TICKS,
        TICKS_PER_MILLISECOND,
    },
    tests::support::{clip_of, nest_of, opacity_mask, prproj_xml, sequence_of, video_media},
    Omission,
};
use fx_schema::EditableFxCompositionDocument;
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

const CONTROLS: &str = include_str!("../../../tests/fixtures/adjustment-geometry2-native.xml");

fn import(project: &PrProjectFile) -> (Value, Vec<Omission>) {
    let sequence = project.single_sequence().unwrap();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut reports = Vec::new();
    let document =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut reports)
            .unwrap()
            .to_json_value()
            .unwrap();
    (document, reports)
}

fn geometry(layers: &Value) -> &Value {
    for layer in layers.as_array().unwrap() {
        if layer["name"]
            .as_str()
            .is_some_and(|name| name.starts_with(super::super::adjustment_geometry::STAGE_NAME))
        {
            return layer;
        }
        if let Some(children) = layer["layers"].as_array() {
            if children.iter().any(|child| {
                child["name"].as_str().is_some_and(|name| {
                    name.starts_with(super::super::adjustment_geometry::STAGE_NAME)
                })
            }) {
                return geometry(&layer["layers"]);
            }
        }
    }
    panic!("no editable adjustment Geometry2 group");
}

fn keys<'a>(document: &'a Value, group: &Value, property: &str) -> &'a Value {
    &document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == group["id"] && entry["target"]["propertyType"] == property
        })
        .unwrap()["animator"]["keyframes"]
}

fn close(actual: &Value, expected: f64) {
    assert!(
        (actual.as_f64().unwrap() - expected).abs() < 1e-9,
        "{actual} != {expected}"
    );
}

fn assert_native_controls(document: &Value, reports: &[Omission]) {
    let group = geometry(&document["composition"]["layers"]);
    assert_eq!(
        group["playback"]["inputRange"],
        json!({"start":2625,"duration":417})
    );
    assert_eq!(
        group["playback"]["mapping"],
        json!({"type":"linear","input":{"start":2625,"duration":417},"output":{"start":0,"duration":417}})
    );
    // Ground truth: immutable native PointComponentParam 165 and readback.
    close(&group["transform"]["anchorPoint"][0], 1178.3063507080078);
    close(&group["transform"]["anchorPoint"][1], 357.76140689849854);
    let expected = [
        (
            "positionX",
            vec![(0, 1174.0000534057617), (333, 1104.9999618530273)],
        ),
        (
            "positionY",
            vec![(0, 353.99999499320984), (333, 356.0000002384186)],
        ),
        (
            "scaleX",
            vec![
                (-167, 100.0),
                (0, 101.599975585938),
                (542, 118.500106811523),
            ],
        ),
        (
            "scaleY",
            vec![
                (-167, 100.0),
                (0, 101.599975585938),
                (542, 118.500106811523),
            ],
        ),
        ("rotation", vec![(0, 0.0), (417, 4.000000476837)]),
    ];
    for (property, expected) in expected {
        let actual = keys(document, group, property).as_array().unwrap();
        assert_eq!(actual.len(), expected.len());
        for (key, (time, value)) in actual.iter().zip(expected) {
            assert_eq!(key["layerTime"], time);
            close(&key["value"]["value"], value);
            if !property.starts_with("scale") {
                assert_eq!(key["easing"], json!({"type":"linear"}));
                assert!(
                    key.get("spatialInTangent").is_none() && key.get("spatialOutTangent").is_none()
                );
            }
        }
    }
    // Native 168 velocity/influence normalized by each native segment's slope;
    // these constants are not produced by the converter's easing helpers.
    let scale = keys(document, group, "scaleY");
    for (key, handles) in [
        (
            1,
            [
                0.21014492745987284,
                -0.02731428085334921,
                0.6666666666666667,
                -0.6029677301598122,
            ],
        ),
        (
            2,
            [
                0.3333333333333333,
                0.4932094844099917,
                0.0,
                0.5516284919948736,
            ],
        ),
    ] {
        assert_eq!(scale[key]["easing"]["type"], "cubicBezier");
        for (name, value) in ["x1", "y1", "x2", "y2"].into_iter().zip(handles) {
            close(&scale[key]["easing"][name], value);
        }
    }
    let children = group["layers"].as_array().unwrap();
    let lower = children
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(lower["sourceRange"], json!({"start":1833,"duration":417}));
    assert_eq!(
        lower["playback"]["inputRange"],
        json!({"start":0,"duration":417})
    );
    assert_eq!(lower["playback"]["inputOffsetMs"], 2625);
    assert_eq!(
        lower["playback"]["mapping"]["input"],
        json!({"start":2625,"duration":417})
    );
    assert_eq!(
        lower["playback"]["mapping"]["output"],
        json!({"start":1833,"duration":417})
    );
    close(&lower["transform"]["position"][0], 853.3168601989746);
    close(&lower["transform"]["position"][1], 604.9593257904053);
    assert_eq!(lower["transform"]["anchorPoint"], json!([960.0, 540.0]));
    close(&lower["transform"]["scale"][0], 113.199981689453);
    close(&lower["transform"]["scale"][1], 113.199981689453);
    let guide = children
        .iter()
        .find(|layer| layer["id"] == group["masks"][0]["layer"])
        .unwrap();
    assert_eq!(guide["type"], "Rect");
    assert_eq!(guide["parent"], group["id"]);
    assert_eq!(guide["rect"]["size"], json!([1920.0, 1080.0]));
    assert_eq!(guide["rect"]["fillEnabled"], false);
    assert_eq!(guide["rect"]["strokeEnabled"], false);
    assert_eq!(guide["activeRange"], json!({"start":0,"duration":417}));
    assert_eq!(group["masks"][0]["mode"], "add");
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("editable composed Group G x M")),
        "{reports:?}"
    );
    assert!(!document.to_string().contains("JsScript"));
}

/// Supplementary structural setup: graft only the pinned native control records
/// onto J5's *already flagged* neutral adjustment (no media-host substitution).
/// It is not an independently rendered animation oracle.
fn derived_source() -> PrProjectFile {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_adjustment_motion_wipe_26_5_strict.prproj");
    let mut xml = prproj_xml(&fixture);
    let native = roxmltree::Document::parse(&xml).unwrap();
    let reference = native
        .descendants()
        .find(|node| node.has_tag_name("Component") && node.attribute("ObjectRef") == Some("116"))
        .unwrap()
        .range();
    xml.replace_range(reference, "<Component Index=\"0\" ObjectRef=\"10125\"/>");
    let mut controls = CONTROLS.to_owned();
    for id in std::iter::once(125).chain(165..=176) {
        for attribute in ["ObjectID", "ObjectRef"] {
            controls = controls.replace(
                &format!("{attribute}=\"{id}\""),
                &format!("{attribute}=\"{}\"", id + 10000),
            );
        }
    }
    xml = xml.replace(
        "</PremiereData>",
        &controls.replace("<PremiereData Version=\"3\">", ""),
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("derived.prproj");
    crate::test_support::write_prproj(&path, &xml);
    let (native, _) = PrProjectFile::load(&path).unwrap();
    let adjustment = native
        .single_sequence()
        .unwrap()
        .video_items()
        .filter_map(|item| item.media())
        .find(|clip| {
            clip.effects
                .iter()
                .any(|effect| matches!(effect.params, PrEffectParams::AdjustmentGeometry2(_)))
        })
        .unwrap();
    assert!(native.media(adjustment).unwrap().is_adjustment());
    let mut adjustment = adjustment.clone();
    // Native occurrence 899 and lower 851 windows, not a fitted media phase.
    adjustment.start_ticks = 666792000000;
    adjustment.end_ticks = 772632000000;
    adjustment.in_ticks = 914637528000000;
    adjustment.out_ticks = 914743368000000;
    let mut lower = clip_of(
        "source",
        adjustment.start_ticks..adjustment.end_ticks,
        465696000000,
    );
    lower.transform = PrStaticTransform {
        position: [0.44443586468696594, 0.5601475238800049],
        scale: [113.199981689453; 2],
        ..PrStaticTransform::default()
    };
    let mut sequence = sequence_of(
        "Adjustment Geometry2",
        vec![
            PrVideoTrack::media([lower]),
            PrVideoTrack::media([adjustment.clone()]),
        ],
    );
    sequence.frame_rate = FrameRate::Fps24;
    let mut media = video_media();
    let stream = media.values_mut().next().unwrap().video.as_mut().unwrap();
    stream.intrinsic_ticks = 121 * TICKS / 24;
    stream.frame_rate = FrameRate::Fps24.into();
    media.insert(
        adjustment.media.clone(),
        native.media(&adjustment).unwrap().clone(),
    );
    PrProjectFile::from_sequences(vec![sequence], media)
}

fn export(document: Value) -> (PrProjectFile, Vec<Omission>) {
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    // Match package preparation: only media selected for export gets facts.
    let composition = document.composition();
    let facts = crate::convert::exported_video_layers(
        composition.layers(),
        composition.dynamics(),
        [1920, 1080],
    )
    .into_iter()
    .filter_map(|layer| crate::convert::video_data(layer).unwrap())
    .map(|video| {
        (
            crate::convert::active_asset_id(&video.source)
                .as_str()
                .to_owned(),
            MediaFacts::Video(VideoMedia {
                pixel_aspect: Default::default(),
                orientation: crate::schema::VideoOrientation::Identity,
                codec: VideoCodec::H264,
                bit_depth: 8,
                colour: None,
                width: 1920,
                height: 1080,
                timing: crate::media::VideoTiming::for_test(FrameRate::Fps24, 121 * TICKS / 24),
            }),
        )
    })
    .collect();
    let mut reports = Vec::new();
    let project = crate::convert::tesseract_to_premiere(
        &document,
        &facts,
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps24,
        &mut reports,
    )
    .unwrap();
    (project, reports)
}

fn geometry_clip(sequence: &PrSequence) -> &crate::schema::PrVideoOccurrence {
    for item in sequence.video_items() {
        if let Some(clip) = item.media() {
            if clip
                .effects
                .iter()
                .any(|effect| matches!(effect.params, PrEffectParams::AdjustmentGeometry2(_)))
            {
                return clip;
            }
        }
    }
    if let Some(nest) = sequence.nest_occurrences().next() {
        return geometry_clip(&nest.sequence);
    }
    panic!("no native Geometry2 adjustment");
}

fn written(mut project: PrProjectFile) -> (String, PrProjectFile) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("export.prproj");
    for media in project
        .media
        .values_mut()
        .filter(|media| !media.is_generator())
    {
        media.name = "source.mp4".into();
        media.relative_path = Some("./media/source.mp4".into());
        media.relative_paths = vec!["./media/source.mp4".into()];
        media.absolute_paths = vec![(
            crate::schema::records::MediaPathField::FilePath,
            "/media/source.mp4".into(),
        )];
    }
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    (prproj_xml(&path), PrProjectFile::load(&path).unwrap().0)
}

fn geometry_parameter<'a, 'input>(
    wire: &'a roxmltree::Document<'input>,
    id: usize,
) -> roxmltree::Node<'a, 'input> {
    let components: Vec<_> = wire
        .descendants()
        .filter(|node| {
            node.has_tag_name("VideoFilterComponent")
                && node.children().any(|child| {
                    child.has_tag_name("MatchName") && child.text() == Some("AE.ADBE Geometry2")
                })
        })
        .collect();
    assert_eq!(components.len(), 1);
    let parameter_id = id.to_string();
    components[0]
        .descendants()
        .filter(|node| node.has_tag_name("Param"))
        .find_map(|reference| {
            let object_id = reference.attribute("ObjectRef").unwrap();
            wire.descendants().find(|node| {
                node.attribute("ObjectID") == Some(object_id)
                    && node.children().any(|child| {
                        child.has_tag_name("ParameterID")
                            && child.text() == Some(parameter_id.as_str())
                    })
            })
        })
        .unwrap()
}

fn assert_export(document: Value, edited_rotation: bool) {
    let (project, reports) = export(document.clone());
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("editable composed Group G x M")),
        "{reports:?}"
    );
    let clip = geometry_clip(project.single_sequence().unwrap());
    assert!(project.media(clip).unwrap().is_adjustment());
    assert_eq!(clip.transform, PrStaticTransform::default());
    let source_in = clip.in_ticks;
    let (xml, reread) = written(project);
    assert_eq!(
        xml.matches("<MatchName>AE.ADBE Geometry2</MatchName>")
            .count(),
        1
    );
    assert_eq!(
        xml.matches("<AdjustmentLayer>true</AdjustmentLayer>")
            .count(),
        1
    );
    // Inspect raw emitted wire independently of our reader: parameter identities,
    // spatial Linear Position (0), signed/window-external keys and edited Rotation.
    let wire = roxmltree::Document::parse(&xml).unwrap();
    for (id, name, times, values) in [
        (
            2,
            "Position",
            vec![0, 333],
            vec![0.611_458_361_148_834_2, 0.575_520_813_465_118_4],
        ),
        (
            3,
            "Scale Height",
            vec![-167, 0, 542],
            vec![100.0, 101.599975585938, 118.500106811523],
        ),
        (
            7,
            "Rotation",
            vec![0, 417],
            vec![
                0.0,
                if edited_rotation {
                    12.0
                } else {
                    4.000000476837
                },
            ],
        ),
    ] {
        let param = geometry_parameter(&wire, id);
        assert!(param
            .children()
            .any(|child| child.has_tag_name("Name") && child.text() == Some(name)));
        let text = param
            .children()
            .find(|child| child.has_tag_name("Keyframes"))
            .unwrap()
            .text()
            .unwrap();
        let fields: Vec<_> = text
            .trim_end_matches(';')
            .split(';')
            .map(|key| key.split(',').collect::<Vec<_>>())
            .collect();
        assert_eq!(fields.len(), times.len());
        for ((fields, time), value) in fields.iter().zip(times).zip(values) {
            assert_eq!(
                fields[0].parse::<i64>().unwrap() - source_in,
                time * TICKS_PER_MILLISECOND
            );
            assert!(
                (fields[1].split(':').next().unwrap().parse::<f64>().unwrap() - value).abs() < 1e-5
            );
            if name == "Position" {
                assert_eq!(fields[8], "0");
            }
            if name == "Scale Height" {
                // Native mode is outgoing: the final key has no segment.
                assert_eq!(fields[2], if time == 542 { "0" } else { "5" });
            }
        }
    }
    let (again, _) = import(&reread);
    let group = geometry(&again["composition"]["layers"]);
    let original = geometry(&document["composition"]["layers"]);
    for property in ["positionX", "positionY", "scaleX", "scaleY", "rotation"] {
        let actual = keys(&again, group, property).as_array().unwrap();
        let expected = keys(&document, original, property).as_array().unwrap();
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual["layerTime"], expected["layerTime"]);
            assert_eq!(actual["easing"]["type"], expected["easing"]["type"]);
            if expected["easing"]["type"] == "cubicBezier" {
                for handle in ["x1", "y1", "x2", "y2"] {
                    assert!(
                        (actual["easing"][handle].as_f64().unwrap()
                            - expected["easing"][handle].as_f64().unwrap())
                        .abs()
                            < 0.005
                    );
                }
            }
            // Native scalar publication is float32; point values remain doubles.
            assert!(
                (actual["value"]["value"].as_f64().unwrap()
                    - expected["value"]["value"].as_f64().unwrap())
                .abs()
                    < 1e-4
            );
        }
        assert_eq!(actual.len(), expected.len());
    }
    let lower = |group: &Value| {
        group["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap()
            .clone()
    };
    let (actual, expected) = (lower(group), lower(original));
    assert_eq!(actual["sourceRange"], expected["sourceRange"]);
    for (property, axes) in [("position", 2), ("scale", 2), ("anchorPoint", 2)] {
        for axis in 0..axes {
            assert!(
                (actual["transform"][property][axis].as_f64().unwrap()
                    - expected["transform"][property][axis].as_f64().unwrap())
                .abs()
                    < 1e-4
            );
        }
    }
    // A reimported ordinary outer nest must not collide with the stage marker.
    let (second, reports) = export(again);
    assert!(
        !reports
            .iter()
            .any(|report| report.reason.contains("not exported")),
        "{reports:?}"
    );
    assert!(second
        .media(geometry_clip(second.single_sequence().unwrap()))
        .unwrap()
        .is_adjustment());
}

#[test]
fn adjustment_geometry2_nested_opacity_mask_keeps_distinct_guides_and_lower_clocks() {
    // Supplementary interaction control, not an independently native-authored
    // masked Geometry2 oracle. Reuse the pinned controls and bounded nest mask.
    fn assert_lower_source(layers: &Value, expected: &Value) -> usize {
        layers
            .as_array()
            .unwrap()
            .iter()
            .map(|layer| {
                if layer["type"] == "Video" {
                    assert_eq!(&layer["sourceRange"], expected);
                    1
                } else if let Some(children) = layer.get("layers") {
                    assert_lower_source(children, expected)
                } else {
                    0
                }
            })
            .sum()
    }

    let source = derived_source();
    let sequence = source.single_sequence().unwrap().clone();
    let mut mask = opacity_mask();
    mask.feather = 0.0;
    mask.opacity = 100.0;
    let mut nest = nest_of(sequence.clone(), 0..sequence.timeline_end_ticks, 0);
    nest.opacity_mask = Some(mask.clone());
    let mut outer = sequence_of(
        "Masked Geometry2",
        vec![PrVideoTrack {
            items: Vec::new(),
            nests: vec![nest],
            transitions: Vec::new(),
        }],
    );
    outer.frame_rate = FrameRate::Fps24;
    let (document, _) = import(&PrProjectFile::from_sequences(vec![outer], source.media));

    for opacity in [100.0, 60.0] {
        let mut document = document.clone();
        let owner = &mut document["composition"]["layers"][0];
        let coverage_guide = owner["masks"][0]["layer"].clone();
        let stage = owner["layers"][0]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| {
                layer["name"].as_str().is_some_and(|name| {
                    name.starts_with(super::super::adjustment_geometry::STAGE_NAME)
                })
            })
            .unwrap();
        assert_ne!(coverage_guide, stage["masks"][0]["layer"]);
        stage["transform"]["opacity"] = json!(opacity);
        let expected_source = stage["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap()["sourceRange"]
            .clone();
        let (project, reports) = export(document);
        assert!(
            !reports
                .iter()
                .any(|report| report.reason.contains("not exported")),
            "{reports:?}"
        );
        let expected_report = if opacity == 100.0 {
            "editable composed Group G x M"
        } else {
            "ordinary moved nest"
        };
        assert!(reports
            .iter()
            .any(|report| report.reason.contains(expected_report)));

        let assert_picture = |project: &PrProjectFile| {
            let root = project.single_sequence().unwrap();
            assert_eq!(root.video_items().count(), 0, "no unmasked root picture");
            assert_eq!(root.nest_occurrences().count(), 1);
            let masked = root.nest_occurrences().next().unwrap();
            assert_eq!(masked.opacity_mask.as_ref(), Some(&mask));
            assert!(masked.effects.is_empty());
            assert_eq!(masked.sequence.video_items().count(), 0, "no Shape Graphic");
            let picture = masked.sequence.nest_occurrences().next().unwrap();
            assert!(picture.opacity_mask.is_none());
            let stage = picture.sequence.nest_occurrences().next().unwrap();
            assert!(
                stage.opacity_mask.is_none(),
                "Geometry2 guide is not Opacity"
            );
            assert_eq!(stage.opacity, opacity);
            assert_eq!(stage.start_ticks, 2625 * TICKS_PER_MILLISECOND);
            let clips = stage.sequence.video_items().collect::<Vec<_>>();
            assert!(
                clips.iter().all(|item| item.media().is_some()),
                "no guide Graphics"
            );
            assert_eq!(clips.len(), if opacity == 100.0 { 2 } else { 1 });
            let lower = clips
                .iter()
                .filter_map(|item| item.media())
                .find(|clip| !project.media(clip).unwrap().is_adjustment())
                .unwrap();
            assert_eq!(lower.start_ticks, 0);
            assert_eq!(lower.in_ticks, 1833 * TICKS_PER_MILLISECOND);
            assert_eq!(lower.end_ticks, 10 * FrameRate::Fps24.ticks_per_frame());
            if opacity == 100.0 {
                assert!(project
                    .media(geometry_clip(&stage.sequence))
                    .unwrap()
                    .is_adjustment());
            }
        };
        assert_picture(&project);
        let (_, reread) = written(project);
        assert_picture(&reread);
        let (again, _) = import(&reread);
        // Native stage/moved-picture wrappers differ, but there must be exactly
        // one lower Video retaining its authored source clock in either route.
        assert_eq!(
            assert_lower_source(&again["composition"]["layers"], &expected_source),
            1
        );
    }
}

#[test]
fn adjustment_geometry2_correction_mask_keeps_sibling_blur() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_adjustment_motion_wipe_26_5_strict.prproj");
    let (native, _) = PrProjectFile::load(&fixture).unwrap();
    let wipe = native
        .single_sequence()
        .unwrap()
        .video_items()
        .filter_map(|item| item.media())
        .find_map(|clip| clip.linear_wipe.clone())
        .unwrap();
    let mut project = derived_source();
    let adjustment = project.sequences[0].video_tracks[1].clip_mut(0);
    let blur = crate::tests::support::directional_blur(true, 45.0, 30.0);
    adjustment.effects.insert(0, blur.clone());
    adjustment.effects_above_mask = 1;
    adjustment.linear_wipe = Some(wipe);
    for video in project
        .media
        .values_mut()
        .filter_map(|media| media.video.as_mut())
    {
        if let crate::schema::PrMediaKind::Video { codec, .. } = &mut video.kind {
            *codec = Some(VideoCodec::H264);
        }
    }
    // Writer supplies the explicit Blur / Wipe / animated Geometry2 order;
    // the Wipe and Geometry2 controls originate in the pinned native fixtures.
    let (xml, reread) = written(project);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("masked.prproj");
    crate::test_support::write_prproj(&path, &xml);
    let (_, reports) = PrProjectFile::load(&path).unwrap();
    assert!(
        reports.iter().any(
            |report| report.reason.contains("Geometry2 composite staging")
                && report.reason.contains("Crop/Wipe/Matte mask boundary")
                && report
                    .reason
                    .contains("without discarding supported sibling effects")
        ),
        "{reports:?}"
    );
    let adjustment = reread
        .single_sequence()
        .unwrap()
        .video_items()
        .filter_map(|item| item.media())
        .find(|clip| reread.media(clip).unwrap().is_adjustment())
        .unwrap();
    assert!(adjustment.linear_wipe.is_some());
    assert_eq!(adjustment.effects.len(), 1, "{adjustment:?}");
    assert_eq!(adjustment.effects[0].params, blur.params);
    assert_eq!(adjustment.effects_above_mask, 1);
}

fn assert_routed_lower(document: Value, trimmed: bool) {
    let (project, reports) = export(document);
    assert!(
        !reports
            .iter()
            .any(|report| report.reason.contains("not exported")),
        "{reports:?}"
    );
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("editable composed Group G x M")),
        "{reports:?}"
    );
    let sequence = project.single_sequence().unwrap();
    let adjustment = geometry_clip(sequence);
    assert!(project.media(adjustment).unwrap().is_adjustment());
    assert_eq!(adjustment.transform, PrStaticTransform::default());
    let nest = sequence.nest_occurrences().next().unwrap();
    let lower = nest
        .sequence
        .video_items()
        .filter_map(|item| item.media())
        .find(|clip| !project.media(clip).unwrap().is_adjustment())
        .unwrap();
    assert!(lower.end_ticks > lower.start_ticks);
    if trimmed {
        assert!(lower.start_ticks > 0);
        assert!(
            lower.end_ticks - lower.start_ticks < adjustment.end_ticks - adjustment.start_ticks
        );
    }
}

#[test]
fn adjustment_geometry2_correction_routes_trimmed_lower() {
    let (mut document, _) = import(&derived_source());
    let group = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    let lower = group["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    lower["playback"]["inputRange"] = json!({"start":50,"duration":200});
    lower["sourceRange"] = json!({"start":1883,"duration":200});
    lower["source"]["assetId"] = json!("geometry-trimmed-lower");
    let lower_id = fx_schema::LayerId::new(lower["id"].as_u64().unwrap());
    // Exercise the actual missing-facts abort before checking selection itself.
    // No root sibling can supply facts for this unique lower asset.
    assert_routed_lower(document.clone(), true);
    let parsed = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let composition = parsed.composition();
    let selected: Vec<_> = crate::convert::exported_video_layers(
        composition.layers(),
        composition.dynamics(),
        [1920, 1080],
    )
    .into_iter()
    .filter_map(|layer| {
        crate::convert::video_data(layer).unwrap().map(|video| {
            assert_eq!(
                crate::convert::active_asset_id(&video.source).as_str(),
                "geometry-trimmed-lower"
            );
            layer.id()
        })
    })
    .collect();
    assert_eq!(selected, [lower_id]);
}

#[test]
fn adjustment_geometry2_inspection_follows_stage_and_guide_admission() {
    for opacity in [100.0, 60.0] {
        let (mut document, _) = import(&derived_source());
        let group = document["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        group["transform"]["opacity"] = json!(opacity);
        let group_id = fx_schema::LayerId::new(group["id"].as_u64().unwrap());
        let guide_id = group["masks"][0]["layer"].clone();
        let lower = group["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        lower["playback"]["inputRange"] = json!({"start":50,"duration":200});
        lower["sourceRange"] = json!({"start":1883,"duration":200});
        let lower_id = fx_schema::LayerId::new(lower["id"].as_u64().unwrap());
        let parsed = EditableFxCompositionDocument::from_json_value(document.clone()).unwrap();
        let composition = parsed.composition();
        let selected: Vec<_> = crate::convert::exported_video_layers(
            composition.layers(),
            composition.dynamics(),
            [1920, 1080],
        )
        .into_iter()
        .map(|layer| layer.id())
        .collect();
        assert_eq!(selected, [group_id, lower_id], "opacity {opacity}");
        assert!(crate::convert::exported_clip_videos(
            composition.layers(),
            composition.dynamics(),
            [1920, 1080],
        )
        .is_empty());

        let group = document["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        let guide = group["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["id"] == guide_id)
            .unwrap();
        guide["rect"]["size"][0] = json!(1919.0);
        let parsed = EditableFxCompositionDocument::from_json_value(document).unwrap();
        let composition = parsed.composition();
        assert!(
            crate::convert::exported_video_layers(
                composition.layers(),
                composition.dynamics(),
                [1920, 1080],
            )
            .is_empty(),
            "invalid guide, opacity {opacity}"
        );
    }
}

#[test]
fn adjustment_geometry2_correction_routes_identity_lower() {
    let (mut document, _) = import(&derived_source());
    let group = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    let lower = group["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    lower["transform"] =
        serde_json::to_value(super::super::background::identity_transform()).unwrap();
    assert_routed_lower(document, false);
}

#[test]
fn adjustment_geometry2_correction_group_opacity_preserves_picture_as_moved_nest() {
    let (mut document, _) = import(&derived_source());
    let group = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    group["transform"]["anchorPoint"] = json!([360.0, 720.0]);
    let group = group.clone();
    document["composition"]["dynamics"]["entries"].as_array_mut().unwrap().push(json!({
        "target":{"kind":"layer","layerId":group["id"],"propertyType":"opacity"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"opacity-a","layerTime":0,"value":{"type":"float","value":100.0},"easing":{"type":"linear"}},
            {"id":"opacity-b","layerTime":417,"value":{"type":"float","value":50.0},"easing":{"type":"linear"}}
        ]}
    }));
    document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| {
            entry["target"]["layerId"] == group["id"]
                && entry["target"]["propertyType"] == "rotation"
        })
        .unwrap()["animator"]["keyframes"][1]["value"]["value"] = json!(12.0);
    let (project, reports) = export(document.clone());
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("Group opacity")
                && report.reason.contains("ordinary moved nest")),
        "{reports:?}"
    );
    assert!(
        !reports
            .iter()
            .any(|report| report.reason.contains("not exported")),
        "{reports:?}"
    );
    let nest = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert!(!nest
        .sequence
        .name
        .starts_with(super::super::adjustment_geometry::STAGE_NAME));
    let lower = nest
        .sequence
        .video_items()
        .filter_map(|item| item.media())
        .next()
        .unwrap();
    assert!(!project.media(lower).unwrap().is_adjustment());
    assert_eq!(nest.sequence.video_items().count(), 1);
    assert!(nest.animations.iter().any(
        |animation| matches!(animation, crate::schema::PrPropertyAnimation::Opacity(keys)
        if keys.len() == 2 && keys[1].value == 50.0)
    ));
    let (xml, reread) = written(project);
    assert!(!xml.contains("<MatchName>AE.ADBE Geometry2</MatchName>"));
    let (again, _) = import(&reread);
    let moved = again["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    assert!(!moved["name"]
        .as_str()
        .unwrap()
        .starts_with(super::super::adjustment_geometry::STAGE_NAME));
    let lower = moved["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(lower["sourceRange"], json!({"start":1833,"duration":417}));
    for axis in 0..2 {
        assert!(
            (moved["transform"]["anchorPoint"][axis].as_f64().unwrap()
                - group["transform"]["anchorPoint"][axis].as_f64().unwrap())
            .abs()
                < 1e-4
        );
    }
    for property in [
        "positionX",
        "positionY",
        "scaleX",
        "scaleY",
        "rotation",
        "opacity",
    ] {
        let actual = keys(&again, moved, property).as_array().unwrap();
        let expected = keys(&document, &group, property).as_array().unwrap();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual["layerTime"], expected["layerTime"]);
            assert!(
                (actual["value"]["value"].as_f64().unwrap()
                    - expected["value"]["value"].as_f64().unwrap())
                .abs()
                    < 1e-4
            );
        }
    }
    let (_, second_reports) = export(again);
    assert!(
        !second_reports
            .iter()
            .any(|report| report.reason.contains("not exported")),
        "{second_reports:?}"
    );
}

#[test]
fn group_motion_blur_preserves_adjustment_geometry_stage_and_opacity_fallback() {
    for opacity in [100.0, 60.0] {
        let (mut document, _) = import(&derived_source());
        let group = document["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        group["transform"]["opacity"] = json!(opacity);
        let id = group["id"].as_u64().unwrap();
        let (baseline, _) = export(document.clone());
        document["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["id"] == id)
            .unwrap()["motionBlur"] = json!(true);
        let (project, reports) = export(document);
        let nest = project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .next()
            .unwrap();
        let expected = baseline
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .next()
            .unwrap();
        assert_eq!(nest.timeline_ticks(), expected.timeline_ticks());
        assert_eq!(nest.transform, expected.transform);
        assert_eq!(nest.opacity, expected.opacity);
        assert_eq!(nest.animations, expected.animations);
        assert_eq!(
            nest.sequence.video_items().count(),
            expected.sequence.video_items().count()
        );
        assert_eq!(
            nest.sequence
                .video_occurrences()
                .flat_map(|clip| &clip.effects)
                .collect::<Vec<_>>(),
            expected
                .sequence
                .video_occurrences()
                .flat_map(|clip| &clip.effects)
                .collect::<Vec<_>>()
        );
        assert!(
            reports
                .iter()
                .any(|report| report.scope == crate::OmissionScope::Feature
                    && report.record.starts_with(&format!("layer {id} ("))
                    && report.reason.contains("group motion blur was not exported")),
            "{reports:?}"
        );
        let (xml, _) = written(project);
        assert_eq!(
            xml.matches("<MatchName>AE.ADBE Geometry2</MatchName>")
                .count(),
            usize::from(opacity == 100.0)
        );
    }
}

#[test]
fn adjustment_geometry2_static_group_opacity_uses_moved_nest() {
    let (mut document, _) = import(&derived_source());
    let group = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    group["transform"]["opacity"] = json!(60.0);
    let (project, reports) = export(document);
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("Group opacity")
                && report.reason.contains("ordinary moved nest")),
        "{reports:?}"
    );
    let nest = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert_eq!(nest.opacity, 60.0);
    assert_eq!(nest.sequence.video_items().count(), 1);
    let (xml, reread) = written(project);
    assert!(!xml.contains("<MatchName>AE.ADBE Geometry2</MatchName>"));
    let (again, _) = import(&reread);
    let moved = again["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    assert_eq!(moved["transform"]["opacity"], 60.0);
    assert!(moved["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer["type"] == "Video"));
}

#[test]
fn adjustment_geometry2_generated_nest_name_checks_writer_bound() {
    let mut native = derived_source();
    let upper = native.sequences[0].video_tracks[0].clip(0).clone();
    native.sequences[0]
        .video_tracks
        .push(PrVideoTrack::media([upper]));
    let (original, _) = import(&native);
    // Generated prefix/suffix adds 28 characters. Count characters, not UTF-8 bytes.
    for length in [227, 228] {
        let mut document = original.clone();
        let group = document["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        let prefix = super::super::adjustment_geometry::STAGE_NAME;
        group["name"] = json!(format!(
            "{prefix}{}",
            "é".repeat(length - prefix.chars().count())
        ));
        let (project, reports) = export(document);
        assert_eq!(
            reports
                .iter()
                .any(|report| report.reason.contains("sequence label")),
            length == 228,
            "{reports:?}"
        );
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .nest_occurrences()
                .count(),
            1
        );
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .nest_occurrences()
                .next()
                .unwrap()
                .sequence
                .name
                .chars()
                .count(),
            (length + 28).min(255)
        );
        written(project);
    }
}

#[test]
fn adjustment_geometry2_native_controls_and_quantized_roundtrip() {
    let (document, reports) = import(&derived_source());
    assert_native_controls(&document, &reports);
    assert_export(document, false);
}

#[test]
fn adjustment_geometry2_edited_rotation_and_animated_anchor_export() {
    let (mut document, _) = import(&derived_source());
    let group = geometry(&document["composition"]["layers"]).clone();
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    entries
        .iter_mut()
        .find(|entry| {
            entry["target"]["layerId"] == group["id"]
                && entry["target"]["propertyType"] == "rotation"
        })
        .unwrap()["animator"]["keyframes"][1]["value"]["value"] = json!(12.0);
    assert_export(document.clone(), true);
    // Explicit edited FX input, supplementary structural Anchor animation proof.
    for (property, values) in [
        ("anchorPointX", [960.0, 1100.0]),
        ("anchorPointY", [540.0, 600.0]),
    ] {
        document["composition"]["dynamics"]["entries"].as_array_mut().unwrap().push(json!({
            "target":{"kind":"layer","layerId":group["id"],"propertyType":property},
            "animator":{"type":"keyframes","enabled":true,"keyframes":[
                {"id":format!("{property}-a"),"layerTime":-167,"value":{"type":"float","value":values[0]},"easing":{"type":"linear"}},
                {"id":format!("{property}-b"),"layerTime":542,"value":{"type":"float","value":values[1]},"easing":{"type":"hold"}}
            ]}
        }));
    }
    let (project, reports) = export(document);
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("animated Anchor retained")),
        "{reports:?}"
    );
    let source_in = geometry_clip(project.single_sequence().unwrap()).in_ticks;
    let (xml, reread) = written(project);
    let wire = roxmltree::Document::parse(&xml).unwrap();
    let anchor = geometry_parameter(&wire, 1);
    assert!(anchor.has_tag_name("PointComponentParam"));
    let fields: Vec<_> = anchor
        .children()
        .find(|node| node.has_tag_name("Keyframes"))
        .unwrap()
        .text()
        .unwrap()
        .trim_end_matches(';')
        .split(';')
        .map(|key| key.split(',').collect::<Vec<_>>())
        .collect();
    assert_eq!(fields.len(), 2);
    for (index, (time, values)) in [(-167, [0.5, 0.5]), (542, [1100.0 / 1920.0, 600.0 / 1080.0])]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            fields[index][0].parse::<i64>().unwrap() - source_in,
            time * TICKS_PER_MILLISECOND
        );
        let actual: Vec<f64> = fields[index][1]
            .split(':')
            .map(|value| value.parse().unwrap())
            .collect();
        assert_eq!(actual.len(), 2);
        for (actual, expected) in actual.into_iter().zip(values) {
            assert!((actual - expected).abs() < 1e-9);
        }
    }
    assert_eq!(
        fields[0][2], "4",
        "Anchor first outgoing segment must be Hold"
    );
    let (again, reports) = import(&reread);
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("animated Anchor retained")),
        "{reports:?}"
    );
    let group = geometry(&again["composition"]["layers"]);
    for (property, values) in [
        ("anchorPointX", [960.0, 1100.0]),
        ("anchorPointY", [540.0, 600.0]),
    ] {
        let track = keys(&again, group, property);
        assert_eq!(track[0]["layerTime"], -167);
        assert_eq!(track[1]["layerTime"], 542);
        close(&track[0]["value"]["value"], values[0]);
        close(&track[1]["value"]["value"], values[1]);
        assert_eq!(track[1]["easing"], json!({"type":"hold"}));
    }
    // Supplementary constructed-model boundary: these Anchor keys belong only
    // to adjustment Geometry2. The native reader already rejects ordinary
    // Transform keys, and the shared stage mapper must not bypass that limit.
    let ordinary_xml = xml.replace(
        "<MatchName>AE.ADBE Geometry2</MatchName>",
        "<MatchName>AE.ADBE Geometry</MatchName>",
    );
    assert_ne!(ordinary_xml, xml);
    let (_, reports) = crate::format::inspect_project_with_omissions(
        &ordinary_xml,
        reread.single_sequence().unwrap().id(),
    )
    .unwrap();
    assert!(
        reports.iter().any(|report| report
            .reason
            .contains("keyframed Anchor Point is not supported; only static values convert")),
        "{reports:?}"
    );
    let mut ordinary = geometry_clip(reread.single_sequence().unwrap())
        .effects
        .iter()
        .find(|effect| matches!(effect.params, PrEffectParams::AdjustmentGeometry2(_)))
        .unwrap()
        .clone();
    let PrEffectParams::AdjustmentGeometry2(transform) = ordinary.params else {
        panic!("expected adjustment Geometry2");
    };
    ordinary.params = PrEffectParams::Transform(transform);
    let error = super::super::premiere_to_tesseract::transform_stage_tracks(
        &ordinary,
        &transform,
        [1920, 1080],
        source_in,
        fx_schema::LayerId::new(1),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("keyframed Transform Anchor Point has no layer property"),
        "{error}"
    );
}

#[test]
fn adjustment_geometry2_upper_scope_and_unsupported_stack_preserve_lower() {
    let mut project = derived_source();
    let lower = project.single_sequence().unwrap().video_tracks[0]
        .clip(0)
        .clone();
    let mut upper = lower.clone();
    upper.transform.rotation = 27.0;
    project.sequences[0]
        .video_tracks
        .push(PrVideoTrack::media([upper]));
    let (document, _) = import(&project);
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers[0]["type"], "Video");
    assert_eq!(layers[0]["transform"]["rotation"], 27.0);
    assert_eq!(layers[0]["playback"]["inputRange"]["start"], 2625);
    assert_eq!(
        geometry(&document["composition"]["layers"])["layers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // Competing effects must not be reordered onto the lower children.
    let adjustment = project.sequences[0].video_tracks[1].clip_mut(0);
    adjustment
        .effects
        .push(crate::tests::support::directional_blur(true, 45.0, 30.0));
    let (document, reports) = import(&project);
    assert!(!document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer["type"] == "Group"));
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("requires Geometry2 alone")),
        "{reports:?}"
    );
    let unchanged = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video" && layer["transform"]["rotation"] == 0.0)
        .unwrap();
    assert_eq!(
        unchanged["sourceRange"],
        json!({"start":1833,"duration":417})
    );
    let mut project = derived_source();
    let mut lower_adjustment = project.sequences[0].video_tracks[1].clip(0).clone();
    lower_adjustment.effects.clear();
    project.sequences[0]
        .video_tracks
        .insert(1, PrVideoTrack::media([lower_adjustment]));
    let (document, reports) = import(&project);
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Adjustment")
            .count(),
        2
    );
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .count(),
        1
    );
    assert!(!layers.iter().any(|layer| layer["type"] == "Group"));
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("Adjustment layer")
                && report
                    .reason
                    .contains("cannot be relocated into an editable picture stage")
                && report.reason.contains("supported layer types")),
        "{reports:?}"
    );
}

#[test]
fn adjustment_geometry2_preserves_lower_keys_and_diagnoses_curved_position_and_crossing_windows() {
    let mut project = derived_source();
    let lower = project.sequences[0].video_tracks[0].clip_mut(0);
    lower
        .animations
        .push(crate::schema::PrPropertyAnimation::Rotation(vec![
            crate::schema::PrScalarKeyframe {
                source_ticks: lower.in_ticks - 100 * TICKS_PER_MILLISECOND,
                value: -8.0,
                easing: crate::schema::PrKeyframeEasing::Linear,
            },
            crate::schema::PrScalarKeyframe {
                source_ticks: lower.in_ticks + 700 * TICKS_PER_MILLISECOND,
                value: 16.0,
                easing: crate::schema::PrKeyframeEasing::Hold,
            },
        ]));
    let (document, _) = import(&project);
    let group = geometry(&document["composition"]["layers"]);
    let child = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    let track = keys(&document, child, "rotation");
    assert_eq!(track[0]["layerTime"], -100);
    assert_eq!(track[1]["layerTime"], 700);
    assert_eq!(track[0]["value"]["value"], -8.0);
    assert_eq!(track[1]["value"]["value"], 16.0);
    assert_eq!(track[1]["easing"], json!({"type":"hold"}));

    // Deliberately curved derivative: retained handles need an explicit traversal
    // diagnostic, unlike the independently read-back straight native segment.
    let position = project.sequences[0].video_tracks[1].clip_mut(0).effects[0]
        .animations
        .iter_mut()
        .find(|animation| animation.param.id == crate::schema::TRANSFORM_POSITION.id)
        .unwrap();
    let crate::schema::PrEffectParamKeys::Point(points) = &mut position.keys else {
        panic!("native Position is point keyed")
    };
    points[0].spatial_out_tangent = Some([-0.01, 0.1]);
    let (document, reports) = import(&project);
    assert!(
        reports.iter().any(|report| report
            .reason
            .contains("parametrically rather than native constant-speed")),
        "{reports:?}"
    );
    let group = geometry(&document["composition"]["layers"]);
    assert!(keys(&document, group, "positionY")[0]
        .get("spatialOutTangent")
        .is_some());
    let (exported, reports) = export(document);
    assert!(
        reports.iter().any(|report| report
            .reason
            .contains("parametrically rather than native constant-speed")),
        "{reports:?}"
    );
    assert!(exported
        .media(geometry_clip(exported.single_sequence().unwrap()))
        .unwrap()
        .is_adjustment());

    let lower = project.sequences[0].video_tracks[0].clip_mut(0);
    lower.end_ticks += TICKS / 24;
    lower.out_ticks += TICKS / 24;
    let (document, reports) = import(&project);
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("crossing/partial lower windows")),
        "{reports:?}"
    );
    assert!(!document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer["type"] == "Group"));
    let lower = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(
        lower["playback"]["inputRange"],
        json!({"start":2625,"duration":458})
    );
    assert_eq!(lower["sourceRange"], json!({"start":1833,"duration":458}));
}

/// Supplementary composition model using native-derived G controls. Not a new
/// independently Adobe-saved or rendered combined-effect source.
fn suffix_source() -> PrProjectFile {
    use crate::schema::{
        PrEffect, PrEffectParamAnimation, PrEffectParamKeys, PrGaussianBlur, PrKeyframeEasing,
        PrScalarKeyframe, GAUSSIAN_BLUR_BLURRINESS, NOISE_AMOUNT,
    };
    let mut project = derived_source();
    let adjustment = project.sequences[0].video_tracks[1].clip_mut(0);
    let origin = adjustment.in_ticks;
    let keyed = |param, values: [f64; 2]| PrEffectParamAnimation {
        param,
        keys: PrEffectParamKeys::Scalar(vec![
            PrScalarKeyframe {
                source_ticks: origin - 100 * TICKS_PER_MILLISECOND,
                value: values[0],
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: origin + 700 * TICKS_PER_MILLISECOND,
                value: values[1],
                easing: PrKeyframeEasing::Hold,
            },
        ]),
    };
    adjustment.effects.extend([
        PrEffect {
            enabled: true,
            mask: None,
            params: PrEffectParams::GaussianBlur(PrGaussianBlur {
                blurriness: 11.4,
                repeat_edge_pixels: false,
            }),
            animations: vec![keyed(&GAUSSIAN_BLUR_BLURRINESS, [11.4, 28.5])],
        },
        PrEffect {
            enabled: false,
            mask: None,
            params: PrEffectParams::Noise { amount: 5.0 },
            animations: vec![keyed(&NOISE_AMOUNT, [5.0, 25.0])],
        },
    ]);
    project
}

#[test]
fn adjustment_geometry2_suffix_composes_lower_and_keeps_effect_ids_and_clocks() {
    let mut project = suffix_source();
    let mut second = project.sequences[0].video_tracks[0].clip(0).clone();
    second.transform.rotation = -19.0;
    second
        .animations
        .push(crate::schema::PrPropertyAnimation::Rotation(vec![
            crate::schema::PrScalarKeyframe {
                source_ticks: second.in_ticks - 100 * TICKS_PER_MILLISECOND,
                value: -19.0,
                easing: crate::schema::PrKeyframeEasing::Hold,
            },
        ]));
    project.sequences[0]
        .video_tracks
        .insert(1, PrVideoTrack::media([second.clone()]));
    second.transform.rotation = 27.0;
    second.animations.clear();
    project.sequences[0]
        .video_tracks
        .push(PrVideoTrack::media([second]));
    // A nonzero, time-disjoint lower sibling must stay below G without being
    // captured as a child, moved above the inserted stage or relocated to zero.
    let mut disjoint = project.sequences[0].video_tracks[0].clip(0).clone();
    disjoint.start_ticks = 500 * TICKS_PER_MILLISECOND;
    disjoint.end_ticks = disjoint.start_ticks + disjoint.out_ticks - disjoint.in_ticks;
    disjoint.transform.rotation = 41.0;
    project.sequences[0]
        .video_tracks
        .insert(0, PrVideoTrack::media([disjoint]));
    let (document, reports) = import(&project);
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .map(|layer| layer["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Video", "Adjustment", "Group", "Video", "Rect"]
    );
    assert_eq!(
        layers
            .iter()
            .take(4)
            .map(|layer| layer["id"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [5, 4, 6, 1]
    );
    assert_eq!(layers[0]["transform"]["rotation"], 27.0);
    let (suffix, group) = (&layers[1], &layers[2]);
    assert_eq!(suffix["id"], 4); // Original occurrence identity, not a new host.
    assert_eq!(group["id"], 6);
    let disjoint = &layers[3];
    assert!(disjoint["parent"].is_null());
    assert_eq!(disjoint["transform"]["rotation"], 41.0);
    assert_eq!(
        disjoint["sourceRange"],
        json!({"start":1833,"duration":417})
    );
    assert_eq!(
        disjoint["playback"]["inputRange"],
        json!({"start":500,"duration":417})
    );
    assert_eq!(
        disjoint["playback"]["mapping"],
        json!({"type":"linear","input":{"start":500,"duration":417},"output":{"start":1833,"duration":417}})
    );
    assert_eq!(suffix["activeRange"], json!({"start":2625,"duration":417}));
    assert_eq!(suffix["effects"][0]["id"], 1);
    assert_eq!(suffix["effects"][1]["id"], 2);
    assert_eq!(suffix["effects"][1]["enabled"], false);
    assert!(!document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["target"]["layerId"] == suffix["id"]));
    assert!(reports.iter().any(|report| report
        .reason
        .contains("root Adjustment above a Geometry2 stage")
        && report.reason.contains("exposed exterior/root black")));
    assert!(group.get("effects").is_none()); // Empty effects are omitted on the wire.
    assert_eq!(group["layers"].as_array().unwrap().len(), 3);
    let lower = &group["layers"][0];
    assert_eq!(lower["id"], 3);
    assert_eq!(lower["parent"], group["id"]);
    assert_eq!(lower["sourceRange"], json!({"start":1833,"duration":417}));
    assert_eq!(lower["playback"]["inputOffsetMs"], 2625);
    assert_eq!(keys(&document, lower, "rotation")[0]["layerTime"], -100);
    assert_eq!(
        keys(&document, lower, "rotation")[0]["value"]["value"],
        -19.0
    );
    assert_native_controls(&document, &reports);
    for (effect_id, param, values) in [
        (1, "blurriness", [11.4, 28.5]),
        (2, "intensity", [2.0, 10.0]),
    ] {
        let entry = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["target"]["effectId"] == effect_id)
            .unwrap();
        assert!(entry["target"].get("layerId").is_none()); // Effect ID owns this track.
        assert!(suffix["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|effect| effect["id"] == effect_id));
        assert_eq!(entry["target"]["paramName"], param);
        let track = &entry["animator"]["keyframes"];
        assert_eq!(track[0]["layerTime"], -100);
        assert_eq!(track[1]["layerTime"], 700);
        assert_eq!(track[0]["value"]["value"], values[0]);
        assert_eq!(track[1]["value"]["value"], values[1]);
        assert_eq!(track[1]["easing"], json!({"type":"hold"}));
    }
}

#[test]
fn adjustment_geometry2_suffix_reuses_lumetri_and_sharpen_payloads() {
    // Native donor controls on other hosts, explicitly grafted into this model.
    let (lumetri, _) = crate::format::inspect_project_with_omissions(
        include_str!("../../../tests/fixtures/human_lumetri_contrast.xml"),
        None,
    )
    .unwrap();
    let sharpen_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_sharpen_strict.prproj");
    let (sharpen, _) =
        PrProjectFile::load_selected(&sharpen_path, Some("72a26059-6f85-4033-827d-63692bb9859b"))
            .unwrap();
    let mut project = suffix_source();
    let adjustment = project.sequences[0].video_tracks[1].clip_mut(0);
    let origin = adjustment.in_ticks;
    let mut donor = lumetri.sequences[0].video_tracks[0].clip(0).effects.clone();
    assert_eq!(donor.len(), 6);
    donor.push(sharpen.sequences[0].video_tracks[0].clip(3).effects[0].clone());
    for effect in &mut donor {
        for animation in &mut effect.animations {
            let crate::schema::PrEffectParamKeys::Scalar(keys) = &mut animation.keys else {
                panic!("donor controls are scalar")
            };
            for key in keys {
                key.source_ticks += origin;
            }
        }
    }
    adjustment.effects.extend(donor);
    let (document, _) = import(&project);
    let suffix = &document["composition"]["layers"][0];
    let group = geometry(&document["composition"]["layers"]);
    assert_ne!(suffix["id"], group["id"]);
    let effects = suffix["effects"].as_array().unwrap();
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect["effect"]["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "gaussianBlur",
            "grain",
            "exposure",
            "brightnessContrast",
            "hueSaturation",
            "temperatureTint",
            "temperatureTint",
            "vignette",
            "sharpen"
        ]
    );
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect["id"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        (1..=9).collect::<Vec<_>>()
    );
    assert_eq!(effects[2]["effect"]["exposure"], 1.0);
    assert_eq!(effects[3]["effect"]["contrast"], 25.0);
    assert_eq!(effects[8]["effect"]["amount"], 20.0);
    for (id, param, times, values) in [
        (5, "saturation", vec![0, 2513], vec![30.0, -40.0]),
        (9, "amount", vec![1000, 1500, 2500], vec![20.0, 80.0, 50.0]),
    ] {
        let entry = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["target"]["effectId"] == id)
            .unwrap();
        assert!(entry["target"].get("layerId").is_none());
        assert_eq!(entry["target"]["paramName"], param);
        let track = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(
            track
                .iter()
                .map(|key| key["layerTime"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            times
        );
        assert_eq!(
            track
                .iter()
                .map(|key| key["value"]["value"].as_f64().unwrap())
                .collect::<Vec<_>>(),
            values
        );
    }
    let (exported, reports) = export(document);
    let suffix = exported
        .single_sequence()
        .unwrap()
        .video_items()
        .filter_map(|item| item.media())
        .find(|clip| exported.media(clip).unwrap().is_adjustment())
        .unwrap();
    assert_eq!(suffix.effects.len(), 4); // Blur, Noise, native Contrast replacement, Sharpen.
    assert!(matches!(
        suffix.effects[2].params,
        PrEffectParams::BrightnessContrast(_)
    ));
    assert!(matches!(
        suffix.effects[3].params,
        PrEffectParams::Sharpen(_)
    ));
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("exposure")
                && report.reason.contains("not exported")),
        "{reports:?}"
    );
    let (_, reread) = written(exported);
    let (again, _) = import(&reread);
    assert_eq!(
        again["composition"]["layers"][0]["effects"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        again["composition"]["layers"][0]["effects"][3]["effect"]["type"],
        "sharpen"
    );
    geometry(&again["composition"]["layers"]);
}

fn effect_wire_owner<'a, 'input>(
    wire: &'a roxmltree::Document<'input>,
    match_name: &str,
) -> (
    roxmltree::Node<'a, 'input>,
    roxmltree::Node<'a, 'input>,
    roxmltree::Node<'a, 'input>,
) {
    let component = wire
        .descendants()
        .find(|node| {
            node.has_tag_name("VideoFilterComponent")
                && node.children().any(|child| {
                    child.has_tag_name("MatchName") && child.text() == Some(match_name)
                })
        })
        .unwrap();
    let chain = wire
        .descendants()
        .find(|node| {
            node.has_tag_name("VideoComponentChain")
                && node.descendants().any(|reference| {
                    reference.has_tag_name("Component")
                        && reference.attribute("ObjectRef") == component.attribute("ObjectID")
                })
        })
        .unwrap();
    let placement = wire
        .descendants()
        .find(|node| {
            node.has_tag_name("VideoClipTrackItem")
                && node.descendants().any(|reference| {
                    reference.has_tag_name("Components")
                        && reference.attribute("ObjectRef") == chain.attribute("ObjectID")
                })
        })
        .unwrap();
    let sub_id = placement
        .descendants()
        .find(|node| node.has_tag_name("SubClip"))
        .unwrap()
        .attribute("ObjectRef")
        .unwrap();
    let sub = wire
        .descendants()
        .find(|node| node.attribute("ObjectID") == Some(sub_id))
        .unwrap();
    let clip_id = sub
        .descendants()
        .find(|node| node.has_tag_name("Clip"))
        .unwrap()
        .attribute("ObjectRef")
        .unwrap();
    let clip = wire
        .descendants()
        .find(|node| node.attribute("ObjectID") == Some(clip_id))
        .unwrap();
    (component, chain, clip)
}

#[test]
fn adjustment_geometry2_suffix_current_edits_export_separate_native_owners() {
    let (mut document, _) = import(&suffix_source());
    let group_id = geometry(&document["composition"]["layers"])["id"].clone();
    document["composition"]["layers"][0]["effects"]
        .as_array_mut()
        .unwrap()
        .reverse();
    document["composition"]["layers"][0]["effects"][1]["enabled"] = json!(false);
    for entry in document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
    {
        if entry["target"]["layerId"] == group_id && entry["target"]["propertyType"] == "rotation" {
            entry["animator"]["keyframes"][1]["value"]["value"] = json!(12.0);
        }
        if entry["target"]["effectId"] == 1 {
            entry["animator"]["keyframes"][0]["value"]["value"] = json!(17.1);
            entry["animator"]["keyframes"][1]["value"]["value"] = json!(34.2);
        }
    }
    let (project, reports) = export(document.clone());
    assert!(reports.iter().any(|report| report
        .reason
        .contains("root Adjustment above a Geometry2 stage")
        && report
            .reason
            .contains("native backdrop and alpha fidelity remain unmeasured")));
    assert!(
        !reports
            .iter()
            .any(|report| report.reason.contains("not exported")),
        "{reports:?}"
    );
    let root = project.single_sequence().unwrap();
    let nest = root.nest_occurrences().next().unwrap();
    assert_eq!(nest.transform, PrStaticTransform::default());
    let suffix = root
        .video_items()
        .filter_map(|item| item.media())
        .next()
        .unwrap();
    assert!(project.media(suffix).unwrap().is_adjustment());
    assert!(root
        .video_tracks
        .last()
        .unwrap()
        .items
        .iter()
        .any(|item| item.media().is_some_and(|clip| std::ptr::eq(clip, suffix))));
    assert!(matches!(
        suffix.effects[0].params,
        PrEffectParams::Noise { .. }
    ));
    assert!(matches!(
        suffix.effects[1].params,
        PrEffectParams::FilmImpactBlur(_)
    ));
    assert_eq!(nest.sequence.video_items().count(), 2);
    let (xml, reread) = written(project);
    assert_eq!(
        xml.matches("<AdjustmentLayer>true</AdjustmentLayer>")
            .count(),
        2
    );
    assert_eq!(
        xml.matches("<IsAdjustmentLayer>true</IsAdjustmentLayer>")
            .count(),
        1 // Both placements use the canonical shared flagged generator master.
    );
    assert!(!xml.contains("Adjustment sequence canvas"));
    let wire = roxmltree::Document::parse(&xml).unwrap();
    let (blur, suffix_chain, suffix_clip) = effect_wire_owner(&wire, "AE.Impact_Blur_FX");
    let (_, geometry_chain, geometry_clip) = effect_wire_owner(&wire, "AE.ADBE Geometry2");
    assert_ne!(
        suffix_chain.attribute("ObjectID"),
        geometry_chain.attribute("ObjectID")
    );
    assert_ne!(
        suffix_clip.attribute("ObjectID"),
        geometry_clip.attribute("ObjectID")
    );
    assert_eq!(
        blur.descendants()
            .find(|node| node.has_tag_name("Bypass"))
            .unwrap()
            .text(),
        Some("true")
    );
    let mut native_order: Vec<_> = suffix_chain
        .descendants()
        .filter(|node| node.has_tag_name("Component"))
        .filter_map(|reference| {
            let component = wire
                .descendants()
                .find(|node| node.attribute("ObjectID") == reference.attribute("ObjectRef"))?;
            let name = component
                .children()
                .find(|node| node.has_tag_name("MatchName"))?
                .text()?;
            Some((
                reference
                    .attribute("Index")
                    .unwrap()
                    .parse::<u32>()
                    .unwrap(),
                name,
            ))
        })
        .collect();
    native_order.sort_by(|a, b| b.0.cmp(&a.0));
    assert_eq!(
        native_order
            .iter()
            .map(|(_, name)| *name)
            .collect::<Vec<_>>(),
        ["AE.ADBE Noise2", "AE.Impact_Blur_FX"]
    );
    for clip in [suffix_clip, geometry_clip] {
        assert_eq!(
            clip.children()
                .find(|node| node.has_tag_name("AdjustmentLayer"))
                .unwrap()
                .text(),
            Some("true")
        );
    }
    let source_in: i64 = suffix_clip
        .descendants()
        .find(|node| node.has_tag_name("InPoint"))
        .unwrap()
        .text()
        .unwrap()
        .parse()
        .unwrap();
    let amount_ref = blur
        .descendants()
        .filter(|node| node.has_tag_name("Param"))
        .find_map(|reference| {
            wire.descendants().find(|node| {
                node.attribute("ObjectID") == reference.attribute("ObjectRef")
                    && node
                        .children()
                        .any(|child| child.has_tag_name("ParameterID") && child.text() == Some("3"))
            })
        })
        .unwrap();
    let text = amount_ref
        .children()
        .find(|node| node.has_tag_name("Keyframes"))
        .unwrap()
        .text()
        .unwrap();
    let fields: Vec<_> = text
        .trim_end_matches(';')
        .split(';')
        .map(|key| key.split(',').collect::<Vec<_>>())
        .collect();
    assert_eq!(fields.len(), 2);
    for (key, time, amount) in [(&fields[0], -100, 3.0), (&fields[1], 700, 6.0)] {
        assert_eq!(
            key[0].parse::<i64>().unwrap() - source_in,
            time * TICKS_PER_MILLISECOND
        );
        assert!((key[1].parse::<f64>().unwrap() - amount).abs() < 1e-6);
    }
    let rotation = geometry_parameter(&wire, 7);
    let rotation_keys = rotation
        .children()
        .find(|node| node.has_tag_name("Keyframes"))
        .unwrap()
        .text()
        .unwrap();
    assert_eq!(
        rotation_keys
            .trim_end_matches(';')
            .split(';')
            .next_back()
            .unwrap()
            .split(',')
            .nth(1)
            .unwrap()
            .parse::<f64>()
            .unwrap(),
        12.0
    );
    let (again, _) = import(&reread);
    assert_eq!(again["composition"]["layers"][0]["type"], "Adjustment");
    let effects = &again["composition"]["layers"][0]["effects"];
    assert_eq!(effects[0]["effect"]["type"], "grain");
    assert_eq!(effects[1]["effect"]["type"], "gaussianBlur");
    assert_eq!(effects[1]["enabled"], false);
    let group = geometry(&again["composition"]["layers"]);
    assert_eq!(keys(&again, group, "rotation")[1]["value"]["value"], 12.0);
    let lower = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(lower["sourceRange"], json!({"start":1833,"duration":417}));
    // Current G Opacity moves only its nest; the suffix stays outside.
    document["composition"]["layers"][1]["transform"]["opacity"] = json!(60.0);
    let (project, reports) = export(document);
    let root = project.single_sequence().unwrap();
    assert_eq!(root.nest_occurrences().next().unwrap().opacity, 60.0);
    assert_eq!(root.video_items().count(), 1);
    assert_eq!(
        root.nest_occurrences()
            .next()
            .unwrap()
            .sequence
            .video_items()
            .count(),
        1
    );
    assert!(reports
        .iter()
        .any(|report| report.reason.contains("ordinary moved nest")));
}

#[test]
fn adjustment_geometry2_suffix_rejection_and_invalid_stage_keep_independent_suffix() {
    let mut project = suffix_source();
    project.sequences[0].video_tracks[1]
        .clip_mut(0)
        .effects
        .swap(0, 1);
    let (document, reports) = import(&project);
    assert_eq!(document["composition"]["layers"][0]["type"], "Adjustment");
    assert_eq!(document["composition"]["layers"][1]["type"], "Video");
    assert_eq!(
        document["composition"]["layers"][1]["sourceRange"]["start"],
        1833
    );
    assert!(reports
        .iter()
        .any(|report| report.reason.contains("prefix/mixed")
            && report.reason.contains("Brightness & Contrast")
            && report
                .reason
                .contains("existing layers and tracks were left unchanged")));
    let mut source = suffix_source();
    source.sequences[0].video_tracks.push(PrVideoTrack::media([
        clip_of("source", 0..TICKS, 0), // Unrelated picture allows a useful partial export.
    ]));
    let (mut document, _) = import(&source);
    document["composition"]["layers"][2]["layers"][1]["rect"]["size"][0] = json!(1919.0);
    let typed = EditableFxCompositionDocument::from_json_value(document.clone()).unwrap();
    let selected = crate::convert::exported_video_layers(
        typed.composition().layers(),
        typed.composition().dynamics(),
        [1920, 1080],
    );
    assert_eq!(selected.len(), 2); // Independent upper picture and suffix, no G/lower.
    assert!(selected.iter().all(|layer| matches!(
        layer.data(),
        fx_schema::LayerData::Video(_) | fx_schema::LayerData::Adjustment(_)
    )));
    let (project, reports) = export(document);
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("whole-canvas Add guide")),
        "{reports:?}"
    );
    let root = project.single_sequence().unwrap();
    assert_eq!(root.nest_occurrences().count(), 0);
    assert_eq!(root.video_items().count(), 2);
    assert!(reports.iter().any(|report| report
        .reason
        .contains("root Adjustment above a Geometry2 stage")
        && report
            .reason
            .contains("the remaining Adjustment does not restore that input")));
    let suffix = root
        .video_items()
        .filter_map(|item| item.media())
        .find(|clip| project.media(clip).unwrap().is_adjustment())
        .unwrap();
    assert!(project.media(suffix).unwrap().is_adjustment());
    assert_eq!(suffix.effects.len(), 2); // Does not restore the omitted dry picture.

    // All rejected suffix payloads still leave a valid G and empty ordinary A.
    let mut project = suffix_source();
    let adjustment = project.sequences[0].video_tracks[1].clip_mut(0);
    adjustment.effects.truncate(2);
    adjustment.effects[1].params = PrEffectParams::GaussianBlur(crate::schema::PrGaussianBlur {
        blurriness: -1.0,
        repeat_edge_pixels: false,
    });
    adjustment.effects[1].animations.clear();
    let (document, reports) = import(&project);
    let typed = EditableFxCompositionDocument::from_json_value(document.clone()).unwrap();
    let fx_schema::LayerData::Adjustment(adjustment) = typed.composition().layers()[0].data()
    else {
        panic!("empty suffix must retain its ordinary Adjustment owner")
    };
    assert!(adjustment.effects.is_empty());
    geometry(&document["composition"]["layers"]);
    assert!(reports.iter().any(|report| report
        .reason
        .contains("Blurriness must be finite and nonnegative")));
    for enabled in [false, true] {
        let mut project = suffix_source();
        let adjustment = project.sequences[0].video_tracks[1].clip_mut(0);
        adjustment.effects.truncate(2);
        adjustment.effects[1].enabled = enabled;
        adjustment.effects[1].animations.clear();
        adjustment.effects[1].params =
            PrEffectParams::GaussianBlur(crate::schema::PrGaussianBlur {
                blurriness: 0.0,
                repeat_edge_pixels: false,
            });
        let (document, _) = import(&project);
        assert_eq!(document["composition"]["layers"][0]["type"], "Adjustment");
        geometry(&document["composition"]["layers"]);
    }
    let mut project = suffix_source();
    let inner = project.sequences.pop().unwrap();
    let nest = crate::tests::support::nest_of(inner.clone(), 0..inner.end_ticks(), 0);
    let mut outer = sequence_of(
        "Outer",
        vec![PrVideoTrack {
            items: Vec::new(),
            nests: vec![nest],
            transitions: Vec::new(),
        }],
    );
    outer.frame_rate = FrameRate::Fps24;
    project.sequences.push(outer);
    let (document, reports) = import(&project);
    assert!(!document
        .to_string()
        .contains("Premiere adjustment Geometry2 "));
    assert!(
        reports
            .iter()
            .any(|report| report.reason.contains("require a root sequence")),
        "{reports:?}"
    );
}
