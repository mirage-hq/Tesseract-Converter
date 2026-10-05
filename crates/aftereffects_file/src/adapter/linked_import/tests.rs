use std::{fs, io::Read};

use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::read::GzDecoder;
use fx_conv::ImportToTesseract;
use fx_schema::{GroupLayer, Layer, LayerData, Position};
use tesseract_file::TesseractFile;

use super::*;
use crate::{AfterEffectsImportOptions, structure_document::MediaAssetKind};

const SOURCE: &[u8] =
    include_bytes!("../../../tests/fixtures/hybrid/identity/linked-compositions.aep");
const PREMIERE: &[u8] =
    include_bytes!("../../../tests/fixtures/hybrid/identity/native-linked.prproj");
const SOURCE_SHA: &str = "90ba0883e9c54ee006a8e33d8b497666706a2f25297dbeb3cac71e4c2127f3ff";
const FORMAT96: &[u8] =
    include_bytes!("../../../tests/fixtures/hybrid/format96/coeditor-format96.rifx");

mod format96;

fn guid(id: u32) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..4].copy_from_slice(&id.to_be_bytes());
    bytes
}

fn group(layer: &Layer) -> &GroupLayer {
    let LayerData::Group(group) = layer.data() else {
        panic!("expected editable Group, not flattened media");
    };
    group
}

#[test]
fn collected_media_freshness_checks_every_missing_higher_priority_path() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("collected.wav");
    fs::write(&path, b"collected bytes").unwrap();
    let missing_paths = vec![
        directory.path().join("authored.wav"),
        directory.path().join("alias.wav"),
    ];
    let mut media = LinkedMedia::default();
    media
        .add_source(&media::SourceFile {
            path,
            missing_paths: missing_paths.clone(),
            decoded_sha256: None,
        })
        .unwrap();
    media.verify_sources().unwrap();
    for priority in missing_paths {
        fs::write(&priority, b"new higher-priority bytes").unwrap();
        assert!(
            matches!(media.verify_sources(), Err(AepConversionError::MediaChanged(changed)) if changed == priority)
        );
        fs::remove_file(&priority).unwrap();
        media.verify_sources().unwrap();
    }
}

#[test]
fn linked_import_accepts_prefixes_above_the_former_byte_quota() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.aep");
    fs::write(&path, SOURCE).unwrap();
    let prepared = AfterEffects.prepare_linked_import(&path).unwrap();
    let selected = prepared.resolve_composition(&guid(1)).unwrap();
    let prefix = "linked-".repeat(30);
    assert!(selected.import_editable_picture(100, &prefix).is_ok());
    assert!(selected.import_editable_picture(0, &prefix).is_err());
}

#[test]
fn hybrid_native_compositions_use_caller_reserved_ids_without_reparsing() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.aep");
    fs::write(&path, SOURCE).unwrap();
    let prepared = AfterEffects.prepare_linked_import(&path).unwrap();
    let mut next = 100;
    for id in [1, 16, 1] {
        let mut imported = prepared
            .resolve_composition(&guid(id))
            .unwrap()
            .import_editable_picture(next, &format!("linked-{next}-"))
            .unwrap();
        let document = imported.take_document().unwrap();
        assert_eq!(group(&document.composition().layers()[0]).id.value(), next);
        assert!(imported.next_id > next);
        next = imported.next_id;
    }
    assert_eq!(fs::read(path).unwrap(), SOURCE);
}

#[test]
fn native_same_name_guids_import_distinct_editable_red_and_blue_compositions() {
    assert_eq!(format!("{:x}", Sha256::digest(SOURCE)), SOURCE_SHA);
    assert_eq!(
        format!("{:x}", Sha256::digest(PREMIERE)),
        "a03b8d06787e500a803fe2b28ebca4df2b3f817a6d91098638acc80ea877ef04"
    );
    // Independent native Premiere payloads, not GUIDs produced by our writer.
    let mut xml = String::new();
    GzDecoder::new(PREMIERE).read_to_string(&mut xml).unwrap();
    let native = roxmltree::Document::parse(&xml).unwrap();
    let identities: Vec<_> = native
        .descendants()
        .filter(|node| node.has_tag_name("ImporterPrefs"))
        .map(|node| {
            let bytes = STANDARD.decode(node.text().unwrap().trim()).unwrap();
            assert_eq!(bytes.len(), 72);
            let units: Vec<_> = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16(&units).unwrap()
        })
        .collect();
    assert_eq!(
        identities,
        [
            "00000001-0000-0000-0000-000000000000",
            "00000010-0000-0000-0000-000000000000",
        ]
    );

    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, SOURCE).unwrap();
    let prepared = AfterEffects.prepare_linked_import(&input).unwrap();
    assert_eq!(prepared.source_path(), input);
    assert_eq!(
        *prepared.source_sha256(),
        <[u8; 32]>::from(Sha256::digest(SOURCE))
    );
    for (id, label, color) in [
        (1, "Hybrid_Identity_Red_v1_solid", [1.0, 0.0, 0.0, 1.0]),
        (16, "Hybrid_Identity_Blue_v1_solid", [0.0, 0.0, 1.0, 1.0]),
    ] {
        let selected = prepared.resolve_composition(&guid(id)).unwrap();
        assert_eq!(selected.composition_id(), id);
        assert_eq!(selected.name(), "Hybrid_Duplicate_Name");
        assert!(std::ptr::eq(selected.source(), &prepared));
        let output = root.path().join(format!("selected-{id}"));
        let checked = selected
            .import_to_tesseract(&output, ConversionMode::Check)
            .unwrap();
        assert!(!output.exists());
        let ordinary = AfterEffects
            .import_to_tesseract(
                &input,
                &output,
                &AfterEffectsImportOptions {
                    composition: Some(id),
                    ..Default::default()
                },
                ConversionMode::Check,
            )
            .unwrap();
        assert_eq!(checked, ordinary);
        let written = selected
            .import_to_tesseract(&output, ConversionMode::Write)
            .unwrap();
        assert_eq!(checked, written);
        let archive = TesseractFile::open(output.join(OUTPUT_NAME)).unwrap();
        assert!(archive.metadata().assets.is_empty());
        let document = archive.project();
        assert_eq!(
            (document.dimensions().width, document.dimensions().height),
            (1920, 1080)
        );
        assert_eq!(document.duration().as_secs(), 2.0);
        assert_eq!(document.composition().layers().len(), 1);
        let selected_group = group(&document.composition().layers()[0]);
        assert_eq!(selected_group.name, "Hybrid_Duplicate_Name");
        assert_eq!(selected_group.layers.len(), 1);
        let occurrence = group(&selected_group.layers[0]);
        assert_eq!(occurrence.name, label);
        assert_eq!(
            occurrence.transform.position,
            Position::TwoD([960.0, 540.0])
        );
        assert_eq!(occurrence.transform.anchor_point, [960.0, 540.0]);
        assert_eq!(occurrence.layers.len(), 1);
        let content = group(&occurrence.layers[0]);
        assert_eq!(content.layers.len(), 1);
        let LayerData::Rect(rect) = content.layers[0].data() else {
            panic!("native solid must remain an editable Rect");
        };
        assert_eq!(rect.rect.size, [1920.0, 1080.0]);
        assert_eq!(rect.rect.fill_color, color);
        assert!(rect.rect.fill_enabled);
        assert!(!rect.rect.stroke_enabled);
    }
    assert_eq!(fs::read(input).unwrap(), SOURCE);
}

#[test]
fn unknown_guids_never_fall_back_to_name_root_or_encounter_order() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, SOURCE).unwrap();
    let prepared = AfterEffects.prepare_linked_import(&input).unwrap();
    let footage = prepared
        .project
        .items
        .iter()
        .find(|item| matches!(item.kind, ItemKind::Footage))
        .unwrap();
    for id in [u32::MAX, 0x1000_0000, footage.id] {
        assert!(
            matches!(prepared.resolve_composition(&guid(id)), Err(DynamicLinkImportError::MissingComposition(actual)) if actual == id)
        );
    }
    let mut suffix = guid(16);
    suffix[15] = 1;
    for unknown in [guid(0), suffix] {
        assert!(matches!(
            prepared.resolve_composition(&unknown),
            Err(DynamicLinkImportError::UnsupportedGuid)
        ));
    }
    for _ in 0..2 {
        assert_eq!(
            prepared
                .resolve_composition(&guid(16))
                .unwrap()
                .composition_id(),
            16
        );
    }
}

#[test]
fn unknown_native_header_profiles_are_not_generalized() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let at = SOURCE
        .windows(8)
        .position(|v| v == b"head\0\0\0\x14")
        .unwrap()
        + 8;
    for offset in [1, 3, 7] {
        let mut changed = SOURCE.to_vec();
        changed[at + offset] ^= 1;
        fs::write(&input, changed).unwrap();
        assert!(matches!(
            AfterEffects.prepare_linked_import(&input),
            Err(DynamicLinkImportError::UnsupportedProfile { .. })
        ));
    }
}

#[test]
fn malformed_and_duplicate_native_identities_are_rejected_before_selection() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let mut duplicate = SOURCE.to_vec();
    let at = SOURCE
        .windows(28)
        .position(|v| &v[..8] == b"idta\0\0\0\x54" && v[24..28] == 16_u32.to_be_bytes())
        .unwrap();
    duplicate[at + 24..at + 28].copy_from_slice(&1_u32.to_be_bytes());
    for bytes in [duplicate, SOURCE[..20].to_vec()] {
        fs::write(&input, bytes).unwrap();
        assert!(matches!(
            AfterEffects.prepare_linked_import(&input),
            Err(DynamicLinkImportError::Input(AepConversionError::Read(_)))
        ));
    }
}

#[test]
fn same_guid_in_another_file_does_not_transfer_source_ownership() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.aep");
    let second = root.path().join("second.aep");
    fs::write(&first, SOURCE).unwrap();
    fs::write(&second, SOURCE).unwrap();
    let a = AfterEffects.prepare_linked_import(&first).unwrap();
    let b = AfterEffects.prepare_linked_import(&second).unwrap();
    fs::write(&first, b"changed after resolution").unwrap();
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = root.path().join("out");
        assert!(
            matches!(a.resolve_composition(&guid(16)).unwrap().import_to_tesseract(&output, mode), Err(AepConversionError::SourceChanged(path)) if path == first)
        );
        assert!(!output.exists());
    }
    b.resolve_composition(&guid(16))
        .unwrap()
        .import_to_tesseract(&root.path().join("other"), ConversionMode::Check)
        .unwrap();
    assert_ne!(a.source_path(), b.source_path());
    assert_eq!(a.source_sha256(), b.source_sha256());
}

#[test]
fn source_drift_after_archive_staging_prevents_publication_and_cleans_owned_temp() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let output = root.path().join("out");
    fs::write(&input, SOURCE).unwrap();
    let prepared = AfterEffects.prepare_linked_import(&input).unwrap();
    let imported = import_builder(
        &input,
        &prepared.project,
        Some(16),
        &ExpressionSamples::default(),
    )
    .unwrap();
    let result = write_project_checked(imported.builder, &output, || {
        // Deterministic injection at the real pre-publication seam, not a race.
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        assert!(!output.exists());
        fs::write(&input, b"source replaced during import").unwrap();
        prepared.verify_source()
    });
    assert!(matches!(result, Err(AepConversionError::SourceChanged(_))));
    assert!(!output.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn existing_destinations_are_not_replaced_by_linked_import() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let output = root.path().join("out");
    fs::write(&input, SOURCE).unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(output.join("user.txt"), b"keep").unwrap();
    let prepared = AfterEffects.prepare_linked_import(&input).unwrap();
    let selected = prepared.resolve_composition(&guid(1)).unwrap();
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        assert!(selected.import_to_tesseract(&output, mode).is_err());
    }
    assert_eq!(fs::read(output.join("user.txt")).unwrap(), b"keep");
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
}

#[test]
fn native_ae26_3_header_profile_is_accepted_and_its_neighbors_are_not() {
    // Supplementary: an unchanged public AE 26.3x87 file shares the private
    // Dynamic Link package's header profile. It accepts the profile only; the
    // GUID-to-item evidence for that producer is the private native record.
    const AE26_3: &[u8] = include_bytes!("../../../tests/fixtures/ae26_one_comp.aep");
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, AE26_3).unwrap();
    let prepared = AfterEffects.prepare_linked_import(&input).unwrap();
    assert_eq!(
        prepared.resolve_composition(&guid(1)).unwrap().name(),
        "classic-3d"
    );
    let at = AE26_3
        .windows(8)
        .position(|v| v == b"head\0\0\0\x14")
        .unwrap()
        + 8;
    assert_eq!(&AE26_3[at..at + 8], &[0, 97, 0, 7, 0x0f, 0x91, 0x86, 0x57]);
    for offset in [3, 5, 7] {
        let mut changed = AE26_3.to_vec();
        changed[at + offset] ^= 1;
        fs::write(&input, changed).unwrap();
        assert!(matches!(
            AfterEffects.prepare_linked_import(&input),
            Err(DynamicLinkImportError::UnsupportedProfile { .. })
        ));
    }
}

#[test]
fn accepted_profiles_match_only_as_whole_pairs_and_keep_the_guid_layout() {
    // Mixing any observed header with another producer is unproven. Admission
    // covers whole native pairs, never a Cartesian product of their fields.
    const AE26_3: &[u8] = include_bytes!("../../../tests/fixtures/ae26_one_comp.aep");
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let profiles = [
        (SOURCE, 0x0f92_8659_u32),
        (AE26_3, 0x0f91_8657),
        (FORMAT96, 0x0f8a_0656),
    ];
    for (source, producer) in profiles {
        let at = source
            .windows(8)
            .position(|v| v == b"head\0\0\0\x14")
            .unwrap()
            + 8;
        for (_, other_producer) in profiles {
            if producer == other_producer {
                continue;
            }
            let mut mixed = source.to_vec();
            mixed[at + 4..at + 8].copy_from_slice(&other_producer.to_be_bytes());
            fs::write(&input, mixed).unwrap();
            assert!(matches!(
                AfterEffects.prepare_linked_import(&input),
                Err(DynamicLinkImportError::UnsupportedProfile { .. })
            ));
        }
        let mut revision = source.to_vec();
        revision[at + 1] ^= 1;
        fs::write(&input, revision).unwrap();
        assert!(matches!(
            AfterEffects.prepare_linked_import(&input),
            Err(DynamicLinkImportError::UnsupportedProfile { .. })
        ));
    }
    fs::write(&input, AE26_3).unwrap();
    let prepared = AfterEffects.prepare_linked_import(&input).unwrap();
    let mut suffix = guid(1);
    suffix[15] = 1;
    for unknown in [guid(0), suffix] {
        assert!(matches!(
            prepared.resolve_composition(&unknown),
            Err(DynamicLinkImportError::UnsupportedGuid)
        ));
    }
    assert!(matches!(
        prepared.resolve_composition(&guid(2)),
        Err(DynamicLinkImportError::MissingComposition(2))
    ));
}

/// An unchanged AE 26.5x89 fixture, read in place: its WAV alias relinks by
/// its native relative location to the tracked Premiere fixture.
fn audio_media_controls() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/media/import_audio_media_controls.aep")
}

fn target(parent: u64, first_id: u64) -> LinkedPictureTarget<'static> {
    LinkedPictureTarget {
        parent: LayerId::new(parent),
        first_id,
        asset_namespace: "host-aep-1",
    }
}

/// Every numeric identity in `value`, a layer's wire form.
fn numeric_ids(value: &serde_json::Value, ids: &mut Vec<u64>) {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, field) in fields {
                match (name.as_str(), field.as_u64()) {
                    ("id", Some(id)) => ids.push(id),
                    _ => numeric_ids(field, ids),
                }
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| numeric_ids(item, ids)),
        _ => {}
    }
}

/// Every string identity in `value`: keyframe ids of animation or playback.
fn keyframe_ids(value: &serde_json::Value, ids: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, field) in fields {
                match (name.as_str(), field.as_str()) {
                    ("id", Some(id)) => ids.push(id.to_owned()),
                    _ => keyframe_ids(field, ids),
                }
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| keyframe_ids(item, ids)),
        _ => {}
    }
}

fn layers<'a>(layer: &'a Layer, output: &mut Vec<&'a Layer>) {
    output.push(layer);
    for child in layer.child_layers().into_iter().flatten() {
        layers(child, output);
    }
}

/// A host document whose Groups own `pictures`, packaged with `media`.
fn host_archive(pictures: Vec<LinkedPicture>, media: &LinkedMedia, output: &Path) -> TesseractFile {
    let mut hosts = Vec::new();
    let mut entries = Vec::new();
    for picture in pictures {
        let mut host = picture.root.wire_value().clone();
        let fields = host.as_object_mut().unwrap();
        fields.remove("parent");
        fields.remove("description");
        fields.insert(
            "id".into(),
            picture.root.parent_id().unwrap().value().into(),
        );
        fields.insert("name".into(), "host placement".into());
        fields.insert(
            "layers".into(),
            vec![picture.root.wire_value().clone()].into(),
        );
        hosts.push(serde_json::from_value::<Layer>(host).unwrap());
        entries.extend(picture.animations);
    }
    let composition = fx_schema::FXComposition::try_from_parts(
        fx_schema::CompositionId::new("host"),
        "host",
        fx_schema::AnimationGraph::from_entries(entries).unwrap(),
        hosts,
    )
    .unwrap();
    let document = fx_schema::EditableFxCompositionDocument::new(
        fx_schema::Dimensions::new(1920, 1080),
        fx_schema::Duration::from_secs(3.0),
        None,
        composition,
    )
    .unwrap();
    let builder = media
        .add_to(TesseractFileBuilder::try_new(document).unwrap())
        .unwrap();
    builder.validate().unwrap();
    builder.write(output).unwrap()
}

#[test]
fn linked_picture_is_muted_and_uses_only_the_callers_identities() {
    let prepared = AfterEffects
        .prepare_linked_import(&audio_media_controls())
        .unwrap();
    let mut media = LinkedMedia::default();
    let composition = prepared.resolve_composition(&guid(63)).unwrap();
    assert_eq!(composition.name(), "AUDIO_GAIN_KEYED");
    assert_eq!(composition.dimensions(), [1920, 1080]);
    let picture = composition
        .import_picture(target(7, 1000), &mut media)
        .unwrap();
    assert_eq!(picture.root.parent_id(), Some(LayerId::new(7)));
    let mut ids = Vec::new();
    numeric_ids(picture.root.wire_value(), &mut ids);
    assert!(!ids.is_empty());
    assert!(
        ids.iter().all(|id| (1000..picture.next_id).contains(id)),
        "{ids:?} outside 1000..{}",
        picture.next_id
    );
    let mut all = Vec::new();
    layers(&picture.root, &mut all);
    let sounds: Vec<_> = all
        .iter()
        .filter_map(|layer| match layer.data() {
            LayerData::Audio(audio) => Some(audio),
            _ => None,
        })
        .collect();
    assert!(!sounds.is_empty(), "the composition has an audio layer");
    assert!(sounds.iter().all(|audio| audio.is_hidden));
    assert!(
        sounds
            .iter()
            .all(|audio| audio.source.asset_id.as_str() == "host-aep-1-item-1")
    );
    assert!(picture.animations.iter().all(|entry| {
        entry
            .target
            .as_property()
            .is_none_or(|property| property.property_type() != fx_schema::PropType::AudioVolume)
    }));
    assert!(picture.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("preview background is not materialized")
    }));
    // The keyed Audio Levels that a document keeps are discarded by muting.
    let standalone = AfterEffects
        .import_to_tesseract(
            &audio_media_controls(),
            &tempfile::tempdir().unwrap().path().join("check"),
            &AfterEffectsImportOptions {
                composition: Some(63),
                ..Default::default()
            },
            ConversionMode::Check,
        )
        .unwrap();
    assert!(!standalone.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("preview background is not materialized")
    }));
    // The WAV that the native relative location relinked is packaged once.
    let root = tempfile::tempdir().unwrap();
    let archive = host_archive(vec![picture], &media, &root.path().join("host.tsrct"));
    let wav = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../premiere_file/tests/fixtures/audio-stereo.wav");
    assert_eq!(archive.metadata().assets.len(), 1);
    assert_eq!(
        archive
            .asset("host-aep-1-item-1")
            .unwrap()
            .descriptor()
            .sha256,
        format!("{:x}", Sha256::digest(fs::read(wav).unwrap()))
    );
}

#[test]
fn repeated_pictures_of_one_composition_have_disjoint_identities() {
    const MOTION_BLUR: &[u8] =
        include_bytes!("../../../tests/fixtures/compositing/import_motion_blur_cases.aep");
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, MOTION_BLUR).unwrap();
    let prepared = AfterEffects.prepare_linked_import(&input).unwrap();
    let composition = prepared.resolve_composition(&guid(34)).unwrap();
    let mut media = LinkedMedia::default();
    let first = composition
        .import_picture(target(3, 50), &mut media)
        .unwrap();
    let second = composition
        .import_picture(target(4, first.next_id), &mut media)
        .unwrap();
    let mut first_ids = Vec::new();
    numeric_ids(first.root.wire_value(), &mut first_ids);
    let mut second_ids = Vec::new();
    numeric_ids(second.root.wire_value(), &mut second_ids);
    assert_eq!(first_ids.len(), second_ids.len());
    assert!(first_ids.iter().all(|id| (50..first.next_id).contains(id)));
    assert!(
        second_ids
            .iter()
            .all(|id| (first.next_id..second.next_id).contains(id))
    );
    let keys = |picture: &LinkedPicture| {
        let mut ids = Vec::new();
        keyframe_ids(
            &serde_json::to_value(&picture.animations).unwrap(),
            &mut ids,
        );
        keyframe_ids(picture.root.wire_value(), &mut ids);
        ids
    };
    let (first_keys, second_keys) = (keys(&first), keys(&second));
    assert!(!first_keys.is_empty(), "the composition is animated");
    assert!(first_keys.iter().all(|id| !second_keys.contains(id)));
    // Both pictures and their animation form one valid host document.
    let archive = host_archive(vec![first, second], &media, &root.path().join("host.tsrct"));
    assert_eq!(archive.project().composition().layers().len(), 2);
    assert!(archive.metadata().assets.is_empty());
}

#[test]
fn a_linked_picture_takes_its_composition_motion_blur_switch() {
    const MOTION_BLUR: &[u8] =
        include_bytes!("../../../tests/fixtures/compositing/import_motion_blur_cases.aep");
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, MOTION_BLUR).unwrap();
    let prepared = AfterEffects.prepare_linked_import(&input).unwrap();
    let blurred = |picture: &LinkedPicture| {
        let mut all = Vec::new();
        layers(&picture.root, &mut all);
        all.iter()
            .any(|layer| matches!(layer.data(), LayerData::Group(group) if group.motion_blur))
    };
    for (id, enabled) in [(1, false), (34, true)] {
        let picture = prepared
            .resolve_composition(&guid(id))
            .unwrap()
            .import_picture(target(1, 10), &mut LinkedMedia::default())
            .unwrap();
        // COMP_OFF blurs no layer in AE, although its layer switch is on.
        assert_eq!(picture.motion_blur.enabled, enabled, "{id}");
        assert_eq!(blurred(&picture), enabled, "{id}");
    }
}

#[test]
fn a_linked_asset_namespace_must_be_a_flat_asset_id() {
    let prepared = AfterEffects
        .prepare_linked_import(&audio_media_controls())
        .unwrap();
    let composition = prepared.resolve_composition(&guid(2)).unwrap();
    for namespace in ["", "a/b", ".."] {
        let target = LinkedPictureTarget {
            asset_namespace: namespace,
            ..target(1, 10)
        };
        assert!(matches!(
            composition.import_picture(target, &mut LinkedMedia::default()),
            Err(AepConversionError::Input(_))
        ));
    }
}

#[test]
fn a_linked_picture_s_normalized_media_lives_until_its_host_archive_is_written() {
    use crate::{aep, rifx::Chunk};
    fn relink(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: serde_json::Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = "two_layers.psd".into();
                *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children);
            }
        }
    }
    // The Adobe-authored PSD source (AE 26.5x89), relinked to its pinned PSD.
    let mut native = aep::Project::parse(include_bytes!(
        "../../../tests/fixtures/psd_import/psd_sources_v2.aep"
    ))
    .unwrap();
    relink(&mut native.chunks);
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, native.encode().unwrap()).unwrap();
    fs::write(
        root.path().join("two_layers.psd"),
        include_bytes!("../../../tests/fixtures/psd_import/two_layers_v2.psd"),
    )
    .unwrap();
    let prepared = AfterEffects.prepare_linked_import(&input).unwrap();
    let composition = prepared.resolve_composition(&guid(2)).unwrap();
    let mut media = LinkedMedia::default();
    let pictures = vec![
        composition
            .import_picture(target(1, 10), &mut media)
            .unwrap(),
        composition
            .import_picture(target(2, 100), &mut media)
            .unwrap(),
    ];
    // Both placements name one merged-PSD asset, a PNG that preflight wrote;
    // the second placement's own PNG backs nothing and is already deleted.
    assert_eq!(media.assets.len(), 1);
    let (_, normalized, kind) = media.assets.values().next().unwrap().clone();
    assert_eq!(kind, AssetKind::Image);
    assert!(normalized.exists());
    assert!(!normalized.starts_with(root.path()));
    assert_eq!(
        media
            .normalized
            .iter()
            .map(|file| file.path())
            .collect::<Vec<_>>(),
        [normalized.as_path()]
    );
    let archive = host_archive(pictures, &media, &root.path().join("host.tsrct"));
    let asset = archive.asset("host-aep-1-item-1").unwrap();
    assert_eq!(asset.descriptor().content_type, "image/png");
    assert!(
        normalized.exists(),
        "the kept PNG outlives the archive write"
    );
    // The packaged PNG is no source; the PSD that it was decoded from is.
    media.verify_packaged(&archive).unwrap();
    media.verify_sources().unwrap();
    let psd = root.path().join("two_layers.psd");
    let mut bytes = fs::read(&psd).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&psd, bytes).unwrap();
    assert!(matches!(
        media.verify_sources(),
        Err(AepConversionError::MediaChanged(path)) if path == psd
    ));
    drop(media);
    assert!(
        !normalized.exists(),
        "dropping the media releases its normalized files"
    );
}

/// `import_audio_media_controls.aep` (AE 26.5x89) in `directory`, its one WAV
/// footage relinked to `source.wav` beside it, which holds the pinned WAV.
fn relinked_audio_controls(directory: &Path) -> (PathBuf, PathBuf) {
    use crate::{aep, rifx::Chunk};
    fn relink(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: serde_json::Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = "source.wav".into();
                *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children);
            }
        }
    }
    let mut native = aep::Project::parse(&fs::read(audio_media_controls()).unwrap()).unwrap();
    relink(&mut native.chunks);
    let input = directory.join("controls.aep");
    fs::write(&input, native.encode().unwrap()).unwrap();
    let wav = directory.join("source.wav");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../premiere_file/tests/fixtures/audio-stereo.wav"),
        &wav,
    )
    .unwrap();
    (input, wav)
}

#[test]
fn linked_footage_bytes_are_verified_at_the_source_and_in_the_archive() {
    let root = tempfile::tempdir().unwrap();
    let (input, wav) = relinked_audio_controls(root.path());
    let prepared = AfterEffects.prepare_linked_import(&input).unwrap();
    let mut media = LinkedMedia::default();
    let picture = prepared
        .resolve_composition(&guid(2))
        .unwrap()
        .import_picture(target(1, 10), &mut media)
        .unwrap();
    media.verify_sources().unwrap();
    let archive = host_archive(vec![picture], &media, &root.path().join("host.tsrct"));
    media.verify_packaged(&archive).unwrap();
    // Bytes of the same length pass the archive writer's own check.
    let mut bytes = fs::read(&wav).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&wav, &bytes).unwrap();
    assert!(matches!(
        media.verify_sources(),
        Err(AepConversionError::MediaChanged(path)) if path == wav
    ));
    let picture = prepared
        .resolve_composition(&guid(2))
        .unwrap()
        .import_picture(target(1, 10), &mut LinkedMedia::default())
        .unwrap();
    let changed = host_archive(vec![picture], &media, &root.path().join("changed.tsrct"));
    assert!(matches!(
        media.verify_packaged(&changed),
        Err(AepConversionError::MediaChanged(path)) if path == wav
    ));
}

/// A footage request for `authored`, as the AE importer makes it.
fn footage_request(authored: &Path, kind: MediaAssetKind) -> MediaAssetRequest {
    MediaAssetRequest {
        logical_id: AssetId::new("host-aep-1-item-1").unwrap(),
        source_item_id: 1,
        authored_path: authored.to_str().unwrap().into(),
        relative_location: None,
        relative_hint_malformed: false,
        kind,
        photoshop_source: None,
        dimensions: [0, 0],
    }
}

#[test]
fn a_relinked_source_changes_when_its_authored_path_reappears() {
    // `<package>/project.aep` authored `<old>/media/a.wav`; AE relinks it at
    // `<package>/media/a.wav` only while the authored file is missing.
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("package");
    fs::create_dir_all(package.join("media")).unwrap();
    fs::write(package.join("media/a.wav"), b"moved bytes").unwrap();
    let input = package.join("project.aep");
    let authored = directory.path().join("old/media/a.wav");
    let request = MediaAssetRequest {
        relative_location: crate::alias::RelativeLocation::new(1, 2),
        ..footage_request(&authored, MediaAssetKind::Audio)
    };
    let mut preflight = media::MediaPreflight::new(&input);
    assert!(preflight.available(&request));
    let mut linked = LinkedMedia::default();
    linked
        .record(preflight, std::slice::from_ref(&request))
        .unwrap();
    assert_eq!(linked.relocated, BTreeSet::from([authored.clone()]));
    linked.verify_sources().unwrap();
    fs::create_dir_all(authored.parent().unwrap()).unwrap();
    fs::write(&authored, b"moved bytes").unwrap();
    assert!(matches!(
        linked.verify_sources(),
        Err(AepConversionError::MediaChanged(path)) if path == authored
    ));
}

#[test]
fn lowered_vector_sources_are_verified_although_no_asset_packages_them() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let artwork = directory.path().join("artwork.ai");
    fs::write(
        &artwork,
        include_bytes!("../../../tests/fixtures/vector_media/spec_case_1.ai"),
    )
    .unwrap();
    let request = footage_request(Path::new("artwork.ai"), MediaAssetKind::Image);
    let mut preflight = media::MediaPreflight::new(&input);
    assert!(matches!(
        preflight.resolve_media(&request),
        crate::structure_document::MediaResolution::Vector(_)
    ));
    let mut linked = LinkedMedia::default();
    linked.record(preflight, &[]).unwrap();
    assert!(linked.assets.is_empty());
    assert_eq!(linked.sources.keys().collect::<Vec<_>>(), [&artwork]);
    linked.verify_sources().unwrap();
    let mut bytes = fs::read(&artwork).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&artwork, bytes).unwrap();
    assert!(matches!(
        linked.verify_sources(),
        Err(AepConversionError::MediaChanged(path)) if path == artwork
    ));
}
