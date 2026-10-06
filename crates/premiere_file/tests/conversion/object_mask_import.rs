#![cfg(feature = "ffmpeg-library")]

//! Public import coverage. The raster payload is actual native saved coverage;
//! the surrounding 30-frame clock/canvas and Motion are supplementary scaffolds
//! for the existing public 1920x1080 video fixture, not a new Adobe oracle.
use super::support::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use premiere_file::OmissionKind;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};
use tesseract_file::TesseractFile;

const STEP: u64 = 8_511_237_907;
const SIDE: &str = "dd06d550-fb83-4fb0-b9e8-6d3d6fcdedf1.prmf";
const RASTER: &[u8] = include_bytes!("../fixtures/object_mask/propagation-first-two.prmf");

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}
fn field(bytes: &[u8], table: usize, slot: usize) -> usize {
    let delta = i32::from_le_bytes(bytes[table..table + 4].try_into().unwrap());
    let vtable = (table as i64 - i64::from(delta)) as usize;
    table
        + usize::from(u16::from_le_bytes(
            bytes[vtable + 4 + slot * 2..vtable + 6 + slot * 2]
                .try_into()
                .unwrap(),
        ))
}

/// Repeat the second native compressed crop 30 times to match the existing
/// public video's sample count. Only the clock/index/canvas are synthetic;
/// pixels remain byte-exact native frame 1. This is built in the temporary test
/// directory, not a new picture fixture or committed decoded-image dump.
fn supplemental_sidecar(step: u64) -> Vec<u8> {
    let index = u64::from_le_bytes(RASTER[8..16].try_into().unwrap()) as usize;
    let metadata = &RASTER[index..];
    let root = u32_at(metadata, 0);
    let vector_slot = field(metadata, root, 2);
    let vector = vector_slot + u32_at(metadata, vector_slot);
    let element = vector + 8;
    let table = element + u32_at(metadata, element);
    let vt = (table as i64
        - i64::from(i32::from_le_bytes(
            metadata[table..table + 4].try_into().unwrap(),
        ))) as usize;
    let vlen = usize::from(u16::from_le_bytes(metadata[vt..vt + 2].try_into().unwrap()));
    let length = usize::from(u16::from_le_bytes(
        metadata[vt + 2..vt + 4].try_into().unwrap(),
    ));
    let offset = u64::from_le_bytes(
        metadata[field(metadata, table, 2)..field(metadata, table, 2) + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    let size = u32_at(metadata, field(metadata, table, 6));
    let payload = &RASTER[offset..offset + size];
    let mut result = RASTER[..32].to_vec();
    let mut new_index = metadata[..56].to_vec();
    new_index.extend_from_slice(&30u32.to_le_bytes());
    new_index.resize(60 + 30 * 4, 0);
    for frame in 0..30 {
        let payload_offset = result.len() as u64;
        result.extend_from_slice(payload);
        while !(new_index.len() + vlen).is_multiple_of(8) {
            new_index.push(0);
        }
        let new_vt = new_index.len();
        new_index.extend_from_slice(&metadata[vt..vt + vlen]);
        let new_table = new_index.len();
        new_index.extend_from_slice(&metadata[table..table + length]);
        new_index[new_table..new_table + 4]
            .copy_from_slice(&((new_table - new_vt) as i32).to_le_bytes());
        let entry = 60 + frame * 4;
        new_index[entry..entry + 4].copy_from_slice(&((new_table - entry) as u32).to_le_bytes());
        for (slot, bytes) in [
            (1, (frame as u64 * step).to_le_bytes()),
            (2, payload_offset.to_le_bytes()),
            (
                4,
                [1920u32.to_le_bytes(), 1080u32.to_le_bytes()]
                    .concat()
                    .try_into()
                    .unwrap(),
            ),
        ] {
            let at = field(metadata, table, slot) - table + new_table;
            new_index[at..at + 8].copy_from_slice(&bytes);
        }
    }
    let offset = result.len() as u64;
    result[8..16].copy_from_slice(&offset.to_le_bytes());
    result[16..24].copy_from_slice(&(new_index.len() as u64).to_le_bytes());
    result.extend(new_index);
    result
}

fn motion_records(with_blur: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = document(dir.path());
    let video = doc["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    video["transform"]["rotation"] = json!(15);
    if with_blur {
        video["effects"] =
            json!([{"id":1,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":25.0}}]);
    }
    let edited = archive(dir.path(), &doc, &dir.path().join("source.mp4"));
    let output = dir.path().join("native");
    tesseract_to_premiere(&edited, &output, false).unwrap();
    let xml = read_xml(&output.join("project.prproj"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let records: Vec<_> = parsed
        .root_element()
        .children()
        .filter(|node| node.is_element())
        .collect();
    let motion = records
        .iter()
        .find(|node| {
            node.children().any(|child| {
                child.has_tag_name("MatchName") && child.text() == Some("AE.ADBE Motion")
            })
        })
        .unwrap();
    let blur = with_blur.then(|| {
        records
            .iter()
            .find(|node| {
                node.children().any(|child| {
                    child.has_tag_name("MatchName") && child.text() == Some("AE.Impact_Blur_FX")
                })
            })
            .unwrap()
            .attribute("ObjectID")
            .unwrap()
    });
    let mut selected = BTreeSet::from([motion.attribute("ObjectID").unwrap()]);
    selected.extend(blur);
    loop {
        let before = selected.len();
        let refs: Vec<_> = records
            .iter()
            .filter(|node| {
                node.attribute("ObjectID")
                    .is_some_and(|id| selected.contains(id))
            })
            .flat_map(|node| {
                node.descendants()
                    .filter_map(|child| child.attribute("ObjectRef"))
            })
            .collect();
        selected.extend(refs);
        if selected.len() == before {
            break;
        }
    }
    let mut result = records
        .iter()
        .filter(|node| {
            node.attribute("ObjectID")
                .is_some_and(|id| selected.contains(id))
        })
        .map(|node| &xml[node.range()])
        .collect::<Vec<_>>()
        .join("\n");
    let mut ids = vec![motion.attribute("ObjectID").unwrap()];
    ids.extend(blur);
    ids.extend(
        selected
            .into_iter()
            .filter(|id| !ids.contains(id))
            .collect::<Vec<_>>(),
    );
    // Temporary textual names prevent a renumbering from matching another old ID.
    for (i, id) in ids.iter().enumerate() {
        for attr in ["ObjectID", "ObjectRef"] {
            result = result.replace(
                &format!("{attr}=\"{id}\""),
                &format!("{attr}=\"motion-{i}\""),
            );
        }
    }
    for i in 0..ids.len() {
        result = result.replace(&format!("\"motion-{i}\""), &format!("\"{}\"", 2000 + i));
    }
    result
}

fn source(root: &Path, inverted: bool, step: u64) -> std::path::PathBuf {
    source_with_blur(root, inverted, step, false)
}

fn source_with_blur(root: &Path, inverted: bool, step: u64, with_blur: bool) -> std::path::PathBuf {
    let mut records = include_str!("../fixtures/object_mask/opacity.xml").to_owned();
    let wrapped = format!("<R>{records}</R>");
    let parsed = roxmltree::Document::parse(&wrapped).unwrap();
    let encoded = parsed
        .descendants()
        .find(|node| node.attribute("ObjectID") == Some("1237"))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("StartKeyframeValue"))
        .unwrap()
        .text()
        .unwrap()
        .trim()
        .to_owned();
    let mut tracker = STANDARD.decode(&encoded).unwrap();
    tracker[228..232].copy_from_slice(&30u32.to_le_bytes());
    tracker[128..136].copy_from_slice(&step.to_le_bytes());
    tracker[232..240].copy_from_slice(&step.to_le_bytes());
    records = records.replace(&encoded, &STANDARD.encode(tracker));
    edit_record(
        &mut records,
        "<VideoComponentParam ObjectID=\"1076\"",
        "</VideoComponentParam>",
        |record| record.replace(",100.,", ",60.,"),
    );
    if inverted {
        edit_record(
            &mut records,
            "<VideoComponentParam ObjectID=\"1259\"",
            "</VideoComponentParam>",
            |record| record.replace(",false,", ",true,"),
        );
    }
    let mut xml = one_second().replace(
        "<FrameRate>8467200000</FrameRate>",
        &format!("<FrameRate>{step}</FrameRate>"),
    );
    xml = xml.replace("<Duration>254016000000</Duration>", &format!("<Duration>{}</Duration>", 30 * step))
        .replace("<OriginalDuration>254016000000</OriginalDuration>", &format!("<OriginalDuration>{}</OriginalDuration>", 30 * step))
        .replace("<InPoint>0</InPoint>", &format!("<InPoint>{step}</InPoint>"))
        .replace("<OutPoint>254016000000</OutPoint>", &format!("<OutPoint>{}</OutPoint>", 4 * step))
        .replace("<End>254016000000</End>", &format!("<End>{}</End>", 3 * step))
        .replace("<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
            "<VideoComponentChain ObjectID=\"4\"><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"2000\"/><Component Index=\"1\" ObjectRef=\"665\"/></Components></ComponentChain></VideoComponentChain>")
        .replace("</PremiereData>", &format!("{}{} </PremiereData>", motion_records(with_blur), records));
    if with_blur {
        xml = xml.replace(
            "<Component Index=\"1\" ObjectRef=\"665\"/>",
            "<Component Index=\"1\" ObjectRef=\"665\"/><Component Index=\"2\" ObjectRef=\"2001\"/>",
        );
    }
    fixture(root, &xml)
}

#[test]
fn object_mask_public_import_recovers_native_pixels_with_local_motion_trim_and_inversion() {
    for inverted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let input = source(root, inverted, STEP);
        let masks = root.join("project Masks");
        fs::create_dir(&masks).unwrap();
        fs::write(masks.join(SIDE), supplemental_sidecar(STEP)).unwrap();
        fs::write(
            masks.join("25490698-2453-4e9d-a7d3-42bc54742782.prmf"),
            b"invalid decoy",
        )
        .unwrap();
        let output = root.join("converted");
        let notes = premiere_to_tesseract(&input, &output, None, false).unwrap();
        assert!(notes
            .iter()
            .any(|note| note.reason.contains("Object Mask sequence cadence")));
        assert!(!notes
            .iter()
            .any(|note| note.reason.contains("native sequence clock retained")));
        assert!(
            notes
                .iter()
                .any(|note| note.kind == OmissionKind::Approximated
                    && note.reason.contains("30 fps")),
            "{notes:?}"
        );
        assert!(
            notes
                .iter()
                .any(|note| note.reason.contains("AI re-propagation")),
            "{notes:?}"
        );
        assert!(
            !notes
                .iter()
                .any(|note| note.reason.contains("masked occurrence is omitted")),
            "{notes:?}"
        );
        let archive = TesseractFile::open(first_project(&output)).unwrap();
        let doc = archive.project_json().unwrap();
        let owner = doc["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        assert_eq!(owner["transform"]["rotation"].as_f64(), Some(15.0));
        assert_eq!(owner["transform"]["opacity"].as_f64(), Some(60.0));
        assert_eq!(
            owner["trackMatte"]["mode"],
            if inverted { "alphaInverted" } else { "alpha" }
        );
        let children = owner["layers"].as_array().unwrap();
        let video = children
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        assert_eq!(video["transform"]["rotation"].as_f64(), Some(0.0));
        assert_eq!(video["transform"]["opacity"].as_f64(), Some(100.0));
        assert_eq!(video["parent"], owner["id"]);
        let provider = children
            .iter()
            .find(|layer| layer["id"] == owner["trackMatte"]["layer"])
            .unwrap();
        assert_eq!(provider["parent"], owner["id"]);
        assert_eq!(provider["transform"]["rotation"].as_f64(), Some(0.0));
        let frames = provider["layers"].as_array().unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0]["activeRange"], json!({"start":0,"duration":67}));
        assert_eq!(frames[1]["activeRange"], json!({"start":67,"duration":33}));
        assert_eq!(frames[2]["activeRange"], json!({"start":100,"duration":1}));
        let asset = frames[0]["source"]["assetId"].as_str().unwrap();
        assert!(
            asset.ends_with("-frame-1"),
            "source trim must select frame 1: {asset}"
        );
        let png = archive
            .asset(asset)
            .unwrap()
            .read_verified_bytes(16 * 1024 * 1024)
            .unwrap();
        let image = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (1920, 1080));
        let crop: Vec<u8> = (51..720)
            .flat_map(|y| (280..1039).map(move |x| (x, y)))
            .map(|(x, y)| image.get_pixel(x, y)[3])
            .collect();
        assert_eq!(
            format!("{:x}", Sha256::digest(crop)),
            "0f18966209b2f6bfaafa7eba81148f1ca911629a02cc7e4d7749156e84e5847b"
        );
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        let source_asset = video["source"]["assetId"].as_str().unwrap();
        assert_eq!(
            archive
                .asset(source_asset)
                .unwrap()
                .read_verified_bytes(16 * 1024 * 1024)
                .unwrap(),
            MEDIA
        );
        fs::remove_file(masks.join(SIDE)).unwrap();
        let error = premiere_to_tesseract(&input, root.join("missing"), None, false)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(SIDE)
                && error.contains("missing")
                && error.contains("owning masked occurrence is omitted"),
            "{error}"
        );
        assert!(!root.join("missing").exists());
        fs::write(masks.join(SIDE), b"invalid referenced prmf").unwrap();
        let error = premiere_to_tesseract(&input, root.join("invalid"), None, false)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("prmf") && error.contains("owning masked occurrence is omitted"),
            "{error}"
        );
        assert!(!root.join("invalid").exists());
    }
}

#[test]
fn object_mask_current_edited_export_links_timed_matte_without_controller_replay() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // This supplementary export scaffold uses the packaged video's actual 30fps
    // clock. The unlisted native cadence has separate import/sampling proof;
    // declaring 30 native frames at that cadence would invent 1005ms for a
    // 1000ms file and correctly fail export's existing media-duration guard.
    let step = 8_467_200_000;
    let input = source(root, false, step);
    fs::create_dir(root.join("project Masks")).unwrap();
    fs::write(
        root.join("project Masks").join(SIDE),
        supplemental_sidecar(step),
    )
    .unwrap();
    let imported = root.join("imported");
    premiere_to_tesseract(&input, &imported, None, false).unwrap();
    let mut archive = TesseractFile::open(first_project(&imported)).unwrap();
    let mut doc = archive.project_json().unwrap();
    doc["duration"] = json!(0.3);
    let canvas = doc["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Rect")
        .unwrap();
    canvas["activeRange"]["duration"] = json!(300);
    let owner = doc["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    owner["transform"]["position"] = json!([1040, 560]);
    owner["transform"]["rotation"] = json!(25);
    owner["transform"]["opacity"] = json!(70);
    owner["trackMatte"]["mode"] = json!("alphaInverted");
    owner["playback"] = crate::test_support::linear_playback(
        json!({"start":200,"duration":67}),
        json!({"start":0,"duration":67}),
    );
    let children = owner["layers"].as_array_mut().unwrap();
    let video = children
        .iter_mut()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    let source_asset = video["source"]["assetId"].as_str().unwrap().to_owned();
    video["playback"] = crate::test_support::linear_playback(
        json!({"start":0,"duration":67}),
        json!({"start":67,"duration":67}),
    );
    video["sourceRange"] = json!({"start":67,"duration":67});
    let provider = children
        .iter_mut()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    provider["transform"]["position"] = json!([12, 8]);
    provider["transform"]["opacity"] = json!(55);
    provider["playback"] = crate::test_support::linear_playback(
        json!({"start":0,"duration":67}),
        json!({"start":0,"duration":67}),
    );
    let frames = provider["layers"].as_array_mut().unwrap();
    frames.remove(0);
    frames[0]["activeRange"] = json!({"start":0,"duration":33});
    frames[1]["activeRange"] = json!({"start":33,"duration":34});
    let json_path = root.join("edited.json");
    fs::write(&json_path, serde_json::to_vec(&doc).unwrap()).unwrap();
    archive.commit_project_json(&json_path).unwrap();
    let edited = root.join("edited.tsrct");
    archive.save_as(&edited).unwrap();
    assert_eq!(
        archive
            .asset(&source_asset)
            .unwrap()
            .read_verified_bytes(16 * 1024 * 1024)
            .unwrap(),
        MEDIA
    );
    let options = premiere_file::PremiereExportOptions::default();
    let prepared = premiere_file::Premiere
        .prepare_export(&archive, archive.project(), &options)
        .unwrap();
    assert!(
        prepared
            .losses()
            .losses
            .iter()
            .all(|loss| matches!(loss.source, premiere_file::ExportLossSource::Document)),
        "{:?}",
        prepared.losses()
    );
    drop(prepared);
    let native = root.join("exported");
    let notes = tesseract_to_premiere(&edited, &native, false).unwrap();
    assert!(
        !notes
            .iter()
            .any(|note| note.scope == premiere_file::OmissionScope::Occurrence),
        "{notes:?}"
    );
    let xml = read_xml(&native.join("project.prproj"));
    assert!(!xml.contains("AEMask") && !xml.contains("Tracker") && !xml.contains(".prmf"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let records = parsed.root_element();
    let text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    let key = records
        .children()
        .find(|node| text(*node, "MatchName").as_deref() == Some("AE.ADBE Legacy Key Track Matte"))
        .unwrap();
    let params: std::collections::BTreeMap<_, _> = key
        .descendants()
        .filter(|node| node.has_tag_name("Param"))
        .map(|reference| {
            let param = records
                .children()
                .find(|node| node.attribute("ObjectID") == reference.attribute("ObjectRef"))
                .unwrap();
            (
                text(param, "ParameterID").unwrap(),
                text(param, "StartKeyframe")
                    .unwrap()
                    .split(',')
                    .nth(1)
                    .unwrap()
                    .to_owned(),
            )
        })
        .collect();
    assert_eq!(params["2"], "0", "alpha, not luma");
    assert_eq!(params["3"], "true", "current inverted polarity");
    let track = records
        .children()
        .find(|node| {
            node.has_tag_name("VideoClipTrack")
                && node.descendants().any(|child| {
                    child.has_tag_name("ID") && child.text() == Some(params["1"].as_str())
                })
        })
        .unwrap();
    assert!(
        track
            .descendants()
            .any(|node| node.has_tag_name("TrackItem")),
        "linked source track exists"
    );
    let follow = |node: roxmltree::Node<'_, '_>, tag: &str| {
        let reference = node
            .descendants()
            .find(|child| child.has_tag_name(tag) && child.attribute("ObjectRef").is_some())
            .unwrap();
        records
            .children()
            .find(|record| record.attribute("ObjectID") == reference.attribute("ObjectRef"))
            .unwrap()
    };
    let provider_item = follow(track, "TrackItem");
    let provider_clip = follow(follow(provider_item, "SubClip"), "Clip");
    assert!(follow(provider_clip, "Source").has_tag_name("VideoSequenceSource"));
    assert_eq!(
        provider_clip
            .descendants()
            .find(|node| node.has_tag_name("OutPoint"))
            .unwrap()
            .text()
            .unwrap()
            .parse::<i64>()
            .unwrap(),
        2 * TICKS / 30
    );
    assert!(
        records.children().any(|node| node.has_tag_name("VideoClip")
            && node.descendants().any(|field| field.has_tag_name("InPoint")
                && field.text() == Some((67 * TICKS / 1000).to_string().as_str()))
            && node
                .descendants()
                .any(|field| field.has_tag_name("OutPoint")
                    && field.text()
                        == Some((67 * TICKS / 1000 + 2 * TICKS / 30).to_string().as_str()))),
        "current physical-video trim"
    );
    for expected in [
        [1040.0 / 1920.0, 560.0 / 1080.0],
        [12.0 / 1920.0, 8.0 / 1080.0],
    ] {
        assert!(
            records
                .children()
                .filter(|node| text(*node, "Name").as_deref() == Some("Position"))
                .filter_map(|node| text(node, "StartKeyframe"))
                .any(|wire| {
                    let values: Vec<f64> = wire
                        .split(',')
                        .nth(1)
                        .unwrap()
                        .split(':')
                        .map(|part| part.parse().unwrap())
                        .collect();
                    values.len() == 2
                        && (values[0] - expected[0]).abs() < 1e-8
                        && (values[1] - expected[1]).abs() < 1e-8
                }),
            "current position {expected:?}"
        );
    }
    for (name, value) in [("Opacity", "70"), ("Opacity", "55"), ("Rotation", "25")] {
        assert!(
            records
                .children()
                .any(|node| node.has_tag_name("VideoComponentParam")
                    && text(node, "Name").as_deref() == Some(name)
                    && text(node, "StartKeyframe")
                        .is_some_and(|wire| wire.split(',').nth(1) == Some(value))),
            "current {name}={value}"
        );
    }
    assert!(track_item_ticks(&parsed, "Start").contains(&(TICKS / 5)));
    // The original picture is packaged byte-for-byte, not rendered with coverage.
    assert!(fs::read_dir(native.join("media"))
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| fs::read(entry.path()).is_ok_and(|bytes| bytes == MEDIA)));

    // Compact admission panel; all are valid FX edits, but not this bounded provider.
    for case in [
        "geometry", "hidden", "overlap", "tail", "canvas", "effects", "keys",
    ] {
        let mut rejected = doc.clone();
        let owner = rejected["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        let provider = owner["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        match case {
            "geometry" => provider["layers"][0]["transform"]["position"] = json!([1, 0]),
            "hidden" => provider["layers"][0]["isHidden"] = json!(true),
            "overlap" => provider["layers"][1]["activeRange"]["start"] = json!(0),
            "tail" => provider["layers"][1]["activeRange"]["duration"] = json!(33),
            "canvas" => provider["layers"][0]["source"]["sourceRect"]["width"] = json!(1000),
            "effects" => {
                provider["effects"] = json!([{"id":1,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":25.0}}])
            }
            "keys" => {}
            _ => unreachable!(),
        }
        if case == "keys" {
            let id = provider["id"].clone();
            rejected["composition"]["dynamics"] = json!({"entries":[{
                "target":{"kind":"layer","layerId":id,"propertyType":"opacity"},
                "animator":{"type":"keyframes","enabled":true,"keyframes":[
                    {"id":"m0","layerTime":0,"value":{"type":"float","value":100.0},"easing":{"type":"linear"}},
                    {"id":"m1","layerTime":33,"value":{"type":"float","value":50.0},"easing":{"type":"linear"}}
                ]}
            }]});
        }
        fs::write(&json_path, serde_json::to_vec(&rejected).unwrap()).unwrap();
        archive.commit_project_json(&json_path).unwrap();
        let unsupported = root.join(format!("unsupported-{case}.tsrct"));
        archive.save_as(&unsupported).unwrap();
        let out = root.join(format!("unsupported-{case}-native"));
        let error = tesseract_to_premiere(&unsupported, &out, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("supplied matte"), "{case}: {error}");
        assert!(!out.exists());
    }
}

#[test]
fn object_mask_converted_blur_precedes_coverage_and_static_mask_opacity_scales_provider() {
    for (with_blur, inverted, mask_opacity) in
        [(true, false, 100), (false, false, 50), (false, true, 50)]
    {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let input = source_with_blur(root, inverted, 8_467_200_000, with_blur);
        let mut xml = read_xml(&input);
        edit_record(
            &mut xml,
            "<VideoComponentParam ObjectID=\"1257\"",
            "</VideoComponentParam>",
            |record| record.replace(",100.,", &format!(",{mask_opacity}.,")),
        );
        write_prproj(&input, &xml);
        fs::create_dir(root.join("project Masks")).unwrap();
        fs::write(
            root.join("project Masks").join(SIDE),
            supplemental_sidecar(8_467_200_000),
        )
        .unwrap();
        let out = root.join("converted");
        if inverted {
            let error = premiere_to_tesseract(&input, &out, None, false)
                .unwrap_err()
                .to_string();
            assert!(error.contains("G3b"), "{error}");
            assert!(!out.exists());
            continue;
        }
        let notes = premiere_to_tesseract(&input, &out, None, false).unwrap();
        assert!(
            !notes
                .iter()
                .any(|note| note.scope == premiere_file::OmissionScope::Occurrence),
            "{notes:?}"
        );
        let archive = TesseractFile::open(first_project(&out)).unwrap();
        let doc = archive.project_json().unwrap();
        let stage = doc["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["type"] == "Group")
            .unwrap();
        let children = stage["layers"].as_array().unwrap();
        let video = children.iter().find(|l| l["type"] == "Video").unwrap();
        let matte = children
            .iter()
            .find(|l| l["id"] == stage["trackMatte"]["layer"])
            .unwrap();
        assert_eq!(
            matte["transform"]["opacity"].as_f64(),
            Some(f64::from(mask_opacity))
        );
        assert!(stage["effects"].as_array().is_none_or(Vec::is_empty));
        assert_eq!(
            video["effects"].as_array().map_or(0, Vec::len),
            usize::from(with_blur)
        );
        let native = root.join("native");
        let notes = tesseract_to_premiere(first_project(&out), &native, false).unwrap();
        assert!(
            !notes
                .iter()
                .any(|note| note.scope == premiere_file::OmissionScope::Occurrence),
            "{notes:?}"
        );
        let xml = read_xml(&native.join("project.prproj"));
        assert_eq!(xml.contains("AE.Impact_Blur_FX"), with_blur);
        assert!(xml.contains("AE.ADBE Legacy Key Track Matte"));
    }
}

#[test]
fn object_mask_native_cadence_last_sample_and_flat_matte_export() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let input = source(root, false, 8_467_200_000);
    fs::create_dir(root.join("project Masks")).unwrap();
    fs::write(
        root.join("project Masks").join(SIDE),
        supplemental_sidecar(8_467_200_000),
    )
    .unwrap();
    let imported = root.join("imported");
    premiere_to_tesseract(&input, &imported, None, false).unwrap();
    let mut archive = TesseractFile::open(first_project(&imported)).unwrap();
    let original = archive.project_json().unwrap();
    let stage_index = original["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .position(|l| l["type"] == "Group")
        .unwrap();
    let stage = &original["composition"]["layers"][stage_index];
    let children = stage["layers"].as_array().unwrap();
    let video = children.iter().find(|l| l["type"] == "Video").unwrap();
    let provider = children.iter().find(|l| l["type"] == "Group").unwrap();
    let json_path = root.join("current.json");
    // Newly admitted sibling-provider path, with ordinary on-grid windows.
    let mut flat = original.clone();
    let mut picture = video.clone();
    picture.as_object_mut().unwrap().remove("parent");
    picture["trackMatte"] = stage["trackMatte"].clone();
    let mut matte = provider.clone();
    matte.as_object_mut().unwrap().remove("parent");
    let layers = flat["composition"]["layers"].as_array_mut().unwrap();
    layers[stage_index] = picture;
    layers.insert(stage_index, matte);
    fs::write(&json_path, serde_json::to_vec(&flat).unwrap()).unwrap();
    archive.commit_project_json(&json_path).unwrap();
    let flat_path = root.join("flat.tsrct");
    archive.save_as(&flat_path).unwrap();
    tesseract_to_premiere(&flat_path, root.join("flat-native"), false).unwrap();

    // Truthful supplementary FX source, not a repaired human movie: use the
    // existing 10s physical fixture for a 6835ms unit-speed selection, and repeat
    // a known native crop on the exact 204-frame raster clock. Actual full-file
    // crop diversity is checked separately by the env-gated production decoder.
    let mut doc = original.clone();
    doc["duration"] = json!(6.835);
    let layers = doc["composition"]["layers"].as_array_mut().unwrap();
    layers.iter_mut().find(|l| l["type"] == "Rect").unwrap()["activeRange"]["duration"] =
        json!(6835);
    let owner = &mut layers[stage_index];
    owner["playback"] = crate::test_support::linear_playback(
        json!({"start":0,"duration":6835}),
        json!({"start":0,"duration":6835}),
    );
    let children = owner["layers"].as_array_mut().unwrap();
    let picture = children.iter_mut().find(|l| l["type"] == "Video").unwrap();
    picture["playback"] = crate::test_support::linear_playback(
        json!({"start":0,"duration":6835}),
        json!({"start":0,"duration":6835}),
    );
    picture["sourceRange"] = json!({"start":0,"duration":6835});
    picture["sourceIntrinsicDuration"] = json!(10000);
    let asset = picture["source"]["assetId"].as_str().unwrap().to_owned();
    archive
        .add_asset(
            &asset,
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-30fps-10s.mp4"),
            tesseract_file::AssetKind::Video,
        )
        .unwrap();
    let matte = children.iter_mut().find(|l| l["type"] == "Group").unwrap();
    matte["playback"] = crate::test_support::linear_playback(
        json!({"start":0,"duration":6835}),
        json!({"start":0,"duration":6835}),
    );
    let template = matte["layers"][0].clone();
    let boundary =
        |i: u64| ((i * STEP).div_ceil(8_467_200_000) as f64 * 1000.0 / 30.0).round() as u64;
    let frames: Vec<_> = (0..204)
        .map(|i| {
            let mut frame = template.clone();
            let start = boundary(i);
            let end = boundary(i + 1).min(6835);
            frame["id"] = json!(100 + i);
            frame["activeRange"] = json!({"start":start,"duration":end-start});
            frame
        })
        .collect();
    assert_eq!(
        frames.last().unwrap()["activeRange"],
        json!({"start":6833,"duration":2})
    );
    matte["layers"] = json!(frames);
    fs::write(&json_path, serde_json::to_vec(&doc).unwrap()).unwrap();
    archive.commit_project_json(&json_path).unwrap();
    let edited = root.join("native-clock.tsrct");
    archive.save_as(&edited).unwrap();
    let archive = TesseractFile::open(&edited).unwrap();
    let options = premiere_file::PremiereExportOptions::default();
    let prepared = premiere_file::Premiere
        .prepare_export(&archive, archive.project(), &options)
        .unwrap();
    assert!(
        prepared
            .losses()
            .losses
            .iter()
            .all(|loss| matches!(loss.source, premiere_file::ExportLossSource::Document)),
        "{:?}",
        prepared.losses()
    );
    drop(prepared);
    let native = root.join("native-clock-export");
    let notes = tesseract_to_premiere(&edited, &native, false).unwrap();
    assert!(
        !notes
            .iter()
            .any(|note| note.scope == premiere_file::OmissionScope::Occurrence),
        "{notes:?}"
    );
    let xml = read_xml(&native.join("project.prproj"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let ends = track_item_ticks(&parsed, "End");
    assert_eq!(*ends.iter().max().unwrap(), 206 * TICKS / 30);
    assert_eq!(
        parsed
            .descendants()
            .filter(|node| node.has_tag_name("VideoClipTrackItem"))
            .count(),
        206,
        "204 matte frames plus consumer and nest"
    );
    // Same off-grid window with the admitted flat sibling provider. Both root
    // items must cover all206 samples, not merely produce a writable package.
    let mut flat = doc.clone();
    let owner = flat["composition"]["layers"][stage_index].clone();
    let mut picture = owner["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["type"] == "Video")
        .unwrap()
        .clone();
    let mut provider = owner["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["type"] == "Group")
        .unwrap()
        .clone();
    picture.as_object_mut().unwrap().remove("parent");
    provider.as_object_mut().unwrap().remove("parent");
    picture["trackMatte"] = owner["trackMatte"].clone();
    let layers = flat["composition"]["layers"].as_array_mut().unwrap();
    layers[stage_index] = picture;
    layers.insert(stage_index, provider);
    let mut archive = TesseractFile::open(&edited).unwrap();
    fs::write(&json_path, serde_json::to_vec(&flat).unwrap()).unwrap();
    archive.commit_project_json(&json_path).unwrap();
    let input = root.join("flat-native-clock.tsrct");
    archive.save_as(&input).unwrap();
    let output = root.join("flat-native-clock-export");
    tesseract_to_premiere(&input, &output, false).unwrap();
    let xml = read_xml(&output.join("project.prproj"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let mut ranges: Vec<_> = parsed
        .descendants()
        .filter(|n| n.has_tag_name("VideoClipTrackItem"))
        .map(|item| {
            let tick = |name| {
                item.descendants()
                    .find(|n| n.has_tag_name(name))
                    .and_then(|n| n.text())
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap()
            };
            (tick("Start"), tick("End"))
        })
        .collect();
    let end = 206 * TICKS / 30;
    assert_eq!(
        ranges.iter().filter(|&&r| r == (0, end)).count(),
        2,
        "consumer and provider must match"
    );
    ranges.retain(|r| *r != (0, end));
    ranges.sort();
    assert_eq!(ranges.len(), 204);
    assert_eq!(ranges[0].0, 0);
    assert_eq!(ranges.last().unwrap().1, end);
    assert!(ranges.windows(2).all(|r| r[0].1 == r[1].0));
    for sample in 0..206 {
        let tick = sample * TICKS / 30;
        assert_eq!(
            ranges.iter().filter(|r| r.0 <= tick && tick < r.1).count(),
            1
        );
    }
    // Concrete fractional-origin counterexample: native end ticks must not
    // round through local milliseconds and then acquire the origin a second time.
    let mut shifted = doc.clone();
    shifted["duration"] = json!(6.9);
    shifted["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|l| l["type"] == "Rect")
        .unwrap()["activeRange"]["duration"] = json!(6900);
    shifted["composition"]["layers"][stage_index]["playback"] =
        crate::test_support::linear_playback(
            json!({"start":50,"duration":6835}),
            json!({"start":0,"duration":6835}),
        );
    let mut shifted_archive = TesseractFile::open(&edited).unwrap();
    fs::write(&json_path, serde_json::to_vec(&shifted).unwrap()).unwrap();
    shifted_archive.commit_project_json(&json_path).unwrap();
    let shifted_path = root.join("shifted-50.tsrct");
    shifted_archive.save_as(&shifted_path).unwrap();
    let shifted_output = root.join("shifted-50-export");
    tesseract_to_premiere(&shifted_path, &shifted_output, false).unwrap();
    let xml = read_xml(&shifted_output.join("project.prproj"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let mut ranges: Vec<_> = parsed
        .descendants()
        .filter(|n| n.has_tag_name("VideoClipTrackItem"))
        .map(|item| {
            let tick = |name| {
                item.descendants()
                    .find(|n| n.has_tag_name(name))
                    .and_then(|n| n.text())
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap()
            };
            (tick("Start"), tick("End"))
        })
        .collect();
    let outer = (2 * TICKS / 30, 207 * TICKS / 30);
    assert_eq!(ranges.iter().filter(|&&r| r == outer).count(), 2);
    ranges.retain(|r| *r != outer);
    for sample in 0..205 {
        let t = sample * TICKS / 30;
        assert_eq!(
            ranges.iter().filter(|r| r.0 <= t && t < r.1).count(),
            1,
            "shifted sample{sample}"
        );
    }
    assert_eq!(ranges.iter().map(|r| r.1).max(), Some(205 * TICKS / 30));

    for kind in ["nest", "rect"] {
        let mut variant = doc.clone();
        let mut owner = variant["composition"]["layers"][stage_index].clone();
        let mut provider = owner["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["type"] == "Group")
            .unwrap()
            .clone();
        provider.as_object_mut().unwrap().remove("parent");
        let picture = owner["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["type"] == "Video")
            .unwrap()
            .clone();
        owner["transform"] = picture["transform"].clone();
        owner["layers"] = json!([picture]);
        owner["name"] = json!("Plain nest consumer");
        let layers = variant["composition"]["layers"].as_array_mut().unwrap();
        if kind == "rect" {
            let mut rect = layers.iter().find(|l| l["type"] == "Rect").unwrap().clone();
            rect["id"] = json!(9002);
            rect["activeRange"]["duration"] = json!(6835);
            rect["trackMatte"] = owner["trackMatte"].clone();
            rect["rect"]["fillColor"] = json!([1., 0.5, 0., 1.]);
            owner = rect;
        }
        layers[stage_index] = owner;
        layers.insert(stage_index, provider);
        let mut archive = TesseractFile::open(&edited).unwrap();
        fs::write(&json_path, serde_json::to_vec(&variant).unwrap()).unwrap();
        archive.commit_project_json(&json_path).unwrap();
        let input = root.join(format!("{kind}.tsrct"));
        archive.save_as(&input).unwrap();
        let output = root.join(format!("{kind}-export"));
        tesseract_to_premiere(&input, &output, false).unwrap();
        if kind == "nest" {
            for canvas_variant in [
                "missed-sample",
                "shorter-boundary",
                "missing",
                "nonopaque",
                "later-content",
            ] {
                let mut invalid = variant.clone();
                let layers = invalid["composition"]["layers"].as_array_mut().unwrap();
                let at = layers.iter().position(|l| l["type"] == "Rect").unwrap();
                match canvas_variant {
                    "missed-sample" => layers[at]["activeRange"]["duration"] = json!(6832),
                    "shorter-boundary" => layers[at]["activeRange"]["duration"] = json!(6834),
                    "missing" => {
                        layers.remove(at);
                    }
                    "nonopaque" => layers[at]["rect"]["fillColor"] = json!([0., 0., 0., 0.5]),
                    "later-content" => {
                        let mut later = layers[at].clone();
                        later["id"] = json!(9900);
                        later["activeRange"] = json!({"start":6900,"duration":33});
                        later["rect"]["fillColor"] = json!([1., 0., 0., 1.]);
                        layers.push(later);
                        invalid["duration"] = json!(6.933);
                    }
                    _ => unreachable!(),
                }
                fs::write(&json_path, serde_json::to_vec(&invalid).unwrap()).unwrap();
                archive.commit_project_json(&json_path).unwrap();
                let path = root.join(format!("{canvas_variant}.tsrct"));
                archive.save_as(&path).unwrap();
                let output = root.join(format!("{canvas_variant}-export"));
                tesseract_to_premiere(&path, &output, false).unwrap();
                let xml = read_xml(&output.join("project.prproj"));
                assert!(
                    xml.contains("AE.ADBE Legacy Key Track Matte"),
                    "{canvas_variant}"
                );
            }
        }

        let xml = read_xml(&output.join("project.prproj"));
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        let mut ranges: Vec<_> = parsed
            .descendants()
            .filter(|n| n.has_tag_name("VideoClipTrackItem"))
            .map(|item| {
                let tick = |name| {
                    item.descendants()
                        .find(|n| n.has_tag_name(name))
                        .and_then(|n| n.text())
                        .unwrap_or("0")
                        .parse::<i64>()
                        .unwrap()
                };
                (tick("Start"), tick("End"))
            })
            .collect();
        // A nested consumer has both its outer placement and its real picture
        // child through sample205. Equal outer ranges alone would miss black.
        assert_eq!(
            ranges.iter().filter(|&&r| r == (0, end)).count(),
            if kind == "nest" { 3 } else { 2 },
            "{kind}"
        );
        assert!(xml.contains("AE.ADBE Legacy Key Track Matte"));
        if kind == "nest" {
            // The real video child has source picture through the final sample,
            // not only an extended outer nest with an empty picture tail.
            assert!(parsed
                .descendants()
                .filter(|n| n.has_tag_name("VideoClip"))
                .any(|clip| {
                    let point = |name| {
                        clip.descendants()
                            .find(|n| n.has_tag_name(name))
                            .and_then(|n| n.text())
                            .and_then(|n| n.parse::<i64>().ok())
                    };
                    point("InPoint") == Some(0) && point("OutPoint") == Some(end)
                }));
        }

        ranges.retain(|r| *r != (0, end));
        ranges.sort();
        assert_eq!(ranges.len(), 204);
        for sample in 0..206 {
            let t = sample * TICKS / 30;
            assert_eq!(
                ranges.iter().filter(|r| r.0 <= t && t < r.1).count(),
                1,
                "{kind} sample{sample}"
            );
        }
    }
}

#[test]
fn object_mask_off_grid_origin_is_covered_before_first_output_sample() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let input = source(root, true, STEP);
    let xml = read_xml(&input).replace(
        &format!("<End>{}</End>", 3 * STEP),
        &format!("<Start>{STEP}</Start><End>{}</End>", 4 * STEP),
    );
    write_prproj(&input, &xml);
    fs::create_dir(root.join("project Masks")).unwrap();
    fs::write(
        root.join("project Masks").join(SIDE),
        supplemental_sidecar(STEP),
    )
    .unwrap();
    let out = root.join("converted");
    premiere_to_tesseract(&input, &out, None, false).unwrap();
    let archive = TesseractFile::open(first_project(&out)).unwrap();
    let doc = archive.project_json().unwrap();
    let owner = doc["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["type"] == "Group")
        .unwrap();
    assert_eq!(owner["playback"]["inputRange"]["start"], json!(34));
    let matte = owner["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == owner["trackMatte"]["layer"])
        .unwrap();
    assert_eq!(matte["layers"][0]["activeRange"]["start"], json!(0));
    assert_eq!(owner["trackMatte"]["mode"], json!("alphaInverted"));
}
