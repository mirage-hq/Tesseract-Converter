//! Supplemental switch/occurrence tests derived from pinned native records.
//! Mutated AV descriptors do not constitute native visual/audio fidelity proof.

use super::*;

fn av_project() -> StructuralProject {
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/media/audioEnabled.aep"
    ))
    .unwrap();
    let source = project.items.iter_mut().find(|item| item.id == 13).unwrap();
    let descriptor = source.media.as_mut().unwrap().as_mut().unwrap();
    descriptor.kind = crate::structure::MediaKind::AudioVideo;
    descriptor.width = 320;
    descriptor.height = 240;
    project
}

fn switches(layer: &mut Layer, video: bool, audio: bool) {
    let flags = (layer.record.raw_bytes()[39] & !3) | u8::from(video) | (u8::from(audio) << 1);
    patch(layer, 39, &[flags]);
}

fn media_children(group: &GroupLayer) -> (&fx_schema::VideoLayer, &fx_schema::AudioLayer) {
    let content = as_group(&group.layers[0]);
    let video = content
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            FxLayer::Video(video) => Some(video),
            _ => None,
        })
        .unwrap();
    let audio = content
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            FxLayer::Audio(audio) => Some(audio),
            _ => None,
        })
        .unwrap();
    (video, audio)
}

#[test]
fn eye_and_audio_switches_remain_independent_for_av_content() {
    for (video_enabled, audio_enabled) in [(false, true), (true, false), (true, true)] {
        let mut project = av_project();
        switches(
            &mut composition_mut(&mut project, 1).layers[0],
            video_enabled,
            audio_enabled,
        );
        let converted =
            to_structural_fx_document_with_assets(&project, Some(1), &mut |_| true).unwrap();
        let occurrence = as_group(&root(&converted).layers[0]);
        assert!(!occurrence.is_hidden);
        let (video, audio) = media_children(occurrence);
        assert_eq!(video.is_hidden, !video_enabled);
        assert_eq!(audio.is_hidden, !audio_enabled);
        assert_eq!(
            video.volume, None,
            "AV sound is emitted exactly once by its audio sibling"
        );
        assert_eq!(has_enabled_audio(occurrence), audio_enabled);
        assert_eq!(converted.assets.len(), 1);
    }
}

#[test]
fn precomp_eye_off_preserves_descendant_audio_but_audio_off_mutes_it() {
    let mut project = av_project();
    switches(&mut composition_mut(&mut project, 1).layers[0], true, true);
    let mut enclosing = project.item(1).unwrap().clone();
    enclosing.id = 100;
    let ItemKind::Composition(comp) = &mut enclosing.kind else {
        unreachable!()
    };
    patch(&mut comp.layers[0], 40, &1_u32.to_be_bytes());
    switches(&mut comp.layers[0], false, true);
    project.items.push(enclosing);
    let converted =
        to_structural_fx_document_with_assets(&project, Some(100), &mut |_| true).unwrap();
    let outer = as_group(&root(&converted).layers[0]);
    assert!(!outer.is_hidden);
    let source_clock = as_group(&outer.layers[0]);
    let nested = as_group(&source_clock.layers[0]);
    let (video, audio) = media_children(nested);
    assert!(video.is_hidden);
    assert!(!audio.is_hidden);
    switches(
        &mut composition_mut(&mut project, 100).layers[0],
        false,
        false,
    );
    let muted = to_structural_fx_document_with_assets(&project, Some(100), &mut |_| true).unwrap();
    assert!(!has_enabled_audio(root(&muted)));
}

#[test]
fn matte_sample_ignores_provider_eye_without_duplicating_its_audio() {
    let mut project = av_project();
    let comp = composition_mut(&mut project, 1);
    switches(&mut comp.layers[0], false, true);
    let mut provider = comp.layers[0].clone();
    patch(&mut provider, 0, &999_u32.to_be_bytes());
    patch(&mut comp.layers[0], 107, &[1]);
    patch(&mut comp.layers[0], 160, &999_u32.to_be_bytes());
    comp.layers.push(provider);
    let converted =
        to_structural_fx_document_with_assets(&project, Some(1), &mut |_| true).unwrap();
    let layers = &root(&converted).layers;
    assert_eq!(layers.len(), 3);
    let matte = as_group(&layers[0]).track_matte.as_ref().unwrap();
    let helper = layers
        .iter()
        .map(as_group)
        .find(|layer| layer.id == matte.layer)
        .unwrap();
    assert!(!helper.is_hidden);
    let (video, audio) = media_children(helper);
    assert!(
        !video.is_hidden,
        "disabled native provider must still supply matte pixels"
    );
    assert!(audio.is_hidden);
    assert_eq!(video.volume, None);
    assert!(has_enabled_audio(as_group(&layers[0])));
    assert!(has_enabled_audio(as_group(&layers[1])));
    assert!(!has_enabled_audio(helper));
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();
}

/// One footage occurrence with the native CAP2 comp 625 layer 640 clock:
/// start 11000/23976s and source in point -11000/23976s, so its native parent
/// span starts at composition zero although media time zero starts later.
fn negative_source_prefix_project(kind: crate::structure::MediaKind) -> StructuralProject {
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/media/audioEnabled.aep"
    ))
    .unwrap();
    let source = project.items.iter_mut().find(|item| item.id == 13).unwrap();
    let descriptor = source.media.as_mut().unwrap().as_mut().unwrap();
    descriptor.kind = kind;
    descriptor.width = 320;
    descriptor.height = 240;
    let layer = &mut composition_mut(&mut project, 1).layers[0];
    for (numerator_offset, numerator) in [(12, 11_000_i32), (20, -11_000), (28, 31_000)] {
        patch(layer, numerator_offset, &numerator.to_be_bytes());
        patch(layer, numerator_offset + 4, &23_976_u32.to_be_bytes());
    }
    assert_eq!(layer.record.stretch(), Some(1.0));
    project
}

fn source_content(converted: &StructuralConversion) -> &GroupLayer {
    let occurrence = as_group(&root(converted).layers[0]);
    let content = as_group(&occurrence.layers[0]);
    assert_eq!(content.name, "Source content clock");
    content
}

#[test]
fn solid_source_keeps_its_native_parent_span_before_the_layer_start() {
    // Clock edits are supplementary to the pinned native source regression.
    // Denominators are 30; reverse/stretch cases keep ascending native bounds.
    for (start, input, output, stretch, begin_ms, duration_ms) in [
        (27_i32, -27_i32, 301_i32, 1_i32, 0_u64, 10_933_u64),
        (27, 0, 60, 1, 900, 2_000),
        (27, -27, 60, 2, 0, 4_900),
        (120, -27, 60, -1, 2_000, 2_900),
    ] {
        let mut project = read_project(include_bytes!(
            "../../../tests/fixtures/render/solid_color_1080.aep"
        ))
        .expect("independently Adobe-authored Solid source");
        let layer = &mut composition_mut(&mut project, 1).layers[0];
        assert_eq!(layer.record.id(), 15);
        for (offset, numerator) in [(12, start), (20, input), (28, output)] {
            patch(layer, offset, &numerator.to_be_bytes());
            patch(layer, offset + 4, &30_u32.to_be_bytes());
        }
        patch(layer, 8, &stretch.to_be_bytes());
        patch(layer, 108, &1_u32.to_be_bytes());
        let source_id = layer.record.source_id();
        let source = project
            .items
            .iter()
            .find(|item| item.id == source_id)
            .unwrap();
        assert!(is_solid_source(source));
        let color = source.solid.as_ref().unwrap().as_ref().unwrap().color;

        let converted = to_structural_fx_document(&project, Some(1)).unwrap();
        let content = source_content(&converted);
        assert_eq!(content.playback.input_range().start.as_millis(), begin_ms);
        assert_eq!(
            content.playback.input_range().duration.as_millis(),
            duration_ms
        );
        assert!(
            content.playback.time_remap().is_none(),
            "a constant Solid raster has no sampled media clock"
        );
        assert!(!content.is_hidden);
        let solid = content
            .layers
            .iter()
            .find_map(|layer| match layer.data() {
                FxLayer::Rect(rect) => Some(rect),
                _ => None,
            })
            .expect("editable native Solid content");
        assert!(solid.rect.fill_enabled);
        assert_eq!(solid.rect.size, [1920.0, 1080.0]);
        assert_eq!(
            solid.rect.fill_color,
            [
                f64::from(color[0]),
                f64::from(color[1]),
                f64::from(color[2]),
                1.0
            ]
        );
        EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
            .unwrap();
    }
}

#[test]
fn still_image_keeps_its_native_parent_span_before_the_layer_start() {
    let project = negative_source_prefix_project(crate::structure::MediaKind::StillImage);
    let converted =
        to_structural_fx_document_with_assets(&project, Some(1), &mut |_| true).unwrap();
    let content = source_content(&converted);
    // Parent span is start + in .. start + out = 0 .. 42000/23976s.
    assert_eq!(content.playback.input_range().start.as_millis(), 0);
    assert_eq!(content.playback.input_range().duration.as_millis(), 1_752);
    assert!(
        content.playback.time_remap().is_none(),
        "a still has no media clock"
    );
    assert!(!content.is_hidden);
    let image = content
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            FxLayer::Image(image) => Some(image),
            _ => None,
        })
        .expect("editable still image");
    assert!(!image.is_hidden);
    assert_eq!(
        image
            .source
            .asset()
            .expect("asset-backed still")
            .asset_id
            .as_str(),
        "aep-local-item-13"
    );
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();
}

#[test]
fn video_with_the_same_clock_keeps_its_nonnegative_media_clock() {
    let project = negative_source_prefix_project(crate::structure::MediaKind::Video);
    let converted =
        to_structural_fx_document_with_assets(&project, Some(1), &mut |_| true).unwrap();
    let content = source_content(&converted);
    // Video has no frames before media time zero, at the layer start.
    assert_eq!(content.playback.input_range().start.as_millis(), 459);
    assert_eq!(content.playback.input_range().end().as_millis(), 1_752);
    let playback = content
        .playback
        .time_remap()
        .expect("video keeps its affine source clock");
    let first = &playback.keyframes()[0];
    assert_eq!((first.time.as_millis(), first.value.as_millis()), (459, 0));
    assert!(
        content
            .layers
            .iter()
            .any(|layer| matches!(layer.data(), FxLayer::Video(_)))
    );
}

/// Gives a pinned native footage layer one Roto Brush instance. The effect
/// envelope is writer-generated; only its match name is AE's Roto Brush.
fn with_roto_brush(layer: &mut Layer, enabled: bool) {
    fn rename(chunks: &mut [crate::rifx::Chunk], from: &[u8], to: &[u8]) {
        for chunk in chunks {
            if chunk.id() == *b"tdmn"
                && chunk.data_payload().is_some_and(|bytes| {
                    bytes
                        .iter()
                        .copied()
                        .take_while(|byte| *byte != 0)
                        .eq(from.iter().copied())
                })
            {
                let mut name = to.to_vec();
                name.resize(chunk.data_payload().unwrap().len(), 0);
                *chunk = crate::rifx::Chunk::data(*b"tdmn", name).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                rename(children, from, to);
            }
        }
    }
    let mut effect =
        crate::writer::effects::new_effect("ADBE Gaussian Blur 2", enabled, [320.0, 240.0])
            .unwrap();
    effect.properties.clear();
    let mut parade =
        crate::writer::effects::effect_parade(&[effect], layer.record.id(), [320.0, 240.0])
            .unwrap();
    rename(
        std::slice::from_mut(&mut parade),
        b"ADBE Gaussian Blur 2",
        b"ADBE Samurai",
    );
    let root = layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(crate::rifx::Chunk::children_mut)
        .expect("native property root");
    let mut name = b"ADBE Effect Parade".to_vec();
    name.resize(40, 0);
    root.push(crate::rifx::Chunk::data(*b"tdmn", name).unwrap());
    root.push(parade);
}

/// The innermost Group of one native layer occurrence.
fn occurrence(group: &GroupLayer, comp_id: u32, layer_id: u32) -> Option<&GroupLayer> {
    let marker = format!("AEP comp={comp_id} layer={layer_id} ");
    if group.description.starts_with(&marker)
        && !group.description.ends_with("independent matte sample copy")
        && group.layers.first().is_some_and(|layer| {
            matches!(layer.data(), FxLayer::Group(child) if child.name == "Source content clock")
        })
    {
        return Some(group);
    }
    group.layers.iter().find_map(|layer| match layer.data() {
        FxLayer::Group(child) => occurrence(child, comp_id, layer_id),
        _ => None,
    })
}

fn roto_notes(converted: &StructuralConversion) -> Vec<&ImportDiagnostic> {
    converted
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("Roto Brush"))
        .collect()
}

/// CAP2 comp 281 shape: an active Roto Brush foreground above a plate of the
/// same footage, with a child parented to the foreground.
fn roto_paint_project(effect_enabled: bool, effects_switch: bool) -> (StructuralProject, [u32; 3]) {
    let mut project = av_project();
    let comp = composition_mut(&mut project, 1);
    switches(&mut comp.layers[0], true, true);
    let roto = comp.layers[0].record.id();
    let mut child = comp.layers[0].clone();
    patch(&mut child, 0, &998_u32.to_be_bytes());
    patch(&mut child, 132, &roto.to_be_bytes());
    let mut plate = comp.layers[0].clone();
    patch(&mut plate, 0, &999_u32.to_be_bytes());
    with_roto_brush(&mut comp.layers[0], effect_enabled);
    let flags = comp.layers[0].record.raw_bytes()[39] & !4 | u8::from(effects_switch) << 2;
    patch(&mut comp.layers[0], 39, &[flags]);
    comp.layers.insert(0, child);
    comp.layers.push(plate);
    (project, [998, roto, 999])
}

#[test]
fn active_roto_brush_hides_only_its_unsegmented_painted_footage() {
    let (project, [child, roto, plate]) = roto_paint_project(true, true);
    let converted =
        to_structural_fx_document_with_assets(&project, Some(1), &mut |_| true).unwrap();
    let top = root(&converted);
    let owner = occurrence(top, 1, roto).expect("Roto Brush owner occurrence");
    assert!(!owner.is_hidden, "authored owner visibility is kept");
    let (video, audio) = media_children(owner);
    assert!(
        video.is_hidden,
        "unsegmented footage must not cover lower layers"
    );
    assert!(!audio.is_hidden, "Roto Brush does not change audio");
    for sibling in [child, plate] {
        let (video, _) = media_children(occurrence(top, 1, sibling).unwrap());
        assert!(!video.is_hidden, "layer {sibling} keeps its paint");
    }
    // The child is still placed through a transform copy of its parent.
    let child_top = top
        .layers
        .iter()
        .map(as_group)
        .find(|group| occurrence(group, 1, child).is_some())
        .unwrap();
    assert!(!std::ptr::eq(child_top, occurrence(top, 1, child).unwrap()));
    let notes = roto_notes(&converted);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(
        (
            notes[0].limitation,
            notes[0].composition_id,
            notes[0].layer_id
        ),
        (Limitation::Properties, Some(1), Some(roto))
    );
    assert!(
        notes[0]
            .message
            .contains("foreground and its occlusion are lost")
    );
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();

    // A disabled effect or layer effect switch leaves AE's raw footage.
    for (effect_enabled, effects_switch) in [(false, true), (true, false)] {
        let (project, [_, roto, _]) = roto_paint_project(effect_enabled, effects_switch);
        let converted =
            to_structural_fx_document_with_assets(&project, Some(1), &mut |_| true).unwrap();
        let (video, _) = media_children(occurrence(root(&converted), 1, roto).unwrap());
        assert!(!video.is_hidden, "{effect_enabled} {effects_switch}");
        assert!(roto_notes(&converted).is_empty());
    }
}

fn select_matte(consumer: &mut Layer, provider: u32) {
    patch(consumer, 107, &[1]);
    patch(consumer, 160, &provider.to_be_bytes());
}

#[test]
fn matte_alpha_from_an_omitted_roto_brush_leaves_consumers_unmasked() {
    // Direct provider, plus an unaffected consumer of ordinary footage.
    let mut project = av_project();
    let comp = composition_mut(&mut project, 1);
    switches(&mut comp.layers[0], true, true);
    let roto = comp.layers[0].record.id();
    let template = comp.layers[0].clone();
    let layer = |id: u32| {
        let mut layer = template.clone();
        patch(&mut layer, 0, &id.to_be_bytes());
        layer
    };
    let (mut consumer, mut kept, plain) = (layer(997), layer(996), layer(999));
    select_matte(&mut consumer, roto);
    select_matte(&mut kept, 999);
    with_roto_brush(&mut comp.layers[0], true);
    comp.layers.insert(0, consumer);
    comp.layers.extend([kept, plain]);
    let converted =
        to_structural_fx_document_with_assets(&project, Some(1), &mut |_| true).unwrap();
    let top = root(&converted);
    let consumer = occurrence(top, 1, 997).unwrap();
    assert!(
        consumer.track_matte.is_none(),
        "raw full-frame alpha is not the omitted segmentation"
    );
    let kept = occurrence(top, 1, 996).unwrap();
    let matte = kept.track_matte.as_ref().expect("ordinary matte is kept");
    assert_eq!(matte.mode, fx_schema::TrackMatteType::Alpha);
    let helpers: Vec<_> = top
        .layers
        .iter()
        .map(as_group)
        .filter(|group| group.description.ends_with("independent matte sample copy"))
        .collect();
    assert_eq!(helpers.len(), 2);
    let roto_helper = helpers
        .iter()
        .find(|helper| {
            helper
                .description
                .starts_with(&format!("AEP comp=1 layer={roto} "))
        })
        .unwrap();
    assert!(
        roto_helper.is_hidden,
        "an unlinked provider copy must not paint"
    );
    assert!(
        helpers
            .iter()
            .any(|helper| helper.id == matte.layer && !helper.is_hidden)
    );
    let unmasked = converted.diagnostics.iter().any(|diagnostic| {
        diagnostic.limitation == Limitation::TrackMatte
            && diagnostic.layer_id == Some(997)
            && diagnostic.message.contains("left unmasked")
    });
    assert!(unmasked, "the consumer's approximation is diagnosed");

    // A precomposition provider whose source contains the cutout.
    let mut project = av_project();
    let source = composition_mut(&mut project, 1);
    switches(&mut source.layers[0], true, true);
    with_roto_brush(&mut source.layers[0], true);
    let mut enclosing = project.item(1).unwrap().clone();
    enclosing.id = 100;
    let ItemKind::Composition(comp) = &mut enclosing.kind else {
        unreachable!()
    };
    comp.layers[0] = template.clone();
    let mut provider = template.clone();
    patch(&mut provider, 0, &995_u32.to_be_bytes());
    patch(&mut provider, 40, &1_u32.to_be_bytes());
    select_matte(&mut comp.layers[0], 995);
    comp.layers.push(provider);
    project.items.push(enclosing);
    let converted =
        to_structural_fx_document_with_assets(&project, Some(100), &mut |_| true).unwrap();
    let consumer = occurrence(root(&converted), 100, roto).unwrap();
    assert!(
        consumer.track_matte.is_none(),
        "an emptied nested provider must not mask"
    );
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic.composition_id == Some(100)
            && diagnostic.layer_id == Some(995)
            && diagnostic
                .message
                .contains("depends on an omitted Roto Brush")
    }));
}

fn renumbered(template: &Layer, id: u32, source: Option<u32>) -> Layer {
    let mut layer = template.clone();
    patch(&mut layer, 0, &id.to_be_bytes());
    if let Some(source) = source {
        patch(&mut layer, 40, &source.to_be_bytes());
    }
    layer
}

/// A copy of composition 1 with other layers, for nested-precomposition cases.
fn add_composition(project: &mut StructuralProject, id: u32, layers: Vec<Layer>) {
    let mut item = project.item(1).unwrap().clone();
    item.id = id;
    let ItemKind::Composition(comp) = &mut item.kind else {
        unreachable!()
    };
    comp.layers = layers;
    project.items.push(item);
}

#[test]
fn unavailable_cutout_alpha_propagates_through_matte_chains() {
    // 997 uses 996 as its matte, and 996 uses the Roto Brush layer as its own.
    let mut project = av_project();
    let comp = composition_mut(&mut project, 1);
    switches(&mut comp.layers[0], true, true);
    let roto = comp.layers[0].record.id();
    let template = comp.layers[0].clone();
    let mut last = renumbered(&template, 997, None);
    select_matte(&mut last, 996);
    let mut middle = renumbered(&template, 996, None);
    select_matte(&mut middle, roto);
    let mut independent = renumbered(&template, 995, None);
    select_matte(&mut independent, 999);
    with_roto_brush(&mut comp.layers[0], true);
    comp.layers.splice(0..0, [last, middle]);
    comp.layers
        .extend([independent, renumbered(&template, 999, None)]);
    let converted =
        to_structural_fx_document_with_assets(&project, Some(1), &mut |_| true).unwrap();
    let top = root(&converted);
    for consumer in [997, 996] {
        let occurrence = occurrence(top, 1, consumer).unwrap();
        assert!(occurrence.track_matte.is_none(), "layer {consumer}");
    }
    let middle_helper = top
        .layers
        .iter()
        .map(as_group)
        .find(|group| {
            group.description.starts_with("AEP comp=1 layer=996 ")
                && group.description.ends_with("independent matte sample copy")
        })
        .expect("middle provider copy");
    assert!(
        middle_helper.is_hidden,
        "a chained copy must not paint or mask"
    );
    let kept = occurrence(top, 1, 995).unwrap();
    let matte = kept
        .track_matte
        .as_ref()
        .expect("independent matte is kept");
    assert!(
        top.layers
            .iter()
            .map(as_group)
            .any(|group| group.id == matte.layer && !group.is_hidden)
    );
    let context = |layer: u32, text: &str| {
        converted.diagnostics.iter().any(|diagnostic| {
            diagnostic.limitation == Limitation::TrackMatte
                && (diagnostic.composition_id, diagnostic.layer_id) == (Some(1), Some(layer))
                && diagnostic.message.contains(text)
        })
    };
    assert!(context(
        996,
        &format!("through its own track matte from layer {roto}")
    ));
    assert!(context(997, "track matte from layer 996 omitted"));
    assert!(context(
        996,
        &format!("track matte from layer {roto} omitted")
    ));
}

/// Consumer 992 in composition 200 uses precomposition 993 of composition 100
/// as its matte. Composition 100 holds precomposition 990 of the Roto Brush
/// composition 1, and a plain footage layer.
fn nested_cutout_matte(branch_drawn: bool, effect_enabled: bool, provider_eye: bool) -> bool {
    let mut project = av_project();
    let source = composition_mut(&mut project, 1);
    switches(&mut source.layers[0], true, true);
    let template = source.layers[0].clone();
    with_roto_brush(&mut source.layers[0], effect_enabled);
    let mut branch = renumbered(&template, 990, Some(1));
    switches(&mut branch, branch_drawn, true);
    add_composition(
        &mut project,
        100,
        vec![branch, renumbered(&template, 991, None)],
    );
    let mut consumer = renumbered(&template, 992, None);
    select_matte(&mut consumer, 993);
    let mut provider = renumbered(&template, 993, Some(100));
    switches(&mut provider, provider_eye, true);
    add_composition(&mut project, 200, vec![consumer, provider]);
    let converted =
        to_structural_fx_document_with_assets(&project, Some(200), &mut |_| true).unwrap();
    occurrence(root(&converted), 200, 992)
        .unwrap()
        .track_matte
        .is_some()
}

#[test]
fn only_drawn_nested_cutouts_make_a_matte_sample_unavailable() {
    assert!(
        nested_cutout_matte(false, true, true),
        "an undrawn nested branch adds no alpha to the sample"
    );
    assert!(
        nested_cutout_matte(false, false, true),
        "disabled cutout control"
    );
    assert!(
        !nested_cutout_matte(true, true, true),
        "a drawn nested cutout makes the sample unavailable"
    );
    assert!(
        !nested_cutout_matte(true, true, false),
        "the provider's own eye switch does not stop matte sampling"
    );
}
