use super::*;
use crate::properties;
use crate::structure::{ItemKind, read_project};
use crate::writer::{KeyframeEasing, NumericKeyframe};

const NATIVE_ANIMATED: &[u8] =
    include_bytes!("../../../tests/fixtures/effects_coverage/native_animated_controls.aep");

fn named_property<'a>(chunks: &'a [Chunk], name: &str) -> Option<&'a [Chunk]> {
    for pair in chunks.windows(2) {
        if pair[0].id() == *b"tdmn"
            && pair[0]
                .data_payload()
                .is_some_and(|bytes| bytes.split(|byte| *byte == 0).next() == Some(name.as_bytes()))
            && pair[1].list_kind() == Some(*b"tdbs")
        {
            return pair[1].children();
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| named_property(children, name))
}

fn assert_native_animation_record(composition_id: u32, effect_name: &str, property_name: &str) {
    // This independent Adobe-authored source is the native descriptor oracle;
    // its separately pinned Adobe render is the RGB oracle. The complete
    // descriptor AND storage flags are the contract: keys that our
    // reader (or Adobe's UI) can see can still render as a frozen initial value.
    let source = read_project(NATIVE_ANIMATED).expect("pinned native animated controls");
    let ItemKind::Composition(comp) = &source.item(composition_id).unwrap().kind else {
        panic!("pinned target must be a composition");
    };
    let native = named_property(&comp.layers[0].content, property_name).unwrap();
    assert!(properties::unique_list(native, *b"list").is_ok());

    // Shape effects use the composition's coordinate plane, not the painted
    // rectangle's 120x80 content bounds. Keep the native descriptor aspect ratio.
    let size = [320.0, 180.0];
    let mut effect = new_effect(effect_name, true, size).unwrap();
    let property = effect
        .properties
        .iter_mut()
        .find(|property| property.match_name == property_name)
        .unwrap();
    property.animation = Some(Track {
        keys: [0, 1000]
            .into_iter()
            .map(|time_millis| NumericKeyframe {
                time_millis,
                values: property.values.clone(),
                easing: vec![KeyframeEasing::Hold; property.values.len()],
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            })
            .collect(),
    });
    let generated = plugin(&effect, 13, size).unwrap();
    let fresh = named_property(generated.children().unwrap(), property_name).unwrap();
    assert_eq!(
        properties::data(fresh, *b"tdb4").unwrap(),
        properties::data(native, *b"tdb4").unwrap(),
        "{property_name}: native animation descriptor"
    );
    assert_eq!(
        properties::data(fresh, *b"tdsb").unwrap(),
        properties::data(native, *b"tdsb").unwrap(),
        "{property_name}: native animation storage flags"
    );
    assert!(properties::unique_list(fresh, *b"list").is_ok());
}

#[test]
fn native_animation_fixed_scalar_record() {
    assert_native_animation_record(443, "ADBE Radial Blur", "ADBE Radial Blur-0001");
}

#[test]
fn native_animation_point_record() {
    assert_native_animation_record(443, "ADBE Radial Blur", "ADBE Radial Blur-0002");
}

#[test]
fn native_animation_float_record() {
    assert_native_animation_record(92, "ADBE Exposure2", "ADBE Exposure2-0003");
}

#[test]
fn native_animation_discrete_record() {
    assert_native_animation_record(326, "ADBE Mosaic", "ADBE Mosaic-0001");
    assert_native_animation_record(1, "ADBE Bulge", "ADBE Bulge-0007");
    assert_native_animation_record(196, "ADBE Ramp", "ADBE Ramp-0005");
}

#[test]
fn native_animation_angle_record() {
    assert_native_animation_record(664, "ADBE Wave Warp", "ADBE Wave Warp-0004");
}

#[test]
fn native_animation_color_record() {
    assert_native_animation_record(196, "ADBE Ramp", "ADBE Ramp-0002");
}
