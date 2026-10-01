//! Linked After Effects compositions import as editable pictures.
//!
//! The Adobe-authored H-IDENTITY-01 package is the native source. Typed
//! sequences over pinned AEPs are supplementary coverage of the placement,
//! identity, asset, audio and diagnostic rules; none is render proof.

use crate::{
    format::{FrameRate, MediaId, PrMedia, PrSequence},
    schema::{
        after_effects::LINKED_AUDIO_REASON, AudioChannels, PrAfterEffectsComposition,
        PrAudioOccurrence, PrAudioStream, PrMediaKind, PrVideoStream, PrVideoTrack, TICKS,
    },
    tesseract_import::TesseractImport,
    tesseract_output::convert_premiere_sequence,
    tests::support::{clip_of, nest_of, sequence_of},
    Omission, OmissionScope,
};
use fx_schema::LinearGain;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tesseract_file::TesseractFile;

const IDENTITY: &str = "../aftereffects_file/tests/fixtures/hybrid/identity";
const RED: &str = "00000001-0000-0000-0000-000000000000";
const BLUE: &str = "00000010-0000-0000-0000-000000000000";
/// `AUDIO_UNITY` of `import_audio_media_controls.aep`, which packages a WAV.
const UNITY: &str = "00000002-0000-0000-0000-000000000000";

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// The native identity package, staged with its one ordinary media file.
fn identity_package(root: &Path) -> PathBuf {
    fn relink(xml: &mut String, tag: &str, file: &Path) -> usize {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let replacement = quick_xml::escape::escape(file.to_str().unwrap());
        let mut count = 0;
        let mut offset = 0;
        while let Some(start) = xml[offset..]
            .find(&open)
            .map(|index| offset + index + open.len())
        {
            let end = start + xml[start..].find(&close).unwrap();
            if Path::new(&xml[start..end]).file_name() == file.file_name() {
                xml.replace_range(start..end, &replacement);
                offset = start + replacement.len() + close.len();
                count += 1;
            } else {
                offset = end + close.len();
            }
        }
        count
    }

    let package = root.join("package");
    fs::create_dir_all(&package).unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        package.join("linked-compositions.aep"),
    )
    .unwrap();
    fs::copy(
        fixture("tests/fixtures/feature_linked_av_source.mp4"),
        package.join("background.mp4"),
    )
    .unwrap();

    // The Adobe-authored project retains absolute author-host hints. Relink only
    // the temporary copy so source selection cannot escape this test package.
    let mut xml = String::new();
    std::io::Read::read_to_string(
        &mut flate2::read::GzDecoder::new(
            fs::File::open(fixture(IDENTITY).join("native-linked.prproj")).unwrap(),
        ),
        &mut xml,
    )
    .unwrap();
    for (file, expected) in [
        (package.join("linked-compositions.aep"), 2),
        (package.join("background.mp4"), 1),
    ] {
        for tag in ["ActualMediaFilePath", "FilePath"] {
            assert_eq!(
                relink(&mut xml, tag, &file),
                expected,
                "native fixture changed its {tag} hints for {}",
                file.display()
            );
        }
    }
    let project = package.join("native-linked.prproj");
    crate::test_support::write_prproj(&project, &xml);
    project
}

/// Linked media of the composition `guid` in the package file `aep`.
fn linked(aep: &str, guid: &str, canvas: [u32; 2]) -> PrMedia {
    PrMedia {
        name: aep.into(),
        relative_path: Some(format!("./{aep}")),
        relative_paths: vec![format!("./{aep}")],
        absolute_paths: Vec::new(),
        video: Some(PrVideoStream {
            orientation: crate::schema::VideoOrientation::Identity,
            kind: PrMediaKind::AfterEffectsComposition(
                PrAfterEffectsComposition::parse(guid).unwrap(),
            ),
            intrinsic_ticks: 2 * TICKS,
            frame_rate: (FrameRate::Fps30).into(),
            width: canvas[0],
            height: canvas[1],
        }),
        audio: None,
    }
}

/// Converts `sequence` from a project in `package` and writes its archive.
fn convert(
    package: &Path,
    sequence: PrSequence,
    media: BTreeMap<MediaId, PrMedia>,
) -> (Value, Vec<Omission>) {
    let mut omissions = Vec::new();
    // Import resolves its project path first, as `TesseractImport` does.
    let package = fs::canonicalize(package).unwrap();
    let pending = convert_premiere_sequence(
        &package.join("project.prproj"),
        sequence,
        Arc::new(media),
        &mut omissions,
    )
    .unwrap()
    .unwrap();
    let archive = package.join("converted.tsrct");
    pending.write_to_staging(&archive).unwrap();
    let document = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();
    (document, omissions)
}

fn layers<'a>(layers: &'a Value, output: &mut Vec<&'a Value>) {
    for layer in layers.as_array().into_iter().flatten() {
        output.push(layer);
        self::layers(&layer["layers"], output);
    }
}

fn all_layers(value: &Value) -> Vec<&Value> {
    let mut output = Vec::new();
    layers(&value["layers"], &mut output);
    output
}

/// The picture groups of linked clips, in document order.
fn linked_groups(document: &Value) -> Vec<&Value> {
    all_layers(&document["composition"])
        .into_iter()
        .filter(|layer| {
            layer["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("Premiere linked composition"))
        })
        .collect()
}

/// The guide that clips a linked composition's root to its canvas.
const CANVAS_GUIDE: &str = "Linked composition canvas";

/// The fills of the composition content's rects under `group`; the canvas
/// guide never paints ([`assert_canvas_clip`]).
fn rect_fills(group: &Value) -> Vec<&Value> {
    all_layers(group)
        .into_iter()
        .filter(|layer| layer["type"] == "Rect" && layer["name"] != CANVAS_GUIDE)
        .map(|layer| &layer["rect"]["fillColor"])
        .collect()
}

/// Every numeric identity of a layer, effect or mask under `group`.
fn identities(group: &Value) -> BTreeSet<u64> {
    let mut ids = BTreeSet::new();
    for layer in std::iter::once(group).chain(all_layers(group)) {
        let items = ["effects", "masks"]
            .iter()
            .flat_map(|field| layer[*field].as_array().into_iter().flatten());
        for id in std::iter::once(&layer["id"]).chain(items.map(|item| &item["id"])) {
            if let Some(id) = id.as_u64() {
                assert!(ids.insert(id), "{id} repeats under {}", group["id"]);
            }
        }
    }
    ids
}

/// The FX engine requires unique layer, item, effect and keyframe ids.
fn assert_unique_identities(document: &Value) {
    let composition = &document["composition"];
    let mut ids = BTreeSet::new();
    for layer in composition["layers"].as_array().unwrap() {
        let subtree = identities(layer);
        assert!(ids.is_disjoint(&subtree), "{subtree:?}");
        ids.extend(subtree);
    }
    let mut keys = BTreeSet::new();
    for entry in composition["dynamics"]["entries"]
        .as_array()
        .into_iter()
        .flatten()
    {
        for key in entry["animator"]["keyframes"]
            .as_array()
            .into_iter()
            .flatten()
        {
            assert!(keys.insert(key["id"].as_str().unwrap().to_owned()), "{key}");
        }
    }
}

fn playback(group: &Value) -> Vec<(u64, u64)> {
    serde_json::Value::Array(crate::tests::support::playback_keys(group))
        .as_array()
        .into_iter()
        .flatten()
        .map(|key| {
            (
                key["time"].as_u64().unwrap(),
                key["value"].as_u64().unwrap(),
            )
        })
        .collect()
}

fn range(layer: &Value) -> (u64, u64) {
    (
        (*crate::test_support::layer_range(layer))["start"]
            .as_u64()
            .unwrap(),
        (*crate::test_support::layer_range(layer))["duration"]
            .as_u64()
            .unwrap(),
    )
}

/// The source group of linked clip `group`, which plays the composition on
/// its clock when the clip needs one; without it, the clip group holds the
/// composition's clipped root and its canvas guide itself.
fn source_group(group: &Value) -> Option<&Value> {
    match group["layers"].as_array().unwrap().as_slice() {
        [child] => {
            assert!(
                child["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("Premiere linked source")),
                "{child}"
            );
            Some(child)
        }
        [_, guide] => {
            assert_eq!(guide["name"], CANVAS_GUIDE, "{group}");
            None
        }
        children => panic!("{} children under {}", children.len(), group["id"]),
    }
}

/// Asserts that the composition root of linked clip `group` is clipped to
/// its `canvas`, as After Effects renders a composition: a canvas rect at the
/// origin of the root's own frame, beside the root and over its range, whose
/// shape the root's one Add mask takes. The clip's Motion and masks stay on
/// the groups above.
fn assert_canvas_clip(group: &Value, canvas: [f64; 2]) {
    let parent = source_group(group).unwrap_or(group);
    let [root, guide] = parent["layers"].as_array().unwrap().as_slice() else {
        panic!("{parent}");
    };
    assert_eq!(guide["type"], "Rect");
    assert_eq!(guide["name"], CANVAS_GUIDE);
    assert_eq!(guide["parent"], parent["id"]);
    assert_eq!(
        (*crate::test_support::layer_range(guide)),
        (*crate::test_support::layer_range(root))
    );
    assert_eq!(guide["rect"]["size"], serde_json::json!(canvas));
    assert_eq!(guide["rect"]["position"], serde_json::json!([0.0, 0.0]));
    assert_eq!(
        guide["transform"]["position"],
        serde_json::json!([0.0, 0.0])
    );
    assert_eq!(
        guide["transform"]["anchorPoint"],
        serde_json::json!([0.0, 0.0])
    );
    assert_eq!(
        guide["transform"]["scale"],
        serde_json::json!([100.0, 100.0])
    );
    assert_eq!(guide["transform"]["rotation"], 0.0);
    let [mask] = root["masks"].as_array().unwrap().as_slice() else {
        panic!("{root}");
    };
    assert_eq!(mask["layer"], guide["id"]);
    assert_eq!(mask["mode"], "add");
    assert_eq!(mask["inverted"], false);
}

/// The identity-rate playback that seeds linked clip `group`'s clock on the
/// document clock over its `range`: its start to zero and its end to its
/// duration.
fn document_clock_seed(group: &Value) -> [(u64, u64); 2] {
    let (start, duration) = range(group);
    [(start, 0), (start + duration, duration)]
}

/// The approximation of clip `record` whose `subject`, a linked composition
/// under a later-starting stage group or nest, has time-remapped content that
/// cannot keep its runtime timing.
fn offset_clock_approximation(record: &str, subject: &str) -> Omission {
    Omission {
        scope: OmissionScope::Feature,
        kind: crate::OmissionKind::Approximated,
        record: record.into(),
        reason: format!("{subject} under a stage group or nest that starts after the document start: the FX runtime evaluates its time-remapped After Effects animation on the document clock, early by that start; placement, visibility and media times are kept"),
    }
}

/// The composition's editable root under linked clip `group`.
fn composition_root(group: &Value) -> &Value {
    let parent = source_group(group).unwrap_or(group);
    let root = &parent["layers"][0];
    assert_eq!(root["parent"], parent["id"]);
    root
}

#[test]
fn native_links_import_editable_same_name_compositions_by_exact_guid() {
    let root = tempfile::tempdir().unwrap();
    let project = identity_package(root.path());
    let output = root.path().join("converted");
    let checked = crate::premiere_to_tesseract(&project, &output, None, true).unwrap();
    assert!(!output.exists(), "check mode publishes nothing");
    let written = crate::premiere_to_tesseract(&project, &output, None, false).unwrap();
    assert_eq!(checked, written);
    assert!(!written
        .iter()
        .any(|omission| omission.scope == OmissionScope::Occurrence));
    let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
    // The AEP is not a decoded video asset; its solids have no media.
    assert_eq!(
        archive.metadata().assets.keys().collect::<Vec<_>>(),
        ["premiere-video-1"]
    );
    let document = archive.project_json().unwrap();
    assert_unique_identities(&document);
    let groups = linked_groups(&document);
    assert_eq!(groups.len(), 2);
    let (red, blue) = (groups[0], groups[1]);
    // Premiere placed red at 0-1 s from source 0 and blue at 1-2 s from 0.5 s:
    // blue's source group plays the composition from 0.5 s on its clip clock.
    assert_eq!(range(red), (0, 1000));
    assert!(source_group(red).is_none());
    assert_eq!(range(blue), (1000, 1000));
    for group in [red, blue] {
        assert_eq!(playback(group), document_clock_seed(group));
    }
    let source = source_group(blue).unwrap();
    assert_eq!(range(source), (0, 1000));
    assert_eq!(playback(source), [(0, 500), (1000, 1500)]);
    assert_eq!(source["parent"], blue["id"]);
    for (group, fill) in [(red, [1.0, 0.0, 0.0, 1.0]), (blue, [0.0, 0.0, 1.0, 1.0])] {
        let root = composition_root(group);
        assert_eq!(root["type"], "Group");
        assert_eq!(root["name"], "Hybrid_Duplicate_Name");
        assert_canvas_clip(group, [1920.0, 1080.0]);
        assert_eq!(rect_fills(group), [&serde_json::json!(fill)]);
        assert_eq!(
            group["transform"]["anchorPoint"],
            serde_json::json!([960.0, 540.0])
        );
        assert_eq!(
            group["transform"]["position"],
            serde_json::json!([960.0, 540.0])
        );
    }
}

#[test]
fn repeated_and_nested_placements_each_own_identities_and_clocks() {
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        root.path().join("comps.aep"),
    )
    .unwrap();
    let inner = sequence_of(
        "Inner",
        vec![PrVideoTrack::media([clip_of("blue", 0..2 * TICKS, 0)])],
    );
    let sequence = sequence_of(
        "Outer",
        vec![
            PrVideoTrack::media([
                clip_of("blue", 0..TICKS, TICKS / 2),
                clip_of("blue", TICKS..2 * TICKS, 0),
            ]),
            PrVideoTrack {
                items: Vec::new(),
                nests: vec![nest_of(inner, 2 * TICKS..3 * TICKS, TICKS)],
                transitions: Vec::new(),
            },
        ],
    );
    let media = BTreeMap::from([(
        MediaId("blue".into()),
        linked("comps.aep", BLUE, [1920, 1080]),
    )]);
    let (document, omissions) = convert(root.path(), sequence, media);
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence),
        "{omissions:?}"
    );
    // Only the nested copy, whose nest starts at 2 s and whose source group
    // remaps it, is reported with its runtime timing approximation.
    assert_eq!(
        omissions
            .iter()
            .filter(
                |omission| **omission == offset_clock_approximation("blue", "linked composition")
            )
            .count(),
        1,
        "{omissions:?}"
    );
    assert_unique_identities(&document);
    let nest = all_layers(&document["composition"])
        .into_iter()
        .find(|layer| layer["name"] == "Inner")
        .unwrap();
    assert_eq!(range(nest), (2000, 1000));
    let groups = linked_groups(&document);
    assert_eq!(groups.len(), 3);
    let (nested, placed): (Vec<&Value>, Vec<&Value>) = groups
        .iter()
        .copied()
        .partition(|group| group["parent"] == nest["id"]);
    let [nested] = nested.as_slice() else {
        panic!("one copy plays inside the nest: {nested:?}");
    };
    let [trimmed, untrimmed] = placed.as_slice() else {
        panic!("two copies play at the root: {placed:?}");
    };
    // Each placement owns its composition copy on its own clock; a root
    // placement seeds the runtime clock, the nested copy cannot.
    for group in [trimmed, untrimmed] {
        assert_eq!(playback(group), document_clock_seed(group));
    }
    assert_eq!(
        nested["playback"],
        crate::test_support::linear_playback(
            crate::test_support::layer_range(nested).clone(),
            serde_json::json!({"start": 0, "duration": 1000})
        )
    );
    assert_eq!(range(trimmed), (0, 1000));
    assert_eq!(
        playback(source_group(trimmed).unwrap()),
        [(0, 500), (1000, 1500)]
    );
    assert_eq!(range(untrimmed), (1000, 1000));
    assert!(source_group(untrimmed).is_none());
    // The nest shows inner 1-2 s at outer 2-3 s: its copy starts 1 s in.
    assert_eq!(range(nested), (0, 1000));
    assert_eq!(
        playback(source_group(nested).unwrap()),
        [(0, 1000), (1000, 2000)]
    );
    let copies: Vec<_> = groups.iter().map(|group| identities(group)).collect();
    assert_eq!(copies[0].len(), copies[1].len());
    assert!(copies[0].is_disjoint(&copies[1]) && copies[1].is_disjoint(&copies[2]));
    assert!(copies[0].is_disjoint(&copies[2]));
    for group in groups {
        assert_canvas_clip(group, [1920.0, 1080.0]);
        assert_eq!(
            rect_fills(group),
            [&serde_json::json!([0.0, 0.0, 1.0, 1.0])]
        );
    }
}

#[test]
fn equal_item_ids_of_two_aeps_select_each_file_s_own_composition() {
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        root.path().join("comps.aep"),
    )
    .unwrap();
    fs::copy(
        fixture("tests/fixtures/hybrid/native-title.aep"),
        root.path().join("title.aep"),
    )
    .unwrap();
    let sequence = sequence_of(
        "Two files",
        vec![PrVideoTrack::media([
            clip_of("red", 0..TICKS, 0),
            clip_of("title", TICKS..2 * TICKS, 0),
        ])],
    );
    // Both links name item 1, of different AEPs.
    let media = BTreeMap::from([
        (
            MediaId("red".into()),
            linked("comps.aep", RED, [1920, 1080]),
        ),
        (
            MediaId("title".into()),
            linked("title.aep", RED, [320, 180]),
        ),
    ]);
    let (document, omissions) = convert(root.path(), sequence, media);
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence),
        "{omissions:?}"
    );
    assert_unique_identities(&document);
    let groups = linked_groups(&document);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["layers"][0]["name"], "Hybrid_Duplicate_Name");
    assert_eq!(
        rect_fills(groups[0]),
        [&serde_json::json!([1.0, 0.0, 0.0, 1.0])]
    );
    assert_canvas_clip(groups[0], [1920.0, 1080.0]);
    assert_eq!(groups[1]["layers"][0]["name"], "Hybrid_Title_30fps");
    assert_canvas_clip(groups[1], [320.0, 180.0]);
    assert!(all_layers(groups[1])
        .iter()
        .any(|layer| layer["type"] == "Text"));
    // A 320x180 composition is centred as a clip of that size is.
    assert_eq!(
        groups[1]["transform"]["anchorPoint"],
        serde_json::json!([160.0, 90.0])
    );
}

/// A native AE 26.5 composition (AUDIO_UNITY, item 2) whose one WAV footage,
/// item 1, is relinked to the relative `wav`, which holds `bytes`.
fn audio_composition(directory: &Path, bytes: &[u8]) {
    use aftereffects_file::{aep::Project, rifx::Chunk};
    fn relink(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = "source.wav".into();
                *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children);
            }
        }
    }
    let mut native = Project::parse(
        &fs::read(fixture(
            "../aftereffects_file/tests/fixtures/media/import_audio_media_controls.aep",
        ))
        .unwrap(),
    )
    .unwrap();
    relink(&mut native.chunks);
    fs::create_dir_all(directory).unwrap();
    fs::write(directory.join("controls.aep"), native.encode().unwrap()).unwrap();
    fs::write(directory.join("source.wav"), bytes).unwrap();
}

#[test]
fn equal_footage_ids_of_two_aeps_package_distinct_muted_assets_and_linked_sound_is_omitted() {
    let root = tempfile::tempdir().unwrap();
    let wav = fs::read(fixture("tests/fixtures/audio-stereo.wav")).unwrap();
    let mut other = wav.clone();
    *other.last_mut().unwrap() ^= 1;
    audio_composition(&root.path().join("one"), &wav);
    audio_composition(&root.path().join("two"), &other);
    let mut sequence = sequence_of(
        "Two sounds",
        vec![PrVideoTrack::media([
            clip_of("one", 0..TICKS, 0),
            clip_of("two", TICKS..2 * TICKS, 0),
        ])],
    );
    // Premiere plays a link's sound only through its audio items.
    sequence.audio.push(PrAudioOccurrence {
        id: Some("sound of one".into()),
        media: MediaId("one".into()),
        start_ticks: 0,
        end_ticks: TICKS,
        in_ticks: 0,
        out_ticks: TICKS,
        volume: LinearGain::UNITY,
        volume_keys: None,
    });
    let mut one = linked("one/controls.aep", UNITY, [1920, 1080]);
    one.audio = Some(PrAudioStream {
        intrinsic_ticks: 2 * TICKS,
        channels: AudioChannels::Stereo,
        sample_rate: 48_000,
    });
    let media = BTreeMap::from([
        (MediaId("one".into()), one),
        (
            MediaId("two".into()),
            linked("two/controls.aep", UNITY, [1920, 1080]),
        ),
    ]);
    let (document, omissions) = convert(root.path(), sequence, media);
    assert!(omissions.contains(&Omission {
        scope: OmissionScope::Occurrence,
        kind: crate::OmissionKind::Omitted,
        record: "sound of one".into(),
        reason: LINKED_AUDIO_REASON.into(),
    }));
    assert_unique_identities(&document);
    let sounds: Vec<_> = all_layers(&document["composition"])
        .into_iter()
        .filter(|layer| layer["type"] == "Audio")
        .collect();
    // Each file's item 1 is its own asset, and the pictures play no sound.
    assert_eq!(
        sounds
            .iter()
            .map(|layer| layer["source"]["assetId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["premiere-aep-1-item-1", "premiere-aep-2-item-1"]
    );
    assert!(sounds.iter().all(|layer| layer["isHidden"] == true));
    let archive = TesseractFile::open(root.path().join("converted.tsrct")).unwrap();
    for (id, bytes) in [
        ("premiere-aep-1-item-1", &wav),
        ("premiere-aep-2-item-1", &other),
    ] {
        assert_eq!(
            archive
                .asset(id)
                .unwrap()
                .read_verified_bytes(1 << 20)
                .unwrap(),
            *bytes
        );
    }
}

#[test]
fn unresolvable_links_are_reported_and_their_siblings_convert() {
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        root.path().join("comps.aep"),
    )
    .unwrap();
    // An AE 2023 header profile without Dynamic Link identity evidence.
    fs::copy(
        fixture("../aftereffects_file/tests/fixtures/compositions.aep"),
        root.path().join("legacy.aep"),
    )
    .unwrap();
    let cases = [
        ("missing", linked("missing.aep", RED, [1920, 1080]), "linked After Effects project: missing media"),
        (
            "absent item",
            linked("comps.aep", "00000099-0000-0000-0000-000000000000", [1920, 1080]),
            "has no composition for Dynamic Link GUID 00000099-0000-0000-0000-000000000000: Dynamic Link item 153 is absent or is not a composition in this AEP; the composition is never chosen by name",
        ),
        (
            "legacy",
            linked("legacy.aep", RED, [1920, 1080]),
            "is not supported: unsupported Dynamic Link AEP profile",
        ),
        (
            "resized",
            linked("comps.aep", RED, [1280, 720]),
            "is 1920x1080 in",
        ),
    ];
    let mut clips: Vec<_> = cases
        .iter()
        .enumerate()
        .map(|(index, (name, _, _))| {
            let start = i64::try_from(index).unwrap() * TICKS;
            let mut clip = clip_of(name, start..start + TICKS, 0);
            clip.id = Some(format!("clip {name}"));
            clip
        })
        .collect();
    clips.push(clip_of("blue", 4 * TICKS..5 * TICKS, 0));
    let mut media: BTreeMap<_, _> = cases
        .iter()
        .map(|(name, media, _)| (MediaId((*name).into()), media.clone()))
        .collect();
    media.insert(
        MediaId("blue".into()),
        linked("comps.aep", BLUE, [1920, 1080]),
    );
    let (document, omissions) = convert(
        root.path(),
        sequence_of("Unresolved", vec![PrVideoTrack::media(clips)]),
        media,
    );
    for (name, _, reason) in cases {
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.record == format!("clip {name}")
                    && omission.reason.contains(reason)),
            "{name}: {omissions:?}"
        );
    }
    let groups = linked_groups(&document);
    assert_eq!(groups.len(), 1, "only the resolvable sibling imports");
    assert_eq!(
        rect_fills(groups[0]),
        [&serde_json::json!([0.0, 0.0, 1.0, 1.0])]
    );
    assert_eq!(range(groups[0]), (4000, 1000));
}

#[test]
fn a_custom_canvas_link_imports_only_at_the_frame_its_source_declares() {
    // A portrait sequence links the 1920x1080 red composition four times.
    // Only the clip whose media declares exactly that frame imports: on the
    // portrait canvas, centred by its default Motion as a clip of that frame
    // is, and clipped to the composition's canvas. A transposed frame, or one
    // that matches only one side, omits its own clip.
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        root.path().join("comps.aep"),
    )
    .unwrap();
    let declared = [
        ("landscape", [1920, 1080]),
        ("portrait", [1080, 1920]),
        ("same width", [1920, 1920]),
        ("same height", [1080, 1080]),
    ];
    let clips = declared
        .iter()
        .enumerate()
        .map(|(index, (name, _))| {
            let start = i64::try_from(index).unwrap() * TICKS;
            let mut clip = clip_of(name, start..start + TICKS, 0);
            clip.id = Some(format!("clip {name}"));
            clip
        })
        .collect::<Vec<_>>();
    let media = declared
        .iter()
        .map(|(name, frame)| (MediaId((*name).into()), linked("comps.aep", RED, *frame)))
        .collect();
    let mut sequence = sequence_of("Portrait", vec![PrVideoTrack::media(clips)]);
    (sequence.width, sequence.height) = (1080, 1920);
    let (document, omissions) = convert(root.path(), sequence, media);
    assert_eq!(
        document["dimensions"],
        serde_json::json!({"width": 1080, "height": 1920})
    );
    let aep = fs::canonicalize(root.path()).unwrap().join("comps.aep");
    let mut occurrences: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| (omission.record.clone(), omission.reason.clone()))
        .collect();
    occurrences.sort_unstable();
    let mut expected: Vec<_> = declared[1..]
        .iter()
        .map(|(name, [width, height])| {
            (
                format!("clip {name}"),
                format!("linked After Effects composition 1 (\"Hybrid_Duplicate_Name\", GUID {RED}) is 1920x1080 in {aep:?}, but Premiere links it as {width}x{height}; placement geometry would change"),
            )
        })
        .collect();
    expected.sort_unstable();
    assert_eq!(occurrences, expected);
    assert_unique_identities(&document);
    let [group] = linked_groups(&document)[..] else {
        panic!("only the clip that declares the composition's frame imports");
    };
    assert_eq!(range(group), (0, 1000));
    assert_canvas_clip(group, [1920.0, 1080.0]);
    assert_eq!(
        rect_fills(group),
        [&serde_json::json!([1.0, 0.0, 0.0, 1.0])]
    );
    assert_eq!(
        group["transform"]["anchorPoint"],
        serde_json::json!([960.0, 540.0])
    );
    assert_eq!(
        group["transform"]["position"],
        serde_json::json!([540.0, 960.0])
    );
}

#[test]
fn linked_footage_that_changes_before_publication_is_never_published() {
    // The linked picture packages its composition's WAV. Bytes of the same
    // length pass the archive writer's own check; the digest that conversion
    // read does not.
    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("package");
    let wav = fs::read(fixture("tests/fixtures/audio-stereo.wav")).unwrap();
    audio_composition(&package, &wav);
    let project = package.join("linked-av.prproj");
    crate::test_support::write_prproj(
        &project,
        &linked_av_as_composition_xml("controls.aep", UNITY),
    );
    let output = root.path().join("converted");
    let import = TesseractImport::convert(&project, &output, None).unwrap();
    let mut changed = wav;
    *changed.last_mut().unwrap() ^= 1;
    fs::write(package.join("source.wav"), changed).unwrap();
    let error = import.write().unwrap_err().to_string();
    assert!(
        error.contains("linked AEP media changed after conversion read it")
            && error.contains("source.wav"),
        "{error}"
    );
    assert!(!output.exists());
    let mut entries: Vec<_> = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    entries.sort();
    assert_eq!(entries, ["package"]);
}

#[test]
fn a_linked_psd_source_that_changes_after_normalization_is_never_published() {
    use aftereffects_file::{aep::Project, rifx::Chunk};
    fn relink(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = "two_layers.psd".into();
                *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children);
            }
        }
    }
    // The Adobe-authored merged-PSD composition (AE 26.5x89, item 2, 64x48),
    // relinked to its pinned PSD, which conversion normalizes to a PNG.
    let root = tempfile::tempdir().unwrap();
    let fixtures = fixture("../aftereffects_file/tests/fixtures/psd_import");
    let mut native =
        Project::parse(&fs::read(fixtures.join("psd_sources_v2.aep")).unwrap()).unwrap();
    relink(&mut native.chunks);
    fs::write(root.path().join("psd.aep"), native.encode().unwrap()).unwrap();
    let psd = root.path().join("two_layers.psd");
    fs::copy(fixtures.join("two_layers_v2.psd"), &psd).unwrap();
    let sequence = sequence_of(
        "Merged PSD",
        vec![PrVideoTrack::media([clip_of("psd", 0..TICKS, 0)])],
    );
    const MERGED: &str = "00000002-0000-0000-0000-000000000000";
    let media = BTreeMap::from([(MediaId("psd".into()), linked("psd.aep", MERGED, [64, 48]))]);
    let package = fs::canonicalize(root.path()).unwrap();
    let mut omissions = Vec::new();
    let pending = convert_premiere_sequence(
        &package.join("project.prproj"),
        sequence,
        Arc::new(media),
        &mut omissions,
    )
    .unwrap()
    .unwrap();
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let mut bytes = fs::read(&psd).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&psd, bytes).unwrap();
    let error = pending
        .write_to_staging(&package.join("converted.tsrct"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("linked AEP media changed after conversion read it")
            && error.contains("two_layers.psd"),
        "{error}"
    );
}

/// The pinned Adobe linked picture-and-sound project, whose one A/V media
/// record is rewritten as a Dynamic Link to composition `guid` of the AEP at
/// the relative path `aep`: a derived, not Adobe-saved, link with both a
/// video and an audio item.
/// The five seconds of `feature_linked_av_strict.prproj`'s A/V media, in ticks.
const LINKED_AV_SOURCE_TICKS: &str = "1270080000000";

/// That project's A/V clip, relinked to the composition `guid` in `aep` and
/// declared, as Premiere records a link, with the two seconds of the
/// compositions that it links.
fn linked_av_as_composition_xml(aep: &str, guid: &str) -> String {
    linked_av_declaring(aep, guid, &(2 * TICKS).to_string())
}

/// That project's A/V clip, relinked to the composition `guid` in `aep`,
/// with its media, clip and sequence spanning `ticks`.
fn linked_av_declaring(aep: &str, guid: &str, ticks: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let mut xml = String::new();
    std::io::Read::read_to_string(
        &mut flate2::read::GzDecoder::new(
            fs::File::open(fixture("tests/fixtures/feature_linked_av_strict.prproj")).unwrap(),
        ),
        &mut xml,
    )
    .unwrap();
    let guid = STANDARD.encode(
        guid.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let relative = format!("<RelativePath>{aep}</RelativePath>");
    let link = format!(
        r#"<ImporterPrefs Encoding="base64" BinaryHash="0d6b9d1e-5c2f-4d8a-9e39-2f6c6a1b7e01">{guid}</ImporterPrefs><ImplementationID>{}</ImplementationID>"#,
        crate::schema::after_effects::IMPORTER_ID
    );
    for (from, to) in [
        (
            "<ImplementationID>1fa18bfa-255c-44b1-ad73-56bcd99fceaf</ImplementationID>",
            link.as_str(),
        ),
        (
            "<RelativePath>feature_linked_av_source.mp4</RelativePath>",
            relative.as_str(),
        ),
        ("<IgnoreAlpha>true</IgnoreAlpha>", ""),
        (
            "<CodecType>1635148593</CodecType>",
            "<CodecType>1145854285</CodecType>",
        ),
        (
            r#"<OriginalColorSpace>{"baseColorProfile":{"colorProfileData":"AQAAAP////8=","colorProfileName":"BT.709,8-bit,Display-Referred"},"baseProfileType":1}</OriginalColorSpace>"#,
            r#"<OriginalColorSpace>{"baseColorProfile":{"colorProfileName":"BT.709 RGB Full"},"baseProfileType":1}</OriginalColorSpace>"#,
        ),
        (
            "<AlphaType>3</AlphaType>",
            "<AlphaType>1</AlphaType><OriginalFieldType>4</OriginalFieldType>",
        ),
    ] {
        assert_eq!(xml.matches(from).count(), 1, "{from}");
        xml = xml.replace(from, to);
    }
    assert_eq!(xml.matches(LINKED_AV_SOURCE_TICKS).count(), 12);
    xml.replace(LINKED_AV_SOURCE_TICKS, ticks)
}

#[test]
fn linked_video_preflight_is_fatal_and_accepts_an_outer_project_media_map() {
    use aftereffects_file::{aep::Project, rifx::Chunk, AfterEffects};
    use fx_conv::{
        sha256_file, ConversionMode, ImportToTesseract, MediaMap, MediaMapSource, MediaReplacement,
        MediaStatus, ValidatedMediaMap,
    };

    fn relink(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = "original.swf".into();
                *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children);
            }
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let aep = root.join("linked.aep");
    let mut native = Project::parse(
        &fs::read(fixture(
            "../aftereffects_file/tests/fixtures/pr4442_native/sources/media_video.aep",
        ))
        .unwrap(),
    )
    .unwrap();
    relink(&mut native.chunks);
    fs::write(&aep, native.encode().unwrap()).unwrap();
    let targets = AfterEffects.list_import_targets(&aep).unwrap();
    assert_eq!(targets.len(), 1);
    let composition = &targets[0];
    let item: u32 = composition.id.parse().unwrap();
    let guid = format!("{item:08x}-0000-0000-0000-000000000000");
    // Only the source VideoStream rectangle changes; the native sequence remains HD.
    let xml = linked_av_declaring("linked.aep", &guid, &TICKS.to_string()).replacen(
        "0,0,1920,1080",
        &format!(
            "0,0,{},{}",
            composition.width.unwrap(),
            composition.height.unwrap()
        ),
        1,
    );
    let input = root.join("source.prproj");
    crate::test_support::write_prproj(&input, &xml);
    let original = root.join("original.swf");
    fs::write(&original, b"FWS unsupported original video").unwrap();
    let target = crate::Premiere
        .list_import_targets(&input)
        .unwrap()
        .remove(0);
    let options = crate::PremiereImportOptions {
        sequence: Some(target.id.clone()),
    };
    let inspection = crate::Premiere
        .inspect_media(&input, &options, None)
        .unwrap();
    assert!(inspection.media.iter().any(|media| matches!(
        media.status,
        MediaStatus::RequiresTranscode | MediaStatus::InvalidMedia
    )));
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = root.join(format!("unmapped-{mode:?}"));
        assert!(crate::Premiere
            .import_to_tesseract(&input, &output, &options, mode)
            .is_err());
        assert!(!output.exists());
    }

    let prepared = root.join("prepared");
    fs::create_dir(&prepared).unwrap();
    let replacement = prepared.join("movie.mov");
    let bytes = fs::read(fixture(
        "../aftereffects_file/tests/fixtures/audio_e2e/movie.mov",
    ))
    .unwrap();
    fs::write(&replacement, &bytes).unwrap();
    let map = MediaMap {
        version: 1,
        source: MediaMapSource {
            format: "premiere".into(),
            sha256: sha256_file(&input).unwrap(),
            target: target.id,
        },
        replacements: vec![MediaReplacement {
            original: original.canonicalize().unwrap(),
            original_sha256: sha256_file(&original).unwrap(),
            replacement: "movie.mov".into(),
            replacement_sha256: sha256_file(&replacement).unwrap(),
        }],
    };
    let map_path = prepared.join("media-map.json");
    fs::write(&map_path, serde_json::to_vec(&map).unwrap()).unwrap();
    let map = ValidatedMediaMap::load(&map_path).unwrap();
    let inspection = crate::Premiere
        .inspect_media(&input, &options, Some(&map))
        .unwrap();
    assert!(inspection.is_ready(), "{inspection:?}");
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = root.join(format!("mapped-{mode:?}"));
        crate::Premiere
            .import_with_media_map(&input, &output, &options, mode, &map)
            .unwrap();
        if mode == ConversionMode::Check {
            assert!(!output.exists());
        } else {
            let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
            let document = archive.project_json().unwrap();
            let videos: Vec<_> = all_layers(&document["composition"])
                .into_iter()
                .filter(|layer| layer["type"] == "Video")
                .collect();
            assert_eq!(videos.len(), 1);
            let asset = videos[0]["source"]["assetId"].as_str().unwrap();
            assert_eq!(
                archive
                    .asset(asset)
                    .unwrap()
                    .read_verified_bytes(bytes.len().try_into().unwrap())
                    .unwrap(),
                bytes
            );
        }
    }
    fs::write(&original, b"changed linked original").unwrap();
    assert!(crate::Premiere
        .import_with_media_map(
            &input,
            &root.join("stale"),
            &options,
            ConversionMode::Write,
            &map
        )
        .is_err());
    assert!(!root.join("stale").exists());
}

#[test]
fn a_linked_composition_s_sound_item_is_omitted_and_its_picture_imports_muted() {
    let xml = linked_av_as_composition_xml("linked-compositions.aep", RED);
    let (project, omissions) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    // The linked audio stream no longer rejects the composition's picture.
    let picture = sequence.video_occurrences().next().unwrap();
    let source = project.media(picture).unwrap();
    assert!(source.after_effects_composition().is_some());
    assert!(source.audio.is_some());
    assert!(sequence.audio.is_empty());
    let sound = omissions
        .iter()
        .find(|omission| omission.record == "122")
        .unwrap();
    assert_eq!(sound.scope, OmissionScope::Occurrence);
    assert!(
        sound.reason.ends_with(LINKED_AUDIO_REASON),
        "{}",
        sound.reason
    );

    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("package");
    fs::create_dir_all(&package).unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        package.join("linked-compositions.aep"),
    )
    .unwrap();
    let prproj = package.join("linked-av.prproj");
    crate::test_support::write_prproj(&prproj, &xml);
    let output = root.path().join("converted");
    let converted = crate::premiere_to_tesseract(&prproj, &output, None, false).unwrap();
    assert!(converted.contains(sound));
    let document = TesseractFile::open(output.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    // One owner for the sound: neither the omitted item nor the picture plays it.
    assert!(!all_layers(&document["composition"])
        .iter()
        .any(|layer| layer["type"] == "Audio"));
    let groups = linked_groups(&document);
    assert_eq!(groups.len(), 1);
    assert_canvas_clip(groups[0], [1920.0, 1080.0]);
    assert_eq!(
        rect_fills(groups[0]),
        [&serde_json::json!([1.0, 0.0, 0.0, 1.0])]
    );
}

/// `layer` without the fields that only its picture kind has: a video's
/// source or a linked composition's content and clock.
fn placement(layer: &Value) -> Value {
    let mut layer = layer.clone();
    let fields = layer.as_object_mut().unwrap();
    let picture = match fields["type"].as_str() {
        Some("Video") => true,
        Some("Group") => fields["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("Premiere linked composition")),
        _ => false,
    };
    if picture {
        fields.retain(|name, _| {
            [
                "id",
                "parent",
                "isHidden",
                "blendMode",
                "trackMatte",
                "masks",
                "activeRange",
                "effects",
                "motionBlur",
                "transform",
            ]
            .contains(&name.as_str())
        });
    } else if let Some(children) = fields.get_mut("layers") {
        *children = children.as_array().unwrap().iter().map(placement).collect();
    }
    layer
}

/// A converted document and its omissions.
type Converted = (Value, Vec<Omission>);

/// The sequence that `sequence` builds around clips of a media id, converted
/// once with a packaged 10 s video and once with the red composition of the
/// identity AEP linked in its place.
fn as_video_and_linked(sequence: impl Fn(&str) -> PrSequence) -> (Converted, Converted) {
    use crate::{
        convert::sequence_document, linked_compositions::LinkedCompositions,
        tests::support::video_media,
    };
    let root = tempfile::tempdir().unwrap();
    let aep = fs::canonicalize(root.path()).unwrap().join("comps.aep");
    fs::copy(fixture(IDENTITY).join("linked-compositions.aep"), &aep).unwrap();
    let convert = |media_id: &str,
                   media: BTreeMap<MediaId, PrMedia>,
                   asset_ids: BTreeMap<MediaId, fx_schema::AssetId>,
                   linked: &mut LinkedCompositions| {
        let mut omissions = Vec::new();
        let document = sequence_document(
            &sequence(media_id),
            &media,
            &asset_ids,
            linked,
            &mut omissions,
        )
        .unwrap();
        (document.to_json_value().unwrap(), omissions)
    };
    let video = convert(
        "source",
        video_media(),
        BTreeMap::from([(
            MediaId("source".into()),
            fx_schema::AssetId::from_trusted("premiere-video-1"),
        )]),
        &mut LinkedCompositions::default(),
    );
    let id = MediaId("red".into());
    let media = linked("comps.aep", RED, [1920, 1080]);
    let mut links = LinkedCompositions::default();
    links
        .link(&id, &media, &aep, &crate::hash::hash(&aep).unwrap())
        .unwrap()
        .unwrap();
    let composition = convert(
        "red",
        BTreeMap::from([(id, media)]),
        BTreeMap::new(),
        &mut links,
    );
    (video, composition)
}

/// Clip `keyed` of `media` over sequence 0.5-1.5 s from source 1/3 s, at 50 %
/// scale, turned 15° to 45°, faded from 60 to 20 %, cropped and blurred by
/// 12 to 36, every key on the source clock.
fn keyed_clip(media: &str) -> crate::schema::PrVideoOccurrence {
    use crate::{
        schema::{
            PrEffect, PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams, PrGaussianBlur,
            PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe, GAUSSIAN_BLUR_BLURRINESS,
        },
        tests::support::left_crop,
    };
    let linear = |keys: [(i64, f64); 2]| {
        keys.map(|(source_ticks, value)| PrScalarKeyframe {
            source_ticks,
            value,
            easing: PrKeyframeEasing::Linear,
        })
        .to_vec()
    };
    let mut clip = clip_of(media, TICKS / 2..3 * TICKS / 2, TICKS / 3);
    clip.id = Some("keyed".into());
    clip.transform.scale = [50.0, 50.0];
    clip.transform.position = [0.25, 0.75];
    clip.transform.rotation = 15.0;
    clip.opacity = 60.0;
    clip.crop = left_crop();
    clip.effects = vec![PrEffect {
        enabled: true,
        params: PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness: 12.0,
            repeat_edge_pixels: false,
        }),
        animations: vec![PrEffectParamAnimation {
            param: &GAUSSIAN_BLUR_BLURRINESS,
            keys: PrEffectParamKeys::Scalar(linear([(TICKS / 3, 12.0), (TICKS, 36.0)])),
        }],
    }];
    clip.animations = vec![
        PrPropertyAnimation::Rotation(linear([(TICKS / 3, 15.0), (TICKS, 45.0)])),
        PrPropertyAnimation::Opacity(linear([(TICKS / 3, 60.0), (TICKS, 20.0)])),
    ];
    clip
}

/// The animation entries of `document` whose targets the Premiere clip
/// layers own, not the composition's own layers.
fn clip_entries(document: &Value) -> Vec<&Value> {
    let composition: BTreeSet<_> = linked_groups(document)
        .into_iter()
        .flat_map(|group| all_layers(composition_root(group)))
        .chain(linked_groups(document).into_iter().map(composition_root))
        .filter_map(|layer| layer["id"].as_u64())
        .collect();
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            entry["target"]["layerId"]
                .as_u64()
                .is_none_or(|id| !composition.contains(&id))
        })
        .collect()
}

/// `omissions` without the notes of the composition's own import.
fn clip_omissions(omissions: &[Omission]) -> Vec<&Omission> {
    omissions
        .iter()
        .filter(|omission| {
            !omission
                .reason
                .starts_with("linked After Effects composition: ")
        })
        .collect()
}

#[test]
fn a_linked_clip_is_placed_and_keyed_on_the_video_clip_clock() {
    let ((video, video_omissions), (composition, omissions)) = as_video_and_linked(|media| {
        sequence_of("Placed", vec![PrVideoTrack::media([keyed_clip(media)])])
    });
    // The clip and the guide of its Crop match; only the canvas after them
    // takes a later identity.
    let clip_layers = |document: &Value| {
        let layers = document["composition"]["layers"].as_array().unwrap();
        assert_eq!(layers.last().unwrap()["name"], "Premiere black canvas");
        layers[..layers.len() - 1]
            .iter()
            .map(placement)
            .collect::<Vec<_>>()
    };
    assert_eq!(clip_layers(&composition), clip_layers(&video));
    assert_eq!(clip_layers(&video).len(), 2, "the clip and its Crop guide");
    // Every key, with its time, value and easing, is the video clip's: on the
    // clip clock of a layer without playback (Rotation 15° at the clip start),
    // on the clip layer and the Crop guide alike.
    let video_entries: Vec<&Value> = video["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .collect();
    // Rotation on the clip and its guide, Opacity, and Blurriness.
    assert_eq!(video_entries.len(), 4, "{video_entries:?}");
    assert_eq!(clip_entries(&composition), video_entries);
    let group = linked_groups(&composition)[0];
    let rotation = video_entries
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == group["id"]
                && entry["target"]["propertyType"] == "rotation"
        })
        .unwrap();
    let keys: Vec<_> = rotation["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_u64().unwrap(),
                key["value"]["value"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(keys, [(0, 15.0), (667, 45.0)]);
    assert_eq!(
        clip_omissions(&omissions),
        clip_omissions(&video_omissions).as_slice()
    );
    // The clip group seeds the runtime clock; only the composition plays from
    // source 1/3 s, under a source group that carries no placement of its own.
    assert_eq!(playback(group), document_clock_seed(group));
    let source = source_group(group).unwrap();
    assert_eq!(range(source), (0, 1000));
    assert_eq!(playback(source), [(0, 333), (1000, 1333)]);
    for (field, value) in [
        ("anchorPoint", serde_json::json!([0.0, 0.0])),
        ("position", serde_json::json!([0.0, 0.0])),
        ("scale", serde_json::json!([100.0, 100.0])),
        ("rotation", serde_json::json!(0.0)),
        ("opacity", serde_json::json!(100.0)),
    ] {
        assert_eq!(source["transform"][field], value, "{field}");
    }
    assert!(source["effects"].as_array().is_none_or(Vec::is_empty));
    assert!(source["masks"].as_array().is_none_or(Vec::is_empty));
    assert_canvas_clip(group, [1920.0, 1080.0]);
}

#[test]
fn a_retimed_linked_clip_plays_its_composition_at_the_clip_speed_with_static_keys() {
    // Sequence 0.5-1 s of the keyed clip at 2x and in reverse: the source
    // group plays the composition at that speed on the clip clock.
    let retimed = |media: &str, rate: f64| {
        let mut clip = keyed_clip(media);
        clip.end_ticks = clip.start_ticks + TICKS / 2;
        clip.playback_rate = rate;
        clip.out_ticks = clip.in_ticks + (TICKS as f64 / 2.0 * rate.abs()) as i64;
        clip
    };
    let effect_keys = "effect animation was not imported: keys on a retimed, reversed or time-remapped clip are not converted; static values were kept";
    for (rate, clock) in [
        (2.0, [(0, 333), (500, 1333)]),
        (-1.0, [(0, 1667), (500, 1167)]),
    ] {
        let ((video, video_omissions), (composition, omissions)) = as_video_and_linked(|media| {
            sequence_of("Retimed", vec![PrVideoTrack::media([retimed(media, rate)])])
        });
        let group = linked_groups(&composition)[0];
        assert_eq!(range(group), (500, 500), "{rate}");
        assert_eq!(playback(group), document_clock_seed(group), "{rate}");
        assert_eq!(playback(source_group(group).unwrap()), clock, "{rate}");
        assert_canvas_clip(group, [1920.0, 1080.0]);
        if rate > 0.0 {
            // Forward, the video's playback on the sequence clock is the same
            // clock from the clip start (a reverse range depends on the media's
            // own duration, which the 10 s video does not share).
            let shifted: Vec<_> = playback(&video["composition"]["layers"][0])
                .into_iter()
                .map(|(time, value)| (time - 500, value))
                .collect();
            assert_eq!(shifted, clock);
        }
        // Native keys are on the source clock, which neither clip clock
        // matches: the Motion and Opacity keys are omitted as for the video,
        // and the linked clip's effect keys too, keeping static values.
        assert!(clip_entries(&composition).is_empty(), "{rate}");
        let mut expected = clip_omissions(&video_omissions);
        let effect = Omission {
            scope: OmissionScope::Feature,
            kind: crate::OmissionKind::Omitted,
            record: "keyed".into(),
            reason: effect_keys.into(),
        };
        expected.push(&effect);
        let mut actual = clip_omissions(&omissions);
        actual.sort_by_key(|omission| &omission.reason);
        expected.sort_by_key(|omission| &omission.reason);
        assert_eq!(actual, expected, "{rate}");
    }
}

#[test]
fn a_linked_clip_in_a_nest_is_keyed_on_its_clip_clock() {
    // Inner shows the keyed clip at 0.5-1.5 s; the nest shows inner
    // 0.75-1.75 s at outer 2-3 s, so its copy starts 1/4 s into the clip.
    let ((video, video_omissions), (composition, omissions)) = as_video_and_linked(|media| {
        let inner = sequence_of("Inner", vec![PrVideoTrack::media([keyed_clip(media)])]);
        sequence_of(
            "Outer",
            vec![PrVideoTrack {
                items: Vec::new(),
                nests: vec![nest_of(inner, 2 * TICKS..3 * TICKS, 3 * TICKS / 4)],
                transitions: Vec::new(),
            }],
        )
    });
    assert_eq!(clip_entries(&composition), clip_entries(&video));
    assert!(!clip_entries(&video).is_empty());
    // The nest starts after the document start: the source group's remapped
    // animation is reported, not claimed on time. Nothing else differs.
    let approximation = offset_clock_approximation("keyed", "linked composition");
    let mut expected = clip_omissions(&video_omissions);
    expected.push(&approximation);
    assert_eq!(clip_omissions(&omissions), expected);
    let nest = all_layers(&composition["composition"])
        .into_iter()
        .find(|layer| layer["name"] == "Inner")
        .unwrap();
    let [group] = linked_groups(&composition)[..] else {
        panic!("one linked copy");
    };
    assert_eq!(group["parent"], nest["id"]);
    assert_eq!(range(group), (0, 750));
    assert_eq!(
        group["playback"],
        crate::test_support::linear_playback(
            crate::test_support::layer_range(group).clone(),
            serde_json::json!({"start": 0, "duration": 750})
        ),
        "no seed under the nest"
    );
    // The copy shows the composition from 1/3 + 1/4 s.
    assert_eq!(
        playback(source_group(group).unwrap()),
        [(0, 583), (750, 1333)]
    );
    assert_canvas_clip(group, [1920.0, 1080.0]);
}

/// Converts `sequence`, whose clips play the 10 s video `source` and the red
/// composition `red` of the identity AEP.
fn convert_with_red(sequence: &PrSequence) -> Converted {
    use crate::{
        convert::sequence_document, linked_compositions::LinkedCompositions,
        tests::support::video_media,
    };
    let root = tempfile::tempdir().unwrap();
    let aep = fs::canonicalize(root.path()).unwrap().join("comps.aep");
    fs::copy(fixture(IDENTITY).join("linked-compositions.aep"), &aep).unwrap();
    let id = MediaId("red".into());
    let red = linked("comps.aep", RED, [1920, 1080]);
    let mut links = LinkedCompositions::default();
    links
        .link(&id, &red, &aep, &crate::hash::hash(&aep).unwrap())
        .unwrap()
        .unwrap();
    let mut media = video_media();
    media.insert(id, red);
    let asset_ids = BTreeMap::from([(
        MediaId("source".into()),
        fx_schema::AssetId::from_trusted("premiere-video-1"),
    )]);
    let mut omissions = Vec::new();
    let document =
        sequence_document(sequence, &media, &asset_ids, &mut links, &mut omissions).unwrap();
    (document.to_json_value().unwrap(), omissions)
}

/// Clip `keyed` of `media` over `start`-`start` + 1 s from source 0.5 s, at
/// Scale 50, keyed by the clip on the track above: a stage group holds it.
fn staged_clip(media: &str, start: i64) -> crate::schema::PrVideoOccurrence {
    use crate::schema::{PrMatteChannel, PrTrackMatte};
    let mut clip = clip_of(media, start..start + TICKS, TICKS / 2);
    clip.id = Some("keyed".into());
    clip.track_matte = Some(PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    });
    clip.transform.scale = [50.0; 2];
    clip
}

#[test]
fn a_linked_matte_keeps_its_clock_seed_only_under_a_stage_on_the_document_clock() {
    // The keyed video clip stages; its matte, the red composition from source
    // 0.5 s over the same range, moves under the stage group.
    let note = offset_clock_approximation("keyed", "Track Matte Key's linked composition matte");
    for (start, on_document_clock) in [(0, true), (TICKS, false)] {
        let mut keyed = staged_clip("source", start);
        keyed.in_ticks = 0;
        keyed.out_ticks = TICKS;
        let (document, omissions) = convert_with_red(&sequence_of(
            "Main",
            vec![
                PrVideoTrack::media([keyed]),
                PrVideoTrack::media([clip_of("red", start..start + TICKS, TICKS / 2)]),
            ],
        ));
        let stage = &document["composition"]["layers"][0];
        let [matte] = linked_groups(&document)[..] else {
            panic!("one linked matte: {document}");
        };
        assert_eq!(matte["parent"], stage["id"]);
        assert_eq!(stage["trackMatte"]["layer"], matte["id"]);
        assert_eq!(range(matte), (0, 1000));
        // The matte still shows the composition from 0.5 s on its own clock.
        assert_eq!(
            playback(source_group(matte).unwrap()),
            [(0, 500), (1000, 1500)]
        );
        if on_document_clock {
            // Its seed moves onto the stage clock, where the matte starts at 0.
            assert_eq!(playback(matte), [(0, 0), (1000, 1000)]);
            assert!(!omissions.contains(&note), "{omissions:?}");
        } else {
            // A stage at 1 s leaves nothing to seed: reported, not on time.
            assert_eq!(matte["playback"]["mapping"]["type"], "timeRemap");
            assert_eq!(playback(matte), [(0, 0), (1000, 1000)]);
            assert!(omissions.contains(&note), "{omissions:?}");
        }
    }
}

#[test]
fn a_staged_linked_clip_after_the_document_start_reports_its_remapped_animation() {
    let note = offset_clock_approximation("keyed", "linked composition");
    for (start, on_document_clock) in [(0, true), (TICKS, false)] {
        let (document, omissions) = convert_with_red(&sequence_of(
            "Main",
            vec![
                PrVideoTrack::media([staged_clip("red", start)]),
                PrVideoTrack::media([clip_of("source", start..start + TICKS, 0)]),
            ],
        ));
        let stage = &document["composition"]["layers"][0];
        let [group] = linked_groups(&document)[..] else {
            panic!("one linked clip: {document}");
        };
        assert_eq!(group["parent"], stage["id"]);
        assert_eq!(range(stage), (start as u64 * 1000 / TICKS as u64, 1000));
        assert_eq!(
            playback(source_group(group).unwrap()),
            [(0, 500), (1000, 1500)]
        );
        if on_document_clock {
            assert_eq!(playback(group), [(0, 0), (1000, 1000)]);
            assert!(!omissions.contains(&note), "{omissions:?}");
        } else {
            assert_eq!(
                group["playback"],
                crate::test_support::linear_playback(
                    crate::test_support::layer_range(group).clone(),
                    serde_json::json!({"start": 0, "duration": 1000})
                )
            );
            assert!(omissions.contains(&note), "{omissions:?}");
        }
    }
}

#[test]
fn a_clip_past_its_composition_s_end_is_omitted_and_its_siblings_convert() {
    // Premiere declares the link as 4 s, but the red composition lasts 2 s: a
    // clip that shows source 1-3 s has no content to show past 2 s, while its
    // sibling showing source 0-1 s converts.
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        root.path().join("comps.aep"),
    )
    .unwrap();
    let mut stale = linked("comps.aep", RED, [1920, 1080]);
    stale.video.as_mut().unwrap().intrinsic_ticks = 4 * TICKS;
    let mut long = clip_of("red", 0..2 * TICKS, TICKS);
    long.id = Some("long".into());
    let mut short = clip_of("red", 2 * TICKS..3 * TICKS, 0);
    short.id = Some("short".into());
    let (document, omissions) = convert(
        root.path(),
        sequence_of("Stale link", vec![PrVideoTrack::media([long, short])]),
        BTreeMap::from([(MediaId("red".into()), stale)]),
    );
    let omitted: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .collect();
    assert_eq!(
        omitted,
        [&Omission {
            scope: OmissionScope::Occurrence,
            kind: crate::OmissionKind::Omitted,
            record: "long".into(),
            reason: "linked After Effects composition 1 (\"Hybrid_Duplicate_Name\") lasts 2000 ms, shorter than the source range to 3000 ms that the clip shows".into(),
        }]
    );
    assert_unique_identities(&document);
    let [group] = linked_groups(&document)[..] else {
        panic!("only the clip within the composition imports");
    };
    assert_eq!(range(group), (2000, 1000));
    assert_canvas_clip(group, [1920.0, 1080.0]);
    assert_eq!(
        rect_fills(group),
        [&serde_json::json!([1.0, 0.0, 0.0, 1.0])]
    );
}

/// `clip` as `record`, whose Transform blurs it at Shutter Angle `angle`, the
/// composition's shutter angle off: it requests the composition's shutter.
fn blurred(
    mut clip: crate::schema::PrVideoOccurrence,
    record: &str,
    angle: f64,
) -> crate::schema::PrVideoOccurrence {
    use crate::{
        schema::PrTransform,
        tests::support::{transform_effect, DEFAULT_PR_TRANSFORM},
    };
    clip.id = Some(record.into());
    clip.effects = vec![transform_effect(
        PrTransform {
            composition_shutter_angle: false,
            shutter_angle: angle,
            ..DEFAULT_PR_TRANSFORM
        },
        Vec::new(),
    )];
    clip
}

/// The document's one motion blur: whether it is on, its shutter angle and
/// its phase.
fn composition_shutter(document: &Value) -> [&Value; 3] {
    let settings = &document["composition"]["motionBlur"];
    ["enabled", "shutterAngle", "shutterPhase"].map(|field| &settings[field])
}

/// The requests that the composition's shutter, set by an earlier request,
/// overrode: the requesting clip and its report.
fn shutter_conflicts(omissions: &[Omission]) -> Vec<(&str, &str)> {
    omissions
        .iter()
        .filter(|omission| {
            omission.reason.starts_with("composition shutter set to")
                || omission
                    .reason
                    .starts_with("composition motion blur set by")
        })
        .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
        .collect()
}

#[test]
fn a_clip_omitted_past_its_composition_s_end_leaves_the_shutter_to_its_sibling() {
    // The first clip, blurred at 90°, shows the red composition past its 2 s
    // end and is omitted after its Transform is read; the composition's one
    // shutter is its sibling's 180°, and nothing reports the omitted clip.
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        root.path().join("comps.aep"),
    )
    .unwrap();
    let mut stale = linked("comps.aep", RED, [1920, 1080]);
    stale.video.as_mut().unwrap().intrinsic_ticks = 4 * TICKS;
    let (document, omissions) = convert(
        root.path(),
        sequence_of(
            "Stale link",
            vec![PrVideoTrack::media([
                blurred(clip_of("red", 0..2 * TICKS, TICKS), "long", 90.0),
                blurred(clip_of("red", 2 * TICKS..3 * TICKS, 0), "short", 180.0),
            ])],
        ),
        BTreeMap::from([(MediaId("red".into()), stale)]),
    );
    let occurrences: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
        .collect();
    assert_eq!(
        occurrences,
        [(
            "long",
            "linked After Effects composition 1 (\"Hybrid_Duplicate_Name\") lasts 2000 ms, shorter than the source range to 3000 ms that the clip shows"
        )]
    );
    assert_eq!(
        (
            composition_shutter(&document),
            shutter_conflicts(&omissions)
        ),
        (
            [
                &serde_json::json!(true),
                &serde_json::json!(180.0),
                &serde_json::json!(0.0)
            ],
            Vec::new()
        )
    );
    let [group] = linked_groups(&document)[..] else {
        panic!("only the clip within the composition imports: {document}");
    };
    assert_eq!(group["motionBlur"], true);
}

/// A caller's editable content for one placement, with identities from
/// `first`: a `width`x1080 composition of 1 s whose root Group holds one red
/// 120x80 rect, as `import_editable_picture` returns it.
fn supplied_document(first: u64, width: u32) -> fx_schema::EditableFxCompositionDocument {
    let identity = crate::convert::identity_transform();
    fx_schema::EditableFxCompositionDocument::from_json_value(serde_json::json!({
        "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion": 1,
        "dimensions": {"width": width, "height": 1080},
        "duration": 1.0,
        "composition": {"id": "main", "name": "Resolved AEP", "layers": [{
            "type": "Group", "id": first, "name": "Source composition",
            "playback": crate::test_support::linear_playback(serde_json::json!({"start": 0, "duration": 1000}), serde_json::json!({"start": 0, "duration": 1000})), "transform": identity,
            "layers": [{
                "type": "Rect", "id": first + 1, "name": "Editable content",
                "activeRange": {"start": 0, "duration": 1000}, "transform": identity,
                "rect": {"size": [120.0, 80.0], "fillColor": [1.0, 0.0, 0.0, 1.0]}
            }]
        }]}
    }))
    .unwrap()
}

/// Converts `sequence` from a project in `package`, with every linked
/// picture supplied by `resolver`, and writes its archive.
fn convert_supplied<'a>(
    package: &Path,
    sequence: PrSequence,
    media: BTreeMap<MediaId, PrMedia>,
    resolver: &'a mut crate::LinkedCompositionResolver<'a>,
) -> crate::error::Result<(TesseractFile, Vec<Omission>)> {
    let mut omissions = Vec::new();
    let package = fs::canonicalize(package).unwrap();
    let pending = crate::tesseract_output::convert_premiere_sequence_with_links(
        &package.join("project.prproj"),
        sequence,
        Arc::new(media),
        &mut omissions,
        Some(resolver),
        fx_conv::Progress::default(),
    )?
    .unwrap();
    let archive = package.join("converted.tsrct");
    pending.write_to_staging(&archive)?;
    Ok((TesseractFile::open(&archive).unwrap(), omissions))
}

#[test]
fn caller_supplied_compositions_take_the_linked_clip_placement() {
    // Two placements of one link from source 0.3 s, the second screened:
    // each is placed as a built-in picture would be, on its own clip clock
    // and identities, and each call's assets are packaged as returned.
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        root.path().join("comps.aep"),
    )
    .unwrap();
    let still = fixture("tests/fixtures/a1_bg_chart.png");
    let first = clip_of("blue", TICKS / 10..3 * TICKS / 10, 3 * TICKS / 10);
    let mut second = clip_of("blue", 4 * TICKS / 10..6 * TICKS / 10, 3 * TICKS / 10);
    second.blend_mode = crate::schema::PrBlendMode::Screen;
    let mut calls = Vec::new();
    let mut resolve = |path: &Path, identity: PrAfterEffectsComposition, first: u64| {
        calls.push((path.to_owned(), identity.dynamic_link_guid(), first));
        Ok(crate::LinkedComposition {
            document: supplied_document(first, 1920),
            next_id: first + 2,
            assets: vec![(
                fx_schema::AssetId::from_trusted(format!("supplied-{first}")),
                still.clone(),
                tesseract_file::AssetKind::Image,
            )],
        })
    };
    let (archive, omissions) = convert_supplied(
        root.path(),
        sequence_of("Supplied", vec![PrVideoTrack::media([first, second])]),
        BTreeMap::from([(
            MediaId("blue".into()),
            linked("comps.aep", BLUE, [1920, 1080]),
        )]),
        &mut resolve,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let aep = fs::canonicalize(root.path()).unwrap().join("comps.aep");
    let firsts: Vec<_> = calls.iter().map(|(_, _, first)| *first).collect();
    assert_eq!(
        calls,
        firsts
            .iter()
            .map(|first| (aep.clone(), BLUE.to_owned(), *first))
            .collect::<Vec<_>>()
    );
    let document = archive.project_json().unwrap();
    assert_unique_identities(&document);
    let groups = linked_groups(&document);
    assert_eq!(groups.len(), 2);
    for (group, start, first, blend) in [
        (groups[0], 100, firsts[0], "normal"),
        (groups[1], 400, firsts[1], "screen"),
    ] {
        assert_eq!(range(group), (start, 200));
        assert_eq!(playback(group), document_clock_seed(group));
        assert_eq!(group["blendMode"], blend);
        let source = source_group(group).unwrap();
        assert_eq!(playback(source), [(0, 300), (200, 500)]);
        assert!(source["blendMode"].is_null() || source["blendMode"] == "normal");
        let root = composition_root(group);
        assert_eq!(root["id"], first);
        assert_eq!(root["name"], "Source composition");
        assert_canvas_clip(group, [1920.0, 1080.0]);
        assert_eq!(
            rect_fills(group),
            [&serde_json::json!([1.0, 0.0, 0.0, 1.0])]
        );
    }
    // The second placement's identities follow the first's canvas guide.
    assert!(firsts[1] >= firsts[0] + 4, "{firsts:?}");
    assert_eq!(
        archive.metadata().assets.keys().collect::<Vec<_>>(),
        [
            &format!("supplied-{}", firsts[0]),
            &format!("supplied-{}", firsts[1])
        ]
    );
}

#[test]
fn a_caller_supplied_composition_that_cannot_be_placed_omits_only_its_clip() {
    // Content on another canvas and an unsupported After Effects source are
    // omitted clip by clip, as the built-in import omits them, without their
    // assets; the sibling converts. Any other importer failure stops the
    // import before anything is written.
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        root.path().join("comps.aep"),
    )
    .unwrap();
    const ABSENT: &str = "00000009-0000-0000-0000-000000000000";
    let still = fixture("tests/fixtures/a1_bg_chart.png");
    let clips = ["wide", "absent", "blue"]
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let start = i64::try_from(index).unwrap() * TICKS;
            let mut clip = clip_of(name, start..start + TICKS / 2, 0);
            clip.id = Some(format!("clip {name}"));
            clip
        })
        .collect::<Vec<_>>();
    let media = || {
        BTreeMap::from([
            (
                MediaId("wide".into()),
                linked("comps.aep", RED, [1920, 1080]),
            ),
            (
                MediaId("absent".into()),
                linked("comps.aep", ABSENT, [1920, 1080]),
            ),
            (
                MediaId("blue".into()),
                linked("comps.aep", BLUE, [1920, 1080]),
            ),
        ])
    };
    let mut resolve = |_: &Path, identity: PrAfterEffectsComposition, first: u64| match identity
        .dynamic_link_guid()
        .as_str()
    {
        ABSENT => Err(aftereffects_file::DynamicLinkImportError::MissingComposition(9).into()),
        guid => Ok(crate::LinkedComposition {
            document: supplied_document(first, if guid == RED { 1280 } else { 1920 }),
            next_id: first + 2,
            assets: vec![(
                fx_schema::AssetId::from_trusted(format!("supplied-{guid}")),
                still.clone(),
                tesseract_file::AssetKind::Image,
            )],
        }),
    };
    let (archive, omissions) = convert_supplied(
        root.path(),
        sequence_of("Supplied", vec![PrVideoTrack::media(clips.clone())]),
        media(),
        &mut resolve,
    )
    .unwrap();
    let aep = fs::canonicalize(root.path()).unwrap().join("comps.aep");
    let occurrences: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| (omission.record.as_str(), omission.reason.clone()))
        .collect();
    assert_eq!(
        occurrences,
        [
            (
                "clip wide",
                format!("the linked composition supplied for GUID {RED} is 1280x1080 in {aep:?}, but Premiere links it as 1920x1080; placement geometry would change")
            ),
            (
                "clip absent",
                format!("linked After Effects composition GUID {ABSENT} in {aep:?} forms no editable picture: Dynamic Link item 9 is absent or is not a composition in this AEP")
            ),
        ]
    );
    let document = archive.project_json().unwrap();
    let [group] = linked_groups(&document)[..] else {
        panic!("only the placeable sibling imports");
    };
    assert_eq!(range(group), (2000, 500));
    assert_canvas_clip(group, [1920.0, 1080.0]);
    assert_eq!(
        archive.metadata().assets.keys().collect::<Vec<_>>(),
        [&format!("supplied-{BLUE}")]
    );

    let failed = root.path().join("failed");
    fs::create_dir(&failed).unwrap();
    fs::copy(root.path().join("comps.aep"), failed.join("comps.aep")).unwrap();
    let mut broken = |_: &Path, _: PrAfterEffectsComposition, _: u64| {
        Err(anyhow::anyhow!("the importer lost its scratch volume"))
    };
    let error = convert_supplied(
        &failed,
        sequence_of("Supplied", vec![PrVideoTrack::media(clips)]),
        media(),
        &mut broken,
    )
    .err()
    .unwrap()
    .to_string();
    assert_eq!(
        error,
        "linked composition import failed: the importer lost its scratch volume"
    );
    assert!(!failed.join("converted.tsrct").exists());
}

#[test]
fn a_caller_supplied_composition_that_forms_no_picture_leaves_the_shutter_to_its_sibling() {
    // The importer finds no composition for the first clip, blurred at 90°,
    // which is omitted after its Transform is read. The sibling's Transform
    // asks for 180° before its composition asks for its own 360°: the
    // composition's one shutter is the sibling Transform's, and only the
    // sibling's second request is reported.
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        fixture(IDENTITY).join("linked-compositions.aep"),
        root.path().join("comps.aep"),
    )
    .unwrap();
    const ABSENT: &str = "00000009-0000-0000-0000-000000000000";
    let mut resolve = |_: &Path, identity: PrAfterEffectsComposition, first: u64| match identity
        .dynamic_link_guid()
        .as_str()
    {
        ABSENT => Err(aftereffects_file::DynamicLinkImportError::MissingComposition(9).into()),
        _ => {
            let mut document = supplied_document(first, 1920).to_json_value().unwrap();
            document["composition"]["motionBlur"] =
                serde_json::json!({"enabled": true, "shutterAngle": 360.0, "shutterPhase": 0.0});
            Ok(crate::LinkedComposition {
                document: fx_schema::EditableFxCompositionDocument::from_json_value(document)
                    .unwrap(),
                next_id: first + 2,
                assets: Vec::new(),
            })
        }
    };
    let (archive, omissions) = convert_supplied(
        root.path(),
        sequence_of(
            "Supplied",
            vec![PrVideoTrack::media([
                blurred(clip_of("absent", 0..TICKS / 2, 0), "absent", 90.0),
                blurred(clip_of("blue", TICKS..3 * TICKS / 2, 0), "blue", 180.0),
            ])],
        ),
        BTreeMap::from([
            (
                MediaId("absent".into()),
                linked("comps.aep", ABSENT, [1920, 1080]),
            ),
            (
                MediaId("blue".into()),
                linked("comps.aep", BLUE, [1920, 1080]),
            ),
        ]),
        &mut resolve,
    )
    .unwrap();
    let aep = fs::canonicalize(root.path()).unwrap().join("comps.aep");
    let occurrences: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| (omission.record.as_str(), omission.reason.clone()))
        .collect();
    assert_eq!(
        occurrences,
        [(
            "absent",
            format!("linked After Effects composition GUID {ABSENT} in {aep:?} forms no editable picture: Dynamic Link item 9 is absent or is not a composition in this AEP")
        )]
    );
    let document = archive.project_json().unwrap();
    assert_eq!(
        (
            composition_shutter(&document),
            shutter_conflicts(&omissions)
        ),
        (
            [
                &serde_json::json!(true),
                &serde_json::json!(180.0),
                &serde_json::json!(0.0)
            ],
            vec![(
                "blue",
                "composition shutter set to 180° by clip blue; clip blue requested 360°"
            )]
        )
    );
    let [group] = linked_groups(&document)[..] else {
        panic!("only the sibling imports: {document}");
    };
    assert_eq!(group["motionBlur"], true);
}

#[test]
fn a_caller_supplied_import_editable_picture_matches_the_built_in_import() {
    // The compatibility route: the caller converts each placement with
    // `import_editable_picture`, as the former CLI resolver did. Its pictures
    // are the built-in import's, identity for identity; only the AE notes
    // stay with the caller.
    let root = tempfile::tempdir().unwrap();
    let project = identity_package(root.path());
    let built_in =
        crate::premiere_to_tesseract(&project, root.path().join("built-in"), None, false).unwrap();
    let mut prepared = BTreeMap::new();
    let mut notes = Vec::new();
    let mut guards = Vec::new();
    let mut resolve = |path: &Path, identity: PrAfterEffectsComposition, first: u64| {
        let source = match prepared.entry(path.to_owned()) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(aftereffects_file::AfterEffects.prepare_linked_import(path)?)
            }
        };
        let composition = source.resolve_composition(&identity.guid_bytes())?;
        let mut converted =
            composition.import_editable_picture(first, &format!("linked-{first}-"))?;
        notes.append(&mut converted.diagnostics);
        let document = converted.take_document()?;
        let assets = std::mem::take(&mut converted.assets);
        let next_id = converted.next_id;
        guards.push(converted);
        Ok(crate::LinkedComposition {
            document,
            next_id,
            assets,
        })
    };
    let supplied = crate::Premiere
        .import_with_linked_compositions(
            &project,
            &root.path().join("supplied"),
            &crate::PremiereImportOptions::default(),
            fx_conv::ConversionMode::Write,
            &mut resolve,
        )
        .unwrap();
    let open = |name: &str| {
        TesseractFile::open(root.path().join(name).join("project.tsrct"))
            .unwrap()
            .project_json()
            .unwrap()
    };
    assert_eq!(open("supplied"), open("built-in"));
    assert!(!notes.is_empty());
    // The built-in import reports the AE notes per linked media record; the
    // caller keeps its own. Every other diagnostic is the same.
    let linked_note = |omission: &Omission| {
        omission
            .reason
            .starts_with("linked After Effects composition: ")
    };
    assert!(built_in.iter().any(linked_note));
    assert!(!supplied.diagnostics.iter().any(linked_note));
    assert_eq!(
        supplied.diagnostics,
        built_in
            .into_iter()
            .filter(|omission| !linked_note(omission))
            .collect::<Vec<_>>()
    );
    assert_eq!(guards.len(), 2);
}

#[test]
fn linked_tail_dissolve_changes_only_outer_clip_opacity() {
    use crate::schema::{PrVideoTransition, PrVideoTransitionKind};
    let sequence = |media: &str| {
        let mut clip = clip_of(media, TICKS / 2..3 * TICKS / 2, TICKS / 3);
        clip.id = Some("linked-tail-clip".into());
        sequence_of("Tail", vec![PrVideoTrack::media([clip])])
    };
    let ((_, _), (before, before_omissions)) = as_video_and_linked(sequence);
    let ((video, _), (after, omissions)) = as_video_and_linked(|media| {
        let mut project = sequence(media);
        project.video_tracks[0].transitions.push(PrVideoTransition {
            id: "linked-tail".into(),
            kind: PrVideoTransitionKind::FilmImpactDissolve,
            start_ticks: TICKS,
            cut_ticks: 3 * TICKS / 2,
            end_ticks: 3 * TICKS / 2,
            outgoing_clip: Some("linked-tail-clip".into()),
            incoming_clip: None,
        });
        project
    });
    assert_eq!(
        before["composition"]["layers"],
        after["composition"]["layers"]
    );
    assert_eq!(clip_entries(&after), clip_entries(&video));
    let group = linked_groups(&after)[0];
    assert_eq!(playback(group), document_clock_seed(group));
    assert_eq!(
        playback(source_group(group).unwrap()),
        [(0, 333), (1000, 1333)]
    );
    let entry = clip_entries(&after)[0];
    assert_eq!(entry["target"]["layerId"], group["id"]);
    assert_eq!(entry["target"]["propertyType"], "opacity");
    assert_eq!(entry["animator"]["keyframes"][0]["layerTime"], 500);
    assert_eq!(entry["animator"]["keyframes"][1]["layerTime"], 1000);
    assert_eq!(entry["animator"]["keyframes"][1]["value"]["value"], 0.0);
    assert_eq!(
        &omissions[..before_omissions.len()],
        before_omissions.as_slice()
    );
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "linked-tail"
                && item.kind == crate::OmissionKind::Approximated)
    );
}
