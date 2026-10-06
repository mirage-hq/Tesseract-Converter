use super::*;
use crate::formats::import_after_effects;
use aftereffects_file::{aep, properties, rifx::Chunk, structure};
use premiere_file::FrameRate;
use sha2::{Digest, Sha256};

fn relink(chunks: &mut [Chunk], directory: &Path) {
    for chunk in chunks {
        if chunk.id() == *b"alas" {
            let mut alias: Value = serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
            let name = Path::new(alias["fullpath"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_owned();
            assert!(name == "rate50.mp4" || name == "rate60.mp4");
            alias["fullpath"] = json!(directory.join(name));
            *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
        } else if let Some(children) = chunk.children_mut() {
            relink(children, directory);
        }
    }
}

fn opacity(layer: &structure::Layer) -> properties::NumericProperty {
    properties::read_transform(&layer.content)
        .unwrap()
        .into_iter()
        .find(|p| p.match_name == "ADBE Opacity")
        .unwrap()
        .numeric
        .unwrap()
}

#[test]
fn native_high_rate_scopes_keep_composition_media_and_key_clocks() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/aftereffects_file/tests/fixtures/hybrid/rates");
    let bytes = fs::read(directory.join("native.aep")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "b93735f689706df1ca7bd9d6b294974102185bc07e74ced5e875b448567eea82"
    );
    let original = structure::read_project(&bytes).unwrap();
    let parent = tempfile::tempdir().unwrap();
    // Change only media aliases in a disposable copy; clocks and controls come
    // from the independently saved/reopened AE 26.5x89 source.
    let mut relocated = aep::Project::parse(&bytes).unwrap();
    relink(&mut relocated.chunks, &directory);
    let input = parent.path().join("source.aep");
    fs::write(&input, relocated.encode().unwrap()).unwrap();
    for (id, fps, rate, media_hash) in [
        (
            2,
            50.0,
            FrameRate::Fps50,
            "da6bd54c524d5990ed8a859d02e54e69759bacd101d730e503c7d89655948cd0",
        ),
        (
            19,
            60.0,
            FrameRate::Fps60,
            "7b40e274c61d1b8d230ddb4c65f7f47b357f2842a7deba58c01b5874f0084a41",
        ),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(fs::read(directory.join(format!("rate{fps}.mp4"))).unwrap())
            ),
            media_hash
        );
        let structure::ItemKind::Composition(comp) = &original.item(id).unwrap().kind else {
            panic!("native composition");
        };
        assert_eq!(comp.frame_rate, fps);
        assert_eq!(comp.duration_secs, 2.0);
        let native_keys = opacity(
            comp.layers
                .iter()
                .find(|l| l.name.as_ref() == "Rate marker")
                .unwrap(),
        )
        .keyframes;
        assert_eq!(native_keys.len(), 2);
        let video = comp
            .layers
            .iter()
            .find(|l| l.name.as_ref() == "Frame numbered source")
            .unwrap();
        assert_eq!(video.record.in_point(), Some(0.2));
        assert_eq!(video.record.out_point(), Some(1.8));
        let original_media = original
            .item(video.record.source_id())
            .unwrap()
            .native_media
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        assert_eq!(original_media.native_frame_rate.as_f64(), fps);
        let imported = parent.path().join(format!("import-{id}"));
        let mut import = request(&input, &imported, ConversionMode::Write);
        import.composition = Some(id);
        import_after_effects(&import).unwrap();
        let archive = imported.join("project.tsrct");
        let output = parent.path().join(format!("export-{id}"));
        let options = PremiereExportOptions {
            frame_rate: Some(rate),
        };
        let checked = export(&request(&archive, &output, ConversionMode::Check), &options).unwrap();
        assert!(!output.exists());
        let written = export(&request(&archive, &output, ConversionMode::Write), &options).unwrap();
        assert_eq!(checked, written);
        assert!(!written
            .diagnostics
            .iter()
            .any(|d| d.code == "HYBRID-NATIVE-RETAINED"));
        let prproj = output.join("project.prproj");
        let text = xml(&prproj);
        assert!(text.contains("./media/ae-0001/compositions.aep"));
        assert!(text.contains(&format!(
            "<FrameRate>{}</FrameRate>",
            rate.ticks_per_frame()
        )));
        let generated = structure::read_project(
            &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
        )
        .unwrap();
        let mut opacity_keys = Vec::new();
        let mut footage_count = 0;
        for item in &generated.items {
            if let structure::ItemKind::Composition(comp) = &item.kind {
                assert_eq!(comp.frame_rate, fps);
                if item.id == 1 {
                    assert_eq!(comp.duration_secs, 2.0);
                }
                for layer in &comp.layers {
                    assert!(!layer.record.flags().audio_enabled);
                    if layer.name.as_ref() == "Rate marker" {
                        opacity_keys.extend(opacity(layer).keyframes);
                    }
                }
            }
            if let Some(Ok(media)) = &item.native_media {
                assert_eq!(media.native_frame_rate.as_f64(), fps);
                assert_eq!(media.duration, original_media.duration);
                footage_count += 1;
            }
        }
        assert_eq!(footage_count, 1);
        assert_eq!(opacity_keys.len(), 2);
        for (key, native) in opacity_keys.iter().zip(&native_keys) {
            // AE's native key clock is 1/24576 second; do not move keys to a
            // different composition frame grid when selecting an export rate.
            assert!((key.time_secs - native.time_secs).abs() <= 1.0 / 24576.0);
            assert_eq!(key.values, native.values);
        }
        // The first incoming and last outgoing controls have no segment. The
        // writer normalizes those unused controls to Linear, retaining Hold
        // on both ends of the actual segment.
        assert_eq!(
            opacity_keys[0].out_interpolation,
            native_keys[0].out_interpolation
        );
        assert_eq!(
            opacity_keys[1].in_interpolation,
            native_keys[1].in_interpolation
        );
        let reopened = parent.path().join(format!("reimport-{id}"));
        import_premiere(&request(&prproj, &reopened, ConversionMode::Write)).unwrap();
        let result = TesseractFile::open(reopened.join("project.tsrct")).unwrap();
        let all = layers(result.project().composition().layers());
        assert_eq!(
            all.iter()
                .filter(|l| matches!(l.data(), LayerData::Video(v) if !v.is_hidden))
                .count(),
            1
        );
        // AE keeps hidden sound projections of physical A/V footage. Linked
        // pictures must not enable them or their hidden video companions.
        assert!(!all
            .iter()
            .any(|l| matches!(l.data(), LayerData::Audio(a) if !a.is_hidden)));
        let trimmed: Vec<_> = all
            .iter()
            .filter_map(|l| match l.data() {
                LayerData::Group(g)
                    if g.playback.input_range().start.as_millis() == 200
                        && g.playback.input_range().duration.as_millis() == 1600 =>
                {
                    Some(g)
                }
                _ => None,
            })
            .collect();
        assert_eq!(trimmed.len(), 1);
        let remap = trimmed[0].playback.time_remap().unwrap();
        assert_eq!(
            remap
                .keyframes()
                .iter()
                .map(|k| (k.time.as_millis(), k.value.as_millis()))
                .collect::<Vec<_>>(),
            [(200, 200), (1800, 1800)]
        );
    }
    assert_eq!(fs::read(directory.join("native.aep")).unwrap(), bytes);
}

#[test]
fn fractional_scope_rate_retains_native_picture_sound_and_diagnostics() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    archive(&document_value(), &input);
    let output = parent.path().join("output");
    let options = PremiereExportOptions {
        frame_rate: Some(FrameRate::Fps30000Over1001),
    };
    let checked = export(&request(&input, &output, ConversionMode::Check), &options).unwrap();
    assert!(!output.exists());
    let written = export(&request(&input, &output, ConversionMode::Write), &options).unwrap();
    assert_eq!(checked, written);
    let warnings: Vec<_> = written
        .diagnostics
        .iter()
        .filter(|d| d.code == "HYBRID-NATIVE-RETAINED")
        .collect();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("30000/1001"));
    assert!(written
        .diagnostics
        .iter()
        .any(|d| d.message.to_lowercase().contains("glow")));
    assert!(!output.join("media/ae-0001").exists());
    let text = xml(&output.join("project.prproj"));
    assert!(!text.contains("compositions.aep"));
    assert!(text.contains("video-30fps.mp4"));
    assert!(text.contains("audio-mono.wav"));
    assert!(text.contains("<FrameRate>8475667200</FrameRate>"));
}
