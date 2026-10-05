#[cfg(feature = "ffmpeg-library")]
use super::support::{edit_record, read_xml, tesseract_to_premiere, write_prproj};
use super::support::{first_project, premiere_to_tesseract};
use serde_json::{json, Value};
use std::{io::Read, path::Path};
use tesseract_file::TesseractFile;

fn media_layers(file: &TesseractFile) -> Vec<Value> {
    file.project_json().unwrap()["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| matches!(layer["type"].as_str(), Some("Video" | "Audio")))
        .cloned()
        .collect()
}

fn decoded_stereo(path: &Path) -> (u32, Vec<[f32; 2]>) {
    use symphonia::core::{
        audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
        meta::MetadataOptions, probe::Hint,
    };
    let source = MediaSourceStream::new(
        Box::new(std::fs::File::open(path).unwrap()),
        Default::default(),
    );
    let mut format = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            source,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .unwrap()
        .format;
    let track = format.default_track().unwrap();
    let id = track.id;
    let rate = track.codec_params.sample_rate.unwrap();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .unwrap();
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(error) => panic!("{error}"),
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = decoder.decode(&packet).unwrap();
        assert_eq!(decoded.spec().rate, rate);
        assert_eq!(decoded.spec().channels.count(), 2);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        samples.extend(
            buffer
                .samples()
                .chunks_exact(2)
                .map(|frame| [frame[0], frame[1]]),
        );
    }
    (rate, samples)
}

fn assert_selected_samples(bytes: &[u8], rate: u32, samples: &[[f32; 2]], channel: usize) {
    assert_eq!(u16::from_le_bytes(bytes[20..22].try_into().unwrap()), 3);
    assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), rate);
    assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 32);
    let expected: Vec<_> = samples
        .iter()
        .flat_map(|frame| frame[channel].to_le_bytes())
        .collect();
    assert_eq!(wav_data(bytes), expected);
    assert!(
        samples.iter().any(|frame| frame[0] != frame[1]),
        "fixture must distinguish left/right"
    );
}

#[test]
fn compressed_channel_native_selections_keep_editable_ranges_and_packaged_samples() {
    let temp = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let output = temp.path().join("import");
    let omissions = premiere_to_tesseract(
        fixtures.join("feature_compressed_channel.prproj"),
        &output,
        Some("d40c25d5-0359-473e-974e-24f423007763"),
        false,
    )
    .unwrap();
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let layers = media_layers(&file);
    assert_eq!(layers.len(), 3, "{omissions:?}");
    assert_eq!(file.metadata().assets.len(), 1);
    let (rate, samples) = decoded_stereo(&fixtures.join("channel-selection-stereo.mp3"));
    assert_eq!(samples.len(), 27916);
    for ((layer, start), level) in layers.iter().zip([11545, 18252, 24291]).zip([
        0.068_930_536_509,
        0.099_801_637_232,
        0.083_579_503,
    ]) {
        assert_eq!(layer["type"], "Audio");
        assert_eq!(
            crate::test_support::layer_range(layer),
            &json!({"start":start,"duration":500})
        );
        assert_eq!(layer["sourceRange"], json!({"start":0,"duration":500}));
        // Saved StartKeyframe anchors (not CurrentValue readback), with the mono mix.
        let gain = 0.871_435_940_266
            * 0.678_937_554_359
            * std::f64::consts::FRAC_1_SQRT_2
            * (level / 0.177_827_939_391);
        // Compare the persisted JSON number through the archive's parser too.
        let persisted_gain: Value =
            serde_json::from_str(&serde_json::to_string(&gain).unwrap()).unwrap();
        assert_eq!(layer["volume"], persisted_gain);
        let id = layer["source"]["assetId"].as_str().unwrap();
        assert_eq!(
            file.metadata().assets[id].kind,
            tesseract_file::AssetKind::Audio
        );
        let mut bytes = Vec::new();
        file.asset(id)
            .unwrap()
            .open()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_selected_samples(&bytes, rate, &samples, 0);
    }
    assert_ne!(layers[0]["volume"], layers[1]["volume"]);
    assert_ne!(layers[1]["volume"], layers[2]["volume"]);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn compressed_channel_left_right_packaging_keeps_trims_gain_and_mono_export() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (name, priming) in [("audio-stereo.mp3", 0), ("audio-stereo.m4a", 1024)] {
        let temp = tempfile::tempdir().unwrap();
        // Supplementary media/range/channel discriminator, not another Adobe save.
        let mut xml = read_xml(&fixtures.join("feature_compressed_channel.prproj"))
            .replace("channel-selection-stereo.mp3", name)
            .replace("147731472000", "50803200000")
            .replace("127135008000", "50803200000");
        for (id, start) in [
            ("1199", 2932580851200_i64),
            ("1206", 4636189958400),
            ("1217", 6170285721600),
        ] {
            edit_record(
                &mut xml,
                &format!("<AudioClipTrackItem ObjectID=\"{id}\""),
                "</AudioClipTrackItem>",
                |record| {
                    let end_start = record.find("<End>").unwrap() + 5;
                    let end_end = end_start + record[end_start..].find("</End>").unwrap();
                    format!(
                        "{}{}{}",
                        &record[..end_start],
                        start + 50803200000,
                        &record[end_end..]
                    )
                },
            );
        }
        edit_record(
            &mut xml,
            "<SecondaryContent ObjectID=\"7597\"",
            "</SecondaryContent>",
            |record| {
                record.replace(
                    "<ChannelIndex>0</ChannelIndex>",
                    "<ChannelIndex>1</ChannelIndex>",
                )
            },
        );
        // Keep a nonzero source In and a shorter independently editable placement.
        edit_record(
            &mut xml,
            "<AudioClip ObjectID=\"1791\"",
            "</AudioClip>",
            |record| {
                record
                    .replace("<InPoint>0</InPoint>", "<InPoint>12700800000</InPoint>")
                    .replace(
                        "<OutPoint>50803200000</OutPoint>",
                        "<OutPoint>38102400000</OutPoint>",
                    )
            },
        );
        edit_record(
            &mut xml,
            "<AudioClipTrackItem ObjectID=\"1206\"",
            "</AudioClipTrackItem>",
            |record| record.replace("<End>4686993158400</End>", "<End>4661591558400</End>"),
        );
        let input = temp.path().join("selected.prproj");
        write_prproj(&input, &xml);
        std::fs::copy(fixtures.join(name), temp.path().join(name)).unwrap();
        let output = temp.path().join("import");
        let omissions = premiere_to_tesseract(
            &input,
            &output,
            Some("d40c25d5-0359-473e-974e-24f423007763"),
            false,
        )
        .unwrap();
        let archive = first_project(&output);
        let file = TesseractFile::open(&archive).unwrap();
        let layers = media_layers(&file);
        assert_eq!(layers.len(), 3, "{name}: {omissions:?}");
        assert_eq!(file.metadata().assets.len(), 2);
        let (rate, raw) = decoded_stereo(&fixtures.join(name));
        // Pinned M4A facts: elst media_time=1024 at 48 kHz, presentation=200 ms.
        if priming != 0 {
            let bytes = std::fs::read(fixtures.join(name)).unwrap();
            let edit = bytes.windows(4).position(|tag| tag == b"elst").unwrap();
            assert_eq!(
                u32::from_be_bytes(bytes[edit + 12..edit + 16].try_into().unwrap()),
                9600
            );
            let movie = bytes.windows(4).position(|tag| tag == b"mvhd").unwrap();
            assert_eq!(
                u32::from_be_bytes(bytes[movie + 16..movie + 20].try_into().unwrap()),
                48000
            );
            assert_eq!(
                i32::from_be_bytes(bytes[edit + 16..edit + 20].try_into().unwrap()),
                1024
            );
            assert_eq!(rate, 48000);
        }
        let samples = &raw[priming..priming + 9600];
        assert_eq!(layers[1]["sourceRange"], json!({"start":50,"duration":100}));
        assert_eq!(
            crate::test_support::layer_range(&layers[1]),
            &json!({"start":18252,"duration":100})
        );
        for (layer, channel) in layers.iter().zip([0, 1, 0]) {
            let id = layer["source"]["assetId"].as_str().unwrap();
            let mut bytes = Vec::new();
            file.asset(id)
                .unwrap()
                .open()
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            assert_selected_samples(&bytes, rate, samples, channel);
        }
        let native = temp.path().join("export");
        tesseract_to_premiere(&archive, &native, false).unwrap();
        let again = temp.path().join("again");
        premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
        let roundtrip = TesseractFile::open(first_project(&again)).unwrap();
        for (actual, expected) in media_layers(&roundtrip).iter().zip(&layers) {
            assert_eq!(
                crate::test_support::layer_range(actual),
                crate::test_support::layer_range(expected)
            );
            assert_eq!(actual["sourceRange"], expected["sourceRange"]);
            assert_eq!(actual["volume"], expected["volume"]);
        }
    }
}

fn wav_data(bytes: &[u8]) -> &[u8] {
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset..offset + 4] == b"data" {
            return &bytes[offset + 8..offset + 8 + size];
        }
        offset += 8 + size + size % 2;
    }
    panic!("WAV data missing")
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn adobe_merged_clip_keeps_picture_and_selected_sound_offsets_through_export() {
    let temp = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let output = temp.path().join("import");
    let omissions = premiere_to_tesseract(
        fixtures.join("feature_merged_offset.prproj"),
        &output,
        Some("b532b029-e1d0-4a6c-ae91-f429e5e1b771"),
        false,
    )
    .unwrap();
    let archive = first_project(&output);
    let file = TesseractFile::open(&archive).unwrap();
    let layers = media_layers(&file);
    assert_eq!(layers.len(), 3, "{omissions:?}");
    assert_eq!(file.metadata().assets.len(), 3);
    assert_eq!(layers[0]["type"], "Video");
    assert_eq!(
        crate::test_support::layer_range(&layers[0]),
        &json!({"start":1133,"duration":10000})
    );
    assert_eq!(
        layers[0]["sourceRange"],
        json!({"start":0,"duration":10000})
    );
    let source = std::fs::read(fixtures.join("nest_tone_stereo_8s.wav")).unwrap();
    for (channel, layer) in layers[1..].iter().enumerate() {
        assert_eq!(layer["type"], "Audio");
        assert_eq!(
            crate::test_support::layer_range(layer),
            &json!({"start":1133,"duration":7000})
        );
        assert_eq!(layer["sourceRange"], json!({"start":1000,"duration":7000}));
        assert!(
            (layer["volume"].as_f64().unwrap() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12
        );
        let id = layer["source"]["assetId"].as_str().unwrap();
        let mut bytes = Vec::new();
        file.asset(id)
            .unwrap()
            .open()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1);
        let expected: Vec<u8> = wav_data(&source)
            .chunks_exact(4)
            .flat_map(|frame| frame[channel * 2..channel * 2 + 2].iter().copied())
            .collect();
        assert_eq!(wav_data(&bytes), expected);
    }
    let native = temp.path().join("native");
    tesseract_to_premiere(&archive, &native, false).unwrap();
    assert!(!read_xml(&native.join("project.prproj")).contains("MZ.MergeClipUtils"));
    let again = temp.path().join("again");
    premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
    let roundtrip = TesseractFile::open(first_project(&again)).unwrap();
    let reopened = media_layers(&roundtrip);
    assert_eq!(reopened.len(), 3);
    for (actual, expected) in reopened.iter().zip(&layers) {
        assert_eq!(
            crate::test_support::layer_range(actual),
            crate::test_support::layer_range(expected)
        );
        assert_eq!(actual["sourceRange"], expected["sourceRange"]);
        assert_eq!(actual["volume"], expected["volume"]);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn merged_missing_source_channel_omits_only_that_sound_without_stereo_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut xml = read_xml(&fixtures.join("feature_merged_offset.prproj"));
    edit_record(
        &mut xml,
        "<SecondaryContent ObjectID=\"966\"",
        "</SecondaryContent>",
        |record| {
            record.replace(
                "<ChannelIndex>1</ChannelIndex>",
                "<ChannelIndex>2</ChannelIndex>",
            )
        },
    );
    let input = temp.path().join("missing-channel.prproj");
    write_prproj(&input, &xml);
    for name in [
        "feature_two_tracks_gap_clip_a.mp4",
        "nest_tone_stereo_8s.wav",
    ] {
        std::fs::copy(fixtures.join(name), temp.path().join(name)).unwrap();
    }
    let output = temp.path().join("import");
    let omissions = premiere_to_tesseract(
        input,
        &output,
        Some("b532b029-e1d0-4a6c-ae91-f429e5e1b771"),
        false,
    )
    .unwrap();
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let layers = media_layers(&file);
    assert_eq!(layers.len(), 2, "{omissions:?}");
    assert_eq!(layers[0]["type"], "Video");
    assert_eq!(layers[1]["type"], "Audio");
    assert!(
        omissions.iter().any(|item| item.record == "347"
            && item
                .reason
                .contains("mono selection must name channel 0 or 1")),
        "{omissions:?}"
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn merged_source_group_gain_selects_one_chain_and_requires_a_group() {
    let temp = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut xml = read_xml(&fixtures.join("feature_merged_offset.prproj"));
    // Supplementary structural discriminator, not a new Adobe save: copy the
    // fixture's fader/parameter form into source group 1 and set its gain to 0.5.
    let record = |tag: &str, id: &str| {
        let start = xml.find(&format!("<{tag} ObjectID=\"{id}\"")).unwrap();
        let closing = format!("</{tag}>");
        let end = start + xml[start..].find(&closing).unwrap() + closing.len();
        xml[start..end].to_owned()
    };
    let fader = record("AudioFader", "605")
        .replace("ObjectID=\"605\"", "ObjectID=\"91000\"")
        .replace("ObjectRef=\"967\"", "ObjectRef=\"91001\"")
        .replace("ObjectRef=\"968\"", "ObjectRef=\"91002\"");
    let volume = record("AudioComponentParam", "967")
        .replace("ObjectID=\"967\"", "ObjectID=\"91001\"")
        .replace(
            "<Name>Volume</Name>",
            "<Name>Volume</Name><CurrentValue>0.5</CurrentValue>",
        );
    let mute =
        record("AudioComponentParam", "968").replace("ObjectID=\"968\"", "ObjectID=\"91002\"");
    xml = xml.replace(
        "</PremiereData>",
        &format!("{fader}{volume}{mute}</PremiereData>"),
    );
    edit_record(
        &mut xml,
        "<AudioComponentChain ObjectID=\"955\"",
        "</AudioComponentChain>",
        |record| {
            record.replace("<ComponentChain Version=\"3\">",
            "<ComponentChain Version=\"3\"><Components Version=\"1\"><Component Index=\"0\" ObjectRef=\"91000\" /></Components>")
        },
    );
    for name in [
        "feature_two_tracks_gap_clip_a.mp4",
        "nest_tone_stereo_8s.wav",
    ] {
        std::fs::copy(fixtures.join(name), temp.path().join(name)).unwrap();
    }
    let input = temp.path().join("source-group.prproj");
    write_prproj(&input, &xml);
    let output = temp.path().join("gain");
    let omissions = premiere_to_tesseract(
        &input,
        &output,
        Some("b532b029-e1d0-4a6c-ae91-f429e5e1b771"),
        false,
    )
    .unwrap();
    let layers = media_layers(&TesseractFile::open(first_project(&output)).unwrap());
    assert_eq!(layers.len(), 3, "{omissions:?}");
    let mono = std::f64::consts::FRAC_1_SQRT_2;
    assert!((layers[1]["volume"].as_f64().unwrap() - mono).abs() < 1e-12);
    assert!((layers[2]["volume"].as_f64().unwrap() - 0.5 * mono).abs() < 1e-12);
    assert!(
        !omissions
            .iter()
            .any(|item| item.reason.contains("audio level not converted")),
        "{omissions:?}"
    );

    // Without a group identity, only the affected sound is omitted; group 0
    // must not become a silent fallback for group 1.
    edit_record(
        &mut xml,
        "<SubClip ObjectID=\"444\"",
        "</SubClip>",
        |record| record.replace("<OrigChGrp>1</OrigChGrp>", ""),
    );
    write_prproj(&input, &xml);
    let output = temp.path().join("missing-group");
    let omissions = premiere_to_tesseract(
        &input,
        &output,
        Some("b532b029-e1d0-4a6c-ae91-f429e5e1b771"),
        false,
    )
    .unwrap();
    let layers = media_layers(&TesseractFile::open(first_project(&output)).unwrap());
    assert_eq!(layers.len(), 2, "{omissions:?}");
    assert!((layers[1]["volume"].as_f64().unwrap() - mono).abs() < 1e-12);
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "347" && item.reason.contains("OrigChGrp")),
        "{omissions:?}"
    );
}
