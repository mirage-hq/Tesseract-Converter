//! Supplementary admission/boundary controls over unchanged encoder payloads.
//! Original sRGB/ProRes/High10 corpus CLI evidence is private, outside product Git.

mod avc_configuration;

use super::*;

fn be32(bytes: &[u8], offset: usize) -> usize {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}

fn path(bytes: &[u8], tags: &[&[u8; 4]]) -> Vec<usize> {
    let (mut start, mut end) = (0, bytes.len());
    tags.iter()
        .map(|tag| {
            while &bytes[start + 4..start + 8] != *tag {
                start += be32(bytes, start);
                assert!(start < end);
            }
            let offset = start;
            end = start + be32(bytes, start);
            start += 8;
            offset
        })
        .collect()
}

fn srgb_video() -> Vec<u8> {
    let mut bytes = include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec();
    let mut parents = path(
        &bytes,
        &[b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"],
    );
    let entry = parents.last().unwrap() + 16;
    parents.push(entry);
    let end = entry + be32(&bytes, entry);
    assert!(parents[0] > bytes.windows(4).position(|x| x == b"mdat").unwrap());
    // Only insert a container declaration; no encoded frame, clock, range or
    // dimension changes. This is not an independently Adobe-authored oracle.
    let colr = [
        19_u32.to_be_bytes().as_slice(),
        b"colrnclx",
        &[0, 1, 0, 13, 0, 2, 0],
    ]
    .concat();
    bytes.splice(end..end, colr);
    for offset in parents {
        let size = u32::try_from(be32(&bytes, offset) + 19).unwrap();
        bytes[offset..offset + 4].copy_from_slice(&size.to_be_bytes());
    }
    bytes
}

fn prepared_archive_with(
    root: &Path,
    bytes: &[u8],
    configure: impl FnOnce(&mut serde_json::Value),
) -> TesseractFile {
    let mut value = crate::test_support::editable_document();
    let mut sibling = value["composition"]["layers"][0].clone();
    sibling["id"] = json!(3);
    sibling["name"] = json!("Native sibling");
    sibling["source"]["assetId"] = json!("supported");
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(1, sibling);
    configure(&mut value);
    fs::write(root.join("source.mov"), bytes).unwrap();
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("source.mov"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset(
            "supported",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(root.join("input.tsrct"))
        .unwrap()
}

fn prepared_archive(root: &Path, bytes: &[u8]) -> TesseractFile {
    prepared_archive_with(root, bytes, |_| {})
}

fn selected_prepared_archive(root: &Path, bytes: &[u8]) -> TesseractFile {
    prepared_archive_with(root, bytes, |value| {
        value["duration"] = json!(2.0);
        let layer = &mut value["composition"]["layers"][0];
        layer["playback"] = crate::test_support::linear_playback(
            json!({"start": 0, "duration": 1133}),
            json!({"start": 400, "duration": 1133}),
        );
        layer["sourceRange"] = json!({"start": 400, "duration": 1133});
        layer["sourceIntrinsicDuration"] = json!(10_000);
        value["composition"]["layers"][2]["activeRange"]["duration"] = json!(2000);
    })
}

fn with_leading_empty(mut bytes: Vec<u8>) -> Vec<u8> {
    let parents = path(&bytes, &[b"moov", b"trak", b"edts", b"elst"]);
    let edit = parents[3];
    assert_eq!(be32(&bytes, edit + 12), 1);
    // Supplementary container control only: encoded samples and their physical
    // clock stay unchanged. The 33 ms delay is not an integral source frame.
    let empty = [
        33_u32.to_be_bytes().as_slice(),
        (-1_i32).to_be_bytes().as_slice(),
        0x0001_0000_u32.to_be_bytes().as_slice(),
    ]
    .concat();
    bytes.splice(edit + 16..edit + 16, empty);
    for offset in parents {
        let length = u32::try_from(be32(&bytes, offset) + 12).unwrap();
        bytes[offset..offset + 4].copy_from_slice(&length.to_be_bytes());
    }
    bytes[edit + 12..edit + 16].copy_from_slice(&2_u32.to_be_bytes());
    let movie = path(&bytes, &[b"moov", b"mvhd"])[1];
    assert_eq!(be32(&bytes, movie + 20), 1000);
    let movie_duration = u32::try_from(be32(&bytes, movie + 24) + 33).unwrap();
    bytes[movie + 24..movie + 28].copy_from_slice(&movie_duration.to_be_bytes());
    let track = path(&bytes, &[b"moov", b"trak", b"tkhd"])[2];
    let track_duration = u32::try_from(be32(&bytes, track + 28) + 33).unwrap();
    bytes[track + 28..track + 32].copy_from_slice(&track_duration.to_be_bytes());
    bytes
}

fn export_facts(bytes: &[u8]) -> crate::error::Result<crate::media::MediaFacts> {
    crate::media::inspect_export_video_media(
        std::io::Cursor::new(bytes),
        std::io::Cursor::new(bytes),
        bytes.len() as u64,
    )
}

#[test]
fn export_media_delayed_picture_retains_authored_selection_and_source_identity() {
    use crate::{ExportLossDomain, ExportLossSource};

    let bytes = with_leading_empty(
        include_bytes!("../../../../tests/fixtures/feature_timecoded_source.mov").to_vec(),
    );
    let facts = export_facts(&bytes).unwrap();
    assert!(
        matches!(&facts, crate::media::MediaFacts::Video(_)),
        "validated delayed source remains native picture media"
    );
    assert!(
        crate::audio_media::PictureClock::of(&facts).is_none(),
        "packet timing alone must not invent a presentation-padding clock"
    );
    assert!(
        crate::media::inspect_video_media(
            std::io::Cursor::new(&bytes),
            std::io::Cursor::new(&bytes),
            bytes.len() as u64,
        )
        .is_err(),
        "import admission is unchanged"
    );

    let root = tempfile::tempdir().unwrap();
    let file = selected_prepared_archive(root.path(), &bytes);
    let original = file.project_json().unwrap();
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert!(operation.losses().has_native_content);
    assert!(!operation.losses().losses.iter().any(|loss| {
        loss.source == ExportLossSource::Layer(1.into())
            && loss.domain == ExportLossDomain::Picture
            && loss.kind == crate::ExportLossKind::Field(crate::ExportField::PictureMedia)
    }));
    assert!(operation.packing_recipe().is_complete());
    let stage = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("retained"), &[])
        .unwrap();
    let project = read_native(&stage.directory().join("project.prproj"));
    assert_eq!(project.media.len(), 2);
    let clips: Vec<_> = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect();
    assert_eq!(clips.len(), 2);
    let source_in = 400 * crate::schema::TICKS_PER_MILLISECOND;
    let clip = clips
        .iter()
        .find(|clip| clip.in_ticks == source_in)
        .expect("authored selected source");
    assert_eq!(clip.start_ticks, 0);
    assert_eq!(
        clip.end_ticks,
        34 * crate::format::FrameRate::Fps30.ticks_per_frame()
    );
    assert_eq!(clip.out_ticks - clip.in_ticks, clip.end_ticks);
    assert_eq!(clip.playback_rate, 1.0);
    let media = project
        .media
        .values()
        .find(|media| {
            media.video.as_ref().is_some_and(|video| {
                video.intrinsic_ticks == 10_000 * crate::schema::TICKS_PER_MILLISECOND
            })
        })
        .expect("authored source duration");
    assert_eq!(
        media.video.as_ref().unwrap().frame_rate,
        crate::format::FrameRate::Fps30.into()
    );
    assert_eq!(
        fs::read(
            stage
                .directory()
                .join(media.relative_path.as_ref().unwrap())
        )
        .unwrap(),
        bytes
    );
    assert_eq!(file.project_json().unwrap(), original);
    assert_eq!(fs::read(root.path().join("source.mov")).unwrap(), bytes);
}

#[test]
fn edited_repeated_source_uses_one_order_independent_longest_authored_descriptor() {
    use crate::ExportLossSource;

    let bytes = with_leading_empty(
        include_bytes!("../../../../tests/fixtures/feature_timecoded_source.mov").to_vec(),
    );
    for reversed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let file = prepared_archive_with(root.path(), &bytes, |value| {
            value["duration"] = json!(2.0);
            let layers = value["composition"]["layers"].as_array_mut().unwrap();
            layers[0]["playback"] = crate::test_support::linear_playback(
                json!({"start": 0, "duration": 1133}),
                json!({"start": 400, "duration": 1133}),
            );
            layers[0]["sourceRange"] = json!({"start": 400, "duration": 1133});
            layers[0]["sourceIntrinsicDuration"] = json!(10_000);
            layers[1]["source"]["assetId"] = json!("premiere-video-1");
            layers[2]["activeRange"]["duration"] = json!(2000);
            if reversed {
                layers.swap(0, 1);
            }
        });
        let operation = Premiere
            .prepare_export(&file, file.project(), &Default::default())
            .unwrap();
        assert!(operation.losses().losses.iter().any(|loss| {
            loss.source == ExportLossSource::Layer(3.into())
                && loss
                    .omission
                    .reason
                    .contains("sourceIntrinsicDuration 1000 ms conflicts")
                && loss
                    .omission
                    .reason
                    .contains("longest authored extent 10000 ms")
        }));
        let stage = operation
            .stage_with_picture_replacements(
                root.path(),
                &root.path().join(format!("shared-{reversed}")),
                &[],
            )
            .unwrap();
        let project = read_native(&stage.directory().join("project.prproj"));
        assert_eq!(project.media.len(), 1);
        assert_eq!(
            project
                .media
                .values()
                .next()
                .unwrap()
                .video
                .as_ref()
                .unwrap()
                .intrinsic_ticks,
            10_000 * crate::schema::TICKS_PER_MILLISECOND
        );
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .video_occurrences()
                .count(),
            2
        );
        assert_eq!(
            fs::read(stage.directory().join("media/source.mov")).unwrap(),
            bytes
        );
    }
}

#[test]
fn export_media_selected_edit_has_no_invented_picture_or_padding_clock() {
    let mut bytes = include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec();
    let mdhd = path(&bytes, &[b"moov", b"trak", b"mdia", b"mdhd"])[3];
    let scale = u32::try_from(be32(&bytes, mdhd + 20)).unwrap();
    let edit = path(&bytes, &[b"moov", b"trak", b"edts", b"elst"])[3];
    bytes[edit + 16..edit + 20].copy_from_slice(&500_u32.to_be_bytes());
    bytes[edit + 20..edit + 24].copy_from_slice(&(scale / 2).to_be_bytes());
    let movie = path(&bytes, &[b"moov", b"mvhd"])[1];
    bytes[movie + 24..movie + 28].copy_from_slice(&500_u32.to_be_bytes());
    let track = path(&bytes, &[b"moov", b"trak", b"tkhd"])[2];
    bytes[track + 28..track + 32].copy_from_slice(&500_u32.to_be_bytes());
    let facts = export_facts(&bytes).unwrap();
    assert!(matches!(&facts, crate::media::MediaFacts::Video(_)));
    assert!(
        crate::audio_media::PictureClock::of(&facts).is_none(),
        "selected presentation has no packet-only padding clock"
    );
}

#[test]
fn export_media_delayed_malformed_edits_remain_fatal() {
    for (offset, value) in [(12, 3_u32), (16, 0), (20, u32::MAX - 1), (24, 0)] {
        let mut bytes = with_leading_empty(
            include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec(),
        );
        let edit = path(&bytes, &[b"moov", b"trak", b"edts", b"elst"])[3];
        bytes[edit + offset..edit + offset + 4].copy_from_slice(&value.to_be_bytes());
        assert!(
            export_facts(&bytes).is_err(),
            "invalid edit offset {offset}"
        );
    }
    let mut bytes =
        with_leading_empty(include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec());
    let edit = path(&bytes, &[b"moov", b"trak", b"edts", b"elst"])[3];
    bytes[edit + 32..edit + 36].copy_from_slice(&i32::MAX.to_be_bytes());
    assert!(
        export_facts(&bytes).is_err(),
        "playback selects outside the physical source"
    );
}

#[test]
fn export_media_delayed_required_sample_and_codec_checks_remain_fatal() {
    let mut bytes =
        with_leading_empty(include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec());
    let mdhd = path(&bytes, &[b"moov", b"trak", b"mdia", b"mdhd"])[3];
    bytes[mdhd + 24..mdhd + 28].copy_from_slice(&1_u32.to_be_bytes());
    assert!(export_facts(&bytes).is_err(), "sample endpoint mismatch");

    let mut bytes =
        with_leading_empty(include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec());
    let entry = path(
        &bytes,
        &[b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"],
    )[5] + 16;
    bytes[entry + 4..entry + 8].copy_from_slice(b"ap4h");
    assert!(
        export_facts(&bytes).is_err(),
        "AVC bytes are not valid ProRes"
    );

    let mut bytes =
        with_leading_empty(include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec());
    let configuration = bytes.windows(4).position(|part| part == b"avcC").unwrap() + 4;
    bytes[configuration] = 0;
    assert!(
        export_facts(&bytes).is_err(),
        "invalid AVC configuration version"
    );
}

#[test]
fn export_media_delayed_corrupt_source_identity_is_fatal() {
    let root = tempfile::tempdir().unwrap();
    let media =
        with_leading_empty(include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec());
    let file = prepared_archive(root.path(), &media);
    let input = root.path().join("input.tsrct");
    let mut bytes = fs::read(&input).unwrap();
    let start = bytes
        .windows(media.len())
        .position(|part| part == media)
        .unwrap();
    let mdat = media.windows(4).position(|part| part == b"mdat").unwrap();
    bytes[start + mdat + 40] ^= 1;
    fs::write(input, bytes).unwrap();
    assert!(Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .is_err());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn export_media_srgb_records_picture_loss_and_keeps_supported_sibling_boundary() {
    use crate::{ExportLossDomain, ExportLossSource};

    let root = tempfile::tempdir().unwrap();
    let bytes = srgb_video();
    let facts = export_facts(&bytes).unwrap();
    assert!(
        matches!(&facts, crate::media::MediaFacts::UnsupportedVideo(video) if video.timing.is_some())
    );
    assert!(crate::audio_media::PictureClock::of(&facts).is_some());
    let file = prepared_archive(root.path(), &bytes);
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert!(operation.losses().has_native_content);
    assert!(operation.losses().losses.iter().any(|loss| loss.source
        == ExportLossSource::Layer(1.into())
        && loss.domain == ExportLossDomain::Picture
        && loss.kind == crate::ExportLossKind::Field(crate::ExportField::PictureMedia)
        && loss.omission.reason.contains("sRGB")));
    let recipe = operation.packing_recipe();
    assert!(recipe.is_complete());
    assert!(recipe.boundaries().iter().any(|b| b.layer == 1.into()));
    let stage = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("native-only"), &[])
        .unwrap();
    let project = read_native(&stage.directory().join("project.prproj"));
    assert_eq!(project.media.len(), 1);
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
    assert_eq!(
        fs::read(stage.directory().join("media/video-30fps.mp4")).unwrap(),
        include_bytes!("../../../../tests/fixtures/video-30fps.mp4")
    );
    assert_eq!(
        file.project_json().unwrap()["composition"]["layers"][0]["source"]["assetId"],
        "premiere-video-1"
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn export_media_supported_source_has_no_media_picture_loss() {
    use crate::ExportLossDomain;

    let root = tempfile::tempdir().unwrap();
    let file = prepared_archive(
        root.path(),
        include_bytes!("../../../../tests/fixtures/video-30fps.mov"),
    );
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert!(!operation
        .losses()
        .losses
        .iter()
        .any(|loss| loss.domain == ExportLossDomain::Picture));
}

#[test]
fn export_media_malformed_prores_retag_is_fatal_not_an_ae_loss() {
    let root = tempfile::tempdir().unwrap();
    let mut bytes = include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec();
    let parents = path(
        &bytes,
        &[b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"],
    );
    let entry = parents.last().unwrap() + 16;
    bytes[entry + 4..entry + 8].copy_from_slice(b"ap4h");
    let file = prepared_archive(root.path(), &bytes);
    assert!(Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .is_err());
}

#[test]
fn export_media_invalid_clock_remains_fatal_before_picture_loss() {
    let root = tempfile::tempdir().unwrap();
    let mut bytes = srgb_video();
    let edit = path(&bytes, &[b"moov", b"trak", b"edts", b"elst"])[3];
    bytes[edit + 24..edit + 28].copy_from_slice(&0_u32.to_be_bytes());
    let file = prepared_archive(root.path(), &bytes);
    assert!(Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .is_err());
}

#[test]
fn export_media_corrupt_unsupported_asset_is_fatal() {
    let root = tempfile::tempdir().unwrap();
    let media = srgb_video();
    let file = prepared_archive(root.path(), &media);
    let input = root.path().join("input.tsrct");
    let mut bytes = fs::read(&input).unwrap();
    let start = bytes
        .windows(media.len())
        .position(|part| part == media)
        .unwrap();
    let mdat = media.windows(4).position(|part| part == b"mdat").unwrap();
    bytes[start + mdat + 40] ^= 1;
    fs::write(input, bytes).unwrap();
    assert!(Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .is_err());
}
