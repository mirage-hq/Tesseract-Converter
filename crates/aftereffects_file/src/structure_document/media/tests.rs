use std::{cell::Cell, path::Path, sync::Arc};

use fx_schema::{
    Duration, ImageLayer, LayerData as FxLayer, LayerId, MediaFit, Time, TimeRangeProperty,
};
use sha2::{Digest, Sha256};

use super::*;
use crate::{
    media::{MediaDescriptor, MediaDuration, MediaFrameRate},
    rifx::Chunk,
    schema::layer_records::LayerRecord,
    structure::{FootageClassification, FootageSourceKind, ItemKind},
};

fn convert(
    source: &ProjectItem,
    layer: &Layer,
    occurrence: &GroupLayer,
    next_id: u64,
    frame_blending_enabled: bool,
    asset_available: impl FnMut(&MediaAssetRequest) -> bool,
) -> Result<MediaLayerConversion, MediaConversionError> {
    let mut budget = AnimationBudget::default();
    super::convert(
        source,
        layer,
        occurrence,
        super::MediaImportOptions {
            next_id,
            frame_blending_enabled,
            asset_namespace: super::AssetNamespace::STANDALONE,
        },
        asset_available,
        &mut budget,
    )
}

fn descriptor(kind: MediaKind) -> MediaDescriptor {
    MediaDescriptor {
        source_format: *b"MOoV",
        photoshop_source: None,
        width: if kind == MediaKind::Audio { 0 } else { 1920 },
        height: if kind == MediaKind::Audio { 0 } else { 1080 },
        duration: MediaDuration {
            numerator: if kind == MediaKind::StillImage { 0 } else { 5 },
            denominator: 1,
        },
        native_frame_rate: MediaFrameRate {
            integer: if kind == MediaKind::Audio { 0 } else { 24 },
            fractional: 0,
        },
        conform_frame_rate: MediaFrameRate {
            integer: 0,
            fractional: 0,
        },
        display_frame_rate: MediaFrameRate {
            integer: if kind == MediaKind::Audio { 0 } else { 24 },
            fractional: 0,
        },
        pixel_aspect: (1, 1),
        missing_at_save: false,
        audio_sample_rate: if matches!(kind, MediaKind::Audio | MediaKind::AudioVideo) {
            48_000.0
        } else {
            0.0
        },
        sequence_start_frame: 0,
        sequence_end_frame: 0,
        sequence_frame_padding: 0,
        sequence_frame_range_set: false,
        authored_path: "/authored/source.mov".into(),
        target_is_folder: false,
        relative_location: None,
        sequence_names: Vec::new(),
        kind,
    }
}

fn source(kind: MediaKind) -> ProjectItem {
    ProjectItem {
        id: 41,
        name: "source".into(),
        parent_folder: None,
        kind: ItemKind::Footage,
        footage: Some(FootageClassification {
            main_source: FootageSourceKind::File,
            proxy_source: None,
        }),
        solid: None,
        media: Some(Ok(descriptor(kind))),
        native_media: Some(Ok(descriptor(kind))),
    }
}

fn match_name(name: &str) -> Chunk {
    let mut bytes = vec![0; 40];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn audio_levels(values: [f64; 2]) -> Vec<Chunk> {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = 2;
    let numeric = vec![
        Chunk::data(*b"tdb4", meta).unwrap(),
        Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
        Chunk::data(
            *b"cdat",
            values
                .into_iter()
                .flat_map(f64::to_be_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    ];
    vec![Chunk::list(
        *b"tdgp",
        vec![
            match_name("ADBE Audio Group"),
            Chunk::list(
                *b"tdgp",
                vec![
                    match_name("ADBE Audio Levels"),
                    Chunk::list(*b"tdbs", numeric),
                ],
            ),
        ],
    )]
}

fn animated_audio_levels(keys: &[(i32, [f64; 2])]) -> Vec<Chunk> {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = 2;
    meta[12..16].copy_from_slice(&1_000_u32.to_be_bytes());
    meta[68] = 1;
    let mut header = vec![0; 24];
    header[10..12].copy_from_slice(&u16::try_from(keys.len()).unwrap().to_be_bytes());
    header[18..20].copy_from_slice(&88_u16.to_be_bytes());
    header[23] = 4;
    let mut data = Vec::with_capacity(keys.len() * 88);
    for (time, values) in keys {
        let mut key = vec![0; 88];
        key[..4].copy_from_slice(&time.to_be_bytes());
        key[4] = 1;
        key[5] = 1;
        for (index, value) in values.iter().chain([0.0; 8].iter()).enumerate() {
            let start = 8 + index * 8;
            key[start..start + 8].copy_from_slice(&value.to_be_bytes());
        }
        data.extend(key);
    }
    let numeric = vec![
        Chunk::data(*b"tdb4", meta).unwrap(),
        Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
        Chunk::list(
            *b"list",
            vec![
                Chunk::data(*b"lhd3", header).unwrap(),
                Chunk::data(*b"ldat", data).unwrap(),
            ],
        ),
    ];
    vec![Chunk::list(
        *b"tdgp",
        vec![
            match_name("ADBE Audio Group"),
            Chunk::list(
                *b"tdgp",
                vec![
                    match_name("ADBE Audio Levels"),
                    Chunk::list(*b"tdbs", numeric),
                ],
            ),
        ],
    )]
}

fn layer(flags_0: u8, flags_2: u8, content: Vec<Chunk>) -> Layer {
    let mut bytes = vec![0; 164];
    bytes[37] = flags_0;
    bytes[39] = flags_2;
    bytes[131] = 0;
    Layer {
        name: Arc::from("media"),
        record: LayerRecord::decode(&bytes).unwrap(),
        content,
    }
}

fn occurrence() -> GroupLayer {
    super::super::group(
        LayerId::new(2),
        "media".into(),
        Some(LayerId::new(1)),
        TimeRangeProperty::new(Time::ZERO, Duration::from_secs(5.0)),
    )
}

fn collect_images<'a>(layers: &'a [fx_schema::Layer], images: &mut Vec<&'a ImageLayer>) {
    for layer in layers {
        match layer.data() {
            FxLayer::Group(group) => collect_images(&group.layers, images),
            FxLayer::Image(image) => images.push(image),
            _ => {}
        }
    }
}

#[test]
fn image_sequence_packages_original_png_frames_with_native_clock() {
    let mut source = source(MediaKind::ImageSequence);
    source.name = "shot".into();
    let descriptor = source.media.as_mut().unwrap().as_mut().unwrap();
    descriptor.source_format = *b"PNG ";
    descriptor.width = 320;
    descriptor.height = 180;
    descriptor.duration = MediaDuration {
        numerator: 3,
        denominator: 24,
    };
    descriptor.sequence_start_frame = 7;
    descriptor.sequence_end_frame = 9;
    descriptor.sequence_frame_padding = 4;
    descriptor.sequence_frame_range_set = false;
    descriptor.authored_path = "/authored/frames".into();
    descriptor.target_is_folder = true;

    let mut requests = Vec::new();
    let mut budget = AnimationBudget::default();
    let converted = super::convert(
        &source,
        &layer(0, 3, Vec::new()),
        &occurrence(),
        MediaImportOptions {
            next_id: 3,
            frame_blending_enabled: true,
            asset_namespace: super::AssetNamespace::STANDALONE,
        },
        |request| {
            requests.push(request.clone());
            true
        },
        &mut budget,
    )
    .unwrap();

    assert_eq!(
        requests
            .iter()
            .map(|request| request.authored_path.as_str())
            .collect::<Vec<_>>(),
        [
            "/authored/frames/shot_0007.png",
            "/authored/frames/shot_0008.png",
            "/authored/frames/shot_0009.png",
        ]
    );
    assert_eq!(converted.layers.len(), 3);
    let ranges = converted
        .layers
        .iter()
        .map(|layer| match layer {
            FxLayer::Image(image) => {
                let asset = image.source.asset().unwrap();
                assert_eq!(asset.fit, MediaFit::Stretch);
                assert_eq!(
                    asset.frame_rect.unwrap().get(),
                    fx_schema::RectBounds::from_size(320.0, 180.0)
                );
                (
                    image.active_range.start.as_millis(),
                    image.active_range.duration.as_millis(),
                )
            }
            _ => panic!("sequence frame must remain an editable ImageLayer"),
        })
        .collect::<Vec<_>>();
    assert_eq!(ranges, [(0, 42), (42, 41), (83, 42)]);
    assert_eq!(converted.next_id, 6);
    assert!(converted.animations.is_empty());
    assert_eq!(converted.assets, requests);
}

#[test]
fn linked_scopes_with_same_sequence_item_have_distinct_frame_asset_ids() {
    let mut source = source(MediaKind::ImageSequence);
    let descriptor = source.media.as_mut().unwrap().as_mut().unwrap();
    descriptor.source_format = *b"PNG ";
    descriptor.sequence_start_frame = 7;
    descriptor.sequence_end_frame = 8;
    descriptor.sequence_frame_padding = 4;
    descriptor.authored_path = "/authored/frames".into();
    descriptor.target_is_folder = true;

    let mut ids = std::collections::HashSet::new();
    for namespace in ["scope-a", "scope-b"] {
        let mut budget = AnimationBudget::default();
        let mut shape_budget = super::super::shapes::OutputBudget::default();
        let converted = super::convert_resolved(
            &source,
            &layer(0, 3, Vec::new()),
            &occurrence(),
            MediaImportOptions {
                next_id: 3,
                frame_blending_enabled: true,
                asset_namespace: super::AssetNamespace::new(namespace),
            },
            |_| MediaResolution::Asset,
            &mut budget,
            &mut shape_budget,
        )
        .unwrap();
        assert_eq!(converted.assets.len(), 2);
        for request in &converted.assets {
            assert!(
                request
                    .logical_id
                    .as_str()
                    .starts_with(&format!("{namespace}-item-"))
            );
            assert!(
                ids.insert(request.logical_id.clone()),
                "asset IDs must be scope-local"
            );
        }
    }
    assert_eq!(ids.len(), 4);
}

#[test]
fn sequence_native_frame_22_boundary_rounds_without_gap_or_overlap() {
    let mut source = source(MediaKind::ImageSequence);
    source.name = "shot".into();
    let descriptor = source.media.as_mut().unwrap().as_mut().unwrap();
    descriptor.source_format = *b"PNG ";
    descriptor.width = 320;
    descriptor.height = 180;
    descriptor.duration = MediaDuration {
        numerator: 1,
        denominator: 1,
    };
    descriptor.native_frame_rate = MediaFrameRate {
        integer: 24,
        fractional: 0,
    };
    descriptor.sequence_start_frame = 0;
    descriptor.sequence_end_frame = 23;
    descriptor.sequence_frame_padding = 4;
    descriptor.sequence_frame_range_set = true;
    descriptor.authored_path = "/authored/frames".into();
    descriptor.target_is_folder = true;

    let converted = convert(
        &source,
        &layer(0, 3, Vec::new()),
        &occurrence(),
        3,
        true,
        |_| true,
    )
    .unwrap();
    let image = |index: usize| match &converted.layers[index] {
        FxLayer::Image(image) => image,
        _ => panic!("sequence frame must remain an image"),
    };
    assert_eq!(
        (
            image(21).active_range.start.as_millis(),
            image(21).active_range.end().as_millis(),
        ),
        (875, 917)
    );
    assert_eq!(
        (
            image(22).active_range.start.as_millis(),
            image(22).active_range.end().as_millis(),
        ),
        (917, 958)
    );
    assert_eq!(
        (
            image(23).active_range.start.as_millis(),
            image(23).active_range.end().as_millis(),
        ),
        (958, 1_000)
    );
    let native_boundary_ms = 22.0 * 1_000.0 / 24.0;
    assert_eq!(
        Time::from_millis_f64(native_boundary_ms - 0.2).as_millis(),
        916
    );
    assert_eq!(Time::from_millis_f64(native_boundary_ms).as_millis(), 917);
    assert_eq!(
        Time::from_millis_f64(native_boundary_ms + 0.2).as_millis(),
        917
    );
}

#[test]
fn sequence_conform_clock_and_missing_frame_preserve_convertible_siblings() {
    let mut source = source(MediaKind::ImageSequence);
    source.name = "shot".into();
    let descriptor = source.media.as_mut().unwrap().as_mut().unwrap();
    descriptor.source_format = *b"PNG ";
    descriptor.width = 320;
    descriptor.height = 180;
    descriptor.duration = MediaDuration {
        numerator: 3,
        denominator: 12,
    };
    descriptor.native_frame_rate = MediaFrameRate {
        integer: 24,
        fractional: 0,
    };
    descriptor.conform_frame_rate = MediaFrameRate {
        integer: 12,
        fractional: 0,
    };
    descriptor.sequence_start_frame = 7;
    descriptor.sequence_end_frame = 9;
    descriptor.sequence_frame_padding = 4;
    descriptor.sequence_frame_range_set = true;
    descriptor.authored_path = "/authored/frames".into();
    descriptor.target_is_folder = true;

    let converted = convert(
        &source,
        &layer(0, 3, Vec::new()),
        &occurrence(),
        3,
        true,
        |request| !request.authored_path.ends_with("shot_0008.png"),
    )
    .unwrap();
    let images = converted
        .layers
        .iter()
        .map(|layer| match layer {
            FxLayer::Image(image) => image,
            _ => panic!("available sequence siblings must remain images"),
        })
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].name, "media frame 7");
    assert_eq!(images[0].active_range.start.as_millis(), 0);
    assert_eq!(images[0].active_range.duration.as_millis(), 83);
    assert_eq!(images[1].name, "media frame 9");
    assert_eq!(images[1].active_range.start.as_millis(), 167);
    assert_eq!(images[1].active_range.duration.as_millis(), 83);
    assert_eq!(
        converted.next_id, 6,
        "missing frames retain stable reserved IDs"
    );
    assert_eq!(converted.assets.len(), 2);
    assert!(converted.warnings.iter().any(|warning| {
        warning.contains("frame 8") && warning.contains("other frames are preserved")
    }));
    assert!(
        converted
            .warnings
            .iter()
            .any(|warning| warning.contains("conform rate 12 differs from native 24"))
    );
}

#[test]
fn invalid_sequence_range_count_and_names_are_contextual_omissions() {
    let base = || {
        let mut source = source(MediaKind::ImageSequence);
        source.name = "shot".into();
        let descriptor = source.media.as_mut().unwrap().as_mut().unwrap();
        descriptor.source_format = *b"PNG ";
        descriptor.width = 320;
        descriptor.height = 180;
        descriptor.sequence_start_frame = 7;
        descriptor.sequence_end_frame = 9;
        descriptor.sequence_frame_padding = 4;
        descriptor.sequence_frame_range_set = true;
        descriptor.authored_path = "/authored/frames".into();
        descriptor.target_is_folder = true;
        source
    };
    let mut invalid_range = base();
    invalid_range
        .media
        .as_mut()
        .unwrap()
        .as_mut()
        .unwrap()
        .sequence_end_frame = 6;
    let range = convert(
        &invalid_range,
        &layer(0, 3, Vec::new()),
        &occurrence(),
        3,
        true,
        |_| true,
    )
    .unwrap();
    assert!(range.layers.is_empty() && range.assets.is_empty());
    assert_eq!(range.next_id, 3);
    assert!(range.warnings[0].contains("invalid image sequence frame range"));

    let mut unsafe_name = base();
    unsafe_name
        .media
        .as_mut()
        .unwrap()
        .as_mut()
        .unwrap()
        .sequence_names = vec![
        "shot_0007.png".into(),
        "../shot_0008.png".into(),
        "shot_0009.png".into(),
    ];
    let escaped = convert(
        &unsafe_name,
        &layer(0, 3, Vec::new()),
        &occurrence(),
        3,
        true,
        |_| true,
    )
    .unwrap();
    assert!(escaped.layers.is_empty() && escaped.assets.is_empty());
    assert_eq!(escaped.next_id, 3);
    assert!(escaped.warnings[0].contains("unsafe or is not PNG"));

    let mut wrong_count = base();
    wrong_count
        .media
        .as_mut()
        .unwrap()
        .as_mut()
        .unwrap()
        .sequence_names = vec!["shot_0007.png".into(), "shot_0008.png".into()];
    let counted = convert(
        &wrong_count,
        &layer(0, 3, Vec::new()),
        &occurrence(),
        3,
        true,
        |_| true,
    )
    .unwrap();
    assert!(counted.layers.is_empty() && counted.assets.is_empty());
    assert_eq!(counted.next_id, 3);
    assert!(counted.warnings[0].contains("range contains 3 frames"));

    let mut excessive_padding = base();
    excessive_padding
        .media
        .as_mut()
        .unwrap()
        .as_mut()
        .unwrap()
        .sequence_frame_padding = u32::MAX;
    let resolver_calls = Cell::new(0);
    let padded = convert(
        &excessive_padding,
        &layer(0, 3, Vec::new()),
        &occurrence(),
        3,
        true,
        |_| {
            resolver_calls.set(resolver_calls.get() + 1);
            true
        },
    )
    .unwrap();
    assert_eq!(resolver_calls.get(), 0);
    assert!(padded.layers.is_empty() && padded.assets.is_empty());
    assert_eq!(padded.next_id, 3);
    assert!(padded.warnings[0].contains("filename/path exceeds"));
}

#[test]
#[ignore = "requires local licensed AEP_IMAGE_SEQUENCE_SOURCE; source cannot be redistributed"]
fn local_intro_frame_448_packages_source_frame_22_for_each_nested_copy() {
    let bytes =
        std::fs::read(std::env::var_os("AEP_IMAGE_SEQUENCE_SOURCE").expect("licensed source path"))
            .expect("read pinned Intro source");
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
    );
    let project = crate::structure::read_project(&bytes).expect("pinned Intro source parses");
    let mut requests = Vec::new();
    let converted =
        crate::structure_document::to_structural_fx_document_with_media_and_expressions(
            &project,
            Some(705),
            &mut |request| {
                requests.push(request.clone());
                if request.kind == MediaAssetKind::SequenceImage {
                    let source_frame = request
                        .logical_id
                        .as_str()
                        .rsplit('-')
                        .next()
                        .unwrap()
                        .parse::<u32>()
                        .unwrap();
                    if source_frame <= 56 {
                        MediaResolution::AssetDimensions([7_680, 3_200])
                    } else {
                        MediaResolution::AssetDimensions([3_840, 1_600])
                    }
                } else {
                    MediaResolution::Asset
                }
            },
            &crate::expression_samples::ExpressionSamples::default(),
        )
        .expect("fresh master composition import succeeds");

    let frame_22_requests = requests
        .iter()
        .filter(|request| request.logical_id.as_str().ends_with("-sequence-frame-22"))
        .collect::<Vec<_>>();
    assert!(
        frame_22_requests.len() >= 2,
        "both nested source-comp copies must request source frame 22"
    );
    assert!(frame_22_requests.iter().all(|request| {
        request.logical_id == frame_22_requests[0].logical_id
            && Path::new(&request.authored_path)
                .file_name()
                .is_some_and(|name| name == "Sh06_0022.png")
    }));
    let mut images = Vec::new();
    collect_images(converted.document.composition().layers(), &mut images);
    let frame_22_copies = images
        .iter()
        .filter(|image| {
            image
                .source
                .asset()
                .is_some_and(|asset| asset.asset_id.as_str().ends_with("-sequence-frame-22"))
        })
        .collect::<Vec<_>>();
    assert!(
        frame_22_copies.len() >= 2,
        "master comp 705 must contain both nested comp-1223 sequence copies"
    );
    assert!(frame_22_copies.iter().all(|image| {
        image.active_range.start.as_millis() == 917
            && image.active_range.duration.as_millis() == 41
            && image
                .source
                .asset()
                .is_some_and(|asset| asset.fit == MediaFit::Stretch)
    }));
    let sequence_requests = requests
        .iter()
        .filter(|request| request.kind == MediaAssetKind::SequenceImage)
        .collect::<Vec<_>>();
    assert_eq!(
        sequence_requests
            .iter()
            .map(|request| request.logical_id.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        71
    );
    assert_eq!(
        converted
            .assets
            .iter()
            .filter(|request| request.kind == MediaAssetKind::SequenceImage)
            .map(|request| request.logical_id.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        57,
        "unverified half-size frames must not produce bound assets"
    );
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("keeps original 3840x1600 PNG bytes")
            && diagnostic
                .message
                .contains("fixed footage canvas 7680x3200")
    }));
    assert!(
        converted
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .all(|entry| !entry.animator.is_js_script())
    );
}

#[test]
fn resolved_pdf_compatible_ai_replaces_asset_request_with_editable_shapes() {
    let artwork = Arc::new(
        crate::vector_media::decode(include_bytes!(
            "../../../tests/fixtures/vector_media/spec_case_1.ai"
        ))
        .expect("specification-built PDF-compatible AI decodes"),
    );
    let mut source = source(MediaKind::StillImage);
    let descriptor = source.media.as_mut().unwrap().as_mut().unwrap();
    descriptor.authored_path = "artwork.ai".into();
    descriptor.width = 192;
    descriptor.height = 128;
    let mut animation_budget = AnimationBudget::default();
    let mut shape_budget = super::super::shapes::OutputBudget::default();
    let converted = super::convert_resolved(
        &source,
        &layer(0, 3, Vec::new()),
        &occurrence(),
        MediaImportOptions {
            next_id: 3,
            frame_blending_enabled: true,
            asset_namespace: super::AssetNamespace::STANDALONE,
        },
        |_| MediaResolution::Vector(artwork.clone()),
        &mut animation_budget,
        &mut shape_budget,
    )
    .expect("vector media lowers");

    assert_eq!(converted.layers.len(), 3); // Two paints plus the consumed page-clip guide.
    assert!(converted.assets.is_empty());
    assert!(converted.animations.is_empty());
    assert_eq!(converted.next_id, 8); // Guide, two Shapes, two mask IDs.
    assert!(
        converted
            .layers
            .iter()
            .all(|layer| matches!(layer, FxLayer::Shape(_)))
    );
}

#[test]
fn ai_footprint_mismatch_is_omitted_instead_of_guessing_native_crop_selection() {
    let artwork = Arc::new(
        crate::vector_media::decode(include_bytes!(
            "../../../tests/fixtures/vector_media/spec_case_1.ai"
        ))
        .unwrap(),
    );
    // The ordinary test source is not the PDF's 192x128 footprint.
    let source = source(MediaKind::StillImage);
    let converted = super::convert_resolved(
        &source,
        &layer(0, 3, Vec::new()),
        &occurrence(),
        MediaImportOptions {
            next_id: 3,
            frame_blending_enabled: false,
            asset_namespace: super::AssetNamespace::STANDALONE,
        },
        |_| MediaResolution::Vector(artwork.clone()),
        &mut AnimationBudget::default(),
        &mut super::super::shapes::OutputBudget::default(),
    )
    .unwrap();
    assert!(converted.layers.is_empty());
    assert!(converted.assets.is_empty());
    assert_eq!(converted.next_id, 3);
    assert!(converted.warnings[0].contains("page/layer/crop interpretation is unproven"));
}

#[test]
fn unresolved_missing_sequence_and_id_exhaustion_never_emit_unbound_references() {
    let layer = layer(0, 3, Vec::new());
    let unresolved = convert(
        &source(MediaKind::Video),
        &layer,
        &occurrence(),
        3,
        true,
        |_| false,
    )
    .unwrap();
    assert!(unresolved.layers.is_empty());
    assert!(unresolved.animations.is_empty());
    assert!(unresolved.assets.is_empty());
    assert_eq!(unresolved.next_id, 3);

    let mut missing = source(MediaKind::StillImage);
    missing
        .media
        .as_mut()
        .unwrap()
        .as_mut()
        .unwrap()
        .missing_at_save = true;
    let unavailable = convert(&missing, &layer, &occurrence(), 3, true, |_| false).unwrap();
    assert!(unavailable.layers.is_empty() && unavailable.assets.is_empty());
    let restored = convert(&missing, &layer, &occurrence(), 3, true, |_| true).unwrap();
    assert_eq!(restored.layers.len(), 1);
    assert!(
        restored
            .warnings
            .iter()
            .any(|warning| warning.contains("offline when the AEP was saved"))
    );

    let sequence = convert(
        &source(MediaKind::ImageSequence),
        &layer,
        &occurrence(),
        3,
        true,
        |_| true,
    )
    .unwrap();
    assert!(sequence.layers.is_empty() && sequence.assets.is_empty());

    let exhausted = convert(
        &source(MediaKind::AudioVideo),
        &layer,
        &occurrence(),
        u64::MAX - 1,
        true,
        |_| true,
    );
    assert!(matches!(exhausted, Err(MediaConversionError::IdExhausted)));
}

#[test]
fn media_pair_reservation_obeys_generated_id_counter_range() {
    for (start, accepted) in [
        (u64::MAX - 2, true),
        (u64::MAX - 1, false),
        (u64::MAX, false),
    ] {
        let result = convert(
            &source(MediaKind::AudioVideo),
            &layer(0, 3, Vec::new()),
            &occurrence(),
            start,
            true,
            |_| true,
        );
        if accepted {
            let result = result.unwrap();
            assert_eq!(result.layers.len(), 2);
            assert_eq!(result.next_id, u64::MAX);
        } else {
            assert!(matches!(result, Err(MediaConversionError::IdExhausted)));
        }
    }
}

#[test]
fn resolved_av_pair_keeps_both_layers_beyond_ten_thousand_ids() {
    let converted = convert(
        &source(MediaKind::AudioVideo),
        &layer(0, 3, Vec::new()),
        &occurrence(),
        10_000,
        true,
        |_| true,
    )
    .unwrap();
    assert!(
        matches!(&converted.layers[..], [FxLayer::Video(video), FxLayer::Audio(audio)]
        if video.id == LayerId::new(10_000)
            && audio.id == LayerId::new(10_001)
            && video.parent == Some(occurrence().id)
            && audio.parent == Some(occurrence().id))
    );
    assert_eq!(converted.next_id, 10_002);
    assert_eq!(converted.assets.len(), 1);
}

#[test]
fn maps_frame_blending_only_when_composition_master_is_enabled() {
    let layer = layer(1 << 2, (1 << 4) | 3, Vec::new());
    let disabled = convert(
        &source(MediaKind::Video),
        &layer,
        &occurrence(),
        3,
        false,
        |_| true,
    )
    .unwrap();
    let FxLayer::Video(disabled) = &disabled.layers[0] else {
        panic!("video")
    };
    assert_eq!(disabled.frame_blending, None);

    let enabled = convert(
        &source(MediaKind::Video),
        &layer,
        &occurrence(),
        3,
        true,
        |_| true,
    )
    .unwrap();
    let FxLayer::Video(enabled) = &enabled.layers[0] else {
        panic!("video")
    };
    assert_eq!(
        enabled.frame_blending,
        Some(fx_schema::layer::FrameBlendingData::Mode(
            FrameBlendingMode::OpticalFlow
        ))
    );
    assert_eq!(enabled.parent, Some(occurrence().id));
    assert_eq!(enabled.transform, identity_transform());
}

#[test]
fn maps_audio_enabled_and_static_stereo_db_gain() {
    let enabled = layer(0, 3, audio_levels([-6.0, -12.0]));
    let converted = convert(
        &source(MediaKind::Audio),
        &enabled,
        &occurrence(),
        3,
        true,
        |_| true,
    )
    .unwrap();
    let FxLayer::Audio(audio) = &converted.layers[0] else {
        panic!("audio")
    };
    assert!((audio.volume.as_f64() - 10.0_f64.powf(-12.0 / 20.0)).abs() < 1e-12);
    assert!(
        converted
            .warnings
            .iter()
            .any(|warning| warning.contains("channels differ"))
    );
    assert_eq!(converted.assets[0].logical_id.as_str(), "aep-local-item-41");
    assert_eq!(converted.assets[0].kind, MediaAssetKind::Audio);

    let disabled = layer(0, 1, audio_levels([6.0, 6.0]));
    let converted = convert(
        &source(MediaKind::Audio),
        &disabled,
        &occurrence(),
        3,
        true,
        |_| true,
    )
    .unwrap();
    let FxLayer::Audio(audio) = &converted.layers[0] else {
        panic!("audio")
    };
    assert_eq!(audio.volume, LinearGain::ZERO);
}

#[test]
fn coverage_contract_audio_import_stereo_gain_keys() {
    use fx_schema::animator::AnimatorData;

    let layer = layer(
        0,
        3,
        animated_audio_levels(&[(0, [-6.0, -12.0]), (1_000, [-3.0, -9.0])]),
    );
    let converted = convert(
        &source(MediaKind::Audio),
        &layer,
        &occurrence(),
        3,
        true,
        |_| true,
    )
    .unwrap();
    let FxLayer::Audio(audio) = &converted.layers[0] else {
        panic!("audio layer")
    };
    assert!((audio.volume.as_f64() - 10.0_f64.powf(-12.0 / 20.0)).abs() < 1e-12);
    assert_eq!(converted.animations.len(), 1);
    let entry = &converted.animations[0];
    assert_eq!(
        entry.target,
        fx_schema::PropertyTarget::layer(audio.id, PropType::AudioVolume)
    );
    let AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
        panic!("editable AudioVolume keys")
    };
    assert_eq!(
        track
            .keyframes()
            .iter()
            .map(|key| key.layer_time().as_millis())
            .collect::<Vec<_>>(),
        vec![0, 1_000]
    );
    let gains = track
        .keyframes()
        .iter()
        .map(|key| match key.value() {
            fx_schema::PropertyValue::Float(value) => *value,
            value => panic!("expected float gain, got {value:?}"),
        })
        .collect::<Vec<_>>();
    assert!((gains[0] - 10.0_f64.powf(-12.0 / 20.0)).abs() < 1e-12);
    assert!((gains[1] - 10.0_f64.powf(-9.0 / 20.0)).abs() < 1e-12);
    assert!(converted.warnings.iter().any(|warning| {
        warning.contains("animated unequal stereo Audio Levels")
            && warning.contains("quieter channel")
    }));
}

#[test]
fn coverage_contract_audio_import_disabled_keys_remain_muted() {
    let disabled = layer(
        0,
        1,
        animated_audio_levels(&[(0, [-6.0, -12.0]), (1_000, [-3.0, -9.0])]),
    );
    let converted = convert(
        &source(MediaKind::Audio),
        &disabled,
        &occurrence(),
        3,
        true,
        |_| true,
    )
    .unwrap();
    let FxLayer::Audio(audio) = &converted.layers[0] else {
        panic!("audio layer")
    };
    assert_eq!(audio.volume, LinearGain::ZERO);
    assert!(converted.animations.is_empty());
}

#[test]
fn budget_exhaustion_keeps_audio_base_gain_asset_and_layer() {
    let layer = layer(
        0,
        3,
        animated_audio_levels(&[(0, [-6.0, -12.0]), (1_000, [-3.0, -9.0])]),
    );
    let source = source(MediaKind::Audio);
    let occurrence = occurrence();

    let mut baseline_budget = AnimationBudget::default();
    let baseline = super::convert(
        &source,
        &layer,
        &occurrence,
        super::MediaImportOptions {
            next_id: 3,
            frame_blending_enabled: true,
            asset_namespace: super::AssetNamespace::STANDALONE,
        },
        |_| true,
        &mut baseline_budget,
    )
    .unwrap();
    assert_eq!(baseline.animations.len(), 1);
    let serialized_bytes: usize = baseline
        .animations
        .iter()
        .map(super::super::animation_budget::committed_entry_serialized_bytes)
        .sum::<Result<_, _>>()
        .unwrap();
    assert!(serialized_bytes <= baseline_budget.used());

    let mut budget = AnimationBudget::with_limit(0);
    let converted = super::convert(
        &source,
        &layer,
        &occurrence,
        super::MediaImportOptions {
            next_id: 3,
            frame_blending_enabled: true,
            asset_namespace: super::AssetNamespace::STANDALONE,
        },
        |_| true,
        &mut budget,
    )
    .unwrap();
    let FxLayer::Audio(audio) = &converted.layers[0] else {
        panic!("audio layer")
    };
    assert!((audio.volume.as_f64() - 10.0_f64.powf(-12.0 / 20.0)).abs() < 1e-12);
    assert_eq!(audio.source.asset_id.as_str(), "aep-local-item-41");
    assert_eq!(converted.assets.len(), 1);
    assert_eq!(converted.assets[0].logical_id.as_str(), "aep-local-item-41");
    assert!(converted.animations.is_empty());
    assert_eq!(budget.used(), 0);
    assert!(
        converted.warnings.iter().any(|warning| {
            warning.contains("Audio Levels")
                && warning.contains("animation budget")
                && warning.contains("static value retained")
        }),
        "warnings: {:?}",
        converted.warnings
    );
}

#[test]
fn db_conversion_is_linear_amplitude_and_rejects_nonfinite_values() {
    assert_eq!(d_b_to_gain(0.0), Some(1.0));
    assert!((d_b_to_gain(20.0).unwrap() - 10.0).abs() < 1e-12);
    assert_eq!(d_b_to_gain(f64::NAN), None);
}
