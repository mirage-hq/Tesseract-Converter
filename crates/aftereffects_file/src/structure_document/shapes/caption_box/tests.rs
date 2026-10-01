use super::*;
use crate::{
    structure::{ItemKind, read_project},
    structure_document::{
        animation_budget::AnimationBudget,
        shapes::{OutputBudget, import_with_composition},
    },
};
use fx_schema::{Layer as StoredLayer, LayerData, PropertyValue};
use sha2::{Digest, Sha256};

#[test]
fn complete_caption_formulas_keep_binding_names_and_operators() {
    assert_eq!(
        canonical(SIZE),
        canonical(&format!(
            "/* caption */ {} // end",
            SIZE.replace('\n', "\r\n\t")
        ))
    );
    for changed in [
        SIZE.replace("index - 1", "index + 1"),
        SIZE.replace("textLayer", "text Layer"),
        SIZE.replace("0.6", "0 .6"),
        SIZE.replace("textWidth", "text/* split */Width"),
        SIZE.replace("time, false", "time, true"),
        SIZE.replace("0.6", "0.8"),
        SIZE.replace("textWidth + padding", "textWidth - padding"),
        SIZE.replace("Width Padding", "WidthPadding"),
        format!("{SIZE}; value"),
    ] {
        assert_ne!(canonical(SIZE), canonical(&changed));
    }
    assert_ne!(
        canonical(ANCHOR),
        canonical(&ANCHOR.replace("Rectangle 1", "Other Rectangle"))
    );
    assert_ne!(canonical(ANCHOR), canonical(&ANCHOR.replace("w/-2", "w/2")));
    assert!(canonical("/* unterminated").is_none());
    assert!(canonical(&" ".repeat(8_193)).is_none());
}

#[test]
fn caption_reveal_has_two_source_local_eased_keys_and_zero_width_endpoint() {
    let curve = reveal(11.5999, vec![0.0, 104.0], vec![835.1543, 104.0]);
    assert_eq!(curve.keyframes.len(), 2);
    assert_eq!(curve.keyframes[0].time_secs, 11.5999);
    assert_eq!(curve.keyframes[1].time_secs, 12.1999);
    assert_eq!(curve.keyframes[0].values, [0.0, 104.0]);
    for key in &curve.keyframes {
        assert_eq!(key.in_speed, [0.0, 0.0]);
        assert_eq!(key.out_speed, [0.0, 0.0]);
        assert_eq!(key.in_interpolation, 2);
        assert_eq!(key.out_interpolation, 2);
        assert_eq!(key.in_influence, [100.0 / 3.0; 2]);
        assert_eq!(key.out_influence, [100.0 / 3.0; 2]);
    }
}

fn rects(layers: &[StoredLayer], output: &mut Vec<fx_schema::RectLayer>) {
    for layer in layers {
        match layer.data() {
            LayerData::Rect(rect) => output.push(rect.clone()),
            LayerData::Group(group) => rects(&group.layers, output),
            _ => {}
        }
    }
}

#[test]
#[ignore = "requires local licensed AEP_CAPTION_SOURCE, which cannot be redistributed"]
fn local_external_caption_native_layout_maps_separate_paints_and_reveal() {
    let bytes =
        std::fs::read(std::env::var_os("AEP_CAPTION_SOURCE").expect("local native source path"))
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "0026b99dcc616687f05eac377f09f59a7fa4ef0ac73933a27342ef7a8b1205c7"
    );
    let project = read_project(&bytes).unwrap();
    let source_items = project.items.iter().map(|item| (item.id, item)).collect();
    let parent = crate::structure_document::group(
        LayerId::new(1),
        "proof".into(),
        None,
        full_active_range(),
    );
    for composition_id in [474, 536, 559, 576, 592, 608, 624, 640, 656, 672] {
        let ItemKind::Composition(composition) = &project.item(composition_id).unwrap().kind else {
            panic!("native caption composition")
        };
        let layer = &composition.layers[1];
        let imported = import_with_composition(
            layer,
            composition,
            &source_items,
            true,
            &parent,
            8,
            &mut 10_000,
            &mut OutputBudget::default(),
            &mut AnimationBudget::default(),
        )
        .unwrap();
        let mut rectangles = Vec::new();
        for layer in &imported.layers {
            if let LayerData::Group(group) = layer {
                rects(&group.layers, &mut rectangles);
            }
        }
        assert_eq!(
            rectangles.len(),
            2,
            "comp {composition_id}: {:?}",
            imported.warnings
        );
        let fill = rectangles
            .iter()
            .find(|rect| rect.rect.fill_enabled)
            .unwrap();
        let stroke = rectangles
            .iter()
            .find(|rect| rect.rect.stroke_enabled)
            .unwrap();
        assert_eq!(fill.transform.opacity.value(), 50.0);
        assert_eq!(fill.rect.fill_color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(fill.blend_mode, fx_schema::BlendMode::Multiply);
        assert_eq!(stroke.transform.opacity.value(), 100.0);
        assert_eq!(fill.parent, stroke.parent);
        let paint_id = fill.parent.unwrap();
        let fades: Vec<_> = imported
            .animations
            .iter()
            .filter(|entry| entry.target == PropertyTarget::layer(paint_id, PropType::Opacity))
            .collect();
        assert_eq!(
            fades.len(),
            1,
            "comp {composition_id}: missing common paint fade: {:?}",
            imported.warnings
        );
        let keys = fades[0].animator.keyframe_track().unwrap().keyframes();
        assert_eq!(keys.len(), 2);
        let start = layer.record.in_point().unwrap();
        assert_eq!(
            keys[0].layer_time().as_millis(),
            (start * 1000.0).round() as i64
        );
        assert_eq!(
            keys[1].layer_time().as_millis(),
            ((start + 6.0 / composition.frame_rate) * 1000.0).round() as i64
        );
        assert_eq!(keys[0].value(), &PropertyValue::Float(0.0));
        assert_eq!(keys[1].value(), &PropertyValue::Float(100.0));
        assert_eq!(keys[1].easing(), fx_schema::PropertyKeyframeEasing::Linear);
        assert_eq!(
            imported.animations.len(),
            6,
            "five geometry tracks plus one fade"
        );
        assert!(imported.frame_fade_lowered, "comp {composition_id}");

        assert_eq!(stroke.rect.stroke_width.value(), 1.0);
        assert_eq!(fill.rect.roundness, 54.0);
        assert_eq!(fill.rect.size, [0.0, 104.0]);
        let width = crate::structure_document::text::cached_caption_width(&composition.layers[0])
            .unwrap()
            + 72.0;
        for rect in &rectangles {
            let size = imported
                .animations
                .iter()
                .find(|entry| entry.target == PropertyTarget::layer(rect.id, PropType::RectSize))
                .unwrap();
            let keys = size.animator.keyframe_track().unwrap().keyframes();
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0].value(), &PropertyValue::Vector2([0.0, 104.0]));
            assert_eq!(keys[1].value(), &PropertyValue::Vector2([width, 104.0]));
            let start = layer.record.in_point().unwrap();
            assert_eq!(
                keys[0].layer_time().as_millis(),
                (start * 1000.0).round() as i64
            );
            assert_eq!(
                keys[1].layer_time().as_millis(),
                ((start + 0.6) * 1000.0).round() as i64
            );
            assert!(!size.animator.is_js_script());
            assert!(
                !imported
                    .animations
                    .iter()
                    .any(|entry| entry.target == PropertyTarget::layer(rect.id, PropType::ScaleX))
            );
        }
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("fixed cached source-layout glyph bounds"))
        );
        assert!(
            imported
                .animations
                .iter()
                .all(|entry| !entry.animator.is_js_script())
        );
    }
}

/// Edits in-memory clones of native records only; never writes the licensed AEP.
fn edit_chunks(chunks: &[Chunk], edit: &mut impl FnMut(&Chunk) -> Option<Chunk>) -> Vec<Chunk> {
    chunks
        .iter()
        .map(|chunk| {
            if let Some(replacement) = edit(chunk) {
                return replacement;
            }
            match (chunk.list_kind(), chunk.children()) {
                (Some(kind), Some(children)) => Chunk::list(kind, edit_chunks(children, edit)),
                _ => chunk.clone(),
            }
        })
        .collect()
}

#[test]
#[ignore = "requires local licensed AEP_CAPTION_SOURCE, which cannot be redistributed"]
fn local_external_caption_guards_preserve_best_effort_fallback() {
    let bytes =
        std::fs::read(std::env::var_os("AEP_CAPTION_SOURCE").expect("local native source path"))
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "0026b99dcc616687f05eac377f09f59a7fa4ef0ac73933a27342ef7a8b1205c7"
    );
    let project = read_project(&bytes).unwrap();
    let ItemKind::Composition(original) = &project.item(474).unwrap().kind else {
        panic!("native caption composition")
    };
    let source_items = project.items.iter().map(|item| (item.id, item)).collect();
    let parent = crate::structure_document::group(
        LayerId::new(1),
        "proof".into(),
        None,
        full_active_range(),
    );
    for variant in [
        "operator",
        "time",
        "preceding",
        "anchor",
        "roundness",
        "disabled",
        "dynamic",
        "duplicate",
        "cache",
        "animator",
        "text-expression",
        "text-key",
    ] {
        let mut composition = original.clone();
        if variant == "preceding" {
            composition.layers.remove(0);
        }
        let index = if variant == "preceding" { 0 } else { 1 };
        composition.layers[index].content =
            edit_chunks(&composition.layers[index].content, &mut |chunk| {
                if chunk.id() == *b"Utf8" {
                    let expression = std::str::from_utf8(chunk.data_payload()?).ok()?;
                    let replacement = match variant {
                        "operator" if expression.contains("finalWidth =") => {
                            expression.replace("textWidth + padding", "textWidth - padding")
                        }
                        "time" if expression.contains("revealDur =") => {
                            expression.replace("0.6", "0.8")
                        }
                        "anchor" if expression.contains("Rectangle Path 1") => {
                            expression.replace("Rectangle Path 1", "Other Path")
                        }
                        "roundness" if expression.contains("Roundness") => {
                            expression.replace("Roundness", "Other Roundness")
                        }
                        _ => return None,
                    };
                    return Some(Chunk::data(*b"Utf8", replacement.into_bytes()).unwrap());
                }
                if variant == "duplicate" && chunk.id() == *b"tdsn" {
                    let bytes = chunk.data_payload()?;
                    if bytes.get(8..22) == Some(b"Width Override") {
                        let mut replacement = b"Utf8".to_vec();
                        replacement.extend(13_u32.to_be_bytes());
                        replacement.extend(b"Width Padding");
                        return Some(Chunk::data(*b"tdsn", replacement).unwrap());
                    }
                }
                if chunk.list_kind() == Some(*b"tdbs") {
                    let body = chunk.children()?;
                    if variant == "disabled"
                        && body.iter().any(|chunk| {
                            chunk.id() == *b"Utf8"
                                && chunk
                                    .data_payload()
                                    .is_some_and(|bytes| bytes.starts_with(b"textLayer ="))
                        })
                    {
                        let body = edit_chunks(body, &mut |chunk| {
                            if chunk.id() != *b"tdb4" {
                                return None;
                            }
                            let mut bytes = chunk.data_payload()?.to_vec();
                            bytes[119] |= 1;
                            Some(Chunk::data(*b"tdb4", bytes).unwrap())
                        });
                        return Some(Chunk::list(*b"tdbs", body));
                    }
                    if variant == "dynamic"
                        && body.iter().any(|chunk| {
                            chunk.id() == *b"cdat"
                                && chunk.data_payload().and_then(|bytes| bytes.get(..8))
                                    == Some(72.0_f64.to_be_bytes().as_slice())
                        })
                    {
                        let body = edit_chunks(body, &mut |chunk| {
                            if chunk.id() != *b"tdb4" {
                                return None;
                            }
                            let mut bytes = chunk.data_payload()?.to_vec();
                            bytes[68] = 1;
                            Some(Chunk::data(*b"tdb4", bytes).unwrap())
                        });
                        return Some(Chunk::list(*b"tdbs", body));
                    }
                }
                None
            });
        if variant == "cache" {
            composition.layers[0].content =
                edit_chunks(&composition.layers[0].content, &mut |chunk| {
                    (chunk.list_kind() == Some(*b"btdk")).then(|| Chunk::list(*b"btdk", Vec::new()))
                });
        }
        if variant == "animator" {
            composition.layers[0].content =
                edit_chunks(&composition.layers[0].content, &mut |chunk| {
                    if chunk.id() != *b"cdat"
                        || chunk.data_payload().and_then(|bytes| bytes.get(..24))
                            != Some(
                                [0.0_f64, 20.0, 0.0]
                                    .into_iter()
                                    .flat_map(f64::to_be_bytes)
                                    .collect::<Vec<_>>()
                                    .as_slice(),
                            )
                    {
                        return None;
                    }
                    Some(
                        Chunk::data(
                            *b"cdat",
                            [10.0_f64, 20.0, 0.0]
                                .into_iter()
                                .flat_map(f64::to_be_bytes)
                                .collect::<Vec<_>>(),
                        )
                        .unwrap(),
                    )
                });
        }
        if matches!(variant, "text-expression" | "text-key") {
            composition.layers[0].content =
                edit_chunks(&composition.layers[0].content, &mut |chunk| {
                    if chunk.list_kind() != Some(*b"btds") {
                        return None;
                    }
                    let children = edit_chunks(chunk.children()?, &mut |chunk| {
                        if chunk.id() != *b"tdb4" {
                            return None;
                        }
                        let mut bytes = chunk.data_payload()?.to_vec();
                        if variant == "text-expression" {
                            bytes[120] |= 1;
                            bytes[119] &= !1;
                        } else {
                            bytes[68] = 1;
                        }
                        Some(Chunk::data(*b"tdb4", bytes).unwrap())
                    });
                    Some(Chunk::list(*b"btds", children))
                });
        }
        let layer = &composition.layers[index];
        let imported = import_with_composition(
            layer,
            &composition,
            &source_items,
            true,
            &parent,
            8,
            &mut 10_000,
            &mut OutputBudget::default(),
            &mut AnimationBudget::default(),
        )
        .unwrap();
        let mut rectangles = Vec::new();
        for layer in &imported.layers {
            if let LayerData::Group(group) = layer {
                rects(&group.layers, &mut rectangles);
            }
        }
        assert!(rectangles.is_empty(), "{variant} unexpectedly accepted");
        assert!(!imported.frame_fade_lowered, "{variant} claimed the fade");
        assert!(
            !imported.layers.is_empty(),
            "{variant} removed best-effort source geometry"
        );
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("initial outline")
                    || warning.contains("expression mapping rejected")),
            "{variant}: {:?}",
            imported.warnings
        );
        assert!(
            imported
                .animations
                .iter()
                .all(|entry| !entry.animator.is_js_script())
        );
    }
}

#[test]
#[ignore = "requires local licensed AEP_CAPTION_SOURCE, which cannot be redistributed"]
fn local_external_caption_mandatory_track_denial_keeps_visible_fallback() {
    let bytes =
        std::fs::read(std::env::var_os("AEP_CAPTION_SOURCE").expect("local native source path"))
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "0026b99dcc616687f05eac377f09f59a7fa4ef0ac73933a27342ef7a8b1205c7"
    );
    let project = read_project(&bytes).unwrap();
    let ItemKind::Composition(composition) = &project.item(474).unwrap().kind else {
        panic!("native caption composition")
    };
    let parent = crate::structure_document::group(
        LayerId::new(1),
        "proof".into(),
        None,
        full_active_range(),
    );
    let layer = &composition.layers[1];
    let roots = properties::root_runs(&layer.content).unwrap();
    let root = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Root Vectors Group")
        .unwrap()
        .1;
    let children = properties::unique_list(root, *b"tdgp").unwrap();
    let program = Program::parse(children, 8);
    for limit in [0, 500, 1000] {
        let mut budget = AnimationBudget::with_limit(limit);
        let mut next_id = 10_000;
        let mut collector = Collector {
            includes_occurrence_pipeline: true,
            next_id: &mut next_id,
            animation_budget: &mut budget,
            animations: Vec::new(),
            warnings: Vec::new(),
            frame_fade_lowered: false,
        };
        let result = lower(
            &mut collector,
            &program,
            "caption",
            parent.id,
            &mut OutputBudget::default(),
            layer,
            composition,
        )
        .unwrap();
        assert!(
            result.is_none(),
            "incomplete reveal admitted with {limit} bytes"
        );
        assert!(collector.animations.is_empty());
        assert!(!collector.frame_fade_lowered);
        assert_eq!(collector.animation_budget.used(), 0);
        assert_eq!(*collector.next_id, 10_000);
        assert!(
            !collector
                .warnings
                .iter()
                .any(|warning| warning.contains("eased reveal retained"))
        );
        // None delegates to the unchanged ordered-program fallback. That path
        // independently applies its existing accounting and best-effort policy.
    }
}

fn named_color_leaf_mut<'a>(chunks: &'a mut [Chunk], target: &str) -> Option<&'a mut Vec<Chunk>> {
    let index = chunks.iter().enumerate().find_map(|(index, chunk)| {
        (chunk.id() == *b"tdmn"
            && chunk
                .data_payload()
                .is_some_and(|bytes| bytes.starts_with(target.as_bytes())))
        .then(|| {
            let end = chunks[index + 1..]
                .iter()
                .position(|chunk| chunk.id() == *b"tdmn")
                .map_or(chunks.len(), |offset| index + 1 + offset);
            (index + 1..end).find(|index| chunks[*index].list_kind() == Some(*b"tdbs"))
        })
        .flatten()
    });
    if let Some(index) = index {
        return chunks[index].children_mut();
    }
    for chunk in chunks {
        if let Some(children) = chunk.children_mut()
            && let Some(leaf) = named_color_leaf_mut(children, target)
        {
            return Some(leaf);
        }
    }
    None
}

fn match_name(name: &str) -> Chunk {
    let mut bytes = vec![0; 40];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn named_group_mut<'a>(chunks: &'a mut [Chunk], target: &str) -> Option<&'a mut Vec<Chunk>> {
    let index = chunks.iter().enumerate().find_map(|(i, c)| {
        (c.id() == *b"tdmn"
            && c.data_payload()
                .is_some_and(|b| b.split(|v| *v == 0).next() == Some(target.as_bytes())))
        .then(|| {
            let end = chunks[i + 1..]
                .iter()
                .position(|c| c.id() == *b"tdmn")
                .map_or(chunks.len(), |j| i + 1 + j);
            (i + 1..end).find(|j| chunks[*j].list_kind() == Some(*b"tdgp"))
        })
        .flatten()
    });
    if let Some(index) = index {
        return chunks[index].children_mut();
    }
    for c in chunks {
        if let Some(children) = c.children_mut()
            && let Some(found) = named_group_mut(children, target)
        {
            return Some(found);
        }
    }
    None
}

#[test]
#[ignore = "requires local licensed AEP_CAPTION_SOURCE, which cannot be redistributed"]
fn local_external_caption_fade_guards_keep_geometry_and_native_source_unchanged() {
    let bytes =
        std::fs::read(std::env::var_os("AEP_CAPTION_SOURCE").expect("local native source path"))
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "0026b99dcc616687f05eac377f09f59a7fa4ef0ac73933a27342ef7a8b1205c7"
    );
    let project = read_project(&bytes).unwrap();
    let ItemKind::Composition(original) = &project.item(474).unwrap().kind else {
        panic!()
    };
    let parent = crate::structure_document::group(
        LayerId::new(1),
        "proof".into(),
        None,
        full_active_range(),
    );
    for variant in [
        "formula",
        "disabled-formula",
        "controller-name",
        "duplicate-controller",
        "background",
        "animated-background",
        "animated-frames",
        "out-frames",
        "blend",
        "style",
        "mask",
        "compositing-options",
        "private-payload",
        "missing-explicit-value",
    ] {
        let mut comp = original.clone();
        let layer = &mut comp.layers[1];
        match variant {
            "formula" => {
                layer.content = edit_chunks(&layer.content, &mut |c| {
                    if c.id() == *b"Utf8"
                        && c.data_payload()
                            .is_some_and(|b| b.starts_with(b"var fadeInDuration"))
                    {
                        Some(Chunk::data(*b"Utf8", b"value;".to_vec()).unwrap())
                    } else {
                        None
                    }
                });
            }
            "disabled-formula"
            | "background"
            | "animated-background"
            | "animated-frames"
            | "out-frames" => {
                let property = match variant {
                    "disabled-formula" => "ADBE Solid Composite-0001",
                    "animated-frames" => "ADBE CM FadeInOutFrames-0001",
                    "out-frames" => "ADBE CM FadeInOutFrames-0002",
                    _ => "ADBE Solid Composite-0003",
                };
                let leaf = named_color_leaf_mut(&mut layer.content, property).unwrap();
                let tag = if matches!(variant, "background" | "out-frames") {
                    *b"cdat"
                } else {
                    *b"tdb4"
                };
                let c = leaf.iter_mut().find(|c| c.id() == tag).unwrap();
                let mut b = c.data_payload().unwrap().to_vec();
                if tag == *b"cdat" {
                    b = 1_f64.to_be_bytes().to_vec();
                } else if variant == "disabled-formula" {
                    b[119] |= 1;
                } else {
                    b[68] = 1;
                }
                *c = Chunk::data(tag, b).unwrap();
            }
            "controller-name" => {
                layer.content = edit_chunks(&layer.content, &mut |c| {
                    if c.id() == *b"fnam"
                        && c.data_payload().is_some_and(|b| {
                            b.get(8..)
                                .is_some_and(|b| b.starts_with(b"Fade In+Out - frames"))
                        })
                    {
                        let mut b = b"Utf8".to_vec();
                        b.extend(5_u32.to_be_bytes());
                        b.extend(b"Other");
                        Some(Chunk::data(*b"fnam", b).unwrap())
                    } else {
                        None
                    }
                });
            }
            "duplicate-controller" => {
                let parade = named_group_mut(&mut layer.content, "ADBE Effect Parade").unwrap();
                let start = parade
                    .iter()
                    .position(|c| {
                        c.id() == *b"tdmn"
                            && c.data_payload()
                                .is_some_and(|b| b.starts_with(b"ADBE CM FadeInOutFrames\0"))
                    })
                    .unwrap();
                let end = parade[start + 1..]
                    .iter()
                    .position(|c| c.id() == *b"tdmn")
                    .map_or(parade.len(), |j| start + 1 + j);
                let copy = parade[start..end].to_vec();
                parade.extend(copy);
            }
            "blend" => {
                layer.content = edit_chunks(&layer.content, &mut |c| {
                    if c.id() == *b"pard"
                        && c.data_payload()
                            .is_some_and(|b| b[16..48].starts_with(b"Blending Mode\0"))
                    {
                        let mut b = c.data_payload().unwrap().to_vec();
                        b[56..60].copy_from_slice(&2_u32.to_be_bytes());
                        Some(Chunk::data(*b"pard", b).unwrap())
                    } else {
                        None
                    }
                });
            }
            "style" => {
                let style = named_group_mut(&mut layer.content, "dropShadow/enabled").unwrap();
                let flag = style.iter_mut().find(|c| c.id() == *b"tdsb").unwrap();
                *flag = Chunk::data(*b"tdsb", vec![0, 0, 0, 3]).unwrap();
            }
            "mask" => {
                let root = layer
                    .content
                    .iter_mut()
                    .find(|c| c.list_kind() == Some(*b"tdgp"))
                    .unwrap()
                    .children_mut()
                    .unwrap();
                root.push(match_name("ADBE Mask Parade"));
                root.push(Chunk::list(
                    *b"tdgp",
                    vec![
                        match_name("ADBE Mask Atom"),
                        Chunk::list(*b"tdgp", Vec::new()),
                    ],
                ));
            }
            "private-payload" => {
                named_color_leaf_mut(&mut layer.content, "ADBE Solid Composite-0001")
                    .unwrap()
                    .push(Chunk::data(*b"Priv", vec![1]).unwrap());
            }
            "missing-explicit-value" => {
                named_color_leaf_mut(&mut layer.content, "ADBE Solid Composite-0003")
                    .unwrap()
                    .retain(|c| c.id() != *b"cdat");
            }
            "compositing-options" => {
                let options =
                    named_group_mut(&mut layer.content, "ADBE Effect Built In Params").unwrap();
                options.push(match_name("ADBE Effect Opacity"));
                options.push(Chunk::list(*b"tdbs", Vec::new()));
            }
            _ => unreachable!(),
        }
        assert!(
            fade::resolve(&comp.layers[1], &comp).is_err(),
            "{variant} admitted"
        );
        let roots = properties::root_runs(&comp.layers[1].content).unwrap();
        let root = roots
            .iter()
            .find(|(name, _)| *name == "ADBE Root Vectors Group")
            .unwrap()
            .1;
        let program = Program::parse(properties::unique_list(root, *b"tdgp").unwrap(), 8);
        let mut next_id = 10000;
        let mut budget = AnimationBudget::default();
        let mut collector = Collector {
            includes_occurrence_pipeline: true,
            next_id: &mut next_id,
            animation_budget: &mut budget,
            animations: Vec::new(),
            warnings: Vec::new(),
            frame_fade_lowered: false,
        };
        let output = lower(
            &mut collector,
            &program,
            "caption",
            parent.id,
            &mut OutputBudget::default(),
            &comp.layers[1],
            &comp,
        )
        .unwrap()
        .unwrap();
        let mut paints = Vec::new();
        for l in &output {
            if let LayerData::Group(g) = l {
                rects(&g.layers, &mut paints);
            }
        }
        assert_eq!(paints.len(), 2, "{variant}: geometry lost");
        assert_eq!(
            collector.animations.len(),
            5,
            "{variant}: fade survived rejection"
        );
        assert!(!collector.frame_fade_lowered, "{variant} claimed the fade");
        assert!(
            collector
                .warnings
                .iter()
                .any(|w| w.contains("source-opacity fade omitted")),
            "{variant}"
        );
    }
}

#[test]
#[ignore = "requires local licensed AEP_CAPTION_SOURCE, which cannot be redistributed"]
fn local_external_caption_fade_last_track_denial_rolls_back_whole_caption_transaction() {
    let bytes =
        std::fs::read(std::env::var_os("AEP_CAPTION_SOURCE").expect("local native source path"))
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "0026b99dcc616687f05eac377f09f59a7fa4ef0ac73933a27342ef7a8b1205c7"
    );
    let project = read_project(&bytes).unwrap();
    let ItemKind::Composition(comp) = &project.item(474).unwrap().kind else {
        panic!()
    };
    let layer = &comp.layers[1];
    let roots = properties::root_runs(&layer.content).unwrap();
    let run = roots
        .iter()
        .find(|(n, _)| *n == "ADBE Root Vectors Group")
        .unwrap()
        .1;
    let program = Program::parse(properties::unique_list(run, *b"tdgp").unwrap(), 8);
    let mut next_id = 10000;
    let mut unlimited = AnimationBudget::default();
    let mut collector = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut next_id,
        animation_budget: &mut unlimited,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    assert!(
        lower(
            &mut collector,
            &program,
            "caption",
            LayerId::new(1),
            &mut OutputBudget::default(),
            layer,
            comp
        )
        .unwrap()
        .is_some()
    );
    assert_eq!(collector.animations.len(), 6);
    assert!(collector.frame_fade_lowered);
    let last = collector.animations.last().unwrap();
    let fade_bytes = serde_json::to_vec(last).unwrap().len() + 1;
    let allowance = unlimited.used() - fade_bytes;
    let mut budget = AnimationBudget::with_limit(allowance);
    let mut next_id = 10000;
    let mut collector = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut next_id,
        animation_budget: &mut budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    assert!(
        lower(
            &mut collector,
            &program,
            "caption",
            LayerId::new(1),
            &mut OutputBudget::default(),
            layer,
            comp
        )
        .unwrap()
        .is_none()
    );
    assert!(collector.animations.is_empty());
    assert!(!collector.frame_fade_lowered);
    assert_eq!(next_id, 10000);
    assert_eq!(budget.used(), 0);
}
