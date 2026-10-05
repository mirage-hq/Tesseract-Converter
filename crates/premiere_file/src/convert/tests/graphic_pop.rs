use crate::{
    convert::premiere_to_tesseract,
    format::{inspect_project_with_omissions, PrProjectFile},
    schema::PrSequence,
    Omission,
};
use serde_json::{json, Value};

type SequenceMutation = fn(&mut PrSequence);

fn native_xml() -> String {
    let records = include_str!("../../../tests/fixtures/graphic_pop/native.xml")
        .replace("<PremiereData>", "")
        .replace("</PremiereData>", "");
    format!(
        r#"<PremiereData Version="3">
        <Sequence ObjectUID="pop-test-sequence"><Name>Graphic Pop</Name><TrackGroups><TrackGroup><Second ObjectRef="1"/></TrackGroup></TrackGroups></Sequence>
        <VideoTrackGroup ObjectID="1"><TrackGroup><Tracks><Track ObjectURef="pop-track"/></Tracks><FrameRate>8467200000</FrameRate></TrackGroup><FrameRect>0,0,1080,1920</FrameRect><ComponentOwner><Components ObjectRef="2"/></ComponentOwner></VideoTrackGroup>
        <VideoComponentChain ObjectID="2"><ComponentChain/></VideoComponentChain>
        <VideoClipTrack ObjectUID="pop-track"><ClipTrack><Track><ID>1</ID></Track><ClipItems><TrackItems><TrackItem ObjectRef="421"/><TrackItem ObjectRef="422"/><TrackItem ObjectRef="423"/><TrackItem ObjectRef="424"/></TrackItems></ClipItems><TransitionItems><TrackItems><TrackItem ObjectRef="425"/><TrackItem ObjectRef="426"/><TrackItem ObjectRef="427"/></TrackItems></TransitionItems></ClipTrack></VideoClipTrack>
        {records}</PremiereData>"#
    )
}

fn native_project() -> PrProjectFile {
    let (project, omissions) = inspect_project_with_omissions(&native_xml(), None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        project.single_sequence().unwrap().video_tracks[0]
            .transitions
            .len(),
        3
    );
    project
}

fn convert(project: &PrProjectFile, sequence: &PrSequence) -> (Value, Vec<Omission>) {
    let mut omissions = Vec::new();
    let doc = premiere_to_tesseract(
        sequence,
        &project.media,
        &crate::tesseract_output::asset_ids_in_order(sequence, &project.media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    (doc, omissions)
}

fn keys<'a>(doc: &'a Value, owner: &Value, property: &str) -> &'a Vec<Value> {
    doc["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == owner["id"] && entry["target"]["propertyType"] == property
        })
        .unwrap_or_else(|| panic!("missing {property} for {}", owner["id"]))["animator"]
        ["keyframes"]
        .as_array()
        .unwrap()
}

#[test]
fn native_two_sided_graphic_pop_owns_whole_blocks_and_merges_disjoint_windows() {
    let project = native_project();
    let (doc, omissions) = convert(&project, project.single_sequence().unwrap());
    let pop_omissions = omissions
        .iter()
        .filter(|o| o.record.starts_with("VideoTransitionTrackItem:"))
        .collect::<Vec<_>>();
    assert_eq!(pop_omissions.len(), 3);
    assert!(
        pop_omissions
            .iter()
            .all(|o| o.kind == crate::OmissionKind::Approximated),
        "{pop_omissions:?}"
    );
    let layers = doc["composition"]["layers"].as_array().unwrap();
    let opening = layers
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    assert_eq!(opening["layers"].as_array().unwrap().len(), 2);
    assert_eq!(
        opening["playback"]["inputRange"],
        json!({"start":0,"duration":1267})
    );
    let sx = keys(&doc, opening, "scaleX");
    assert_eq!(sx.first().unwrap()["value"]["value"], 100.0);
    assert_eq!(sx.last().unwrap()["layerTime"], 1267);
    assert_eq!(sx.last().unwrap()["value"]["value"], 0.0);
    for layer in layers.iter().filter(|layer| layer["type"] == "Text") {
        let sx = keys(&doc, layer, "scaleX");
        assert_eq!(sx.first().unwrap()["layerTime"], 0);
        assert_eq!(sx.first().unwrap()["value"]["value"], 0.0);
        let start = layer["activeRange"]["start"].as_u64().unwrap();
        if start == 1267 || start == 4400 {
            assert_eq!(sx.len(), 32, "head and tail belong to one track");
            assert_eq!(sx.last().unwrap()["value"]["value"], 0.0);
        } else {
            assert_eq!(sx.last().unwrap()["value"]["value"], 100.0);
        }
    }
}

fn graphic_mut(sequence: &mut PrSequence, index: usize) -> &mut crate::schema::PrGraphic {
    let crate::schema::PrVideoItem::Graphic(graphic) = &mut sequence.video_tracks[0].items[index]
    else {
        panic!("native fixture item is a graphic");
    };
    graphic
}

fn sample(keys: &[Value], time: i64) -> f64 {
    let t = |key: &Value| key["layerTime"].as_i64().unwrap();
    let v = |key: &Value| key["value"]["value"].as_f64().unwrap();
    if time <= t(&keys[0]) {
        return v(&keys[0]);
    }
    for pair in keys.windows(2) {
        if time <= t(&pair[1]) {
            let fraction = (time - t(&pair[0])) as f64 / (t(&pair[1]) - t(&pair[0])) as f64;
            return v(&pair[0]) + fraction * (v(&pair[1]) - v(&pair[0]));
        }
    }
    v(keys.last().unwrap())
}

fn assert_near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}

#[test]
fn graphic_pop_cut_samples_and_settled_plateau_use_owner_local_time() {
    let project = native_project();
    let (doc, _) = convert(&project, project.single_sequence().unwrap());
    let layers = doc["composition"]["layers"].as_array().unwrap();
    let outgoing = layers
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    let incoming = layers
        .iter()
        .find(|layer| layer["activeRange"]["start"] == 1267)
        .unwrap();
    let sx = keys(&doc, outgoing, "scaleX");
    assert_near(sample(sx, 1000), 100.0);
    assert_near(sample(sx, 1220), 112.8);
    assert_near(sample(sx, 1267), 0.0);
    let sx = keys(&doc, incoming, "scaleX");
    assert_near(sample(sx, 0), 0.0);
    assert_near(sample(sx, 33), 103.4);
    assert_near(sample(sx, 46), 112.8);
    assert_near(sample(sx, 1000), 100.0);
    assert_near(sample(sx, 3066), 106.8);
    assert_near(sample(sx, 3133), 0.0);
    assert_eq!(
        outgoing["playback"]["inputRange"]["duration"],
        incoming["activeRange"]["start"]
    );
    // The outgoing owner ends at the cut; no handle extends it past that cut.
    for line in outgoing["layers"].as_array().unwrap() {
        assert_eq!(line["activeRange"], json!({"start":0,"duration":1267}));
        assert_eq!(line["parent"], outgoing["id"]);
        assert!(doc["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["target"]["layerId"] != line["id"]));
    }
    assert!(!outgoing["motionBlur"].as_bool().unwrap_or(false));
    assert!(!incoming["motionBlur"].as_bool().unwrap_or(false));
}

#[test]
fn graphic_pop_keeps_vector_motion_opacity_children_and_fractional_frame_clocks() {
    use crate::{
        format::FrameRate,
        schema::{
            text::{PrGraphicObject, PrTextTransform, PrVectorMotion},
            PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe, TICKS,
        },
    };
    let project = native_project();
    let mut sequence = project.single_sequence().unwrap().clone();
    let original_frame = sequence.frame_rate.ticks_per_frame();
    sequence.frame_rate = FrameRate::Fps30000Over1001;
    let frame = sequence.frame_rate.ticks_per_frame();
    let shifted = |tick: i64| (tick / original_frame + 1) * frame;
    for item in &mut sequence.video_tracks[0].items {
        let crate::schema::PrVideoItem::Graphic(graphic) = item else {
            unreachable!()
        };
        graphic.start_ticks = shifted(graphic.start_ticks);
        graphic.end_ticks = shifted(graphic.end_ticks);
        graphic.in_ticks += 60 * frame;
    }
    for transition in &mut sequence.video_tracks[0].transitions {
        transition.start_ticks = shifted(transition.start_ticks);
        transition.cut_ticks = shifted(transition.cut_ticks);
        transition.end_ticks = shifted(transition.end_ticks);
    }
    sequence.timeline_end_ticks = shifted(sequence.timeline_end_ticks);
    let first = graphic_mut(&mut sequence, 0);
    first.opacity = 75.0;
    first.animations = vec![PrPropertyAnimation::Opacity(vec![
        PrScalarKeyframe {
            source_ticks: first.in_ticks,
            value: 75.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: first.in_ticks + TICKS,
            value: 55.0,
            easing: PrKeyframeEasing::Linear,
        },
    ])];
    first.vector_motion = Some(PrVectorMotion {
        position: [600.0, 700.0],
        anchor: [50.0, 90.0],
        scale: 150.0,
        rotation: -25.0,
        animations: Vec::new(),
    });
    let PrGraphicObject::TextLines(text) = &mut first.objects[0] else {
        unreachable!()
    };
    text.transform = PrTextTransform {
        position: [140.0, 180.0],
        anchor: [13.0, -17.0],
        scale: 80.0,
        rotation: 37.0,
        opacity: 65.0,
    };
    let (doc, omissions) = convert(&project, &sequence);
    assert!(
        !omissions
            .iter()
            .any(|o| o.record.starts_with("VideoTransitionTrackItem:")
                && o.kind == crate::OmissionKind::Omitted),
        "{omissions:?}"
    );
    let owner = doc["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    assert_eq!(owner["transform"]["opacity"], 75.0);
    assert_eq!(owner["transform"]["scale"], json!([150.0, 150.0]));
    assert_eq!(owner["transform"]["rotation"], -25.0);
    assert_eq!(
        owner["playback"]["inputRange"],
        json!({"start":33,"duration":1268})
    );
    let block = &owner["layers"][0];
    assert_eq!(block["type"], "Group");
    assert_ne!(block["id"], owner["id"]);
    assert_eq!(block["parent"], owner["id"]);
    assert_eq!(block["transform"]["opacity"], 65.0);
    assert_eq!(block["transform"]["scale"], json!([80.0, 80.0]));
    assert_eq!(block["layers"].as_array().unwrap().len(), 2);
    assert_eq!(
        block["playback"]["inputRange"],
        json!({"start":0,"duration":1268})
    );
    assert_eq!(keys(&doc, owner, "opacity")[0]["layerTime"], 0);
    assert_eq!(keys(&doc, owner, "opacity")[1]["layerTime"], 1000);
    let sx = keys(&doc, owner, "scaleX");
    let px = keys(&doc, owner, "positionX");
    let py = keys(&doc, owner, "positionY");
    assert_eq!(sx.last().unwrap()["layerTime"], 1268);
    // Independently computed composition of the two authored transforms at
    // block-local [0,0]. Every sample must keep this common pivot fixed.
    let center = [759.9045155208125, 782.0088745931658];
    for ((scale, x), y) in sx.iter().zip(px).zip(py) {
        let q = scale["value"]["value"].as_f64().unwrap() / 150.0;
        assert_near(
            x["value"]["value"].as_f64().unwrap() + q * (center[0] - 600.0),
            center[0],
        );
        assert_near(
            y["value"]["value"].as_f64().unwrap() + q * (center[1] - 700.0),
            center[1],
        );
    }
    assert_eq!(sx.last().unwrap()["value"]["value"], 0.0);
    assert!(doc["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry["target"]["layerId"] != block["id"]));
}

fn single_transition(sequence: &mut PrSequence) {
    sequence.video_tracks[0].items.truncate(2);
    sequence.video_tracks[0].transitions.truncate(1);
    sequence.timeline_end_ticks = sequence.occurrence_end_ticks();
}

#[test]
fn graphic_pop_rejects_unsupported_hosts_keyed_geometry_and_retained_shadows_atomically() {
    use crate::schema::{
        text::PrRgb,
        text::{PrGraphicObject, PrTextFrame, PrVerticalAlign},
        text_shadow::PrTextShadow,
        PrBlendMode, PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe,
    };
    fn shadow() -> PrTextShadow {
        PrTextShadow {
            color: PrRgb([0, 0, 0]),
            opacity: 80.0,
            angle: 135.0,
            distance: 12.0,
            size: 0.0,
            blur: 8.0,
        }
    }
    let project = native_project();
    let mutations: &[(&str, SequenceMutation)] = &[
        ("shapes", |s| {
            graphic_mut(s, 0).objects = crate::tests::support::shape_graphic().objects
        }),
        ("keyed text topology", |s| {
            let graphic = graphic_mut(s, 1);
            let source = graphic.in_ticks;
            let PrGraphicObject::Text(text) = &mut graphic.objects[0] else {
                unreachable!()
            };
            text.source_text_keys
                .push(crate::schema::text::PrSourceTextKey {
                    source_ticks: source,
                    document: text.document.clone(),
                });
        }),
        ("independent object centers", |s| {
            let object = graphic_mut(s, 0).objects[0].clone();
            graphic_mut(s, 0).objects.push(object);
        }),
        ("static point-text", |s| {
            let PrGraphicObject::Text(text) = &mut graphic_mut(s, 1).objects[0] else {
                unreachable!()
            };
            text.document.frame = PrTextFrame::Box {
                width: 500.0,
                height: 400.0,
                vertical: PrVerticalAlign::Center,
            };
        }),
        ("Normal blend", |s| {
            graphic_mut(s, 1).blend_mode = PrBlendMode::Multiply
        }),
        // The mask's guide stays in the sequence frame.
        ("clip Opacity mask", |s| {
            graphic_mut(s, 1).opacity_mask = Some(crate::tests::support::opacity_mask())
        }),
        ("static point-text", |s| {
            let PrGraphicObject::Text(text) = &mut graphic_mut(s, 1).objects[0] else {
                unreachable!()
            };
            text.animations
                .push(PrPropertyAnimation::UniformScale(vec![PrScalarKeyframe {
                    source_ticks: graphic_in(),
                    value: 90.0,
                    easing: PrKeyframeEasing::Linear,
                }]));
        }),
        ("retained text shadow", |s| {
            let PrGraphicObject::TextLines(text) = &mut graphic_mut(s, 0).objects[0] else {
                unreachable!()
            };
            for document in &mut text.documents {
                document.shadow = Some(shadow());
            }
        }),
    ];
    for (reason, mutate) in mutations {
        let mut sequence = project.single_sequence().unwrap().clone();
        single_transition(&mut sequence);
        mutate(&mut sequence);
        let (doc, omissions) = convert(&project, &sequence);
        assert!(
            !doc.to_string().contains("film-impact-pop"),
            "{reason}: {omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .any(|o| o.record == sequence.video_tracks[0].transitions[0].id
                    && o.kind == crate::OmissionKind::Omitted
                    && o.reason.contains(reason)),
            "{reason}: {omissions:?}"
        );
        assert!(!omissions
            .iter()
            .any(|o| o.record == sequence.video_tracks[0].transitions[0].id
                && o.kind == crate::OmissionKind::Approximated));
    }
    // A shadow already rejected by static admission is not resurrected by Pop.
    let mut sequence = project.single_sequence().unwrap().clone();
    single_transition(&mut sequence);
    let PrGraphicObject::TextLines(text) = &mut graphic_mut(&mut sequence, 0).objects[0] else {
        unreachable!()
    };
    text.transform.scale = 90.0;
    for document in &mut text.documents {
        document.shadow = Some(shadow());
    }
    let (doc, omissions) = convert(&project, &sequence);
    assert!(doc.to_string().contains("film-impact-pop"), "{omissions:?}");
    assert!(omissions
        .iter()
        .any(|o| o.reason.contains("shadow not converted")));
}

fn graphic_in() -> i64 {
    crate::format::FrameRate::Fps30.generator_in_ticks()
}

#[test]
fn graphic_pop_rejects_overlapping_windows_conflicting_transitions_and_invalid_halves() {
    use crate::schema::PrVideoTransitionKind;
    let project = native_project();
    let mutations: &[(&str, SequenceMutation)] = &[
        ("overlap", |s| {
            let first = s.video_tracks[0].transitions[0].clone();
            s.video_tracks[0].transitions.push(first);
        }),
        ("overlap", |s| {
            s.video_tracks[0].transitions[0].end_ticks = s.video_tracks[0].transitions[1]
                .start_ticks
                + crate::format::FrameRate::Fps30.ticks_per_frame()
        }),
        ("conflicting transition ownership", |s| {
            s.video_tracks[0].transitions[1].kind = PrVideoTransitionKind::FilmImpactDissolve
        }),
        ("whole number", |s| {
            s.video_tracks[0].transitions[0].end_ticks -= 1
        }),
        ("inside its range", |s| {
            s.video_tracks[0].transitions[0].start_ticks -= 2 * crate::schema::TICKS
        }),
        ("distinct", |s| {
            s.video_tracks[0].transitions[0].outgoing_clip =
                s.video_tracks[0].transitions[0].incoming_clip.clone()
        }),
    ];
    for (reason, mutate) in mutations {
        let mut sequence = project.single_sequence().unwrap().clone();
        mutate(&mut sequence);
        let (doc, omissions) = convert(&project, &sequence);
        assert!(
            omissions
                .iter()
                .any(|o| o.record == sequence.video_tracks[0].transitions[0].id
                    && o.kind == crate::OmissionKind::Omitted
                    && o.reason.contains(reason)),
            "{reason}: {omissions:?}"
        );
        // Rejected first transition never publishes the opening owner's tail.
        let owner = doc["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        assert!(
            doc["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| entry["target"]["layerId"] != owner["id"]),
            "{reason}"
        );
    }
}

#[test]
fn native_graphic_pop_changed_or_keyed_profile_retains_graphics_without_pop() {
    for xml in [
        native_xml().replace("260300.,0,0,0,0,0,0", "260301.,0,0,0,0,0,0"),
        native_xml().replace(
            "<ParameterID>54</ParameterID>",
            "<ParameterID>54</ParameterID><Keyframes>1,2</Keyframes>",
        ),
    ] {
        let (project, read_omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert_eq!(
            project.single_sequence().unwrap().video_tracks[0]
                .items
                .len(),
            4
        );
        assert!(project.single_sequence().unwrap().video_tracks[0]
            .transitions
            .is_empty());
        assert_eq!(
            read_omissions
                .iter()
                .filter(|o| ["425", "426", "427"].contains(&o.record.as_str()))
                .count(),
            3
        );
        let (doc, _) = convert(&project, project.single_sequence().unwrap());
        assert!(!doc.to_string().contains("film-impact-pop"));
    }
}

#[test]
fn graphic_pop_geometry_conflict_preserves_existing_graph_and_rejects_both_halves() {
    use super::{ItemLayer, LayerScope};
    use fx_schema::{AnimationGraph, PropType, Property, PropertyAnimator, PropertyValue};
    let project = native_project();
    let sequence = project.single_sequence().unwrap();
    let mut plain = sequence.clone();
    plain.video_tracks[0].transitions.clear();
    let mut omissions = Vec::new();
    let document =
        premiere_to_tesseract(&plain, &project.media, &Default::default(), &mut omissions).unwrap();
    let layers = document.composition().layers();
    let mut next = 10;
    let mut effects = crate::convert::effects::EffectIdAllocator::default();
    let mut shutter = None;
    let mut linked = crate::linked_compositions::LinkedCompositions::default();
    let clocks = crate::media::PictureClocks::new();
    let mut scope = LayerScope::root(
        sequence.dimensions(),
        &mut next,
        &mut effects,
        &mut shutter,
        &clocks,
        &mut linked,
    );
    for item in &sequence.video_tracks[0].items {
        let graphic = item.graphic().unwrap();
        let start = super::time_from_ticks(graphic.start_ticks).unwrap();
        let layer = layers
            .iter()
            .find(|layer| layer.data().active_range().start == start)
            .unwrap();
        scope
            .item_layers
            .insert((0, graphic.start_ticks), ItemLayer::Plain(layer.id()));
    }
    let mut dynamics = AnimationGraph::new();
    let owner = scope.item_layers[&(0, 0)].id();
    dynamics
        .set_property(
            Property::new(owner, PropType::ScaleX),
            PropertyAnimator::constant(PropertyValue::Float(77.0)).unwrap(),
            Vec::new(),
        )
        .unwrap();
    let before = dynamics.clone();
    omissions.clear();
    super::pop::import_graphics(
        &sequence.video_tracks[0],
        sequence.frame_rate,
        &scope,
        0,
        layers,
        &mut dynamics,
        &mut omissions,
    );
    assert_eq!(
        dynamics, before,
        "conflicting track must not publish either owner half"
    );
    assert_eq!(omissions.len(), 3);
    assert!(
        omissions
            .iter()
            .all(|o| o.kind == crate::OmissionKind::Omitted
                && o.reason.contains("existing geometry animator")),
        "{omissions:?}"
    );
}

#[test]
fn graphic_pop_rejects_enclosing_nest_clocks_without_changing_text_placement() {
    let project = native_project();
    let inner = project.single_sequence().unwrap().clone();
    let nest = crate::tests::support::nest_of(inner.clone(), 0..inner.timeline_end_ticks, 0);
    let mut outer = crate::tests::support::sequence_of(
        "Outer",
        vec![crate::schema::PrVideoTrack {
            items: Vec::new(),
            nests: vec![nest],
            transitions: Vec::new(),
        }],
    );
    outer.width = inner.width;
    outer.height = inner.height;
    let (doc, omissions) = convert(&project, &outer);
    assert!(!doc.to_string().contains("film-impact-pop"));
    assert_eq!(
        omissions
            .iter()
            .filter(|o| o.reason.contains(
                "graphic Pop requires static 2D geometry, Normal blend and the document clock"
            ))
            .count(),
        3
    );
    let group = doc["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    assert_eq!(group["layers"].as_array().unwrap().len(), 4);
}

#[test]
fn graphic_pop_touching_windows_share_one_settled_key() {
    let project = native_project();
    let mut sequence = project.single_sequence().unwrap().clone();
    sequence.video_tracks[0].transitions[0].end_ticks =
        sequence.video_tracks[0].transitions[1].start_ticks;
    let (doc, omissions) = convert(&project, &sequence);
    assert!(
        !omissions
            .iter()
            .any(|o| o.record.starts_with("VideoTransitionTrackItem:")
                && o.kind == crate::OmissionKind::Omitted),
        "{omissions:?}"
    );
    let owner = doc["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["activeRange"]["start"] == 1267)
        .unwrap();
    let sx = keys(&doc, owner, "scaleX");
    assert_eq!(sx.len(), 31);
    assert_eq!(sx.iter().filter(|key| key["layerTime"] == 2933).count(), 1);
    assert_near(sample(sx, 2933), 100.0);
    assert_eq!(sx.first().unwrap()["value"]["value"], 0.0);
    assert_eq!(sx.last().unwrap()["value"]["value"], 0.0);
}
