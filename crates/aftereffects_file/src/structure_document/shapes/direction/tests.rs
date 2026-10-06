use super::*;

fn path(closed: bool) -> ShapePath {
    let mut commands = vec![
        serde_json::json!({"type":"moveTo","x":1,"y":2,"cornerRadius":3}),
        serde_json::json!({"type":"cubicTo","c1x":4,"c1y":5,"c2x":6,"c2y":7,"x":8,"y":9}),
        serde_json::json!({"type":"lineTo","x":12,"y":14}),
    ];
    if closed {
        commands.push(serde_json::json!({"type":"close"}));
    }
    serde_json::from_value(serde_json::json!({"commands":commands})).unwrap()
}

#[test]
fn reversed_legacy_script_is_replaced_by_static_native_path_with_diagnostic() {
    let source = path(true);
    let id = LayerId::new(2);
    let mut layer = ShapeLayer {
        id,
        name: "direction".into(),
        description: String::new(),
        is_hidden: false,
        parent: Some(LayerId::new(1)),
        blend_mode: Default::default(),
        track_matte: None,
        masks: Vec::new(),
        active_range: full_active_range(),
        effects: Vec::new(),
        motion_blur: false,
        transform: identity_transform(),
        shape: content(source.clone(), Decorations::default()),
    };
    let code = format!(
        "var p={};p.commands[0].x+=input.time.seconds;return p;",
        serde_json::to_string(&source).unwrap()
    );
    let mut entries = vec![bindings::entry(
        PropertyTarget::layer(id, PropType::ShapePath),
        crate::structure_document::animation::js_script(code),
        Vec::new(),
    )];
    let mut warnings = Vec::new();
    apply(&mut layer, &mut entries, &mut warnings).unwrap();
    assert!(
        warnings
            .iter()
            .any(|message| message.contains("without motion"))
    );
    assert_eq!(layer.shape.path, reverse(&source).unwrap());
    assert!(entries[0].dependencies.is_empty());
    assert!(matches!(entries[0].animator.data(),
        fx_schema::animator::AnimatorData::Constant {
            value: fx_schema::PropertyValue::Path(path)
        } if path == &layer.shape.path));
    let mut root =
        crate::structure_document::group(LayerId::new(1), "root".into(), None, full_active_range());
    root.layers = crate::structure_document::stored_layers(vec![FxLayer::Shape(layer)]).unwrap();
    fx_schema::FXComposition::try_from_parts(
        fx_schema::CompositionId::new("direction"),
        "direction",
        fx_schema::AnimationGraph::from_entries(entries).unwrap(),
        crate::structure_document::stored_layers(vec![FxLayer::Group(root)]).unwrap(),
    )
    .unwrap();
}

#[test]
fn reversed_paths_preserve_closed_start_swap_open_endpoints_and_cubic_handles() {
    for closed in [false, true] {
        let source = path(closed);
        let result = reverse(&source).unwrap();
        assert_eq!(
            result.commands[0].endpoint(),
            Some(if closed { (1.0, 2.0) } else { (12.0, 14.0) })
        );
        let cubic = result
            .commands
            .iter()
            .find(|command| matches!(command, ShapePathCommand::CubicTo { .. }))
            .unwrap();
        assert!(matches!(
            cubic,
            ShapePathCommand::CubicTo {
                c1x: 6.0,
                c1y: 7.0,
                c2x: 4.0,
                c2y: 5.0,
                x: 1.0,
                y: 2.0,
                ..
            }
        ));
        assert_eq!(cubic.corner_radius(), Some(3.0));
        assert_eq!(
            matches!(result.commands.last(), Some(ShapePathCommand::Close)),
            closed
        );
    }
}
