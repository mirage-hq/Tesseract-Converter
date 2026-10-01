//! Fresh-import assertions for the independently Adobe-authored grouped native corpus.
//!
//! Expected values below come from the reviewed authoring JSX/readback reports and
//! fixture provenance, never from snapshots produced by this importer. Historical
//! script/receipt identities are pinned in tests/fixtures/aep_authoring_provenance.json;
//! unsafe machine-specific scripts are not shipped. The tests establish editable
//! structure only; published 30 fps references are not render-fidelity evidence.

use super::*;

mod compositing;
mod layers_parenting;
mod masks_path;
mod media_essential;
mod properties;

struct NativeSource {
    label: &'static str,
    bytes: &'static [u8],
    byte_len: usize,
    sha256: &'static str,
}

macro_rules! source {
    ($name:ident, $path:literal, $data:expr, $len:literal, $sha:literal) => {
        const $name: NativeSource = NativeSource {
            label: $path,
            bytes: $data,
            byte_len: $len,
            sha256: $sha,
        };
    };
}

source!(
    MOTION_BLUR,
    "compositing/import_motion_blur_cases.aep",
    include_bytes!("../../../tests/fixtures/compositing/import_motion_blur_cases.aep"),
    319_063,
    "41f83161f18bb20db35c547c73c15035e4c27c843173406d924ba4a265400c32"
);
source!(
    BLEND_MODES,
    "compositing/import_remaining_blend_modes.aep",
    include_bytes!("../../../tests/fixtures/compositing/import_remaining_blend_modes.aep"),
    2_010_019,
    "537c9e21cff85ea10f39dcd9a2c7b3038987495b5b1dfac8ffa60267d5b732d6"
);
source!(
    TRACK_MATTES,
    "compositing/import_track_matte_cases.aep",
    include_bytes!("../../../tests/fixtures/compositing/import_track_matte_cases.aep"),
    368_321,
    "7f597f51312021b3948472b56b137b9eeb7c7056086a2e95d150508607cb366a"
);
source!(
    ESSENTIAL_COLOR,
    "essential/import_color_nested_overrides.aep",
    include_bytes!("../../../tests/fixtures/essential/import_color_nested_overrides.aep"),
    292_409,
    "b3a989e5cd394594c6780220a78e6454ec2397146ecd2db334e5da0dc3e85688"
);
source!(
    ESSENTIAL_NESTED,
    "essential/import_nested_override_precedence.aep",
    include_bytes!("../../../tests/fixtures/essential/import_nested_override_precedence.aep"),
    214_983,
    "abc4dbd7c1d1efd72ee8d898e4edd40c553a980e582e13b6344acfd03cfa7fbd"
);
source!(
    ESSENTIAL_TRANSFORM,
    "essential/import_occurrence_overrides.aep",
    include_bytes!("../../../tests/fixtures/essential/import_occurrence_overrides.aep"),
    469_095,
    "b61aa16dce1f7a1cbbacdd6301ba569b625782e562cf0300ea0603e449a78ddb"
);
source!(
    LAYER_SWITCHES,
    "layers/import_layer_switches.aep",
    include_bytes!("../../../tests/fixtures/layers/import_layer_switches.aep"),
    372_121,
    "f1ea16736ad5779bc27e781adfded6509cab5d09ea41ea827e56637582a093e2"
);
source!(
    PRECOMP_STRUCTURE,
    "layers/import_precomp_structure.aep",
    include_bytes!("../../../tests/fixtures/layers/import_precomp_structure.aep"),
    301_561,
    "d3ec4d09406d666621b232d84d1bb72387af56f415e072f5754598a271e212bc"
);
source!(
    TIMING_CONTROLS,
    "layers/import_timing_controls.aep",
    include_bytes!("../../../tests/fixtures/layers/import_timing_controls.aep"),
    396_537,
    "4760111efc7a5b428e7a534bd66b368c73026614fbefb72660f40edd50048cee"
);
source!(
    MASK_CONTROLS,
    "masks/import_mask_controls.aep",
    include_bytes!("../../../tests/fixtures/masks/import_mask_controls.aep"),
    1_178_491,
    "01380c1f8c5ebe486cd068e5dee50447870b86e4864fc842590a99386dd9b417"
);
source!(
    MEDIA_REPLACEMENT,
    "media-replacement/import_media_replacement_cases.aep",
    include_bytes!("../../../tests/fixtures/media-replacement/import_media_replacement_cases.aep"),
    214_785,
    "271e7ac78ec451eb237ee88844c0a9008661d9c8fcd693a88a491156a5aed457"
);
source!(
    AUDIO_MEDIA,
    "media/import_audio_media_controls.aep",
    include_bytes!("../../../tests/fixtures/media/import_audio_media_controls.aep"),
    776_439,
    "36920d6bdc07dbb6292cc305327ace44efbacacb182dc18da3c979439f5473a8"
);
source!(
    FRAME_BLEND_MASTER_OFF,
    "media/import_frame_blend_master_off.aep",
    include_bytes!("../../../tests/fixtures/media/import_frame_blend_master_off.aep"),
    86_243,
    "878206858032ff8c4e5ee0457e1ce7454bb51508d333a6d6b9f1212f9dd2dde5"
);
source!(
    IMAGE_SOURCES,
    "media/import_image_source_controls.aep",
    include_bytes!("../../../tests/fixtures/media/import_image_source_controls.aep"),
    233_425,
    "450f242e0287689989a5b99f2dc67830b43bb2a787dd627f75943b14397e8c04"
);
source!(
    PARENTING,
    "parenting/import_parenting_cases.aep",
    include_bytes!("../../../tests/fixtures/parenting/import_parenting_cases.aep"),
    472_385,
    "3626cf26d4f8aa2e405ec0999b43cc4bf9e2e5f7cb0c2723b35648e0b7771f55"
);
source!(
    PATH_KEYS,
    "path-animation/import_path_key_cases.aep",
    include_bytes!("../../../tests/fixtures/path-animation/import_path_key_cases.aep"),
    239_485,
    "363b3f616d2375a92e0f52e8e026dee28a2b8ce3b7c4b63ae8c2dfade6b50f5d"
);
source!(
    NUMERIC_ANIMATION,
    "properties/import_numeric_animation_cases.aep",
    include_bytes!("../../../tests/fixtures/properties/import_numeric_animation_cases.aep"),
    1_302_753,
    "c8cc89a35b34ff9139f1141f0943b6cf0a1e065989f078b10de21f7cb53c4a98"
);
source!(
    TEMPORAL_CLOCKS,
    "properties/import_temporal_clock_cases.aep",
    include_bytes!("../../../tests/fixtures/properties/import_temporal_clock_cases.aep"),
    332_969,
    "d80216eacadb2ba319ea6434184ef6253f3337317e03d67720137edf92ac9cf9"
);
source!(
    TRANSFORM_COMPONENTS,
    "properties/import_transform_components.aep",
    include_bytes!("../../../tests/fixtures/properties/import_transform_components.aep"),
    1_229_663,
    "871c8fba3d0a33fdca8c49f018a97836a6dc6d98a3a6f18a524702ac7b6246e8"
);

fn pinned_project(source: &NativeSource) -> StructuralProject {
    assert_eq!(
        source.bytes.len(),
        source.byte_len,
        "{} byte pin",
        source.label
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(source.bytes)),
        source.sha256,
        "{} SHA-256 pin",
        source.label
    );
    read_project(source.bytes).unwrap_or_else(|error| panic!("{}: {error}", source.label))
}

fn assert_composition(project: &StructuralProject, id: u32, name: &str) {
    let actual = composition(project, id);
    assert_eq!(
        project.item(id).unwrap().name,
        name,
        "composition item {id}"
    );
    assert_eq!(actual.frame_rate, 24.0, "{name}");
}

fn fresh_import(project: &StructuralProject, id: u32, name: &str) -> StructuralConversion {
    assert_composition(project, id, name);
    let source = composition(project, id);
    let converted = to_structural_fx_document(project, Some(id)).unwrap();
    assert_imported_canvas_matches_source(source, &converted, name);
    assert_eq!(root(&converted).name, name);
    assert_editable(&converted);
    converted
}

fn fresh_import_with_assets(
    project: &StructuralProject,
    id: u32,
    name: &str,
) -> StructuralConversion {
    assert_composition(project, id, name);
    let source = composition(project, id);
    let converted =
        to_structural_fx_document_with_assets(project, Some(id), &mut |_| true).unwrap();
    assert_imported_canvas_matches_source(source, &converted, name);
    assert_eq!(root(&converted).name, name);
    assert_editable(&converted);
    converted
}

fn assert_editable(converted: &StructuralConversion) {
    let bytes = converted.document.to_json_vec().unwrap();
    EditableFxCompositionDocument::from_json_slice(&bytes).unwrap();
}

fn all_groups(group: &GroupLayer) -> Vec<&GroupLayer> {
    fn visit<'a>(group: &'a GroupLayer, output: &mut Vec<&'a GroupLayer>) {
        output.push(group);
        for child in &group.layers {
            if let FxLayer::Group(child) = child.data() {
                visit(child, output);
            }
        }
    }

    let mut output = Vec::new();
    visit(group, &mut output);
    output
}

fn named_group<'a>(group: &'a GroupLayer, name: &str) -> &'a GroupLayer {
    all_groups(group)
        .into_iter()
        .find(|candidate| candidate.name == name)
        .unwrap_or_else(|| panic!("missing editable Group {name:?}"))
}

fn editable_rect(group: &GroupLayer) -> &fx_schema::RectLayer {
    fn find(group: &GroupLayer) -> Option<&fx_schema::RectLayer> {
        group.layers.iter().find_map(|layer| match layer.data() {
            FxLayer::Rect(rect) => Some(rect),
            FxLayer::Group(child) => find(child),
            _ => None,
        })
    }

    find(group).unwrap_or_else(|| panic!("{} has no editable Rect", group.name))
}

fn document_json(converted: &StructuralConversion) -> Value {
    serde_json::from_slice(&converted.document.to_json_vec().unwrap()).unwrap()
}

fn dynamic_entries(json: &Value) -> &[Value] {
    json["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
}

fn entry_for<'a>(entries: &'a [Value], property_type: &str) -> &'a Value {
    entries
        .iter()
        .find(|entry| entry["target"]["propertyType"] == property_type)
        .unwrap_or_else(|| panic!("missing editable dynamics target {property_type:?}"))
}

fn diagnostic_contains(converted: &StructuralConversion, text: &str) -> bool {
    converted
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains(text))
}
