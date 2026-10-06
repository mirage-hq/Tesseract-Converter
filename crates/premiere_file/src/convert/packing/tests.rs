use super::*;
use crate::schema::PrBlendMode;

#[test]
fn cross_dissolve_replacement_replays_unrelated_links_and_replaces_whole_owner() {
    use crate::{
        schema::{PrVideoTransition, PrVideoTransitionKind, TICKS},
        tests::support::{clip_of, sequence_of, video_media},
    };
    for two_sided in [false, true] {
        for replace_owner in [false, true] {
            let mut outgoing = clip_of(
                "source",
                0..if two_sided { 2 * TICKS } else { 5 * TICKS },
                0,
            );
            outgoing.id = Some("out".into());
            let mut items = vec![PrVideoItem::Media(outgoing)];
            if two_sided {
                let mut incoming = clip_of("source", 2 * TICKS..5 * TICKS, 3 * TICKS);
                incoming.id = Some("in".into());
                items.push(PrVideoItem::Media(incoming));
            }
            let transition = PrVideoTransition {
                id: "dissolve".into(),
                kind: PrVideoTransitionKind::CrossDissolve,
                start_ticks: if two_sided { 3 * TICKS / 2 } else { 0 },
                cut_ticks: if two_sided { 2 * TICKS } else { 0 },
                end_ticks: if two_sided { 5 * TICKS / 2 } else { TICKS },
                outgoing_clip: two_sided.then(|| "out".into()),
                incoming_clip: Some(if two_sided { "in" } else { "out" }.into()),
            };
            let mut sequence = sequence_of(
                "dissolve",
                vec![PrVideoTrack {
                    items,
                    transitions: vec![transition.clone()],
                    nests: Vec::new(),
                }],
            );
            let mut packer =
                PicturePacker::new("dissolve", [1920, 1080], FrameRate::Fps30, 5 * TICKS);
            let root = packer.root();
            let unrelated = packer.begin_boundary(root, LayerId::new(10)).unwrap();
            let owner = packer.begin_boundary(root, LayerId::new(20)).unwrap();
            for _ in &sequence.video_tracks[0].items {
                packer.record_item(root, owner, 0).unwrap();
            }
            packer
                .finish_container(root, &mut sequence.video_tracks, 5 * TICKS)
                .unwrap();
            let recipe = packer.finish();
            assert!(recipe.is_complete());
            let request = replacement(
                &recipe,
                root,
                vec![if replace_owner { owner } else { unrelated }],
            );
            let output = apply_replacements(
                Some(PrProjectFile::from_sequences(vec![sequence], video_media())),
                recipe,
                &[request],
                Path::new("/tmp/packing/project.prproj"),
            )
            .unwrap();
            let sequence = output.project.single_sequence().unwrap();
            let transitions: Vec<_> = sequence
                .video_tracks
                .iter()
                .flat_map(|track| &track.transitions)
                .collect();
            if replace_owner {
                assert!(transitions.is_empty());
                assert_eq!(sequence.video_occurrences().count(), 1);
            } else {
                assert_eq!(transitions.len(), 1);
                let actual = transitions[0];
                assert_eq!(
                    (actual.start_ticks, actual.cut_ticks, actual.end_ticks),
                    (
                        transition.start_ticks,
                        transition.cut_ticks,
                        transition.end_ticks
                    )
                );
                let track = sequence
                    .video_tracks
                    .iter()
                    .find(|track| !track.transitions.is_empty())
                    .unwrap();
                assert_eq!(track.items.len(), if two_sided { 2 } else { 1 });
                assert_eq!(actual.outgoing_clip, transition.outgoing_clip);
                assert_eq!(actual.incoming_clip, transition.incoming_clip);
                assert_eq!(
                    sequence.video_occurrences().count(),
                    if two_sided { 3 } else { 2 }
                );
            }
        }
    }
}

#[test]
fn omitted_source_boundary_can_receive_picture_without_native_actions() {
    let end = FrameRate::Fps30.ticks_per_frame() * 30;
    let mut packer = PicturePacker::new("omitted", [1920, 1080], FrameRate::Fps30, end);
    let root = packer.root();
    let boundary = packer.begin_boundary(root, LayerId::new(1)).unwrap();
    let recipe = packer.finish();
    assert_eq!(recipe.retained_picture_boundaries(root).count(), 0);
    let replacement = replacement(&recipe, root, vec![boundary]);
    let output = apply_replacements(
        None,
        recipe,
        &[replacement],
        Path::new("/tmp/packing/project.prproj"),
    )
    .unwrap();
    assert_eq!(
        output.foreign_paths,
        vec![PathBuf::from("media/ae-0001/compositions.aep")]
    );
    assert_eq!(output.project.media.len(), 1);
}

#[test]
fn matte_capture_uses_exact_same_lane_placement_addresses() {
    let mut packer = PicturePacker::new("matte", [1920, 1080], FrameRate::Fps30, 0);
    let root = packer.root();
    let first = packer.begin_boundary(root, LayerId::new(1)).unwrap();
    let first_token = packer.record_item(root, first, 0).unwrap();
    let second = packer.begin_boundary(root, LayerId::new(2)).unwrap();
    let second_token = packer.record_item(root, second, 0).unwrap();
    let guide = packer.begin_boundary(root, LayerId::new(3)).unwrap();
    let guide_token = packer.record_item(root, guide, 1).unwrap();
    packer
        .record_matte(
            root,
            PlacementLocation::Item {
                track: 0,
                position: 1,
            },
            PlacementLocation::Item {
                track: 1,
                position: 0,
            },
        )
        .unwrap();
    let recipe = packer.finish();
    let relations = &recipe.containers[&root].mattes;
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].consumer, second_token);
    assert_ne!(relations[0].consumer, first_token);
    assert_eq!(relations[0].source, guide_token);
    assert_eq!(relations[0].consumer_boundary, second);
}

#[test]
fn capture_exhaustion_keeps_native_lowering_available_but_rejects_replay() {
    let mut packer = PicturePacker::new("bounded", [1920, 1080], FrameRate::Fps30, 0);
    let root = packer.root();
    packer.next_boundary = u32::MAX - 1;
    packer.begin_boundary(root, LayerId::new(1)).unwrap();
    packer.begin_boundary(root, LayerId::new(2)).unwrap();
    packer.finish_container(root, &mut [], 0).unwrap();
    let recipe = packer.finish();
    assert!(!recipe.is_complete());
    assert_eq!(recipe.boundaries().len(), 1);
    let error = apply_replacements(None, recipe, &[], Path::new("/tmp/packing")).unwrap_err();
    assert!(error.to_string().contains("capture is incomplete"));
}

#[test]
fn packing_preserves_names_containers_and_placements_above_former_quotas() {
    let name = "n".repeat((1 << 20) + 1);
    let mut packer = PicturePacker::new(&name, [1920, 1080], FrameRate::Fps30, 0);
    let root = packer.root();
    for id in 0..2049 {
        let boundary = packer.begin_boundary(root, LayerId::new(id)).unwrap();
        packer.record_item(root, boundary, 0).unwrap();
        if id < 1025 {
            packer
                .begin_container(boundary, "child", [1920, 1080], FrameRate::Fps30, 0)
                .unwrap();
        }
    }
    let recipe = packer.finish();
    assert!(recipe.is_complete());
    assert_eq!(recipe.containers.len(), 1026);
    assert_eq!(recipe.containers[&root].header.name, name);
    assert_eq!(recipe.containers[&root].actions.len(), 2049);
}

#[test]
fn more_than_256_picture_replacements_are_validated() {
    let end = FrameRate::Fps30.ticks_per_frame() * 30;
    let mut packer = PicturePacker::new("many", [1920, 1080], FrameRate::Fps30, end);
    let root = packer.root();
    let boundaries: Vec<_> = (0..257)
        .map(|id| packer.begin_boundary(root, LayerId::new(id)).unwrap())
        .collect();
    let recipe = packer.finish();
    let replacements: Vec<_> = boundaries
        .into_iter()
        .enumerate()
        .map(|(index, boundary)| {
            let mut request = replacement(&recipe, root, vec![boundary]);
            request.picture.relative_path =
                format!("media/ae-{:04}/compositions.aep", index + 1).into();
            request
        })
        .collect();
    let plans = validate_replacements(&recipe, &replacements).unwrap();
    assert_eq!(plans[&root].len(), 257);
    let output =
        apply_replacements(None, recipe, &replacements, Path::new("/tmp/packing")).unwrap();
    assert_eq!(output.foreign_paths.len(), 257);
    assert_eq!(
        output
            .project
            .single_sequence()
            .unwrap()
            .video_items()
            .count(),
        257
    );
}

#[test]
fn placement_token_overflow_remains_an_incomplete_capture() {
    let mut packer = PicturePacker::new("tokens", [1920, 1080], FrameRate::Fps30, 0);
    let root = packer.root();
    let boundary = packer.begin_boundary(root, LayerId::new(1)).unwrap();
    packer.next_placement = u32::MAX;
    assert_eq!(
        packer.record_item(root, boundary, 0).unwrap(),
        PlacementToken(u32::MAX)
    );
    assert!(!packer.finish().is_complete());
}

#[test]
fn more_than_1024_source_boundaries_remain_replaceable() {
    let end = FrameRate::Fps30.ticks_per_frame() * 30;
    let mut packer = PicturePacker::new("large", [1920, 1080], FrameRate::Fps30, end);
    let root = packer.root();
    let mut boundaries = Vec::new();
    for id in 0..2049 {
        boundaries.push(packer.begin_boundary(root, LayerId::new(id)).unwrap());
    }
    let recipe = packer.finish();
    assert!(recipe.is_complete());
    assert_eq!(recipe.boundaries().len(), 2049);
    let replacement = replacement(&recipe, root, boundaries);
    let output =
        apply_replacements(None, recipe, &[replacement], Path::new("/tmp/packing")).unwrap();
    assert_eq!(output.foreign_paths.len(), 1);
}

fn replacement(
    recipe: &PicturePackingRecipe,
    container: PictureContainerToken,
    boundaries: Vec<SourceBoundaryToken>,
) -> PictureReplacement {
    let end = recipe.containers[&container].header.timeline_end_ticks;
    PictureReplacement {
        packing_id: recipe.id(),
        container,
        boundaries,
        picture: AfterEffectsPicture {
            composition_guid: "00000001-0000-0000-0000-000000000000".into(),
            relative_path: "media/ae-0001/compositions.aep".into(),
            dimensions: [1920, 1080],
            frame_rate: FrameRate::Fps30,
            intrinsic_duration_ticks: end,
            timeline_ticks: 0..end,
            source_ticks: 0..end,
            enabled: true,
        },
    }
}

#[test]
fn matte_update_targets_second_disjoint_consumer_not_lane_head() {
    use crate::schema::{PrMatteChannel, PrTrackMatte};
    let end = FrameRate::Fps30.ticks_per_frame() * 30;
    let recipe = PicturePacker::new("matte", [1920, 1080], FrameRate::Fps30, end).finish();
    let mut request = replacement(&recipe, recipe.root(), vec![]);
    let mut tracks = vec![];
    for start in [0, end] {
        request.picture.timeline_ticks = start..start + end;
        let mut item = foreign_item(
            &request.picture,
            Path::new("/tmp/packing"),
            &mut BTreeSet::new(),
            &mut BTreeMap::new(),
            &mut vec![],
        )
        .unwrap();
        let PrVideoItem::Media(clip) = &mut item else {
            panic!("expected media");
        };
        clip.track_matte = Some(PrTrackMatte {
            track_index: 1,
            channel: PrMatteChannel::Alpha,
        });
        assert_eq!(place_replayed_item(&mut tracks, item, 0), 0);
    }
    let token = PlacementToken(12);
    let locations = BTreeMap::from([(
        token,
        PlacementLocation::Item {
            track: 0,
            position: 1,
        },
    )]);
    set_matte_track(&mut tracks, token, 3, &locations).unwrap();
    let indices: Vec<_> = tracks[0]
        .items
        .iter()
        .map(|item| {
            let PrVideoItem::Media(clip) = item else {
                panic!("expected media");
            };
            clip.track_matte.unwrap().track_index
        })
        .collect();
    assert_eq!(indices, vec![1, 3]);
}

#[test]
fn omitted_middle_slot_and_empty_interval_start_preserve_native_order() {
    for include_upper in [false, true] {
        let end = FrameRate::Fps30.ticks_per_frame() * 30;
        let mut packer = PicturePacker::new("order", [1920, 1080], FrameRate::Fps30, end);
        let root = packer.root();
        let mut tracks = vec![];
        let mut media = BTreeMap::new();
        let mut used = BTreeSet::new();
        let mut boundaries = vec![];
        for id in 1..=3 {
            let boundary = packer.begin_boundary(root, LayerId::new(id)).unwrap();
            boundaries.push(boundary);
            if id == 2 {
                continue;
            }
            let mut request = replacement(&packer.recipe, root, vec![boundary]);
            request.picture.relative_path =
                format!("media/ae-{:04}/compositions.aep", id + 1).into();
            let item = foreign_item(
                &request.picture,
                Path::new("/tmp/packing"),
                &mut used,
                &mut media,
                &mut vec![],
            )
            .unwrap();
            let track = place_replayed_item(&mut tracks, item, 0);
            packer.record_item(root, boundary, track).unwrap();
        }
        packer.finish_container(root, &mut tracks, end).unwrap();
        let recipe = packer.finish();
        assert_eq!(
            recipe.retained_picture_boundaries(root).collect::<Vec<_>>(),
            vec![boundaries[0], boundaries[2]]
        );
        let mut sequence = recipe.containers[&root].empty_sequence();
        sequence.video_tracks = tracks;
        let selected = if include_upper {
            boundaries[1..].to_vec()
        } else {
            vec![boundaries[1]]
        };
        let request = replacement(&recipe, root, selected);
        let project = PrProjectFile::from_sequences(vec![sequence], media);
        let output =
            apply_replacements(Some(project), recipe, &[request], Path::new("/tmp/packing"))
                .unwrap();
        let (sequences, _) = output.project.into_parts();
        let ids: Vec<_> = sequences[0]
            .video_tracks
            .iter()
            .map(|track| {
                let PrVideoItem::Media(clip) = &track.items[0] else {
                    panic!("expected media");
                };
                clip.media.0.as_str()
            })
            .collect();
        let expected = if include_upper {
            vec!["hybrid-after-effects-0", "hybrid-after-effects-2"]
        } else {
            vec![
                "hybrid-after-effects-0",
                "hybrid-after-effects-2",
                "hybrid-after-effects-1",
            ]
        };
        assert_eq!(ids, expected);
    }
}

#[test]
fn orphan_container_replacement_is_rejected_before_replay() {
    let end = FrameRate::Fps30.ticks_per_frame() * 30;
    let mut packer = PicturePacker::new("orphan", [1920, 1080], FrameRate::Fps30, end);
    let root = packer.root();
    let parent = packer.begin_boundary(root, LayerId::new(1)).unwrap();
    let child = packer
        .begin_container(parent, "child", [1920, 1080], FrameRate::Fps30, end)
        .unwrap();
    let boundary = packer.begin_boundary(child, LayerId::new(2)).unwrap();
    let recipe = packer.finish();
    let request = replacement(&recipe, child, vec![boundary]);
    let error = validate_replacements(&recipe, &[request]).unwrap_err();
    assert!(error.to_string().contains("not retained"));
}

fn pending_nested_recipe() -> (
    PicturePackingRecipe,
    SourceBoundaryToken,
    PictureContainerToken,
    SourceBoundaryToken,
) {
    let end = FrameRate::Fps30.ticks_per_frame() * 30;
    let mut packer = PicturePacker::new("pending", [1920, 1080], FrameRate::Fps30, end);
    let root = packer.root();
    let parent = packer.begin_boundary(root, LayerId::new(1)).unwrap();
    let child = packer
        .begin_container(parent, "child", [1920, 1080], FrameRate::Fps30, end)
        .unwrap();
    let boundary = packer.begin_boundary(child, LayerId::new(2)).unwrap();
    let sequence = packer.recipe.containers[&child].empty_sequence();
    let nest = PrNestOccurrence {
        id: None,
        reverse_source_duration: None,
        playback_rate: 1.0,
        time_remap: None,
        start_ticks: 0,
        end_ticks: end,
        in_ticks: 0,
        out_ticks: end,
        transform: Default::default(),
        opacity: 100.0,
        blend_mode: PrBlendMode::Normal,
        animations: vec![],
        crop: Default::default(),
        linear_wipe: None,
        opacity_mask: None,
        track_matte: None,
        effects: vec![],
        geometry2_masks: Default::default(),
        effects_above_mask: 0,
        enabled: true,
        sequence,
    };
    packer
        .record_pending_nest(root, parent, child, nest)
        .unwrap();
    (packer.finish(), parent, child, boundary)
}

#[test]
fn pending_empty_nest_is_not_retained_native_picture() {
    let (recipe, _, child, _) = pending_nested_recipe();
    assert_eq!(recipe.retained_picture_boundaries(recipe.root()).count(), 0);
    assert_eq!(recipe.retained_picture_boundaries(child).count(), 0);
}

#[test]
fn pending_empty_nested_container_accepts_picture_in_its_local_clock() {
    let (recipe, _, child, boundary) = pending_nested_recipe();
    let request = replacement(&recipe, child, vec![boundary]);
    let output = apply_replacements(None, recipe, &[request], Path::new("/tmp/packing")).unwrap();
    let (sequences, media) = output.project.into_parts();
    let nest = sequences[0].nest_occurrences().next().unwrap();
    assert_eq!(nest.sequence.video_items().count(), 1);
    assert_eq!(media.len(), 1);
}

#[test]
fn indexed_boundary_positions_still_reject_another_containers_boundary() {
    let (recipe, _, _, boundary) = pending_nested_recipe();
    let request = replacement(&recipe, recipe.root(), vec![boundary]);
    let error = validate_replacements(&recipe, &[request]).unwrap_err();
    assert!(error
        .to_string()
        .contains("boundary belongs to the wrong container"));
}

#[test]
fn ancestor_and_descendant_replacements_are_rejected() {
    let (recipe, parent, child, boundary) = pending_nested_recipe();
    let requests = [
        replacement(&recipe, recipe.root(), vec![parent]),
        replacement(&recipe, child, vec![boundary]),
    ];
    let error = validate_replacements(&recipe, &requests).unwrap_err();
    assert!(error.to_string().contains("overlap through an ancestor"));
}

#[test]
fn whole_parent_replacement_removes_descendant_native_witness_owners() {
    let (recipe, parent, _, _) = pending_nested_recipe();
    let request = replacement(&recipe, recipe.root(), vec![parent]);
    let output = apply_replacements(None, recipe, &[request], Path::new("/tmp/packing")).unwrap();
    assert_eq!(
        output
            .project
            .single_sequence()
            .unwrap()
            .video_items()
            .count(),
        1
    );
    assert_eq!(
        output
            .project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .count(),
        0
    );
}

#[test]
fn foreign_paths_require_the_typed_ae_scope_layout() {
    assert!(normalized_foreign_path(Path::new("media/ae-0001/compositions.aep")).is_ok());
    assert!(normalized_foreign_path(Path::new("media/compositions.aep")).is_err());
    assert!(normalized_foreign_path(Path::new("media/ae-0000/compositions.aep")).is_err());
    assert!(normalized_foreign_path(Path::new("media/ae-10000/compositions.aep")).is_err());
    for path in [
        "media//ae-0001/compositions.aep",
        "media/./ae-0001/compositions.aep",
        "media/ae-0001/compositions.aep/",
    ] {
        assert!(normalized_foreign_path(Path::new(path)).is_err(), "{path}");
    }
}
