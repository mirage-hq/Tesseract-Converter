use crate::{
    convert::tesseract_to_premiere,
    format::{FrameRate, MediaId, PrMedia, PrProjectFile, PrVideoOccurrence},
    media::{MediaFacts, VideoMedia},
    schema::{
        color_matte::COLOR_MATTE_INTRINSIC_TICKS,
        text::{
            PrAppearance, PrFill, PrGradient, PrGradientKind, PrGradientStop, PrGraphicObject,
            PrPathVertex, PrRgb, PrShape, PrShapePath, PrShapeStroke, PrTextTransform,
            OPAQUE_OPACITY_STOPS,
        },
        PrColorMatte, PrMediaKind, PrVideoItem, PrVideoTrack, VideoCodec, TICKS,
        TICKS_PER_MILLISECOND,
    },
    test_support::editable_document,
    tests::support::{project_document_with_media, video_media, video_sequence},
    Omission, OmissionKind, OmissionScope,
};
use fx_schema::EditableFxCompositionDocument;

/// A Color Matte placement's source in-point on the 30 fps test sequences.
const COLOR_MATTE_SOURCE_IN_TICKS: i64 = FrameRate::Fps30.generator_in_ticks();
use serde_json::{json, Value};
use std::collections::BTreeMap;

const RED: PrColorMatte = PrColorMatte { rgb: [255, 0, 0] };
const BLACK: PrColorMatte = PrColorMatte { rgb: [0, 0, 0] };

fn matte_media(matte: PrColorMatte) -> PrMedia {
    PrMedia {
        name: "Color Matte".into(),
        relative_path: None,
        relative_paths: Vec::new(),
        absolute_paths: Vec::new(),
        video: Some(crate::schema::PrVideoStream {
            orientation: crate::schema::VideoOrientation::Identity,
            intrinsic_ticks: COLOR_MATTE_INTRINSIC_TICKS,
            frame_rate: (FrameRate::Fps30).into(),
            width: 1920,
            height: 1080,
            kind: PrMediaKind::ColorMatte(matte),
        }),
        audio: None,
    }
}

fn matte_occurrence(id: &str, start_secs: i64, end_secs: i64) -> PrVideoOccurrence {
    PrVideoOccurrence {
        id: None,
        media: MediaId(id.into()),
        start_ticks: start_secs * TICKS,
        end_ticks: end_secs * TICKS,
        in_ticks: COLOR_MATTE_SOURCE_IN_TICKS,
        out_ticks: COLOR_MATTE_SOURCE_IN_TICKS + (end_secs - start_secs) * TICKS,
        playback_rate: 1.0,
        frame_blending: None,
        time_remap: None,
        linear_wipe: None,
        opacity_mask: None,
        track_matte: None,
        opacity: 100.0,
        blend_mode: crate::schema::PrBlendMode::Normal,
        transform: crate::schema::PrStaticTransform::default(),
        crop: crate::schema::PrStaticCrop::default(),
        animations: Vec::new(),
        enabled: true,
        effects: Vec::new(),
        effects_above_mask: 0,
        stroke: None,
        active_transforms: 0,
    }
}

fn layer_names_at(document: &Value, millis: i64) -> Vec<&str> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| {
            let start = (*crate::test_support::layer_range(layer))["start"]
                .as_i64()
                .unwrap();
            let end = start
                + (*crate::test_support::layer_range(layer))["duration"]
                    .as_i64()
                    .unwrap();
            (start..end).contains(&millis)
        })
        .map(|layer| layer["name"].as_str().unwrap())
        .collect()
}

/// Inspected facts for every video asset the document names: 1920×1080 at
/// 30 fps, as long as the layer's `sourceIntrinsicDuration` claims.
fn video_facts(document: &Value) -> BTreeMap<String, MediaFacts> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .map(|layer| {
            (
                layer["source"]["assetId"].as_str().unwrap().to_owned(),
                MediaFacts::Video(VideoMedia {
                    orientation: crate::schema::VideoOrientation::Identity,
                    codec: VideoCodec::H264,
                    bit_depth: 8,
                    colour: None,
                    width: 1920,
                    height: 1080,
                    timing: crate::media::VideoTiming::for_test(
                        FrameRate::Fps30,
                        layer["sourceIntrinsicDuration"].as_i64().unwrap() * TICKS_PER_MILLISECOND,
                    ),
                }),
            )
        })
        .collect()
}

fn export_with_omissions(document: Value) -> crate::error::Result<(PrProjectFile, Vec<Omission>)> {
    let facts = video_facts(&document);
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &facts,
        &BTreeMap::new(),
        &BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )?;
    Ok((project, omissions))
}

fn export(document: Value) -> crate::error::Result<PrProjectFile> {
    export_with_omissions(document).map(|(project, _)| project)
}

/// The kinds of every exported occurrence, bottom track first.
fn track_kinds(project: &PrProjectFile) -> Vec<Vec<PrMediaKind>> {
    project
        .single_sequence()
        .unwrap()
        .video_tracks()
        .map(|track| {
            track
                .iter()
                .map(|item| item.media().unwrap())
                .map(|clip| project.media(clip).unwrap().video.as_ref().unwrap().kind)
                .collect()
        })
        .collect()
}

#[test]
fn mattes_round_trip_as_solid_rectangles_distinct_from_gaps_and_the_canvas() {
    // V1: video 0–5 s, then a black matte 7–9 s (gap 5–7). V2: red matte 2–4 s.
    let mut sequence = video_sequence();
    sequence.video_tracks[0]
        .items
        .push(PrVideoItem::Media(matte_occurrence("black", 7, 9)));
    sequence
        .video_tracks
        .push(PrVideoTrack::media([matte_occurrence("red", 2, 4)]));
    sequence.timeline_end_ticks = 9 * TICKS;
    let mut media = video_media();
    media.extend([
        (MediaId("red".into()), matte_media(RED)),
        (MediaId("black".into()), matte_media(BLACK)),
    ]);
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    assert_eq!(ids.len(), 1, "mattes receive no asset ID");

    let mut document = project_document_with_media(&sequence, &media);
    let layers = document["composition"]["layers"].as_array().unwrap();
    let names: Vec<_> = layers.iter().map(|layer| &layer["name"]).collect();
    // Top track first, then the bottom track, then the implicit canvas.
    assert_eq!(
        names,
        [
            "Premiere color matte 3",
            "Premiere video 1",
            "Premiere color matte 2",
            "Premiere black canvas"
        ]
    );
    // The authored black matte is its own opaque layer, distinct from the
    // canvas; the 5–7 s gap shows only the canvas.
    let black = &layers[2];
    assert_eq!(black["type"], "Rect");
    assert_eq!(
        (*crate::test_support::layer_range(black)),
        json!({"start": 7000, "duration": 2000})
    );
    assert_eq!(black["rect"]["fillColor"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(layer_names_at(&document, 6000), ["Premiere black canvas"]);
    assert_eq!(
        layer_names_at(&document, 8000),
        ["Premiere color matte 2", "Premiere black canvas"]
    );

    // A second red matte later on the top track shares the red generator.
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let mut second_red = layers[0].clone();
    second_red["id"] = json!(9);
    second_red["activeRange"] = json!({"start": 8000, "duration": 1000});
    layers.insert(0, second_red);
    let project = export(document).unwrap();
    let exported = project.single_sequence().unwrap();
    let tracks: Vec<Vec<_>> = exported
        .video_tracks()
        .map(|track| {
            track
                .iter()
                .map(|item| item.media().unwrap())
                .map(|clip| {
                    let source = project.media(clip).unwrap().video.as_ref().unwrap();
                    (
                        clip.start_ticks / TICKS,
                        clip.end_ticks / TICKS,
                        clip.in_ticks,
                        clip.out_ticks - clip.in_ticks,
                        source.kind,
                        source.intrinsic_ticks,
                    )
                })
                .collect()
        })
        .collect();
    let matte = |start, end, matte| {
        (
            start,
            end,
            COLOR_MATTE_SOURCE_IN_TICKS,
            (end - start) * TICKS,
            PrMediaKind::ColorMatte(matte),
            COLOR_MATTE_INTRINSIC_TICKS,
        )
    };
    assert_eq!(
        tracks,
        [
            vec![
                (
                    0,
                    5,
                    0,
                    5 * TICKS,
                    PrMediaKind::Video {
                        codec: Some(VideoCodec::H264),
                        hdr_profile: None,
                    },
                    10 * TICKS
                ),
                matte(7, 9, BLACK),
            ],
            vec![matte(2, 4, RED), matte(8, 9, RED)],
        ]
    );
    let matte_ids: Vec<_> = exported
        .media_in_order()
        .into_iter()
        .filter(|id| {
            matches!(
                project.media[*id].video.as_ref().unwrap().kind,
                PrMediaKind::ColorMatte(_)
            )
        })
        .map(|id| id.as_str().to_owned())
        .collect();
    assert_eq!(matte_ids, ["color-matte:000000", "color-matte:ff0000"]);
}

#[test]
fn a_blended_matte_stays_a_color_matte_with_its_blend_both_ways() {
    use crate::schema::PrBlendMode;
    // V2: a red matte 2–4 s that blends as Darker Color over the video.
    let mut sequence = video_sequence();
    let mut matte = matte_occurrence("red", 2, 4);
    matte.blend_mode = PrBlendMode::DarkerColor;
    sequence.video_tracks.push(PrVideoTrack::media([matte]));
    let mut media = video_media();
    media.insert(MediaId("red".into()), matte_media(RED));
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let document = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    let rect = &document["composition"]["layers"][0];
    assert_eq!(
        (&rect["type"], &rect["blendMode"]),
        (&json!("Rect"), &json!("darkerColor"))
    );
    let reports = |omissions: &[Omission]| -> Vec<OmissionKind> {
        omissions
            .iter()
            .filter(|omission| omission.reason.contains("Blend Mode (4, 7)"))
            .map(|omission| omission.kind)
            .collect()
    };
    assert_eq!(reports(&omissions), [OmissionKind::Approximated]);
    let (project, omissions) = export_with_omissions(document).unwrap();
    assert_eq!(
        track_kinds(&project),
        [
            vec![PrMediaKind::Video {
                codec: Some(VideoCodec::H264),
                hdr_profile: None,
            }],
            vec![PrMediaKind::ColorMatte(RED)],
        ]
    );
    let matte = project
        .single_sequence()
        .unwrap()
        .video_tracks()
        .nth(1)
        .unwrap()[0]
        .media()
        .unwrap();
    assert_eq!(matte.blend_mode, PrBlendMode::DarkerColor);
    assert_eq!(reports(&omissions), [OmissionKind::Approximated]);
}

#[test]
fn fill_channels_round_to_the_nearest_eight_bit_step_within_range() {
    assert_eq!(
        PrColorMatte::from_fill_color([0.5, 0.2, 1.0, 1.0]).unwrap(),
        PrColorMatte {
            rgb: [128, 51, 255]
        }
    );
    for color in [[1.5, 0.0, 0.0, 1.0], [0.0, -0.1, 0.0, 1.0]] {
        assert!(PrColorMatte::from_fill_color(color).is_err(), "{color:?}");
    }
}

/// Hand-written wire input, independent of the Premiere importer: the shared
/// video fixture with a red full-frame solid (layer 3) on top, pivoting on the
/// canvas centre as `InsertFullscreenLayer` writes it.
fn solid_fill_document() -> Value {
    let mut document = editable_document();
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({
                "type": "Rect",
                "id": 3,
                "name": "Red solid",
                "activeRange": {"start": 0, "duration": 1000},
                "transform": {
                    "anchorPoint": [960, 540], "position": [960, 540], "scale": [100, 100],
                    "rotation": 0, "opacity": 100
                },
                "rect": {"size": [1920, 1080], "fillColor": [1, 0, 0, 1]}
            }),
        );
    document
}

/// Independent editable input matching `insert_rect` and `canvas_transform`
/// in `project_mutation/src/actions/fx/insert_fullscreen_layer.rs`: one 2 s
/// solid with anchor = position = canvas centre. Keep the conversion test
/// portable rather than invoking the private mutation host.
fn fullscreen_solid_document(color: [f64; 4]) -> EditableFxCompositionDocument {
    EditableFxCompositionDocument::from_json_value(json!({
        "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion": 1,
        "dimensions": {"width": 1920, "height": 1080},
        "duration": 2.0,
        "composition": {
            "id": "main",
            "name": "Solid",
            "layers": [{
                "type": "Rect",
                "id": 1,
                "name": "Solid",
                "activeRange": {"start": 0, "duration": 2000},
                "transform": {
                    "anchorPoint": [960, 540], "position": [960, 540],
                    "scale": [100, 100], "rotation": 0, "opacity": 100
                },
                "rect": {"size": [1920, 1080], "fillColor": color}
            }]
        }
    }))
    .unwrap()
}

#[test]
fn insert_fullscreen_layer_solids_export_as_color_mattes() {
    // Only the importer's origin-pivot black rectangle is the implicit canvas;
    // an authored black solid stays a matte like any other colour.
    for (color, matte) in [([1.0, 0.0, 0.0, 1.0], RED), ([0.0, 0.0, 0.0, 1.0], BLACK)] {
        let project = tesseract_to_premiere(
            &fullscreen_solid_document(color),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            crate::format::FrameRate::Fps30,
            &mut Vec::new(),
        )
        .unwrap();
        let clips: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .collect();
        assert_eq!(clips.len(), 1, "{color:?}");
        assert_eq!(clips[0].timeline_ticks(), 0..2 * TICKS);
        assert_eq!(
            clips[0].source_ticks(),
            COLOR_MATTE_SOURCE_IN_TICKS..COLOR_MATTE_SOURCE_IN_TICKS + 2 * TICKS
        );
        assert_eq!(
            project
                .media(clips[0])
                .unwrap()
                .video
                .as_ref()
                .unwrap()
                .kind,
            PrMediaKind::ColorMatte(matte)
        );
    }
}

/// What exporting `document` makes of the red solid (layer 3) beside its
/// video, which exports either way: the one Shape of its graphic, which is
/// disabled when the solid is hidden, or the reason its occurrence is
/// omitted; and the layer's Feature reports with their kinds.
fn solid_export(document: Value) -> (Result<PrShape, String>, Vec<(OmissionKind, String)>) {
    let hidden = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer["id"] == 3 && layer["isHidden"] == true);
    let (project, omissions) = export_with_omissions(document).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1, "{omissions:?}");
    let graphics: Vec<_> = sequence
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .collect();
    let reasons = |scope| {
        omissions
            .iter()
            .filter(|item| item.scope == scope && item.record == r#"layer 3 ("Red solid")"#)
            .map(|item| (item.kind, item.reason.clone()))
            .collect::<Vec<_>>()
    };
    let outcome = match (
        graphics.as_slice(),
        reasons(OmissionScope::Occurrence).as_slice(),
    ) {
        ([graphic], []) => match graphic.objects.as_slice() {
            [PrGraphicObject::Shape(shape)] if graphic.enabled != hidden => Ok(shape.clone()),
            objects => panic!("one Shape, enabled unless hidden, not {graphic:?}: {objects:?}"),
        },
        ([], [(_, omission)]) => Err(omission.clone()),
        _ => panic!("{graphics:?}: {omissions:?}"),
    };
    (outcome, reasons(OmissionScope::Feature))
}

#[test]
fn a_video_asset_spelled_like_a_matte_id_does_not_merge_into_the_matte() {
    // With the matte's 12 h duration, only the media kind tells them apart.
    // The inspected facts list every packaged asset, so the solid rejects
    // above or below the video.
    let mut document = solid_fill_document();
    let video = &mut document["composition"]["layers"][1];
    video["source"]["assetId"] = json!("color-matte:ff0000");
    video["sourceIntrinsicDuration"] = json!(12 * 60 * 60 * 1000);
    let mut below = document.clone();
    below["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    for document in [document, below] {
        let error = export(document).unwrap_err().to_string();
        assert!(
            error.contains(
                "layer 3 (\"Red solid\"): unsupported conversion: asset color-matte:ff0000 names both a packaged asset and a Color Matte solid fill"
            ),
            "{error}"
        );
    }
}

#[test]
fn rectangles_other_than_color_mattes_export_as_one_shape_or_with_the_shape_reason() {
    // Edits of the layers of `solid_fill_document`, the red solid (0) over its
    // video (1); unedited, the solid is a Color Matte. `("/+", layer)`, last
    // in its row, adds `layer` on top.
    let edited = |edits: &[(&str, Value)]| {
        let mut document = solid_fill_document();
        let layers = &mut document["composition"]["layers"];
        for (path, value) in edits {
            match path.rsplit_once('/').unwrap() {
                ("", "+") => layers.as_array_mut().unwrap().insert(0, value.clone()),
                (parent, key) => layers.pointer_mut(parent).unwrap()[key] = value.clone(),
            }
        }
        document
    };
    let bar = [
        ("/0/rect/size", json!([400, 100])),
        ("/0/transform/anchorPoint", json!([200, 50])),
        ("/0/transform/position", json!([960, 900])),
    ];
    let stroke = [
        ("/0/rect/strokeEnabled", json!(true)),
        ("/0/rect/strokeColor", json!([1, 0, 1, 1])),
        ("/0/rect/strokeWidth", json!(10)),
    ];
    let shape = |corners: [[f32; 2]; 4],
                 fill: Option<PrFill>,
                 stroke: Option<PrShapeStroke>,
                 transform: PrTextTransform| PrShape {
        name: "Red solid".to_owned(),
        path: PrShapePath {
            vertices: corners
                .map(|point| PrPathVertex {
                    smooth: false,
                    point,
                    in_tangent: point,
                    out_tangent: point,
                })
                .to_vec(),
            closed: true,
        },
        appearance: PrAppearance {
            fill,
            stroke,
            shadow: None,
        },
        transform,
        horizontal_scale: None,
    };
    let placed = |position, anchor, scale, rotation, opacity| PrTextTransform {
        position,
        anchor,
        scale,
        rotation,
        opacity,
    };
    let bar_corners = [[0.0, 0.0], [400.0, 0.0], [400.0, 100.0], [0.0, 100.0]];
    let canvas_corners = [[0.0, 0.0], [1920.0, 0.0], [1920.0, 1080.0], [0.0, 1080.0]];
    let red = || Some(PrFill::Solid(PrRgb([255, 0, 0])));
    let bar_placed = || placed([960.0, 900.0], [200.0, 50.0], 100.0, 0.0, 100.0);
    let canvas_placed = || placed([960.0, 540.0], [960.0, 540.0], 100.0, 0.0, 100.0);
    let vertex = |point, in_tangent, out_tangent| PrPathVertex {
        smooth: true,
        point,
        in_tangent,
        out_tangent,
    };
    // FX's outline of the bar with corner radius `r`: from the top edge
    // clockwise, four lines, each followed by a quarter circle whose handles
    // are r·κ long, κ = 0.5522848 (`scene::Path::rounded_rect`). Every vertex
    // has a tangent off its point, so it is smooth.
    let rounded = |r: f32| {
        let k = r * 0.552_284_8;
        vec![
            vertex([r, 0.0], [r - k, 0.0], [r, 0.0]),
            vertex([400.0 - r, 0.0], [400.0 - r, 0.0], [400.0 - r + k, 0.0]),
            vertex([400.0, r], [400.0, r - k], [400.0, r]),
            vertex(
                [400.0, 100.0 - r],
                [400.0, 100.0 - r],
                [400.0, 100.0 - r + k],
            ),
            vertex(
                [400.0 - r, 100.0],
                [400.0 - r + k, 100.0],
                [400.0 - r, 100.0],
            ),
            vertex([r, 100.0], [r, 100.0], [r - k, 100.0]),
            vertex([0.0, 100.0 - r], [0.0, 100.0 - r + k], [0.0, 100.0 - r]),
            vertex([0.0, r], [0.0, r], [0.0, r - k]),
        ]
    };
    // The same outline of a pill, `r` half the height or, by rounding, a step
    // above it: its vertical lines have no positive length, so the two
    // quarter circles of each side meet at one vertex.
    let pill = |r: f32| {
        let k = r * 0.552_284_8;
        vec![
            vertex([r, 0.0], [r - k, 0.0], [r, 0.0]),
            vertex([400.0 - r, 0.0], [400.0 - r, 0.0], [400.0 - r + k, 0.0]),
            vertex([400.0, r], [400.0, r - k], [400.0, 100.0 - r + k]),
            vertex(
                [400.0 - r, 100.0],
                [400.0 - r + k, 100.0],
                [400.0 - r, 100.0],
            ),
            vertex([r, 100.0], [r, 100.0], [r - k, 100.0]),
            vertex([0.0, 100.0 - r], [0.0, 100.0 - r + k], [0.0, r - k]),
        ]
    };
    let rounded_bar = |vertices, stroke| PrShape {
        path: PrShapePath {
            vertices,
            closed: true,
        },
        ..shape(bar_corners, red(), stroke, bar_placed())
    };
    let magenta = PrShapeStroke {
        color: PrRgb([255, 0, 255]),
        width: 10.0,
    };
    let shape_layer = "shape layer was not exported: unsupported conversion";
    // Red to green along the bar's top edge up to `end` px, over the bar's red
    // fill colour.
    let gradient = |kind: &str, end: f64| {
        json!({
            "type": "gradient", "gradientType": kind, "start": [0, 0], "end": [end, 0],
            "stops": [{"offset": 0, "color": [1, 0, 0, 1]}, {"offset": 1, "color": [0, 1, 0, 1]}]
        })
    };
    let red_to_green = PrFill::Gradient(PrGradient {
        kind: PrGradientKind::Linear,
        start_x: 0.0,
        end_x: 400.0,
        stops: vec![
            PrGradientStop {
                position: 0.0,
                color: PrRgb([255, 0, 0]),
            },
            PrGradientStop {
                position: 1.0,
                color: PrRgb([0, 255, 0]),
            },
        ],
        opacity_stops: OPAQUE_OPACITY_STOPS.to_vec(),
    });
    // A text set on the solid's outline, at the root or in a group: FX paints
    // the solid only through it. Hidden, by its own flag, its group or a group
    // around that, the text leaves the solid content that FX paints.
    let text_on_the_solid = json!({
        "type": "Text", "id": 41, "name": "On a path",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": {"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "sourceText": {"text": "Path", "fontFamily": "Inter-Bold", "fontStyle": "", "fontSize": 80, "fillColor": [1, 1, 1, 1]},
        "pathOptions": {"id": 51, "pathLayer": 3}
    });
    let mut hidden_text = text_on_the_solid.clone();
    hidden_text["isHidden"] = json!(true);
    let mut grouped_text = text_on_the_solid.clone();
    grouped_text["id"] = json!(42);
    grouped_text["parent"] = json!(40);
    grouped_text["pathOptions"]["id"] = json!(52);
    let text_in_group = |hidden: bool| {
        json!({
            "type": "Group", "id": 40, "name": "Group", "isHidden": hidden,
            "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 1000}), json!({"start": 0, "duration": 1000})),
            "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
            "layers": [grouped_text.clone()]
        })
    };
    // The shown group 40 inside a hidden group 43, which hides it too.
    let mut shown_group = text_in_group(false);
    shown_group["parent"] = json!(43);
    let mut text_in_hidden_group = text_in_group(true);
    text_in_hidden_group["id"] = json!(43);
    text_in_hidden_group["layers"] = json!([shown_group]);
    let consumed =
        "graphic was not exported: its shape layer is another layer's track matte, mask or text path";
    let mask = json!([{"id": 50, "mode": "add", "path": {"commands": [
        {"type": "moveTo", "x": 0, "y": 0},
        {"type": "lineTo", "x": 10, "y": 0},
        {"type": "lineTo", "x": 10, "y": 10},
        {"type": "close"}
    ]}}]);
    let turned = [
        ("/0/transform/rotation", json!(30)),
        ("/0/transform/scale", json!([80, 80])),
    ];
    for (edits, expected) in [
        (
            bar.to_vec(),
            Ok(shape(bar_corners, red(), None, bar_placed())),
        ),
        // A hidden bar is a disabled graphic (`solid_export`).
        (
            [&bar[..], &[("/0/isHidden", json!(true))]].concat(),
            Ok(shape(bar_corners, red(), None, bar_placed())),
        ),
        // Outlined only, drawn from a centred origin (`rect.position`).
        (
            vec![
                ("/0/rect/size", json!([400, 100])),
                ("/0/rect/position", json!([-200, -50])),
                ("/0/rect/fillEnabled", json!(false)),
                ("/0/rect/strokeEnabled", json!(true)),
                ("/0/rect/strokeColor", json!([0, 1, 0, 1])),
                ("/0/rect/strokeWidth", json!(6)),
                ("/0/transform/anchorPoint", json!([0, 0])),
                ("/0/transform/position", json!([960, 200])),
            ],
            Ok(shape(
                [
                    [-200.0, -50.0],
                    [200.0, -50.0],
                    [200.0, 50.0],
                    [-200.0, 50.0],
                ],
                None,
                Some(PrShapeStroke {
                    color: PrRgb([0, 255, 0]),
                    width: 6.0,
                }),
                placed([960.0, 200.0], [0.0, 0.0], 100.0, 0.0, 100.0),
            )),
        ),
        (
            [&bar[..], &stroke[..], &turned[..]].concat(),
            Ok(shape(
                bar_corners,
                red(),
                Some(magenta),
                placed([960.0, 900.0], [200.0, 50.0], 80.0, 30.0, 100.0),
            )),
        ),
        // A Color Matte's shape at another opacity, and one with a stored
        // stroke colour, which FX does not draw while the stroke is disabled.
        (
            vec![("/0/transform/opacity", json!(50))],
            Ok(shape(
                canvas_corners,
                red(),
                None,
                placed([960.0, 540.0], [960.0, 540.0], 100.0, 0.0, 50.0),
            )),
        ),
        (
            vec![("/0/rect/strokeColor", json!([0, 0, 1, 1]))],
            Ok(shape(canvas_corners, red(), None, canvas_placed())),
        ),
        // FX draws no stroke 0 px wide, and an undashed stroke whatever its
        // dash offset.
        (
            [&stroke[..], &[("/0/rect/strokeWidth", json!(0))]].concat(),
            Ok(shape(canvas_corners, red(), None, canvas_placed())),
        ),
        (
            [&stroke[..], &[("/0/rect/strokeDashOffset", json!(5))]].concat(),
            Ok(shape(canvas_corners, red(), Some(magenta), canvas_placed())),
        ),
        // Rounded corners, and a roundness above half the height, which FX
        // scales down to it: 80 to 50 exactly, and 85, stroked, by the factor
        // 100/170 to a rounding step above 50. A pill keeps its stroke.
        (
            [&bar[..], &[("/0/rect/roundness", json!(20))]].concat(),
            Ok(rounded_bar(rounded(20.0), None)),
        ),
        (
            [&bar[..], &[("/0/rect/roundness", json!(80))]].concat(),
            Ok(rounded_bar(pill(50.0), None)),
        ),
        (
            [&bar[..], &stroke[..], &[("/0/rect/roundness", json!(85))]].concat(),
            Ok(rounded_bar(pill(85.0 * (100.0 / 170.0)), Some(magenta))),
        ),
        (
            [
                &bar[..],
                &[("/0/rect/fillPaint", gradient("linear", 400.0))],
            ]
            .concat(),
            Ok(shape(
                bar_corners,
                Some(red_to_green.clone()),
                None,
                bar_placed(),
            )),
        ),
        (
            vec![("/0/rect/fillBlendMode", json!("multiply"))],
            Err(format!(
                "{shape_layer}: shape fills blend normally at full opacity with the nonzero rule"
            )),
        ),
        (
            vec![("/0/rect/fillColor", json!([1, 0, 0, 0.5]))],
            Err(format!(
                "{shape_layer}: shape fill must be an opaque color with channels in 0..=1"
            )),
        ),
        (
            [&stroke[..], &[("/0/rect/strokeDashes", json!([10, 5]))]].concat(),
            Err(format!(
                "{shape_layer}: dashed shape strokes are unsupported"
            )),
        ),
        // A bevel, or a miter limit of 1, draws the 90° corners of the miter
        // class differently.
        (
            [&stroke[..], &[("/0/rect/strokeJoin", json!("bevel"))]].concat(),
            Err(format!("{shape_layer}: stroke joins are unverified")),
        ),
        (
            [&stroke[..], &[("/0/rect/strokeMiterLimit", json!(1))]].concat(),
            Err(format!("{shape_layer}: stroke joins are unverified")),
        ),
        (
            vec![("/0/transform/skew", json!(10))],
            Err(format!(
                "{shape_layer}: a graphic shape has no skew or 3D rotation"
            )),
        ),
        (
            vec![("/0/masks", mask)],
            Err("graphic was not exported: its shape layer must have no masks".to_owned()),
        ),
        (
            vec![("/0/trackMatte", json!({"mode": "alpha", "layer": 2}))],
            Err("graphic was not exported: its shape layer must have no track matte".to_owned()),
        ),
        // A text on its path, in a group or at the root, consumes the bar or
        // the canvas-sized solid, which is then no Color Matte.
        (
            [&bar[..], &[("/+", text_in_group(false))]].concat(),
            Err(consumed.to_owned()),
        ),
        (
            vec![("/+", text_on_the_solid.clone())],
            Err(consumed.to_owned()),
        ),
        // Hidden texts on its path leave the bar a Shape.
        (
            [
                &bar[..],
                &[("/+", hidden_text), ("/+", text_in_group(true))],
            ]
            .concat(),
            Ok(shape(bar_corners, red(), None, bar_placed())),
        ),
        (
            [&bar[..], &[("/+", text_in_hidden_group)]].concat(),
            Ok(shape(bar_corners, red(), None, bar_placed())),
        ),
    ] {
        assert_eq!(
            solid_export(edited(&edits)),
            (expected, Vec::new()),
            "{edits:?}"
        );
    }
    // A gradient that the Shape rules do not export is drawn with the fill
    // colour, and the approximation is reported when the Shape exports. A
    // turned gradient exports with the gradient shape's own warning.
    let approximated = |kind: &str, reason: &str| {
        vec![(
            OmissionKind::Approximated,
            format!(
                "rectangle gradient fill {kind} approximated by its solid fill colour: {reason}; colours differ from the gradient's stops by up to 255/255"
            ),
        )]
    };
    for (edits, expected, reported) in [
        (
            [&bar[..], &[("/0/rect/fillPaint", gradient("conic", 400.0))]].concat(),
            Ok(shape(bar_corners, red(), None, bar_placed())),
            approximated(
                "conic",
                "unsupported conversion: reflected and conic gradient shape fills are unsupported (JRB-2015)",
            ),
        ),
        (
            [&bar[..], &turned[..], &[("/0/rect/fillPaint", gradient("linear", 400.0))]].concat(),
            Ok(shape(
                bar_corners,
                Some(red_to_green.clone()),
                None,
                placed([960.0, 900.0], [200.0, 50.0], 80.0, 30.0, 100.0),
            )),
            vec![(OmissionKind::Approximated, "gradient geometry under a shape transform (Scale 80 / Rotation 30) is unmeasured against Premiere; converted in layer space".to_owned())],
        ),
        (
            [&bar[..], &[("/0/rect/fillPaint", gradient("radial", 0.0))]].concat(),
            Ok(shape(bar_corners, red(), None, bar_placed())),
            approximated(
                "radial",
                "invalid Premiere project: a gradient needs a finite axis of at least 0.0001 px and two or more stops in order within 0..=1",
            ),
        ),
        (
            [
                &bar[..],
                &[
                    ("/0/rect/fillPaint", gradient("conic", 400.0)),
                    ("/0/rect/fillBlendMode", json!("multiply")),
                ],
            ]
            .concat(),
            Err(format!(
                "{shape_layer}: shape fills blend normally at full opacity with the nonzero rule"
            )),
            Vec::new(),
        ),
    ] {
        assert_eq!(
            solid_export(edited(&edits)),
            (expected, reported),
            "{edits:?}"
        );
    }
}

#[test]
fn animated_motion_or_opacity_omits_the_solid_instead_of_exporting_a_static_matte() {
    // A keyed solid is no static Color Matte, and graphic Shapes export static.
    for property in [
        "opacity",
        "rotation",
        "scaleX",
        "scaleY",
        "positionX",
        "positionY",
    ] {
        let mut document = solid_fill_document();
        document["composition"]["dynamics"] = json!({"entries": [{
            "target": {"kind": "layer", "layerId": 3, "propertyType": property},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                {"id": "start", "layerTime": 0, "value": {"type": "float", "value": 0.0}, "easing": {"type": "linear"}},
                {"id": "end", "layerTime": 500, "value": {"type": "float", "value": 90.0}, "easing": {"type": "linear"}}
            ]}
        }]});
        assert_eq!(
            solid_export(document).0,
            Err(
                "shape layer was not exported: unsupported conversion: keyed graphic shapes are unsupported"
                    .to_owned()
            ),
            "{property}"
        );
    }
}

#[test]
fn crop_mask_rectangles_are_guides_not_authored_mattes_in_either_layer_order() {
    // Independent full-frame guide for a feather-only Crop: its static shape
    // alone would qualify as a matte, but it is consumed by the video's mask.
    let mut document = solid_fill_document();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["name"] = json!("Crop guide");
    layers[1]["transform"] = layers[0]["transform"].clone();
    layers[1]["masks"] = json!([{
        "id": 4, "layer": 3, "mode": "add", "feather": [12, 12]
    }]);
    let mut reversed = document.clone();
    reversed["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    for document in [document, reversed] {
        let (project, omissions) = export_with_omissions(document.clone()).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(
            track_kinds(&project),
            [vec![PrMediaKind::Video {
                codec: Some(VideoCodec::H264),
                hdr_profile: None,
            }]]
        );
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        assert_eq!(clip.crop.edge_feather, 12.0);

        // An unrepresentable mask omits its video, whose guide it consumes:
        // never a matte or a matte-specific diagnostic. Nothing else exports.
        let mut invalid = document;
        let video = invalid["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        video["masks"][0]["inverted"] = json!(true);
        let error = export_with_omissions(invalid).unwrap_err().to_string();
        assert!(
            error.ends_with(
                "\noccurrence layer 1 (\"Source\"): masks cannot be exported: the mask is inverted; occurrence omitted"
            ),
            "{error}"
        );
        assert!(
            !error.contains("rectangle") && !error.contains("matte"),
            "{error}"
        );
    }
}

#[test]
fn a_crop_guide_serialized_after_the_canvas_keeps_it_the_canvas() {
    // Guides do not paint, so an editor may write one after the canvas. The
    // video starts at 0.5 s: only the canvas covers the first half second.
    let mut document = solid_fill_document();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let mut guide = layers.remove(0);
    guide["name"] = json!("Crop guide");
    guide["activeRange"] = json!({"start": 500, "duration": 500});
    let video = &mut layers[0];
    video["playback"] = crate::test_support::linear_playback(
        crate::test_support::layer_range(&guide).clone(),
        json!({"start": 0, "duration": 500}),
    );
    video["sourceRange"]["duration"] = json!(500);
    video["transform"] = guide["transform"].clone();
    video["masks"] = json!([{"id": 4, "layer": 3, "mode": "add", "feather": [12, 12]}]);
    layers.push(guide);
    let (project, omissions) = export_with_omissions(document).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        track_kinds(&project),
        [vec![PrMediaKind::Video {
            codec: Some(VideoCodec::H264),
            hdr_profile: None,
        }]]
    );
    let clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.timeline_ticks(), TICKS / 2..TICKS);
    assert_eq!(clip.crop.edge_feather, 12.0);
}

#[test]
fn a_non_black_bottom_rectangle_is_an_authored_matte_not_the_canvas() {
    // The hand-written canvas turned red under a video starting at 0.5 s: it is
    // opaque content, so the first half second needs no black canvas.
    let mut document = editable_document();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["playback"] = crate::test_support::linear_playback(
        json!({"start": 500, "duration": 500}),
        json!({"start": 0, "duration": 500}),
    );
    layers[0]["sourceRange"]["duration"] = json!(500);
    layers[1]["rect"]["fillColor"] = json!([1, 0, 0, 1]);
    let (project, omissions) = export_with_omissions(document).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        track_kinds(&project),
        [
            vec![PrMediaKind::ColorMatte(RED)],
            vec![PrMediaKind::Video {
                codec: Some(VideoCodec::H264),
                hdr_profile: None,
            }]
        ]
    );
    let tracks: Vec<_> = project.single_sequence().unwrap().video_tracks().collect();
    assert_eq!(tracks[0][0].timeline_ticks(), 0..TICKS);
    assert_eq!(tracks[1][0].timeline_ticks(), TICKS / 2..TICKS);
}
