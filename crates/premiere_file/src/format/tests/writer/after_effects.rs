//! Supplementary writer cases. These generated graphs are not Adobe proof.

use super::*;
use crate::schema::{PrAfterEffectsComposition, PrMediaKind};

fn linked_project() -> PrProjectFile {
    let mut clip = occurrence(Path::new("/package/media/compositions.aep"));
    clip.start_ticks = TICKS_PER_SECOND / 2;
    clip.end_ticks = 5 * TICKS_PER_SECOND / 2;
    clip.in_ticks = 0;
    clip.out_ticks = 2 * TICKS_PER_SECOND;
    let mut project = project(vec![clip]);
    let video = project
        .media
        .values_mut()
        .next()
        .unwrap()
        .video
        .as_mut()
        .unwrap();
    video.kind = PrMediaKind::AfterEffectsComposition(
        PrAfterEffectsComposition::parse("00000001-0000-0000-0000-000000000000")
            .unwrap()
            .into(),
    );
    video.intrinsic_ticks = 2 * TICKS_PER_SECOND;
    project
}

fn scope_source(project: &mut PrProjectFile, scope: u16) {
    let source = project.media.values_mut().next().unwrap();
    source.relative_path = Some(format!("./media/ae-{scope:04}/compositions.aep"));
    source.absolute_paths.iter_mut().for_each(|(_, path)| {
        *path = format!("/package/media/ae-{scope:04}/compositions.aep").into();
    });
}

#[test]
fn scoped_aep_path_preserves_file_identity_and_source_clock() {
    let mut project = linked_project();
    scope_source(&mut project, 1);
    let xml = project_xml(&project).unwrap();
    let (readback, omissions) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let source = readback.media.values().next().unwrap();
    assert_eq!(
        source.relative_path.as_deref(),
        Some("./media/ae-0001/compositions.aep")
    );
    assert!(source
        .absolute_paths
        .iter()
        .any(|(_, path)| path == Path::new("/package/media/ae-0001/compositions.aep")));
    let clip = readback
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(
        clip.timeline_ticks(),
        TICKS_PER_SECOND / 2..5 * TICKS_PER_SECOND / 2
    );
    assert_eq!(clip.source_ticks(), 0..2 * TICKS_PER_SECOND);
}

#[test]
fn two_aep_files_with_the_same_root_guid_remain_distinct_sources() {
    let mut project = linked_project();
    scope_source(&mut project, 1);
    let mut second = linked_project();
    scope_source(&mut second, 2);
    let source_id = crate::schema::MediaId("second-aep".into());
    project.media.insert(
        source_id.clone(),
        second.media.into_values().next().unwrap(),
    );
    let mut track = second.sequences.remove(0).video_tracks.remove(0);
    let crate::schema::PrVideoItem::Media(clip) = &mut track.items[0] else {
        panic!("expected media")
    };
    clip.media = source_id;
    project.sequences[0].video_tracks.push(track);
    let xml = project_xml(&project).unwrap();
    let (readback, omissions) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(readback.media.len(), 2);
    let sources: Vec<_> = readback.media.values().collect();
    assert_eq!(
        sources[0].after_effects_composition(),
        sources[1].after_effects_composition()
    );
    let paths: std::collections::BTreeSet<_> = sources
        .iter()
        .map(|source| source.relative_path.as_deref().unwrap())
        .collect();
    assert_eq!(
        paths,
        std::collections::BTreeSet::from([
            "./media/ae-0001/compositions.aep",
            "./media/ae-0002/compositions.aep"
        ])
    );
    let clips: Vec<_> = readback
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect();
    assert_eq!(clips.len(), 2);
    assert_ne!(clips[0].media_id(), clips[1].media_id());
}

#[test]
fn scoped_aep_paths_reject_traversal_aliases_and_noncanonical_directories() {
    for relative in [
        "./media/../compositions.aep",
        "./media/ae-0001/../compositions.aep",
        "./media/ae-0001//compositions.aep",
        "./media/ae-0000/compositions.aep",
        "./media/ae-00001/compositions.aep",
        "./media/ae-0001/Compositions.aep",
        "./media/AE-0001/compositions.aep",
        "./media/CON/compositions.aep",
        "./media/ae-0001./compositions.aep",
        "./media/ae-0001 /compositions.aep",
        "./media/ae-０００１/compositions.aep",
        "./media/ae-0001\\compositions.aep",
        "/media/ae-0001/compositions.aep",
        "media/ae-0001/compositions.aep",
    ] {
        let mut project = linked_project();
        project.media.values_mut().next().unwrap().relative_path = Some(relative.into());
        assert!(
            project_xml(&project)
                .unwrap_err()
                .to_string()
                .contains("relative path"),
            "{relative}"
        );
    }
}

#[test]
fn ordinary_video_cannot_use_scoped_media_directories() {
    let mut project = project(vec![occurrence(Path::new("/package/media/background.mp4"))]);
    project.media.values_mut().next().unwrap().relative_path =
        Some("./media/ae-0001/background.mp4".into());
    assert!(project_xml(&project)
        .unwrap_err()
        .to_string()
        .contains("media relative path must be ./media/<the exact media name>"));
}

#[test]
fn scoped_aep_leaf_must_match_its_native_media_name() {
    let mut project = linked_project();
    scope_source(&mut project, 1);
    project.media.values_mut().next().unwrap().name = "different.aep".into();
    assert!(project_xml(&project)
        .unwrap_err()
        .to_string()
        .contains("relative path"));
}

#[test]
fn after_effects_link_writer_preserves_identity_alpha_and_placement() {
    let project = linked_project();
    let xml = project_xml(&project).unwrap();
    assert!(
        xml.contains("<ImplementationID>ec341e53-60c2-4d89-abfc-bdb5c0ff2e0b</ImplementationID>")
    );
    assert!(xml.contains("<CodecType>1145854285</CodecType>"));
    assert!(xml.contains("<OriginalFieldType>4</OriginalFieldType>"));
    assert!(xml.contains("<AlphaType>1</AlphaType>"));
    assert!(!xml.contains("<IgnoreAlpha>true</IgnoreAlpha>"));
    let (readback, omissions) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let source = readback.media.values().next().unwrap();
    assert_eq!(
        source.after_effects_composition(),
        project
            .media
            .values()
            .next()
            .unwrap()
            .after_effects_composition()
    );
    let occurrence = readback
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(
        occurrence.timeline_ticks(),
        TICKS_PER_SECOND / 2..5 * TICKS_PER_SECOND / 2
    );
    assert_eq!(occurrence.source_ticks(), 0..2 * TICKS_PER_SECOND);
}

#[test]
fn after_effects_link_state_matches_the_native_uuid_encoding() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let reference = include_str!("../../../../tests/fixtures/hybrid/native-link-state.xml");
    let xml = project_xml(&linked_project()).unwrap();
    let mut states = Vec::new();
    for text in [reference, &xml] {
        let document = roxmltree::Document::parse(text).unwrap();
        let media = document
            .descendants()
            .find(|node| node.has_tag_name("Media"))
            .unwrap();
        let field = |name| {
            media
                .children()
                .find(|node| node.has_tag_name(name))
                .unwrap()
        };
        assert_eq!(
            field("ImplementationID").text(),
            Some(crate::schema::after_effects::IMPORTER_ID)
        );
        let prefs = STANDARD
            .decode(field("ImporterPrefs").text().unwrap().trim())
            .unwrap();
        let guid = "00000001-0000-0000-0000-000000000000";
        assert_eq!(
            prefs,
            guid.encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        let state =
            uuid::Uuid::parse_str(field("ContentAndMetadataState").text().unwrap()).unwrap();
        let modification = field("ModificationState");
        assert_eq!(modification.attribute("Encoding"), Some("base64"));
        assert_eq!(
            STANDARD
                .decode(modification.text().unwrap().trim())
                .unwrap(),
            state.as_bytes()
        );
        states.push(state);
    }
    // Each export owns a fresh state; the native fixture is not a state donor.
    assert_ne!(states[0], states[1]);
}

#[test]
fn after_effects_link_writer_rejects_wrong_file_type() {
    for wrong_absolute_path in [false, true] {
        let mut project = linked_project();
        let source = project.media.values_mut().next().unwrap();
        if wrong_absolute_path {
            source.absolute_paths.iter_mut().for_each(|(_, path)| {
                path.set_extension("mp4");
            });
        } else {
            source.name = "compositions.mp4".into();
        }
        assert!(project_xml(&project)
            .unwrap_err()
            .to_string()
            .contains("AEP file"));
    }
}

#[test]
fn after_effects_link_writer_does_not_invent_a_guid_from_media_identity() {
    let mut project = linked_project();
    let guid = "0000002a-0000-0000-0000-000000000000";
    project
        .media
        .values_mut()
        .next()
        .unwrap()
        .video
        .as_mut()
        .unwrap()
        .kind = PrMediaKind::AfterEffectsComposition(
        PrAfterEffectsComposition::parse(guid).unwrap().into(),
    );
    let xml = project_xml(&project).unwrap();
    let readback = crate::format::inspect_project_with_media(&xml, None).unwrap();
    assert_eq!(
        readback
            .media
            .values()
            .next()
            .unwrap()
            .after_effects_composition()
            .unwrap()
            .dynamic_link_guid(),
        guid
    );
}

#[test]
fn hdr_unknown_linked_source_domain_retains_geometry_with_explicit_numeric_approximation() {
    let xml = project_xml(&linked_project()).unwrap();
    let original = serde_json::to_string(&crate::schema::ColorSpace::sequence_sdr()).unwrap();
    let original = format!("<OriginalColorSpace>{original}</OriginalColorSpace>");
    assert!(xml.contains(&original));
    let unknown = xml.replace(&original, r#"<OriginalColorSpace>{"baseColorProfile":{"colorProfileName":"Future RGB"},"baseProfileType":1}</OriginalColorSpace>"#);
    let (readback, diagnostics) =
        crate::format::inspect_project_with_omissions(&unknown, None).unwrap();
    assert!(readback.sequences[0].video_items().next().is_some());
    let source = readback
        .media
        .values()
        .next()
        .unwrap()
        .video
        .as_ref()
        .unwrap();
    let PrMediaKind::AfterEffectsComposition(source) = source.kind else {
        panic!("linked composition");
    };
    assert_eq!(source.working_hdr, None);
    assert!(diagnostics.iter().any(|diagnostic| diagnostic
        .to_string()
        .contains("Unrecognized linked-AEP source colour domain")));
    let malformed = xml.replace(&original, "<OriginalColorSpace>[]</OriginalColorSpace>");
    assert!(crate::format::inspect_project_with_media(&malformed, None).is_err());
}

#[test]
fn hdr_sequence_target_roundtrip_requires_agreement_without_retagging_sources() {
    for transfer in [fx_conv::HdrTransfer::Pq, fx_conv::HdrTransfer::Hlg] {
        let mut project = linked_project();
        let settings = crate::schema::SequenceHdr {
            transfer,
            graphics_white: 203.0,
        };
        project.sequences[0].hdr = Some(settings);
        let xml = project_xml(&project).unwrap();
        let (readback, diagnostics) =
            crate::format::inspect_project_with_omissions(&xml, None).unwrap();
        assert_eq!(readback.sequences[0].hdr, Some(settings), "{diagnostics:?}");
        let PrMediaKind::AfterEffectsComposition(source) = readback
            .media
            .values()
            .next()
            .unwrap()
            .video
            .as_ref()
            .unwrap()
            .kind
        else {
            panic!("linked source");
        };
        assert_eq!(
            source.working_hdr, None,
            "target HDR must not become source HDR"
        );
        let tree = roxmltree::Document::parse(&xml).unwrap();
        let root = tree
            .root_element()
            .children()
            .find(|node| node.has_tag_name("Project") && node.attribute("ObjectRef").is_some())
            .unwrap();
        let root_xml = &xml[root.range()];
        let project = tree
            .root_element()
            .children()
            .find(|node| {
                node.has_tag_name("Project")
                    && node.attribute("ObjectID") == root.attribute("ObjectRef")
            })
            .unwrap();
        let project_xml = &xml[project.range()];
        let settings_reference = project
            .children()
            .find(|node| node.has_tag_name("ProjectSettings"))
            .unwrap();
        let settings = tree
            .root_element()
            .children()
            .find(|node| {
                node.has_tag_name("ProjectSettings")
                    && node.attribute("ObjectID") == settings_reference.attribute("ObjectRef")
            })
            .unwrap();
        let settings_xml = &xml[settings.range()];
        let wrong_root = format!(
            "<Project ObjectRef=\"{}\"/>",
            settings_reference.attribute("ObjectRef").unwrap()
        );
        let settings_ref_xml = format!(
            "<ProjectSettings ObjectRef=\"{}\"/>",
            settings_reference.attribute("ObjectRef").unwrap()
        );
        assert!(project_xml.contains(&settings_ref_xml));
        for modified in [
            xml.replace(
                project_xml,
                &project_xml.replace(
                    &settings_ref_xml,
                    &format!("{settings_ref_xml}{settings_ref_xml}"),
                ),
            ),
            xml.replace(root_xml, &format!("{root_xml}{root_xml}")),
            xml.replace(root_xml, &wrong_root),
            xml.replace(
                settings_xml,
                &settings_xml.replace(
                    "\"graphicsWhiteLuminance\":203.0",
                    "\"graphicsWhiteLuminance\":300.0",
                ),
            ),
            xml.replace(
                "<AllowLinearCompositing>false</AllowLinearCompositing>",
                "<AllowLinearCompositing>true</AllowLinearCompositing>",
            ),
            xml.replace(
                "\"workingSpaceIsLinearized\":0",
                "\"workingSpaceIsLinearized\":1",
            ),
            xml.replace(
                "\"graphicsWhiteLuminance\":203.0",
                "\"graphicsWhiteLuminance\":99.0",
            ),
        ] {
            assert_ne!(modified, xml);
            let readback = crate::format::inspect_project_with_media(&modified, None).unwrap();
            assert!(readback.sequences[0].hdr.is_none());
            assert!(
                readback
                    .media
                    .values()
                    .next()
                    .unwrap()
                    .after_effects_composition()
                    .is_some(),
                "independent source geometry survives unknown target"
            );
        }
    }
}

#[test]
fn after_effects_link_hdr_source_is_independent_of_sdr_sequence_and_guid() {
    for transfer in [fx_conv::HdrTransfer::Pq, fx_conv::HdrTransfer::Hlg] {
        let mut project = linked_project();
        let video = project
            .media
            .values_mut()
            .next()
            .unwrap()
            .video
            .as_mut()
            .unwrap();
        let PrMediaKind::AfterEffectsComposition(source) = &mut video.kind else {
            panic!("test requires linked AEP source");
        };
        let composition = source.composition;
        source.working_hdr = Some(transfer);
        assert!(project.sequences[0].hdr.is_none());
        let xml = project_xml(&project).unwrap();
        let readback = crate::format::inspect_project_with_media(&xml, None).unwrap();
        assert!(readback.sequences[0].hdr.is_none());
        let source = readback.media.values().next().unwrap();
        assert_eq!(source.after_effects_composition(), Some(composition));
        let PrMediaKind::AfterEffectsComposition(source) = source.video.as_ref().unwrap().kind
        else {
            panic!("source profile must not reclassify AEP as ordinary video");
        };
        assert_eq!(source.working_hdr, Some(transfer));
    }
}

#[test]
fn after_effects_link_import_without_resolver_reports_omission_instead_of_a_video_asset() {
    let mut project = linked_project();
    let background = super::project(vec![occurrence(Path::new("/package/media/background.mp4"))]);
    let background_id = background.media.keys().next().unwrap().clone();
    let (mut sequences, media) = background.into_parts();
    project.media.extend(media);
    project.sequences[0]
        .video_tracks
        .extend(sequences.remove(0).video_tracks);
    project.sequences[0].timeline_end_ticks = 4 * TICKS_PER_SECOND;
    let asset_ids = std::collections::BTreeMap::from([(
        background_id,
        fx_schema::AssetId::from_trusted("resolved-video"),
    )]);
    let mut omissions = Vec::new();
    let document = crate::convert::premiere_to_tesseract(
        project.single_sequence().unwrap(),
        &project.media,
        &asset_ids,
        &mut omissions,
    )
    .unwrap();
    assert_eq!(
        document
            .composition()
            .layers()
            .iter()
            .filter(|layer| matches!(layer.data(), fx_schema::LayerData::Video(_)))
            .count(),
        1,
        "independent video survives, linked AEP is not flattened or invented"
    );
    assert_eq!(
        document.composition().layers().len(),
        2,
        "only the native video and the existing black canvas are emitted"
    );
    assert!(omissions.iter().any(|omission| omission
        .to_string()
        .contains("not resolved to a composition of an exact AEP")));
}
