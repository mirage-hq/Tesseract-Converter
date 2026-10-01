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
        PrAfterEffectsComposition::parse("00000001-0000-0000-0000-000000000000").unwrap(),
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
        .kind =
        PrMediaKind::AfterEffectsComposition(PrAfterEffectsComposition::parse(guid).unwrap());
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
