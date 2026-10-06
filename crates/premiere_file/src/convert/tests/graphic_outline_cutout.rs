//! Outline glyphs retain editable text and exclude their filled interiors.
//! Raster/native appearance is a separate gate; these assert the editable mask
//! operands, their common placement and their authored key clocks.
use super::*;
use crate::schema::text::{PrTextBackground, PrTextStroke};

fn outline_graphic() -> PrGraphic {
    let mut graphic = text_graphic();
    let text = graphic.text_mut();
    text.document.text = "01".into();
    text.document.font = "BebasNeue-Regular".into();
    text.document.size = 100.0;
    text.document.fill = None;
    text.document.stroke = Some(PrTextStroke {
        color: PrRgb([255; 3]),
        width: 3.0,
    });
    text.document.leading = 0.0;
    text.document.frame = PrTextFrame::Point {
        vertical: PrVerticalAlign::Center,
    };
    graphic
}

fn cutout_children(group: &Value) -> (&Value, &Value) {
    assert_eq!(group["type"], "Group");
    let children = group["layers"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    let paint = &children[0];
    let interior = &children[1];
    assert_eq!(paint["type"], "Text");
    assert_eq!(interior["type"], "Text");
    assert_eq!(paint["trackMatte"]["mode"], "alphaInverted");
    assert_eq!(paint["trackMatte"]["layer"], interior["id"]);
    assert_eq!(paint["parent"], group["id"]);
    assert_eq!(interior["parent"], group["id"]);
    for field in [
        "text",
        "fontFamily",
        "fontStyle",
        "fontSize",
        "justification",
        "tracking",
        "leading",
        "boxText",
        "boxSize",
        "boxPosition",
        "allCaps",
        "verticalAlign",
    ] {
        assert_eq!(
            paint["sourceText"][field], interior["sourceText"][field],
            "{field}"
        );
    }
    assert_eq!(paint["sourceText"]["applyFill"], false);
    assert_eq!(paint["sourceText"]["applyStroke"], true);
    assert_eq!(interior["sourceText"]["applyFill"], true);
    assert_eq!(interior["sourceText"]["applyStroke"], false);
    assert_eq!(
        interior["sourceText"]["fillColor"],
        json!([1.0, 1.0, 1.0, 1.0])
    );
    assert_eq!(paint["transform"], interior["transform"]);
    assert_eq!(paint["transform"]["opacity"], 100.0);
    assert_eq!(paint["transform"]["scale"], json!([100.0, 100.0]));
    assert_eq!(paint["activeRange"], json!({"start": 0, "duration": 2000}));
    assert_eq!(paint["activeRange"], interior["activeRange"]);
    (paint, interior)
}

#[test]
fn outline_only_cutout_keeps_placement_paint_and_editable_glyphs() {
    let graphic = outline_graphic();
    let expected = object_transform(&graphic.text().transform, "text").unwrap();
    let (mut document, omissions) = imported(graphic);
    let group = &document["composition"]["layers"][0];
    let (paint, _) = cutout_children(group);
    assert_eq!(group["transform"], serde_json::to_value(expected).unwrap());
    assert_eq!(
        group["playback"]["inputRange"],
        json!({"start": 1000, "duration": 2000})
    );
    assert_eq!(paint["sourceText"]["strokeWidth"], 6.0);
    assert_eq!(
        paint["sourceText"]["strokeColor"],
        json!([1.0, 1.0, 1.0, 1.0])
    );
    assert!(omissions
        .iter()
        .any(|omission| omission.reason.contains("filled-glyph cutout")));
    let video = document["composition"]["layers"][1].clone();

    // Glyph/layout edits address both editable operands; paint edits need only
    // the visible child, while the common Group owns placement and fades.
    for child in document["composition"]["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
    {
        child["sourceText"]["text"] = json!("04");
        child["sourceText"]["fontSize"] = json!(120.0);
    }
    document["composition"]["layers"][0]["layers"][0]["sourceText"]["strokeWidth"] = json!(8.0);
    let edited = EditableFxCompositionDocument::from_json_value(document)
        .unwrap()
        .to_json_value()
        .unwrap();
    let (paint, interior) = cutout_children(&edited["composition"]["layers"][0]);
    assert_eq!(paint["sourceText"]["text"], "04");
    assert_eq!(interior["sourceText"]["fontSize"], 120.0);
    assert_eq!(paint["sourceText"]["strokeWidth"], 8.0);
    assert_eq!(edited["composition"]["layers"][1], video);
    assert!(!edited.to_string().contains("JsScript"));
}

fn key_values(document: &Value, layer: &Value, property: &str) -> Vec<(i64, Value)> {
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == *layer && entry["target"]["propertyType"] == property
        })
        .unwrap_or_else(|| panic!("no {property} on {layer}"))["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].clone(),
            )
        })
        .collect()
}

#[test]
fn outline_only_source_text_keys_keep_background_outside_glyph_matte() {
    let mut graphic = outline_graphic();
    let start = graphic.in_ticks;
    let text = graphic.text_mut();
    text.document.size = PrTextBackground::CALIBRATED_SIZE;
    text.document.background = Some(PrTextBackground {
        color: PrRgb([0, 255, 0]),
        opacity: 100.0,
        size: 10.0,
        radius: 5.0,
    });
    let mut second = text.document.clone();
    second.text = "04".into();
    text.source_text_keys = vec![
        PrSourceTextKey {
            source_ticks: start,
            document: text.document.clone(),
        },
        PrSourceTextKey {
            source_ticks: start + TICKS,
            document: second,
        },
    ];

    let (mut document, _) = imported(graphic);
    let background = &document["composition"]["layers"][0];
    assert_eq!(background["type"], "Group");
    assert!(background.get("trackMatte").is_none());
    assert_eq!(
        background["fills"],
        json!([{
            "paint": {"type": "solid", "color": [0.0, 1.0, 0.0, 1.0]},
            "fillRule": "nonZeroWinding",
            "blendMode": "normal",
            "opacity": 1.0
        }])
    );
    assert_eq!(background["paddingTop"], 10.0);
    assert_eq!(background["cornerRadiusTopLeft"], 5.0);
    let cutout = &background["layers"][0];
    assert_eq!(cutout["type"], "Group");
    assert!(cutout.get("fills").is_none());
    assert!(cutout.get("trackMatte").is_none());
    let (paint, interior) = cutout_children(cutout);
    assert_eq!(
        key_values(&document, &paint["id"], "textContent"),
        key_values(&document, &interior["id"], "textContent")
    );

    let preserved_background = background["fills"].clone();
    for child in document["composition"]["layers"][0]["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
    {
        child["sourceText"]["text"] = json!("08");
    }
    let edited = EditableFxCompositionDocument::from_json_value(document)
        .unwrap()
        .to_json_value()
        .unwrap();
    let background = &edited["composition"]["layers"][0];
    assert_eq!(background["fills"], preserved_background);
    let (paint, interior) = cutout_children(&background["layers"][0]);
    assert_eq!(paint["sourceText"]["text"], "08");
    assert_eq!(interior["sourceText"]["text"], "08");
}

#[test]
fn outline_only_keys_share_glyph_layout_and_keep_common_alignment_and_opacity() {
    let mut graphic = outline_graphic();
    let start = graphic.in_ticks;
    let text = graphic.text_mut();
    text.document.text = "0\n1".into();
    let mut second = text.document.clone();
    second.text = "04".into();
    second.size = 120.0;
    second.leading = 12.0;
    second.tracking = 40.0;
    second.fill = Some(PrRgb([255, 0, 0]));
    second.stroke.as_mut().unwrap().width = 5.0;
    let anchor = text.transform.anchor[1];
    text.source_text_keys = vec![
        PrSourceTextKey {
            source_ticks: start,
            document: text.document.clone(),
        },
        PrSourceTextKey {
            source_ticks: start + TICKS,
            document: second,
        },
    ];
    text.animations = vec![PrPropertyAnimation::Opacity(vec![
        scalar(start, 75.0, PrKeyframeEasing::Hold),
        scalar(start + TICKS, 25.0, PrKeyframeEasing::Hold),
    ])];
    let (document, _) = imported(graphic);
    let group = &document["composition"]["layers"][0];
    let (paint, interior) = cutout_children(group);
    for property in ["textContent", "fontSize", "leading", "tracking"] {
        assert_eq!(
            key_values(&document, &paint["id"], property),
            key_values(&document, &interior["id"], property),
            "{property}"
        );
    }
    assert_eq!(
        key_values(&document, &paint["id"], "fillEnabled"),
        vec![(0, json!(false)), (1000, json!(true))]
    );
    assert_eq!(
        key_values(&document, &interior["id"], "fillEnabled"),
        vec![(0, json!(true)), (1000, json!(false))]
    );
    assert_eq!(
        key_values(&document, &group["id"], "anchorPointY"),
        vec![
            (0, json!(anchor + automatic_line_spacing(100.0) / 2.0)),
            (1000, json!(anchor)),
        ]
    );
    assert_eq!(
        key_values(&document, &group["id"], "opacity"),
        vec![(0, json!(75.0)), (1000, json!(25.0))]
    );
    assert_eq!(paint["animators"].as_array().unwrap().len(), 1);
    assert!(interior.get("animators").is_none());
    for child in [paint, interior] {
        assert!(!document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["target"]["layerId"] == child["id"]
                && [json!("anchorPointY"), json!("opacity")]
                    .contains(&entry["target"]["propertyType"])));
    }
}

#[test]
fn outline_only_line_cutout_keeps_line_order_baselines_and_filled_sibling() {
    let mut graphic = outline_graphic();
    let text = graphic.text().clone();
    let outline = text.document.clone();
    let mut filled = outline.clone();
    filled.text = "Filled".into();
    filled.fill = Some(PrRgb([255, 0, 0]));
    filled.stroke = None;
    graphic.objects = vec![PrGraphicObject::TextLines(PrTextLines {
        name: text.name,
        documents: vec![filled, outline],
        transform: text.transform,
        animations: Vec::new(),
    })];
    let (document, _) = imported(graphic);
    let block = &document["composition"]["layers"][0];
    let lines = block["layers"].as_array().unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["type"], "Text");
    assert_eq!(lines[0]["sourceText"]["text"], "Filled");
    assert_eq!(
        lines[0]["sourceText"]["fillColor"],
        json!([1.0, 0.0, 0.0, 1.0])
    );
    let (paint, _) = cutout_children(&lines[1]);
    let offset = automatic_line_spacing(100.0) / 2.0;
    assert_eq!(lines[0]["transform"]["position"], json!([0.0, -offset]));
    assert_eq!(lines[1]["transform"]["position"], json!([0.0, offset]));
    assert_eq!(lines[1]["parent"], block["id"]);
    assert_eq!(paint["sourceText"]["text"], "01");
    assert_eq!(document["composition"]["layers"][1]["type"], "Video");
}
