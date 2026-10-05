//! Native G4 effect-mask ownership, with supplementary human numeric records.
use super::*;
use crate::format::{inspect_project_with_omissions, read_xml};
use crate::schema::{PrBrightnessContrast, PrEffect, PrEffectParams, PrInvert};

const FIXTURE: &str = "feature_opacity_masks_26_5_strict.prproj";
const UID: &str = "5fe2e712-90a9-4044-b93a-7a75b79b1320";
const OWNER: &str = "VideoClipTrackItem:90";

fn fixture(name: &str) -> String {
    read_xml(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn parameter(source: &str, owner: &str, param: &str) -> String {
    let doc = roxmltree::Document::parse(source).unwrap();
    let record = doc
        .root_element()
        .children()
        .find(|node| node.attribute("ObjectID") == Some(owner))
        .unwrap();
    let refs: Vec<_> = record
        .descendants()
        .filter_map(|node| node.attribute("ObjectRef"))
        .collect();
    let node = doc
        .root_element()
        .children()
        .find(|node| {
            node.has_tag_name("VideoComponentParam")
                && node
                    .attribute("ObjectID")
                    .is_some_and(|id| refs.contains(&id))
                && node
                    .children()
                    .any(|child| child.has_tag_name("ParameterID") && child.text() == Some(param))
        })
        .unwrap();
    source[node.range()].to_owned()
}

/// The ownership/path remain native G4. Copy the human-authored Shape-mask
/// numeric records and shift only their keys onto G4's eight-second source In.
/// This combination is supplementary, not an independently authored effect-mask
/// animation or a native render reference.
fn numeric_source() -> String {
    let human = fixture("feature_graphic_masks_numeric_26_5.prproj");
    let mut source = fixture(FIXTURE);
    for id in ["14", "15", "16"] {
        let from = parameter(&human, "196", id);
        let to = parameter(&source, "184", id);
        let a = roxmltree::Document::parse(&from).unwrap();
        let b = roxmltree::Document::parse(&to).unwrap();
        let keys = a
            .root_element()
            .children()
            .find(|node| node.has_tag_name("Keyframes"))
            .unwrap()
            .text()
            .unwrap();
        let shifted: String = keys
            .split_terminator(';')
            .map(|key| {
                let (time, rest) = key.split_once(',').unwrap();
                format!("{},{};", time.parse::<i64>().unwrap() + 8 * TICKS, rest)
            })
            .collect();
        let moved = from
            .replacen(
                &format!(
                    "ObjectID=\"{}\"",
                    a.root_element().attribute("ObjectID").unwrap()
                ),
                &format!(
                    "ObjectID=\"{}\"",
                    b.root_element().attribute("ObjectID").unwrap()
                ),
                1,
            )
            .replace(keys, &shifted);
        source = source.replace(&to, &moved);
    }
    source
}

fn native_case(source: &str) -> PrProjectFile {
    let (mut project, omissions) = inspect_project_with_omissions(source, Some(UID)).unwrap();
    let sequence = &mut project.sequences[0];
    assert!(
        sequence
            .video_occurrences()
            .any(|clip| clip.id.as_deref() == Some(OWNER)),
        "{omissions:?}"
    );
    for track in &mut sequence.video_tracks {
        track.items.retain(|item| {
            item.media()
                .is_some_and(|clip| clip.id.as_deref() == Some(OWNER))
        });
    }
    sequence.audio.clear();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.in_ticks, 8 * TICKS);
    assert_eq!(clip.start_ticks, 8 * TICKS);
    assert_eq!(clip.effects.len(), 1);
    assert!(clip.effects[0].mask.is_some());
    let media = clip.media.clone();
    project.media.retain(|id, _| *id == media);
    project
}

fn imported(project: &PrProjectFile) -> Value {
    let sequence = project.single_sequence().unwrap();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut omissions = Vec::new();
    let document =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
            .unwrap();
    assert!(
        omissions
            .iter()
            .all(|note| note.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
    document.to_json_value().unwrap()
}

fn scope(wire: &mut Value) -> &mut Value {
    wire["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Group")
        .unwrap()
}

#[test]
fn native_effect_mask_retains_scope_numeric_edits_and_source_clock() {
    // Native source SHA256: 2fbdd3dd41ccb6fed4eb3dc2f843d4d64979d7ee8a5b30fdefe3eff032ea86fa.
    let mut project = native_case(&numeric_source());
    let clip = project.sequences[0]
        .video_tracks
        .iter_mut()
        .flat_map(|track| &mut track.items)
        .find_map(|item| match item {
            PrVideoItem::Media(clip) => Some(clip),
            _ => None,
        })
        .unwrap();
    let numeric = clip.effects[0].mask.as_ref().unwrap();
    for (_, keys) in numeric.numeric_keys() {
        assert_eq!(
            keys.iter().map(|key| key.source_ticks).collect::<Vec<_>>(),
            [8 * TICKS, 8 * TICKS + 169_344_000_000]
        );
    }
    // Supplementary noncommuting prefix/suffix prove stack positions. Move only
    // placement; neither the eight-second source In nor key times changes.
    clip.start_ticks = TICKS;
    clip.end_ticks = 3 * TICKS;
    let invert = PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Invert(PrInvert {
            blend: 0.0,
            channel: 0,
        }),
        animations: Vec::new(),
    };
    clip.effects.insert(0, invert);
    clip.effects.push(PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::BrightnessContrast(PrBrightnessContrast {
            brightness: 12.0,
            contrast: 30.0,
        }),
        animations: Vec::new(),
    });
    project.sequences[0].timeline_end_ticks = 3 * TICKS;
    let mut wire = imported(&project);
    let owner = scope(&mut wire);
    assert_eq!(owner["effects"].as_array().unwrap().len(), 1);
    assert_eq!(owner["effects"][0]["effect"]["type"], "brightnessContrast");
    let children = owner["layers"].as_array().unwrap();
    assert_eq!(children[2]["effects"].as_array().unwrap().len(), 1);
    assert_eq!(children[2]["effects"][0]["effect"]["type"], "levels");
    assert_eq!(children[0]["effects"][0]["effect"]["type"], "gaussianBlur");
    let mask_id = children[0]["masks"][0]["id"].clone();
    let entries = wire["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    for (name, original, edited) in [
        (
            "feather",
            json!([[0.0, 0.0], [24.0, 24.0]]),
            json!([12.0, 12.0]),
        ),
        ("expansion", json!([-12.0, 20.0]), json!(-6.0)),
        ("opacity", json!([1.0, 0.4]), json!(0.7)),
    ] {
        let entry = entries
            .iter_mut()
            .find(|entry| {
                entry["target"]["itemId"] == mask_id && entry["target"]["propertyName"] == name
            })
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array_mut().unwrap();
        assert_eq!(
            keys.iter()
                .map(|key| key["layerTime"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            [0, 667]
        );
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(key["value"]["value"], original[index]);
            assert_eq!(key["easing"]["type"], "linear");
        }
        keys[1]["layerTime"] = json!(625);
        keys[1]["value"]["value"] = edited;
    }
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    let mut omissions = Vec::new();
    let exported = export_document(
        &document,
        &source(FrameRate::Fps30, 10 * TICKS),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(
        omissions
            .iter()
            .all(|note| note.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
    let clip = first_clip(&exported.project);
    assert_eq!((clip.start_ticks, clip.in_ticks), (TICKS, 8 * TICKS));
    assert_eq!(clip.effects.len(), 3);
    assert!(matches!(clip.effects[0].params, PrEffectParams::Invert(_)));
    assert!(matches!(
        clip.effects[1].params,
        PrEffectParams::FilmImpactBlur(_)
    ));
    assert!(matches!(
        clip.effects[2].params,
        PrEffectParams::BrightnessContrast(PrBrightnessContrast {
            brightness: 12.0,
            contrast: 30.0
        })
    ));
    assert!(clip.opacity_mask.is_none());
    let mask = clip.effects[1].mask.as_ref().unwrap();
    for ((_, keys), value) in mask.numeric_keys().into_iter().zip([12.0, -6.0, 70.0]) {
        assert_eq!(keys[1].value, value);
        assert_eq!(
            keys[1].source_ticks,
            8 * TICKS + 625 * TICKS_PER_MILLISECOND
        );
    }
    for entry in document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter(|entry| entry.target.fx_item_id().is_some())
    {
        assert!(exported.written.contains(&entry.target));
    }
    let expected = clip.effects.clone();
    let reread = write_and_load_with_crate_reader(exported.project);
    assert_eq!(first_clip(&reread).effects, expected);

    scope(&mut wire)["layers"][0]["effects"][0]["enabled"] = json!(false);
    let bypassed = convert(wire.clone()).unwrap();
    let masked = &first_clip(&bypassed).effects[1];
    assert!(!masked.enabled);
    assert!(masked.mask.is_some());
    let reread = write_and_load_with_crate_reader(bypassed);
    assert!(!first_clip(&reread).effects[1].enabled);
    assert!(first_clip(&reread).effects[1].mask.is_some());
    scope(&mut wire)["layers"][0]["effects"][0]["enabled"] = json!(true);

    // A valid editable keyer changes coverage, outside this scope's admitted
    // native pipeline. Do not write it masked and then lose it on reimport.
    let mut coverage = wire.clone();
    scope(&mut coverage)["layers"][0]["effects"][0]["effect"] =
        json!({"type": "lumaKey", "threshold": 0.4, "softness": 0.2, "invert": 0.0});
    let reason = convert_with_omissions(coverage).unwrap_err().to_string();
    assert!(
        reason.contains("no convertible video or audio layers"),
        "{reason}"
    );
    assert!(reason.contains("coverage-changing effects"), "{reason}");

    // A partial adjustment trim must omit the entire picture, not export a
    // generic nest that quietly drops the adjustment mask.
    scope(&mut wire)["layers"][0]["activeRange"]["duration"] = json!(1000);
    let reason = convert_with_omissions(wire).unwrap_err().to_string();
    assert!(
        reason.contains("no convertible video or audio layers"),
        "{reason}"
    );
    assert!(
        reason.contains("effect scope children must share the full local unit-speed clock"),
        "{reason}"
    );
}

#[test]
fn group_motion_blur_preserves_native_effect_mask_scope() {
    let project = native_case(&numeric_source());
    let mut wire = imported(&project);
    let baseline = convert(wire.clone()).unwrap();
    let owner = scope(&mut wire);
    owner["motionBlur"] = json!(true);
    let id = owner["id"].as_u64().unwrap();
    let (exported, reports) = convert_with_omissions(wire).unwrap();
    let clip = first_clip(&exported);
    let expected = first_clip(&baseline);
    assert_eq!(clip.source_ticks(), expected.source_ticks());
    assert_eq!(clip.timeline_ticks(), expected.timeline_ticks());
    assert_eq!(clip.transform, expected.transform);
    assert_eq!(clip.effects, expected.effects);
    assert!(clip.effects.iter().any(|effect| effect.mask.is_some()));
    assert!(
        reports
            .iter()
            .any(|report| report.scope == OmissionScope::Feature
                && report.record.starts_with(&format!("layer {id} ("))
                && report.reason.contains("group motion blur was not exported")),
        "{reports:?}"
    );
}

/// Derived combination, not a new Adobe-authored masked-Alpha oracle: transplant
/// the pinned Alpha controls onto G4's physical video and retained native mask.
#[test]
fn premiere_alpha_bypassed_parent_with_live_effect_mask_keeps_video_and_audio() {
    use sha2::{Digest, Sha256};

    fn record(source: &str, id: &str) -> String {
        let doc = roxmltree::Document::parse(source).unwrap();
        let node = doc
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap();
        source[node.range()].to_owned()
    }

    fn selected(source: &str) -> (PrProjectFile, Vec<crate::Omission>) {
        let (mut project, omissions) = inspect_project_with_omissions(source, Some(UID)).unwrap();
        for track in &mut project.sequences[0].video_tracks {
            track.items.retain(|item| {
                item.media()
                    .is_some_and(|clip| clip.id.as_deref() == Some(OWNER))
            });
        }
        // Keep the independently saved audio, unlike native_case's mask-only setup.
        assert!(!project.sequences[0].audio.is_empty());
        (project, omissions)
    }

    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (path, hash) in [
        (
            FIXTURE,
            "2fbdd3dd41ccb6fed4eb3dc2f843d4d64979d7ee8a5b30fdefe3eff032ea86fa",
        ),
        (
            "premiere_invert_alpha/source/native.prproj",
            "40b3cb3d93261addfae28ad888e9cb7f3bf6203549e8a4f5f4fb89b8e458ce6e",
        ),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(directory.join(path)).unwrap())
            ),
            hash
        );
    }
    let source = fixture(FIXTURE);
    let blur = record(&source, "142");
    let doc = roxmltree::Document::parse(&blur).unwrap();
    let node = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("SubComponents"))
        .unwrap();
    let mask = &blur[node.range()];
    // Leave the saved blur as a supported unmasked sibling in both documents.
    let baseline = source.replace(&blur, &blur.replace(mask, ""));
    let alpha = fixture("premiere_invert_alpha/source/native.prproj");
    let mut effect = record(&alpha, "86")
        .replace("ObjectID=\"86\"", "ObjectID=\"900000\"")
        .replace("ObjectRef=\"96\"", "ObjectRef=\"900001\"")
        .replace("ObjectRef=\"97\"", "ObjectRef=\"900002\"")
        .replace("<ID>3</ID>", "<ID>4</ID>")
        .replace(
            "</Component>",
            &format!("<Bypass>true</Bypass></Component>{mask}"),
        );
    for (old, new) in [("96", "900001"), ("97", "900002")] {
        effect.push_str(&record(&alpha, old).replace(
            &format!("ObjectID=\"{old}\""),
            &format!("ObjectID=\"{new}\""),
        ));
    }
    let chain = record(&baseline, "114");
    let changed_chain = chain.replace(
        "</Components>",
        "<Component Index=\"1\" ObjectRef=\"900000\"/></Components>",
    );
    let derived = baseline
        .replace(&chain, &changed_chain)
        .replace("</PremiereData>", &format!("{effect}</PremiereData>"));
    // RGB control proves the very same independently live mask survives reading;
    // the fix must not discard all masks merely because their parent is bypassed.
    let channel = record(&derived, "900001");
    assert!(channel.contains(",15,0,0,0,0,0,0"));
    let rgb = derived.replace(
        &channel,
        &channel.replace(",15,0,0,0,0,0,0", ",0,0,0,0,0,0,0"),
    );
    let (rgb, _) = selected(&rgb);
    let masked = rgb.sequences[0]
        .video_occurrences()
        .next()
        .unwrap()
        .effects
        .iter()
        .find(|effect| matches!(effect.params, PrEffectParams::Invert(_)))
        .unwrap();
    assert!(!masked.enabled);
    assert!(masked.mask.is_some());

    let (baseline, _) = selected(&baseline);
    let (current, reader_notes) = selected(&derived);
    let expected = imported(&baseline);
    let actual = imported(&current);
    let layers = expected["composition"]["layers"].as_array().unwrap();
    let picture = layers
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert!(!picture["source"].is_null());
    assert_eq!(picture["effects"][0]["effect"]["type"], "gaussianBlur");
    assert!(layers.iter().any(|layer| layer["type"] == "Audio"));
    let clip = baseline.sequences[0].video_occurrences().next().unwrap();
    assert_eq!(
        (clip.start_ticks, clip.end_ticks, clip.in_ticks),
        (8 * TICKS, 10 * TICKS, 8 * TICKS)
    );
    // Exact document equality checks source, playback, transfer, siblings and
    // sound together, and rules out any Alpha wrapper/backing/sample allocation.
    assert_eq!(actual, expected);
    assert!(
        reader_notes.iter().any(|note| {
            note.kind == OmissionKind::Omitted
                && note.record == "VideoFilterComponent:900000"
                && note
                    .reason
                    .contains("bypassed Invert Alpha with an effect mask")
        }),
        "{reader_notes:?}"
    );
}

/// Derived native G4 mask/physical-video owner plus the saved human Levels
/// records. Active channel masks retain strict coverage rejection; neutral-row
/// masters retain their mask, and bypassed parents must not lose the picture.
#[test]
fn channel_levels_masked_reader_keeps_master_and_bypassed_owner() {
    fn record(source: &str, id: &str) -> String {
        let doc = roxmltree::Document::parse(source).unwrap();
        let node = doc
            .root_element()
            .children()
            .find(|n| n.attribute("ObjectID") == Some(id))
            .unwrap();
        source[node.range()].to_owned()
    }
    fn selected(source: &str) -> PrProjectFile {
        let (mut p, _) = inspect_project_with_omissions(source, Some(UID)).unwrap();
        for track in &mut p.sequences[0].video_tracks {
            track
                .items
                .retain(|item| item.media().is_some_and(|c| c.id.as_deref() == Some(OWNER)));
        }
        p
    }
    let source = fixture(FIXTURE);
    let old = record(&source, "142");
    let doc = roxmltree::Document::parse(&old).unwrap();
    let mask = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("SubComponents"))
        .unwrap();
    let levels = include_str!("../../../tests/fixtures/human_levels_master.xml");
    let effect = record(levels, "560")
        .replace("ObjectID=\"560\"", "ObjectID=\"142\"")
        .replace(
            "</Component>",
            &format!("</Component>{}", &old[mask.range()]),
        );
    let params = (903..=922)
        .map(|id| record(levels, &id.to_string()))
        .collect::<String>();
    let derived = source
        .replace(&old, &effect)
        .replace("</PremiereData>", &format!("{params}</PremiereData>"));
    let (project, notes) = inspect_project_with_omissions(&derived, Some(UID)).unwrap();
    assert!(!project.sequences[0]
        .video_occurrences()
        .any(|c| c.id.as_deref() == Some(OWNER)));
    assert!(
        notes.iter().any(|n| n
            .reason
            .contains("Levels has unsupported channel corrections")),
        "{notes:?}"
    );
    // Active per-channel masks retain strict coverage admission. A neutral-row
    // master remains supported; update both its saved controls and private copy.
    let neutral = derived.replace(
        "HgD/AAAA/wBkAAAAyAAAAP8AAgAAAP8AAAD/AGQAAAD/AAAA/wBkAA==",
        "HgD/AAAA/wBkAAAA/wAAAP8AZAAAAP8AAAD/AGQAAAD/AAAA/wBkAA==",
    );
    let white = record(&neutral, "909");
    let neutral = neutral.replace(
        &white,
        &white.replace(",200,0,0,0,0,0,0", ",255,0,0,0,0,0,0"),
    );
    let gamma = record(&neutral, "912");
    let neutral = neutral.replace(&gamma, &gamma.replace(",2,0,0,0,0,0,0", ",100,0,0,0,0,0,0"));
    let neutral_project = selected(&neutral);
    let clip = neutral_project.sequences[0]
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.effects.len(), 1);
    assert!(clip.effects[0].mask.is_some());
    assert!(matches!(
        clip.effects[0].params,
        PrEffectParams::Levels(crate::schema::PrLevels::Master { .. })
    ));
    let mapped = imported(&neutral_project);
    assert!(mapped.to_string().contains("premiere-video-1"));
    // Premiere-native Levels has no admitted parent Bypass field. Keep that
    // existing effect-only rejection, rather than introducing a masked sentinel.
    let bypassed = derived.replace(
        &effect,
        &effect.replace("</Component>", "<Bypass>true</Bypass></Component>"),
    );
    let chain = record(&source, "114");
    let empty = source.replace(
        &chain,
        &chain.replace("<Component Index=\"0\" ObjectRef=\"142\"/>", ""),
    );
    assert_eq!(imported(&selected(&bypassed)), imported(&selected(&empty)));
}
