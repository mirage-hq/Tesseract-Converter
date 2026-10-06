//! Native saved timelines exercise safe omissions without admitting bad media.
//! Codec-tag mutation is supplementary; the original corpus supplies real codecs.

use crate::{Premiere, PremiereImportOptions};
use fx_conv::{ConversionMode, ImportToTesseract, MediaStatus};
use serde_json::json;
use std::{fs, path::Path};
use tesseract_file::TesseractFile;

const ADJACENT: &str = "feature_adjacent_cut_strict.prproj";
const TARGET: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const FIRST: &str = "feature_two_tracks_gap_clip_a.mp4";
const SECOND: &str = "feature_two_tracks_gap_clip_b.mp4";

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

fn videos_in(layers: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    layers
        .iter()
        .flat_map(|layer| {
            if layer["type"] == "Video" {
                vec![layer]
            } else if let Some(children) = layer["layers"].as_array() {
                videos_in(children)
            } else {
                Vec::new()
            }
        })
        .collect()
}

fn unsupported_codec(bytes: &[u8]) -> Vec<u8> {
    let mut bytes = bytes.to_vec();
    let tags: Vec<_> = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(offset, tag)| (tag == b"avc1").then_some(offset))
        .collect();
    assert!(!tags.is_empty());
    for offset in tags {
        bytes[offset..offset + 4].copy_from_slice(b"rle ");
    }
    bytes
}

#[test]
fn media_siblings_native_adjacent_cut_omits_missing_or_unsupported_video() {
    assert_native_adjacent_cut_siblings(false);
}

#[test]
fn media_siblings_relinked_native_cut_omits_missing_or_unsupported_video() {
    assert_native_adjacent_cut_siblings(true);
}

fn assert_native_adjacent_cut_siblings(use_relink: bool) {
    for missing in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join(ADJACENT);
        fs::copy(fixtures().join(ADJACENT), &project).unwrap();
        let healthy = fs::read(fixtures().join(SECOND)).unwrap();
        fs::write(directory.path().join(SECOND), &healthy).unwrap();
        let reason = if missing {
            "missing media"
        } else {
            "video codec \"rle \" is unsupported"
        };
        if !missing {
            fs::write(
                directory.path().join(FIRST),
                unsupported_codec(&fs::read(fixtures().join(FIRST)).unwrap()),
            )
            .unwrap();
        }
        let options = PremiereImportOptions {
            sequence: Some(TARGET.into()),
        };
        // Change only the healthy sibling's path, retaining the native cut and
        // original bytes. Its explicit binding must coexist with safe omissions.
        let relink = use_relink.then(|| {
            let authored = r"E:\collected\healthy.mp4";
            let xml = crate::tests::support::prproj_xml(&project).replace(
                &format!("<RelativePath>{SECOND}</RelativePath>"),
                &format!("<FilePath>{authored}</FilePath>"),
            );
            crate::test_support::write_prproj(&project, &xml);
            crate::ValidatedMediaRelink::new(crate::MediaRelink {
                version: 1,
                source: fx_conv::MediaMapSource {
                    format: "premiere".into(),
                    sha256: crate::hash::hash(&project).unwrap(),
                    target: TARGET.into(),
                },
                bindings: vec![crate::MediaRelinkBinding {
                    media_uid: "93318ede-9c1e-4e0a-94ea-da42c59425c6".into(),
                    authored_path: authored.into(),
                    local_path: directory.path().join(SECOND),
                    sha256: crate::hash::hash(&directory.path().join(SECOND)).unwrap(),
                }],
            })
            .unwrap()
        });
        let inventory = if let Some(relink) = &relink {
            Premiere.inspect_media_with_relink(&project, &options, relink)
        } else {
            Premiere.inspect_media(&project, &options, None)
        }
        .unwrap();
        assert!(inventory
            .media
            .iter()
            .any(|media| media.name == FIRST && media.status != MediaStatus::Supported));
        assert!(
            crate::tesseract_output::require_video_admission(
                &inventory,
                &project.canonicalize().unwrap()
            )
            .is_err(),
            "inventory readiness stays strict"
        );
        let mut reports = Vec::new();
        for mode in [ConversionMode::Check, ConversionMode::Write] {
            let output = directory.path().join(format!("result-{mode:?}"));
            let report = if let Some(relink) = &relink {
                Premiere.import_with_media_relink(&project, &output, &options, mode, relink)
            } else {
                Premiere.import_to_tesseract(&project, &output, &options, mode)
            }
            .unwrap();
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|note| note.record == "VideoClipTrackItem:145"
                        && note.reason.contains(reason)),
                "{report:?}"
            );
            if mode == ConversionMode::Check {
                assert!(!output.exists());
            } else {
                let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
                let document = archive.project_json().unwrap();
                let layers = document["composition"]["layers"].as_array().unwrap();
                let pictures = videos_in(layers);
                assert_eq!(pictures.len(), 1);
                assert!(layers
                    .iter()
                    .all(|layer| layer["type"] == "Video"
                        || layer["name"] == "Premiere black canvas"));
                let picture = pictures[0];
                assert_eq!(picture["type"], "Video");
                assert_eq!(
                    *crate::test_support::layer_range(picture),
                    json!({"start": 2000, "duration": 2000})
                );
                assert_eq!(
                    picture["sourceRange"],
                    json!({"start": 0, "duration": 2000})
                );
                assert_eq!(picture["volume"], 0.0);
                assert_eq!(archive.metadata().assets.len(), 1);
                let asset = picture["source"]["assetId"].as_str().unwrap();
                assert_eq!(
                    archive
                        .asset(asset)
                        .unwrap()
                        .read_verified_bytes(healthy.len() as u64)
                        .unwrap(),
                    healthy
                );
            }
            reports.push(report);
        }
        assert_eq!(reports[0], reports[1]);
    }
}

#[test]
fn media_siblings_malformed_present_video_still_aborts_healthy_native_cut() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join(ADJACENT);
    fs::copy(fixtures().join(ADJACENT), &project).unwrap();
    fs::copy(fixtures().join(SECOND), directory.path().join(SECOND)).unwrap();
    fs::write(directory.path().join(FIRST), b"not a movie").unwrap();
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = directory.path().join(format!("result-{mode:?}"));
        let error = Premiere
            .import_to_tesseract(
                &project,
                &output,
                &PremiereImportOptions {
                    sequence: Some(TARGET.into()),
                },
                mode,
            )
            .unwrap_err();
        assert!(error.to_string().contains("failed admission"), "{error}");
        assert!(!output.exists());
    }
}

#[test]
fn media_siblings_native_failed_matte_does_not_expose_its_target() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory
        .path()
        .join("feature_track_matte_key_26_5_strict.prproj");
    let xml =
        crate::format::read_xml(&fixtures().join("feature_track_matte_key_26_5_strict.prproj"))
            .unwrap();
    // Path-only relocation makes every authored candidate for this native
    // still matte unavailable; live absolute aliases must not rescue it.
    crate::test_support::write_prproj(
        &project,
        &xml.replace("tmk_alpha_rect.png", "missing_alpha_rect.png"),
    );
    for file in [
        "feature_linked_av_source.mp4",
        "feature_timecoded_source.mp4",
        "tmk_red.png",
        "tmk_green.png",
        "tmk_blue.png",
        "tmk_grey128.png",
    ] {
        fs::copy(fixtures().join(file), directory.path().join(file)).unwrap();
    }
    // The saved still matte's absence must remove its keyed consumers,
    // not turn them into opaque picture.
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = directory.path().join(format!("result-{mode:?}"));
        let report = Premiere
            .import_to_tesseract(
                &project,
                &output,
                &PremiereImportOptions {
                    sequence: Some("3776e3eb-791f-4e6a-a2bb-7e77eff235ef".into()),
                },
                mode,
            )
            .unwrap();
        assert!(
            report
                .diagnostics
                .iter()
                .any(|note| note.record == "VideoClipTrackItem:129"
                    && note.reason.contains("matte track 2 holds no clip")),
            "{report:?}"
        );
        if mode == ConversionMode::Write {
            let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
            let document = archive.project_json().unwrap();
            let pictures: Vec<_> = document["composition"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|layer| layer["type"] == "Video")
                .collect();
            assert!(!pictures.is_empty(), "unmasked supported siblings survive");
            assert!(
                !pictures
                    .iter()
                    .any(|layer| *crate::test_support::layer_range(layer)
                        == json!({"start": 0, "duration": 2000})),
                "the failed matte's consumer must not paint"
            );
        } else {
            assert!(!output.exists());
        }
    }
}

// Supplementary model placements reuse the native adjacent-cut media/occurrences.
// They do not claim native authoring proof for a new matte or nesting fixture.
#[test]
fn media_siblings_unavailable_video_matte_omits_consumers_including_nests() {
    use crate::schema::{PrMatteChannel, PrTrackMatte, PrVideoItem, PrVideoTrack};
    use std::sync::Arc;
    for missing in [true, false] {
        for in_nest in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let project = directory.path().join(ADJACENT);
            fs::copy(fixtures().join(ADJACENT), &project).unwrap();
            fs::copy(fixtures().join(SECOND), directory.path().join(SECOND)).unwrap();
            if !missing {
                fs::write(
                    directory.path().join(FIRST),
                    unsupported_codec(&fs::read(fixtures().join(FIRST)).unwrap()),
                )
                .unwrap();
            }
            let project = project.canonicalize().unwrap();
            let (parsed, mut omissions) =
                crate::format::PrProjectFile::load_import(&project, Some(TARGET)).unwrap();
            let (mut sequences, media) = parsed.into_parts();
            let mut sequence = sequences.pop().unwrap();
            let matte = sequence.video_tracks[0].clip(0).clone();
            let healthy = sequence.video_tracks[0].clip(1).clone();
            let mut target = matte.clone();
            target.id = Some("masked-probe".into());
            target.media = healthy.media.clone();
            target.track_matte = Some(PrTrackMatte {
                track_index: 1,
                channel: PrMatteChannel::Alpha,
            });
            sequence.video_tracks = vec![
                PrVideoTrack {
                    items: vec![PrVideoItem::Media(target), PrVideoItem::Media(healthy)],
                    nests: Vec::new(),
                    transitions: Vec::new(),
                },
                PrVideoTrack {
                    items: vec![PrVideoItem::Media(matte)],
                    nests: Vec::new(),
                    transitions: Vec::new(),
                },
            ];
            if in_nest {
                let mut outer = sequence.clone();
                outer.video_tracks = vec![PrVideoTrack {
                    items: Vec::new(),
                    nests: vec![super::support::nest_of(
                        sequence,
                        0..4 * crate::schema::TICKS,
                        0,
                    )],
                    transitions: Vec::new(),
                }];
                sequence = outer;
            }
            let pending = crate::tesseract_output::convert_premiere_sequence(
                &project,
                sequence,
                Arc::new(media),
                &mut omissions,
            )
            .unwrap()
            .unwrap();
            assert!(
                omissions.iter().any(|note| note.record == "masked-probe"
                    && note.reason.contains("matte track 1 holds no clip")),
                "{omissions:?}"
            );
            let output = directory.path().join("result.tsrct");
            pending.write_to_staging(&output).unwrap();
            let archive = TesseractFile::open(output).unwrap();
            let document = archive.project_json().unwrap();
            let layers = document["composition"]["layers"].as_array().unwrap();
            let pictures = videos_in(layers);
            assert_eq!(pictures.len(), 1, "no target or failed matte may paint");
            assert_eq!(archive.metadata().assets.len(), 1);
            assert_eq!(
                *crate::test_support::layer_range(pictures[0]),
                json!({"start": 2000, "duration": 2000})
            );
        }
    }
}
