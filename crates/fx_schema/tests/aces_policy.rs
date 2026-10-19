use fx_schema::{
    validate_render_domains, ColorManagement, ColorManagementError, Compositing, OutputTransform,
};
use serde_json::{json, Value};

fn aces() -> Value {
    json!({"workingSpace":"acescct","outputTransform":"aces2.0","compositing":"footageOnly"})
}

#[test]
fn validated_policy_round_trip_and_legacy_bytes() {
    assert_eq!(
        serde_json::to_string(&ColorManagement::default()).unwrap(),
        r#"{"workingSpace":"legacyDisplay"}"#
    );
    for compositing in [
        Compositing::FootageOnly,
        Compositing::WholeProgram,
        Compositing::PerClip,
    ] {
        let policy = ColorManagement::new_aces(OutputTransform::Aces2_0, compositing);
        assert_eq!(policy.aces().unwrap().compositing, compositing);
        assert!(!policy.is_legacy());
        let bytes = serde_json::to_vec(&policy).unwrap();
        assert_eq!(
            serde_json::from_slice::<ColorManagement>(&bytes).unwrap(),
            policy
        );
        policy.render_preflight().unwrap();
    }
    assert!(ColorManagement::default().render_preflight().is_ok());
}

#[test]
fn required_fields_wrong_types_and_future_values_are_distinct() {
    for field in ["workingSpace", "outputTransform", "compositing"] {
        let mut missing = aces();
        missing.as_object_mut().unwrap().remove(field);
        assert!(matches!(
            ColorManagement::from_value(missing),
            Err(ColorManagementError::Invalid(_))
        ));
        for wrong in [Value::Null, json!(1), json!(true), json!([]), json!({})] {
            let mut value = aces();
            value[field] = wrong;
            assert!(matches!(
                ColorManagement::from_value(value),
                Err(ColorManagementError::Invalid(_))
            ));
        }
        let mut future = aces();
        future[field] = json!("future");
        assert!(matches!(
            ColorManagement::from_value(future),
            Err(ColorManagementError::Unsupported { .. })
        ));
    }
    let mut future = aces();
    future["future"] = json!(null);
    assert!(matches!(
        ColorManagement::from_value(future),
        Err(ColorManagementError::Unsupported { .. })
    ));
    for field in ["outputTransform", "compositing"] {
        let mut legacy = json!({"workingSpace":"legacyDisplay"});
        legacy[field] = json!("future");
        assert!(matches!(
            ColorManagement::from_value(legacy),
            Err(ColorManagementError::Unsupported { .. })
        ));
    }
    for version in [
        Value::Null,
        json!(55),
        json!({}),
        json!([]),
        json!("future"),
    ] {
        let mut value = aces();
        value["version"] = version;
        assert_eq!(
            serde_json::to_value(ColorManagement::from_value(value).unwrap()).unwrap(),
            aces()
        );
    }
}

#[test]
fn overrides_are_preserved_in_legacy_and_outer_scene_wins_with_warning() {
    let scene = json!({"layers":[{"type":"Group","renderDomain":"scene","layers":[{"type":"Image","renderDomain":"display"}]}]});
    for policy in [
        ColorManagement::default(),
        ColorManagement::from_value(aces()).unwrap(),
    ] {
        let diagnostics = validate_render_domains(&policy, &scene).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0]
            .to_string()
            .contains("renderDomain=display has no effect inside a scene group"));
        assert_eq!(scene["layers"][0]["layers"][0]["renderDomain"], "display");
        for valid in [
            json!({"layers":[]}),
            json!({"layers":[{"type":"Group","renderDomain":"display","layers":[{"type":"Image","renderDomain":"scene"}]}]}),
        ] {
            assert!(validate_render_domains(&policy, &valid).unwrap().is_empty());
        }
        let invalid = json!({"layers":[{"type":"Image","renderDomain":"future"}]});
        assert!(matches!(
            validate_render_domains(&policy, &invalid),
            Err(ColorManagementError::Unsupported { .. })
        ));
    }
}
