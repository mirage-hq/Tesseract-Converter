use super::*;

fn named(name: &str) -> Chunk {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn control(values: &[f64]) -> Chunk {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[2..4].copy_from_slice(&u16::try_from(values.len()).unwrap().to_be_bytes());
    Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap(),
            Chunk::data(*b"tdb4", meta).unwrap(),
            Chunk::data(
                *b"cdat",
                values
                    .iter()
                    .flat_map(|v| v.to_be_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        ],
    )
}

fn owner(scale: f64, center: Option<[f64; 2]>, blend: Option<f64>) -> Layer {
    let mut owner = catalog_owner(&[native_effect("ADBE Gaussian Blur 2", &[])]);
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("Shape composition")
    };
    owner.record = comp.layers[0].record.clone();
    let mut controls = vec![Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap()];
    controls.extend([named("CC Tiler-0001"), control(&[scale])]);
    if let Some(center) = center {
        controls.extend([named("CC Tiler-0002"), control(&center)]);
    }
    if let Some(blend) = blend {
        controls.extend([named("CC Tiler-0003"), control(&[blend])]);
    }
    controls.push(named("ADBE Group End"));
    let parade = named_group_mut(&mut owner.content, "ADBE Effect Parade").unwrap();
    parade.splice(
        1..1,
        [
            named("CC Tiler"),
            Chunk::list(*b"sspc", vec![Chunk::list(*b"tdgp", controls)]),
        ],
    );
    owner
}

fn imported(owner: &Layer) -> ImportedEffects {
    import(
        &Default::default(),
        RIPPLE_COMPOSITION,
        owner,
        PLANE,
        PLANE,
        &mut 100,
        &mut Default::default(),
    )
}

#[test]
fn raster_precomposition_motion_tile_uses_its_source_canvas() {
    use fx_schema::LayerData;

    let mut project = read_project(include_bytes!(
        "../../../../tests/fixtures/canvas/transparency.aep"
    ))
    .unwrap();
    let ItemKind::Composition(source) = &project.item(1).unwrap().kind else {
        panic!("native source composition")
    };
    let readback: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/canvas/transparency.readback.json"
    ))
    .unwrap();
    assert_eq!([source.width, source.height], [160, 90]);
    assert_eq!(readback["compositions"][0]["width"], 160);
    assert_eq!(readback["compositions"][0]["height"], 90);
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 32)
        .unwrap()
        .kind
    else {
        panic!("native parent composition")
    };
    comp.width = 320;
    comp.height = 180;
    let layer = &mut comp.layers[0];
    named_group_mut(&mut layer.content, "ADBE Transform Group")
        .unwrap()
        .splice(1..1, [named("ADBE Position"), control(&[80.0, 45.0, 0.0])]);
    let mut tile = new_effect("ADBE Tile", true, [160.0, 90.0]).unwrap();
    tile.properties
        .iter_mut()
        .find(|property| property.match_name == "ADBE Tile-0005")
        .unwrap()
        .values = vec![50.0];
    let parade = effect_parade(
        &[
            tile,
            new_effect("ADBE Gaussian Blur 2", true, [160.0, 90.0]).unwrap(),
        ],
        layer.record.id(),
        [160.0, 90.0],
    )
    .unwrap();
    let properties = layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .unwrap();
    properties.extend([named("ADBE Effect Parade"), parade]);
    let converted = to_structural_fx_document(&project, Some(32)).unwrap();
    let LayerData::Group(root) = converted.document.composition().layers()[0].data() else {
        panic!("root composition")
    };
    let LayerData::Group(owner) = root.layers[0].data() else {
        panic!("native occurrence")
    };
    let LayerData::Group(output) = owner.layers[0].data() else {
        panic!("source plane")
    };
    assert_eq!(output.transform.scale, [50.0, 50.0]);
    let LayerData::Group(input) = output.layers[0].data() else {
        panic!("normalized input")
    };
    assert_eq!(input.transform.scale, [200.0, 200.0]);
    assert_eq!(input.transform.position.xy_array(), [0.0, 0.0]);
    assert_eq!(output.transform.position.xy_array(), [0.0, 0.0]);
    let tile = serde_json::to_value(&output.effects[0]).unwrap();
    assert_eq!(tile["effect"]["type"], "motionTile");
    assert_eq!(tile["effect"]["outputHeight"], 50.0);
    assert_eq!(tile["effect"]["tileCenterX"], 0.5);
    assert_eq!(tile["effect"]["tileCenterY"], 0.5);
    assert_eq!(
        owner.effects.len(),
        1,
        "following blur stays after inverse mapping"
    );
    assert_eq!(owner.transform.position.xy_array(), [80.0, 45.0]);
    assert_eq!(owner.transform.anchor_point, [80.0, 45.0]);
    let LayerData::Group(clock) = input.layers[0].data() else {
        panic!("native source clock")
    };
    assert_eq!(clock.transform.scale, [100.0, 100.0]);
    assert!(clock.effects.is_empty());
    assert_eq!(clock.playback.input_range().duration.as_millis(), 1000);
}

#[test]
fn static_cc_tiler_retains_scale_center_and_effect_order() {
    for (center, expected) in [
        (None, [0.5, 0.5]),
        (
            Some([120., 240.]),
            [120. / f64::from(PLANE[0]), 240. / f64::from(PLANE[1])],
        ),
    ] {
        let imported = imported(&owner(0.35, center, None));
        assert_eq!(imported.effects.len(), 2, "{:?}", imported.warnings);
        assert_eq!(imported.native_ordinals, [1, 2]);
        let value = serde_json::to_value(&imported.effects[0]).unwrap();
        let effect = &value["effect"];
        assert_eq!(effect["type"], "motionTile");
        assert_eq!(effect["tileWidth"], 35.);
        assert_eq!(effect["tileHeight"], 35.);
        assert_eq!(effect["tileCenterX"], expected[0]);
        assert_eq!(effect["tileCenterY"], expected[1]);
        assert_eq!(effect["outputWidth"], 100.);
        assert_eq!(effect["outputHeight"], 100.);
        assert_eq!(effect["mirrorEdges"], false);
        assert!(imported.animations.is_empty());
        assert!(
            imported
                .warnings
                .iter()
                .any(|w| w.contains("CC Tiler") && w.contains("approximat"))
        );
    }
}

#[test]
fn cc_tiler_import_stages_scaled_shape_geometry() {
    use fx_schema::LayerData;

    let mut project = read_project(include_bytes!(
        "../../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!("Shape composition")
    };
    let shape = &mut comp.layers[0];
    *named_group_mut(&mut shape.content, "ADBE Effect Parade").unwrap() =
        named_group_mut(&mut owner(0.35, None, None).content, "ADBE Effect Parade")
            .unwrap()
            .clone();
    named_group_mut(&mut shape.content, "ADBE Transform Group")
        .unwrap()
        .splice(1..1, [named("ADBE Scale"), control(&[2., 2., 1.])]);
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    fn find<'a>(layers: &'a [fx_schema::Layer], name: &str) -> Option<&'a fx_schema::GroupLayer> {
        layers.iter().find_map(|layer| {
            let LayerData::Group(group) = layer.data() else {
                return None;
            };
            if group.name == name {
                Some(group)
            } else {
                find(&group.layers, name)
            }
        })
    }
    let layers = converted.document.composition().layers();
    let output = find(layers, "Tiler source plane").expect("import staged Tiler");
    let input = find(&output.layers, "Tiler normalized input").unwrap();
    assert_eq!(output.transform.scale, [50., 50.]);
    assert_eq!(input.transform.scale, [200., 200.]);
    assert_eq!(output.effects.len(), 1);
}

#[test]
fn cc_tiler_normalizes_centered_source_to_root_effect_plane() {
    use crate::structure_document::{group, tiler_plane};
    use fx_schema::{LayerData, LayerId};
    let range = fx_schema::TimeRangeProperty::new(
        fx_schema::Time::ZERO,
        fx_schema::Duration::from_millis(1000),
    );
    let mut owner = group(LayerId::new(10), "owner".into(), None, range);
    owner.transform.position = fx_schema::Position::xy(960., 540.);
    owner.transform.scale = [200., 200.];
    let content = group(LayerId::new(11), "source".into(), Some(owner.id), range);
    owner
        .layers
        .push(fx_schema::Layer::from_data(&LayerData::Group(content)).unwrap());
    let effects = imported(&self::owner(0.35, None, None));
    let effect = effects.tiler.unwrap();
    owner.effects = effects.effects;
    tiler_plane::apply(
        &mut owner,
        effect,
        None,
        [1920, 1080],
        [3840, 2160],
        0,
        &mut 1000,
    )
    .unwrap();
    let LayerData::Group(output) = owner.layers[0].data() else {
        panic!("output plane")
    };
    let LayerData::Group(input) = output.layers[0].data() else {
        panic!("input plane")
    };
    assert_eq!(output.transform.scale, [25., 25.]);
    assert_eq!(input.transform.scale, [400., 400.]);
    assert_eq!(
        output.transform.position,
        fx_schema::Position::xy(-480., -270.)
    );
    assert_eq!(
        input.transform.position,
        fx_schema::Position::xy(1920., 1080.)
    );
    assert_eq!(output.effects.len(), 1);
    assert_eq!(owner.effects.len(), 1, "suffix remains outside once");
    let LayerData::Group(content) = input.layers[0].data() else {
        panic!("source retained")
    };
    assert_eq!(content.id, LayerId::new(11));
    assert_eq!(content.parent, Some(input.id));
}

#[test]
fn cc_tiler_plane_preserves_anchor_geometry_and_declines_nonplanar_owners_atomically() {
    use crate::structure_document::{group, tiler_plane};
    use fx_schema::{LayerData, LayerId};
    let range = fx_schema::TimeRangeProperty::new(
        fx_schema::Time::ZERO,
        fx_schema::Duration::from_millis(1000),
    );
    let mut owner = group(LayerId::new(10), "owner".into(), None, range);
    owner.transform.position = fx_schema::Position::xy(250., 150.);
    owner.transform.anchor_point = [20., 30.];
    owner.transform.scale = [200., 50.];
    let content = group(LayerId::new(11), "source".into(), Some(owner.id), range);
    owner
        .layers
        .push(fx_schema::Layer::from_data(&LayerData::Group(content)).unwrap());
    let effects = imported(&self::owner(0.35, None, None));
    let effect = effects.tiler.unwrap();
    owner.effects = effects.effects;
    let original = owner.clone();
    let mut next = 1000;
    tiler_plane::apply(
        &mut owner,
        effect,
        None,
        [1920, 1080],
        [3840, 2160],
        0,
        &mut next,
    )
    .unwrap();
    let LayerData::Group(output) = owner.layers[0].data() else {
        panic!("output")
    };
    let LayerData::Group(input) = output.layers[0].data() else {
        panic!("input")
    };
    assert_eq!(input.transform.position.xy_array(), [420., 270.]);
    assert_eq!(input.transform.scale, [400., 100.]);
    assert_eq!(output.transform.position.xy_array(), [-105., -270.]);
    assert_eq!(output.transform.scale, [25., 100.]);
    // Owner × output maps the root-sized effect plane to the native composition.
    for axis in 0..2 {
        let t = &original.transform;
        let s = t.scale[axis] / 100.;
        let origin = t.position.xy_array()[axis]
            + s * (output.transform.position.xy_array()[axis] - t.anchor_point[axis]);
        assert_eq!(origin, 0.);
        assert_eq!(s * output.transform.scale[axis] / 100., 0.5);
    }
    for mode in 0..4 {
        let mut owner = original.clone();
        match mode {
            0 => owner.transform.rotation = 20.,
            1 => owner.transform.skew = 10.,
            2 => owner.transform.scale[0] = 0.,
            3 => owner.transform.position = fx_schema::Position::xyz(0., 0., 10.),
            _ => unreachable!(),
        }
        let before = serde_json::to_value(&owner).unwrap();
        let mut next = 1000;
        assert!(
            tiler_plane::apply(
                &mut owner,
                effect,
                None,
                [1920, 1080],
                [3840, 2160],
                0,
                &mut next
            )
            .is_err()
        );
        assert_eq!(next, 1000);
        assert_eq!(serde_json::to_value(owner).unwrap(), before);
    }
}

#[test]
fn cc_tiler_declines_dynamic_malformed_and_incompatible_controls() {
    let owner = owner(0.35, None, None);
    let (sources, _) = native::read_effects(&owner.content, PLANE.map(f64::from));
    let source = &sources[0];
    for mode in 0..5 {
        let mut changed = source.clone();
        let scale = &mut changed.parameters[0];
        match mode {
            0 => scale.numeric.as_mut().unwrap().animated = true,
            1 => scale.numeric.as_mut().unwrap().expression_present = true,
            2 => scale.numeric.as_mut().unwrap().values = vec![f64::NAN],
            3 => scale.declared_kind = Ok(Some(7)),
            4 => changed.declarations = native::Declarations::Unreadable,
            _ => unreachable!(),
        }
        assert!(crate::effects::tiler::lower(&changed, PLANE.map(f64::from)).is_err());
    }
}

#[test]
fn unsupported_cc_tiler_retains_convertible_siblings() {
    for owner in [
        owner(0., None, None),
        owner(0.009, None, None),
        owner(0.35, None, Some(0.5)),
    ] {
        let imported = imported(&owner);
        assert_eq!(imported.effects.len(), 1);
        assert_eq!(imported.native_ordinals, [2]);
        assert!(
            imported
                .warnings
                .iter()
                .any(|w| w.contains("CC Tiler") && w.contains("omitted"))
        );
    }
    let mut duplicate = owner(0.35, None, None);
    let parade = named_group_mut(&mut duplicate.content, "ADBE Effect Parade").unwrap();
    let controls = parade[2].children_mut().unwrap()[0].children_mut().unwrap();
    controls.splice(1..1, [named("CC Tiler-0001"), control(&[0.2])]);
    let imported = imported(&duplicate);
    assert_eq!(imported.effects.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings.iter().any(|w| w.contains("duplicate")));
}
