//! Native fixture content through ordinary Premiere reader/archive publication.
//! The small Premiere wrapper and PSD relink are supplementary wire coverage.

use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use fx_conv::{ConversionMode, ImportToTesseract};
use zip::{write::SimpleFileOptions, ZipWriter};

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    fn relink(chunks: &mut [aftereffects_file::rifx::Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: serde_json::Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = "two_layers.psd".into();
                *chunk = aftereffects_file::rifx::Chunk::data(
                    *b"alas",
                    serde_json::to_vec(&alias).unwrap(),
                )
                .unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children);
            }
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let media_dir = directory.path().join("media");
    std::fs::create_dir(&media_dir).unwrap();
    let container = media_dir.join("source.aegraphic");
    let mut native = aftereffects_file::aep::Project::parse(include_bytes!(
        "../../../../aftereffects_file/tests/fixtures/psd_import/psd_sources_v2.aep"
    ))
    .unwrap();
    relink(&mut native.chunks);
    select_native_root(&mut native.chunks, 2);
    let mut zip = ZipWriter::new(File::create(&container).unwrap());
    zip.start_file("template.aep", SimpleFileOptions::default())
        .unwrap();
    let bytes = native.encode().unwrap();
    zip.write_all(&bytes).unwrap();
    // The pinned native source has no project-panel folder parent, so Adobe's
    // exact Collect Files identity is this root path. The importer must request
    // it from the full still-image mapper rather than the time-media inventory.
    zip.start_file("(Footage)/two_layers.psd", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(include_bytes!(
        "../../../../aftereffects_file/tests/fixtures/psd_import/two_layers_v2.psd"
    ))
    .unwrap();
    zip.start_file("unused.bin", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"unused archive member").unwrap();
    zip.finish().unwrap();
    let private = serde_json::json!({"capsuleparams":{"capParams":[]},
        "framesize":{"size":{"x":1920.0,"y":1080.0},"topleft":{"x":0.0,"y":0.0}}});
    let private = STANDARD.encode(
        serde_json::to_string(&private)
            .unwrap()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let xml = include_str!("../../../tests/fixtures/one-clip.xml")
        .replace("media/source.mp4", "media/source.aegraphic")
        .replace("<VideoStream ObjectRef=\"8\"/>", &format!("<VideoStream ObjectRef=\"8\"/><ImplementationID>{}</ImplementationID>", crate::schema::after_effects::IMPORTER_ID))
        .replace("8467200000", "8475667200")
        .replace("<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>", "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component ObjectRef=\"30\"/></Components></ComponentChain></VideoComponentChain>")
        .replace("</PremiereData>", &format!(r#"<VideoFilterComponent ObjectID="30"><Component><Params/></Component><MatchName>AE.ADBE Capsule</MatchName><PremiereFilterPrivateData Encoding="base64">{private}</PremiereFilterPrivateData></VideoFilterComponent></PremiereData>"#));
    let source = directory.path().join("project.prproj");
    std::fs::write(&source, xml).unwrap();
    (directory, source, container)
}

fn add_media_dependency(source: &Path) {
    let xml = std::fs::read_to_string(source).unwrap();
    let stored = xml
        .split("<PremiereFilterPrivateData Encoding=\"base64\">")
        .nth(1)
        .unwrap()
        .split('<')
        .next()
        .unwrap();
    let bytes = STANDARD.decode(stored).unwrap();
    let utf16 = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    let mut private: serde_json::Value =
        serde_json::from_str(&String::from_utf16(&utf16).unwrap()).unwrap();
    private["capsuleparams"]["capParams"] = serde_json::json!([{
        "capPropAnimatable": false,
        "capPropDefault": null,
        "capPropMatchName": "capsule-fixture-media",
        "capPropType": 11,
        "capPropUIName": "Replacement"
    }]);
    let private = STANDARD.encode(
        serde_json::to_string(&private)
            .unwrap()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let xml = xml
        .replace(stored, &private)
        .replace(
            "<Component><Params/></Component>",
            "<Component><Params><Param Index=\"0\" ObjectRef=\"31\"/></Params></Component>",
        )
        .replace(
            "<MatchName>AE.ADBE Capsule</MatchName>",
            r#"<MediaDependencyMap Version="1"><MediaDependency Version="1" Index="0"><First>0</First><Second ObjectRef="32"/></MediaDependency></MediaDependencyMap><MatchName>AE.ADBE Capsule</MatchName>"#,
        )
        .replace(
            "</PremiereData>",
            &format!(
                r#"<ArbVideoComponentParam ObjectID="31"><Name>Replacement</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>34</ParameterControlType><StartKeyframePosition>{}</StartKeyframePosition><StartKeyframeValue Encoding="base64"/><ParameterID>0</ParameterID></ArbVideoComponentParam><SubClip ObjectID="32"><Clip ObjectRef="33"/><Name>Replacement</Name><OrigChGrp>0</OrigChGrp></SubClip><VideoClip ObjectID="33"/></PremiereData>"#,
                crate::schema::records::STATIC_KEYFRAME_TIME
            ),
        );
    std::fs::write(source, xml).unwrap();
}

fn select_native_root(chunks: &mut [aftereffects_file::rifx::Chunk], composition: u32) {
    use aftereffects_file::{rifx::Chunk, schema::ItemRecord};
    for chunk in chunks {
        let is_root = chunk.list_kind() == Some(*b"Item")
            && chunk.children().is_some_and(|children| {
                children.iter().any(|child| {
                    child.id() == *b"idta"
                        && ItemRecord::decode(child.data_payload().unwrap())
                            .unwrap()
                            .id()
                            == composition
                })
            });
        if let Some(children) = chunk.children_mut() {
            if is_root {
                // Supplementary layout declaration selects the pinned native
                // composition; no property/resource binding or GUID is made.
                children.retain(|child| !matches!(child.list_kind(), Some(kind) if [*b"CIF3", *b"CIF2", *b"CIFO"].contains(&kind)));
                children.push(Chunk::list(
                    *b"CIF3",
                    vec![Chunk::list(
                        *b"CCtl",
                        vec![
                            Chunk::data(*b"Utf8", b"capsule-fixture-layout".to_vec()).unwrap(),
                            Chunk::data(*b"CTyp", 10_u32.to_be_bytes().to_vec()).unwrap(),
                        ],
                    )],
                ));
            } else {
                select_native_root(children, composition);
            }
        }
    }
}

fn corrupt_member(container: &Path, name: &str) {
    let data_start = {
        let file = File::open(container).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let member = archive.by_name(name).unwrap();
        member.data_start()
    };
    let mut bytes = std::fs::read(container).unwrap();
    bytes[usize::try_from(data_start).unwrap()] ^= 1;
    std::fs::write(container, bytes).unwrap();
}

#[test]
fn capsule_picture_normal_publication_keeps_image_assets_and_fractional_timing() {
    let (directory, source, container) = fixture();
    corrupt_member(&container, "unused.bin");
    let output = directory.path().join("converted");
    crate::Premiere
        .import_to_tesseract(
            &source,
            &output,
            &crate::PremiereImportOptions::default(),
            ConversionMode::Write,
        )
        .unwrap();
    let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
    assert_eq!(archive.metadata().assets.len(), 1);
    assert!(archive
        .metadata()
        .assets
        .values()
        .all(|asset| asset.kind == tesseract_file::AssetKind::Image));
    let document = archive.project_json().unwrap();
    let json = document.to_string();
    assert!(json.contains("\"type\":\"Image\""));
    assert!(archive
        .metadata()
        .assets
        .keys()
        .all(|asset_id| json.contains(asset_id)));
    assert!(!json.contains("JsScript"));
}

#[test]
fn capsule_picture_media_dependency_map_retains_editable_template_picture() {
    let (directory, source, _) = fixture();
    add_media_dependency(&source);
    let output = directory.path().join("converted");
    let report = crate::Premiere
        .import_to_tesseract(
            &source,
            &output,
            &crate::PremiereImportOptions::default(),
            ConversionMode::Write,
        )
        .unwrap();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .reason
            .contains("media replacement dependency SubClip:32 is not mapped")
    }));
    let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
    assert_eq!(archive.metadata().assets.len(), 1);
    let json = archive.project_json().unwrap().to_string();
    assert!(json.contains("\"type\":\"Image\""));
    assert!(archive
        .metadata()
        .assets
        .keys()
        .all(|asset_id| json.contains(asset_id)));
    assert!(!json.contains("JsScript"));
}

#[test]
fn capsule_picture_non_picture_occurrence_keeps_editable_image_sibling() {
    fn images<'a>(
        group: &'a fx_schema::GroupLayer,
        output: &mut Vec<(&'a fx_schema::ImageLayer, &'a fx_schema::GroupLayer)>,
    ) {
        for layer in &group.layers {
            match layer.data() {
                fx_schema::LayerData::Image(image) => output.push((image, group)),
                fx_schema::LayerData::Group(group) => images(group, output),
                _ => {}
            }
        }
    }
    let (directory, source, _) = fixture();
    // Native type.aep composition 42 contains only CameraLayer (layer 54),
    // independently pinned in layers/{provenance,expected}.json. Its picture
    // content is unchanged; only a Capsule layout declaration is added.
    let mut native = aftereffects_file::aep::Project::parse(include_bytes!(
        "../../../../aftereffects_file/tests/fixtures/layers/type.aep"
    ))
    .unwrap();
    select_native_root(&mut native.chunks, 42);
    let mut zip =
        ZipWriter::new(File::create(directory.path().join("media/camera.aegraphic")).unwrap());
    zip.start_file("template.aep", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&native.encode().unwrap()).unwrap();
    zip.finish().unwrap();

    // Reuse the native-wire Premiere wrapper on a higher track so the omitted
    // occurrence is visited before the supported PSD picture sibling.
    let xml = std::fs::read_to_string(&source).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let mut camera = String::new();
    for node in document.root_element().children().filter(|node| {
        node.attribute("ObjectID")
            .is_some_and(|id| ["3", "4", "5", "6", "7", "8", "30"].contains(&id))
            || node.attribute("ObjectUID") == Some("media-1")
    }) {
        camera.push_str(&xml[node.range()]);
    }
    for id in [3, 4, 5, 6, 7, 8, 30] {
        camera = camera
            .replace(
                &format!("ObjectID=\"{id}\""),
                &format!("ObjectID=\"{}\"", id + 30),
            )
            .replace(
                &format!("ObjectRef=\"{id}\""),
                &format!("ObjectRef=\"{}\"", id + 30),
            );
    }
    camera = camera
        .replace("media-1", "media-camera")
        .replace("media/source.aegraphic", "media/camera.aegraphic");
    let xml = xml
        .replace(
            "<Track ObjectURef=\"track-1\"/>",
            "<Track ObjectURef=\"track-1\"/><Track ObjectURef=\"track-camera\"/>",
        )
        .replace(
            "</PremiereData>",
            &format!(r#"<VideoClipTrack ObjectUID="track-camera"><ClipTrack><Track><ID>2</ID><Index>1</Index></Track><ClipItems><TrackItems><TrackItem ObjectRef="33"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>{camera}</PremiereData>"#),
        );
    std::fs::write(&source, xml).unwrap();

    let output = directory.path().join("converted");
    let report = crate::Premiere
        .import_to_tesseract(
            &source,
            &output,
            &crate::PremiereImportOptions::default(),
            ConversionMode::Write,
        )
        .unwrap();
    let omitted: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.scope == crate::OmissionScope::Occurrence)
        .collect();
    assert_eq!(omitted.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(omitted[0].record, "VideoClipTrackItem:33");
    assert_eq!(omitted[0].kind, crate::OmissionKind::Omitted);
    assert!(omitted[0].reason.contains("Capsule"));
    assert!(omitted[0].reason.contains("no editable picture content"));

    let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
    let roots = archive.project().composition().layers();
    let capsules: Vec<_> = roots
        .iter()
        .filter_map(|layer| match layer.data() {
            fx_schema::LayerData::Group(group) if group.name.starts_with("Premiere Capsule ") => {
                Some(group)
            }
            _ => None,
        })
        .collect();
    assert_eq!(capsules.len(), 1, "no empty Capsule placeholder");
    assert_eq!(capsules[0].name, "Premiere Capsule 1");
    assert!(!capsules[0].is_hidden);
    let mut retained = Vec::new();
    images(capsules[0], &mut retained);
    assert_eq!(retained.len(), 1);
    let (image, clock) = retained[0];
    assert_eq!(
        image.description,
        "Editable AE still image from source item 1"
    );
    // The still is unbounded; its containing native clock owns the two-second span.
    assert_eq!(clock.name, "Source content clock");
    assert_eq!(clock.playback.input_range().duration.as_millis(), 2000);
    assert_eq!(image.parent, Some(clock.id));
    assert!(!image.is_hidden);
    assert!(!clock.is_hidden);
    let asset = image.source.asset().unwrap();
    assert_eq!(archive.metadata().assets.len(), 1);
    assert_eq!(
        archive.metadata().assets[asset.asset_id.as_str()].kind,
        tesseract_file::AssetKind::Image,
    );
    let bytes = archive
        .asset(asset.asset_id.as_str())
        .unwrap()
        .read_verified_bytes(1024 * 1024)
        .unwrap();
    let png = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .unwrap()
        .into_rgba8();
    assert_eq!(png.dimensions(), (64, 48));
    assert_eq!(png.get_pixel(12, 12).0, [255, 32, 16, 255]);
    assert_eq!(png.get_pixel(40, 30).0, [16, 64, 255, 128]);
    assert!(!archive
        .project_json()
        .unwrap()
        .to_string()
        .contains("JsScript"));
}

#[test]
fn capsule_picture_consumed_media_crc_is_rejected() {
    let (directory, source, container) = fixture();
    corrupt_member(&container, "(Footage)/two_layers.psd");
    let output = directory.path().join("converted");
    assert!(crate::Premiere
        .import_to_tesseract(
            &source,
            &output,
            &crate::PremiereImportOptions::default(),
            ConversionMode::Write,
        )
        .is_err());
    assert!(!output.exists());
}

#[test]
fn capsule_picture_original_container_drift_is_not_publishable() {
    let (_directory, _, container) = fixture();
    let saved = SavedCapsule {
        component: "Capsule:30".into(),
        size: CapsulePoint {
            x: 1920.0,
            y: 1080.0,
        },
        top_left: CapsulePoint { x: 0.0, y: 0.0 },
        controls: Vec::new(),
        diagnostics: Vec::new(),
    };
    let (source, _) =
        PictureSource::prepare(container.clone(), File::open(&container).unwrap(), &saved).unwrap();
    source.verify().unwrap();
    File::options()
        .append(true)
        .open(&container)
        .unwrap()
        .write_all(b"drift")
        .unwrap();
    assert!(source.verify().unwrap_err().to_string().contains("changed"));
}
