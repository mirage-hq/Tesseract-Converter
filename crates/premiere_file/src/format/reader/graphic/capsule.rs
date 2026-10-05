//! File-backed saved capsules are admitted as ordinary editable Source Graphics.
//! Placement keys keep the saved source clock; no native template replay survives.

use super::{
    super::{
        animation::{chain_components, read_video_animations, read_video_compositing},
        integer, require_zero_subclip_time_offset, required, required_integer,
        video::{playback_rate, report_markers, scale_to_frame_size},
        visibility,
    },
    GraphicClip,
};
use crate::{
    approximate,
    capsule::{decode_template, CapsuleError, SavedCapsule},
    convert,
    error::{ensure, unsupported, BuildError, Result},
    format::{FrameRate, Graph, Located},
    schema::{
        native::{VideoClipTrackItem, VideoComponentChain, VideoFilterComponent},
        text::PrVectorMotion,
        PrAnimatedProperty, PrGraphic, PrStaticTransform,
    },
    Omission,
};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

pub(super) fn has_saved_component(graph: &Graph<'_>, item: &Located<VideoClipTrackItem>) -> bool {
    let Some(reference) = item
        .value
        .clip_track_item
        .as_ref()
        .and_then(|track| track.component_owner.as_ref())
        .and_then(|owner| owner.components.as_ref())
    else {
        return false;
    };
    let Ok(chain) = graph.follow::<VideoComponentChain>(reference, &item.identity) else {
        return false;
    };
    let Ok(components) = chain_components(&chain) else {
        return false;
    };
    components.iter().any(|reference| {
        graph
            .follow::<VideoFilterComponent>(reference, &chain.identity)
            .is_ok_and(|component| component.value.match_name.as_deref() == Some("AE.ADBE Capsule"))
    })
}

pub(super) fn read(
    graph: &Graph<'_>,
    item: Located<VideoClipTrackItem>,
    source: GraphicClip,
    frame: [u32; 2],
    frame_rate: FrameRate,
    omissions: &mut Vec<Omission>,
) -> Result<PrGraphic> {
    let identity = &item.identity;
    let track_item = required(
        item.value.clip_track_item.as_ref(),
        identity,
        "ClipTrackItem",
    )?;
    require_zero_subclip_time_offset(track_item, identity)?;
    let range = required(track_item.track_item.as_ref(), identity, "TrackItem")?;
    let start = range
        .start
        .as_deref()
        .map(|value| integer(value, identity))
        .transpose()?
        .unwrap_or(0);
    let end = integer(&range.end, identity)?;
    let clip = required(
        source.clip.value.clip.as_ref(),
        &source.clip.identity,
        "Clip",
    )?;
    ensure!(
        clip.is_multicam != Some(true) && clip.selected_track_index.is_none(),
        "{identity}: multicam selection on a capsule is unsupported"
    );
    report_markers(graph, clip, &source.clip.identity, omissions);
    ensure!(
        playback_rate(clip, identity)? == 1.0
            && clip.time_remapping.is_none()
            && !source.clip.value.declares_frame_hold()
            && !scale_to_frame_size(&source.clip.value, identity)?,
        "{identity}: capsule retiming/Scale to Frame Size is not converted"
    );
    let source_in = required_integer(clip.in_point.as_deref(), identity, "InPoint")?;
    let source_out = required_integer(clip.out_point.as_deref(), identity, "OutPoint")?;
    ensure!(
        source_in >= 0 && source_out > source_in && start >= 0 && end > start,
        "{identity}: invalid capsule source/timeline ranges"
    );
    ensure!(
        source_out.checked_sub(source_in) == end.checked_sub(start),
        "{identity}: capsule placement/source ranges differ"
    );
    let reference = required(
        track_item
            .component_owner
            .as_ref()
            .and_then(|owner| owner.components.as_ref()),
        identity,
        "component chain",
    )?;
    let chain = graph.follow::<VideoComponentChain>(reference, identity)?;
    let mut capsule = None;
    let mut placement = Vec::new();
    for reference in chain_components(&chain)? {
        let component = graph.follow::<VideoFilterComponent>(reference, &chain.identity)?;
        if component.value.match_name.as_deref() == Some("AE.ADBE Capsule") {
            ensure!(
                capsule.replace(reference).is_none(),
                "{identity}: duplicate capsules"
            );
        } else {
            placement.push(reference);
        }
    }
    let reference = capsule
        .ok_or_else(|| unsupported(format!("{identity}: capsule media has no saved component")))?;
    let id = required(reference.id.as_deref(), identity, "capsule ObjectID")?;
    let saved =
        SavedCapsule::from_graph(graph, id).map_err(|error| capsule_error(identity, error))?;
    let template = read_template(graph, &source.media.value, identity)?;
    let (template, composition_id, text, diagnostics) = saved
        .resolve_instance(&template)
        .map_err(|error| capsule_error(identity, error))?;
    for diagnostic in diagnostics {
        approximate(omissions, &saved.component, diagnostic);
    }
    let text = top_level_text_overrides(text, composition_id, identity, omissions);
    let (layers, warnings) = template
        .editable_layers_in_composition(composition_id, &text)
        .map_err(|error| unsupported(format!("{identity}: {error}")))?;
    for warning in warnings {
        approximate(omissions, identity, warning);
    }
    let objects = convert::template_objects(&layers, frame, omissions, identity)?;
    let motion = read_video_animations(graph, &chain, &placement, true)?;
    ensure!(
        motion.crop.is_default() && motion.linear_wipe.is_none() && motion.track_matte.is_none(),
        "{identity}: capsule Motion crop/wipe/matte is not converted"
    );
    let (opacity, blend_mode, opacity_animation, opacity_mask) =
        read_video_compositing(graph, &chain, &placement, omissions)?;
    ensure!(
        opacity_mask.is_none(),
        "{identity}: capsule Opacity mask frame is unverified"
    );
    let transform = motion.transform;
    let uniform_scale = (transform.scale[0] - transform.scale[1]).abs() < 1e-9;
    ensure!(
        uniform_scale,
        "{identity}: nonuniform capsule clip Motion is not converted"
    );
    let mut animations = Vec::new();
    for animation in motion.animations {
        if matches!(
            animation.property(),
            PrAnimatedProperty::AnchorPoint | PrAnimatedProperty::ScaleWidth
        ) {
            approximate(omissions,identity,format!("capsule clip {:?} keys reduced to static value; ordinary Vector Motion has no matching keyed parameter",animation.property()));
        } else {
            animations.push(animation);
        }
    }
    let vector_motion = PrVectorMotion {
        position: [
            transform.position[0] * f64::from(frame[0]),
            transform.position[1] * f64::from(frame[1]),
        ],
        anchor: [
            transform.anchor_point[0] * saved.size.x + saved.top_left.x,
            transform.anchor_point[1] * saved.size.y + saved.top_left.y,
        ],
        scale: transform.scale[1],
        rotation: transform.rotation,
        animations,
    };
    approximate(omissions,&saved.component,"saved capsule authoring controllers replaced by independent ordinary editable Text/Shape objects; later text edits do not recompute responsive Shape geometry");
    let graphic = PrGraphic {
        id: Some(item.identity.clone()),
        start_ticks: start,
        end_ticks: end,
        in_ticks: source_in,
        vector_motion: Some(vector_motion),
        clip_motion: PrStaticTransform::default(),
        opacity,
        blend_mode,
        animations: opacity_animation.into_iter().collect(),
        opacity_mask: None,
        effect_loss: None,
        objects,
        enabled: !visibility::is_muted(track_item.is_muted.as_deref(), identity)?,
    };
    graphic.validate(frame_rate)?;
    Ok(graphic)
}

// The static ordinary-graphic consumer cannot apply a child's Text override to
// the declaring composition. Isolate that optional override here; the public
// explicit-composition API still rejects mismatched batches and forged bindings.
fn top_level_text_overrides(
    values: Vec<aftereffects_file::graphic_template::SavedGraphicText>,
    composition_id: u32,
    identity: &str,
    omissions: &mut Vec<Omission>,
) -> Vec<aftereffects_file::graphic_template::SavedGraphicText> {
    values.into_iter().filter(|value| {
        if value.composition_id == composition_id {
            true
        } else {
            approximate(omissions, identity, format!(
                "controller {} Text override targets composition {} rather than template composition {composition_id}; child template value remains unchanged, supported top-level content retained",
                value.controller_uuid, value.composition_id
            ));
            false
        }
    }).collect()
}

fn read_template(
    graph: &Graph<'_>,
    media: &crate::schema::native::Media,
    identity: &str,
) -> Result<aftereffects_file::graphic_template::SavedGraphicTemplate> {
    let mut candidates = Vec::new();
    if let Some(root) = graph.source_dir() {
        candidates.extend(
            media
                .relative_paths
                .iter()
                .map(|hint| root.join(hint.replace('\\', "/"))),
        );
    }
    candidates.extend(
        [
            media.file_path.as_deref(),
            media.actual_media_file_path.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(PathBuf::from),
    );
    let mut seen = BTreeSet::new();
    let mut container = None;
    for path in candidates {
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if !["aegraphic", "mogrt"]
            .iter()
            .any(|expected| ext.eq_ignore_ascii_case(expected))
        {
            continue;
        }
        let path = match path.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(BuildError::IoAt {
                    context: format!("{identity}: {}", path.display()),
                    source,
                })
            }
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let context = || format!("{identity}: {}", path.display());
        let mut current = File::open(&path).map_err(|source| BuildError::IoAt {
            context: context(),
            source,
        })?;
        if let Some(previous) = &mut container {
            ensure!(
                same_container_bytes(previous, &mut current).map_err(|source| {
                    BuildError::IoAt {
                        context: context(),
                        source,
                    }
                })?,
                "{identity}: saved container aliases resolve different bytes"
            );
        } else {
            container = Some(current);
        }
    }
    let container = container.ok_or_else(|| {
        BuildError::MissingMedia(format!(
            "{identity}: saved .aegraphic/.mogrt container; relink its media path"
        ))
    })?;
    decode_template(container).map_err(|error| capsule_error(identity, error))
}

// Distinct native aliases must still identify identical source bytes. Compare
// them in bounded chunks instead of buffering potentially large unused media.
fn same_container_bytes(left: &mut File, right: &mut File) -> std::io::Result<bool> {
    let mut remaining = left.metadata()?.len();
    if remaining != right.metadata()?.len() {
        return Ok(false);
    }
    left.seek(SeekFrom::Start(0))?;
    right.seek(SeekFrom::Start(0))?;
    let mut left_chunk = [0; 64 * 1024];
    let mut right_chunk = [0; 64 * 1024];
    while remaining > 0 {
        let count = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(left_chunk.len());
        left.read_exact(&mut left_chunk[..count])?;
        right.read_exact(&mut right_chunk[..count])?;
        if left_chunk[..count] != right_chunk[..count] {
            return Ok(false);
        }
        remaining -= count as u64;
    }
    left.seek(SeekFrom::Start(0))?;
    Ok(true)
}

fn capsule_error(context: &str, error: CapsuleError) -> BuildError {
    match error {
        CapsuleError::Io(source) | CapsuleError::Zip(zip::result::ZipError::Io(source)) => {
            BuildError::IoAt {
                context: context.to_owned(),
                source,
            }
        }
        source => BuildError::Capsule {
            context: context.to_owned(),
            source: Box::new(source),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::native::Reference;

    #[test]
    fn capsule_optional_child_text_mismatch_retains_supported_top_level_override() {
        let document = serde_json::from_value(serde_json::json!({
            "text":"Supported title","fontFamily":"ArialMT","fontStyle":"",
            "fontSize":24.0,"fillColor":[1.0,0.0,0.0,1.0]
        }))
        .unwrap();
        let supported = aftereffects_file::graphic_template::SavedGraphicText {
            controller_uuid: "supported-text".into(),
            composition_id: 1,
            layer_id: 2,
            document,
            postscript_font: "ArialMT".into(),
            diagnostics: Vec::new(),
        };
        let mut child = supported.clone();
        child.controller_uuid = "child-text".into();
        child.composition_id = 3;
        child.document.text = "Child text".into();
        let mut omissions = Vec::new();
        let retained =
            top_level_text_overrides(vec![child, supported], 1, "capsule", &mut omissions);
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].document.text.as_str(), "Supported title");
        assert_eq!(retained[0].document.font_family.as_ref(), "ArialMT");
        assert_eq!(omissions.len(), 1);
        assert!(omissions[0].reason.contains("child-text"));
        assert!(omissions[0].reason.contains("composition 3"));
    }

    #[test]
    fn review_capsule_container_without_capsule_component_is_not_graphic() {
        let xml = format!(
            r#"<PremiereData>
            <VideoClipTrackItem ObjectID="1"><ClipTrackItem><SubClip ObjectRef="2"/><ComponentOwner><Components ObjectRef="6"/></ComponentOwner></ClipTrackItem></VideoClipTrackItem>
            <SubClip ObjectID="2"><Clip ObjectRef="3"/></SubClip>
            <VideoClip ObjectID="3"><Clip><Source ObjectRef="4"/></Clip></VideoClip>
            <VideoMediaSource ObjectID="4"><MediaSource><Media ObjectRef="5"/></MediaSource></VideoMediaSource>
            <Media ObjectID="5"><ImplementationID>{}</ImplementationID><FilePath>synthetic.aegraphic</FilePath></Media>
            <VideoComponentChain ObjectID="6"><ComponentChain><Components><Component ObjectRef="7"/></Components></ComponentChain></VideoComponentChain>
            <VideoFilterComponent ObjectID="7"><MatchName>unrelated effect</MatchName></VideoFilterComponent>
        </PremiereData>"#,
            crate::schema::after_effects::IMPORTER_ID
        );
        let graph = Graph::parse(&xml).unwrap();
        let item = graph
            .follow::<VideoClipTrackItem>(
                &Reference {
                    id: Some("1".into()),
                    uid: None,
                    index: None,
                },
                "test",
            )
            .unwrap();
        assert!(!has_saved_component(&graph, &item));
        assert!(
            super::super::graphic_clip(&graph, &item).is_none(),
            "a shared importer ID and container path do not supply missing capsule semantics"
        );
        // Prove the synthetic source chain resolves: changing only its
        // importer to the ordinary graphic importer admits that same chain.
        let ordinary_xml = xml.replace(
            crate::schema::after_effects::IMPORTER_ID,
            crate::schema::text::GRAPHIC_IMPLEMENTATION_ID,
        );
        let ordinary_graph = Graph::parse(&ordinary_xml).unwrap();
        let ordinary_item = ordinary_graph
            .follow::<VideoClipTrackItem>(
                &Reference {
                    id: Some("1".into()),
                    uid: None,
                    index: None,
                },
                "synthetic source-chain control",
            )
            .unwrap();
        assert!(super::super::graphic_clip(&ordinary_graph, &ordinary_item).is_some());
        let capsule_xml = xml.replace("unrelated effect", "AE.ADBE Capsule");
        let capsule_graph = Graph::parse(&capsule_xml).unwrap();
        let capsule_item = capsule_graph
            .follow::<VideoClipTrackItem>(
                &Reference {
                    id: Some("1".into()),
                    uid: None,
                    index: None,
                },
                "synthetic capsule-classification control",
            )
            .unwrap();
        assert!(has_saved_component(&capsule_graph, &capsule_item));
        assert!(
            super::super::graphic_clip(&capsule_graph, &capsule_item)
                .unwrap()
                .capsule
        );
    }
    #[test]
    fn review_capsule_native_dynamic_link_keeps_linked_media() {
        let mut xml = String::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../../../tests/fixtures/hybrid/native-linked.prproj")[..],
        )
        .read_to_string(&mut xml)
        .unwrap();
        let (project, reports) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
        assert!(
            project
                .media
                .values()
                .any(|media| media.after_effects_composition().is_some()),
            "Dynamic Link must remain a linked composition: {reports:?}"
        );
        assert!(
            !reports
                .iter()
                .any(|report| report.reason.contains("capsule media")),
            "{reports:?}"
        );
    }
    #[test]
    fn review_capsule_io_errors_keep_source_and_context() {
        let error = capsule_error(
            "container.aegraphic",
            CapsuleError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "CRC failure",
            )),
        );
        assert!(
            matches!(error,BuildError::IoAt {ref context,ref source} if context=="container.aegraphic" && source.kind()==std::io::ErrorKind::InvalidData)
        );
        assert!(std::error::Error::source(&error).is_some());
    }
}
