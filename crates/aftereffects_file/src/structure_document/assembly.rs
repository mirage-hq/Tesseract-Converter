use fx_schema::{AnimationGraph, CompositionId, FXComposition, Layer, MotionBlurSettings};

use crate::document::DocumentError;

/// Installs the small composition metadata before adding the potentially large
/// converted trees, so motion-blur replacement never clones those trees.
pub(super) fn composition(
    id: CompositionId,
    name: String,
    dynamics: AnimationGraph,
    layers: Vec<Layer>,
    motion_blur: Option<MotionBlurSettings>,
) -> Result<FXComposition, DocumentError> {
    let mut composition = FXComposition::empty(id, name);
    if let Some(settings) = motion_blur {
        composition.set_motion_blur(settings)?;
    }

    // Start from the schema-generated empty value so this converter does not
    // duplicate canonical fields or their defaults in a private declaration.
    let mut envelope = serde_json::to_value(&composition)?;
    envelope["dynamics"] = serde_json::to_value(&dynamics)?;
    envelope["layers"] = serde_json::to_value(&layers)?;

    match serde_json::from_value(envelope) {
        Ok(result) => Ok(result),
        Err(_) => {
            // Serde wraps structural errors in its own error type. Only on
            // rejection, use the old checked constructor to retain the typed
            // validation error (and its exact message) for callers.
            let mut result = FXComposition::try_from_parts(
                composition.composition_id().clone(),
                composition.name(),
                dynamics,
                layers,
            )?;
            if let Some(settings) = motion_blur {
                result.set_motion_blur(settings)?;
            }
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use fx_schema::{CompositionId, FXComposition, MotionBlurSettings};
    use serde_json::json;

    use super::composition;
    use crate::structure_document::to_structural_fx_document;

    fn reference(
        source: &FXComposition,
        motion_blur: Option<MotionBlurSettings>,
    ) -> Result<FXComposition, crate::document::DocumentError> {
        let mut composition = FXComposition::try_from_parts(
            CompositionId::new("main"),
            source.name(),
            source.dynamics().clone(),
            source.layers().to_vec(),
        )?;
        if let Some(settings) = motion_blur {
            composition.set_motion_blur(settings)?;
        }
        Ok(composition)
    }

    fn animated_shape_composition() -> FXComposition {
        let project = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/implemented_additions/native_parametric_key_channels.aep"
        ))
        .unwrap();
        // Pin the native target rather than relying on source encounter order.
        assert_eq!(project.item(1).unwrap().name.as_str(), "E11_ELLIPSE_SIZE");
        to_structural_fx_document(&project, Some(1))
            .expect("native animated-shape fixture converts")
            .document
            .composition()
            .clone()
    }

    #[test]
    fn value_assembly_matches_prior_assembly_for_default_and_authored_motion_blur() {
        let source = animated_shape_composition();
        assert!(!source.dynamics().entries().is_empty());

        let authored: MotionBlurSettings = serde_json::from_value(json!({
            "enabled": true,
            "shutterAngle": 271,
            "shutterPhase": -137,
            "samplesPerFrame": 23,
            "adaptiveSampleLimit": 191
        }))
        .unwrap();
        // None is the invalid-native-settings fallback: unlike a valid default,
        // it must not introduce an explicit motionBlur wire field.
        for settings in [None, Some(MotionBlurSettings::default()), Some(authored)] {
            let actual = composition(
                CompositionId::new("main"),
                source.name().to_owned(),
                source.dynamics().clone(),
                source.layers().to_vec(),
                settings,
            )
            .unwrap();
            let expected = reference(&source, settings).unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
    }

    #[test]
    fn value_assembly_preserves_nontrivial_float_precision() {
        let source = animated_shape_composition();
        let mut source_value = serde_json::to_value(source).unwrap();
        source_value["layers"][0]["transform"]["position"] =
            json!([189.23635530471802, 430.50996685028076]);
        let source: FXComposition = serde_json::from_value(source_value).unwrap();

        let actual = composition(
            CompositionId::new("main"),
            source.name().to_owned(),
            source.dynamics().clone(),
            source.layers().to_vec(),
            Some(MotionBlurSettings::default()),
        )
        .unwrap();
        let expected = reference(&source, Some(MotionBlurSettings::default())).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[test]
    fn value_assembly_preserves_structural_validation_error() {
        let source = animated_shape_composition();
        let mut entries = source.dynamics().entries().to_vec();
        let missing_layer = fx_schema::LayerId::new(u64::MAX);
        // This synthetic record is only a structural-validation probe; no
        // script is executed or added to the native import result. Keyframe
        // animators reject dependencies before composition assembly runs.
        entries[0].animator = serde_json::from_value(json!({
            "type": "jsScript", "code": "return [0, 0];"
        }))
        .unwrap();
        // Read-only dependencies need no animator, but SourceRange still needs
        // an actual media layer when the complete composition is validated.
        entries[0]
            .dependencies
            .push(fx_schema::Property::new(missing_layer, fx_schema::PropType::SourceRange).into());
        let dynamics = fx_schema::AnimationGraph::from_entries(entries).unwrap();

        let actual = composition(
            CompositionId::new("main"),
            source.name().to_owned(),
            dynamics.clone(),
            source.layers().to_vec(),
            Some(MotionBlurSettings::default()),
        )
        .unwrap_err();
        let expected = FXComposition::try_from_parts(
            CompositionId::new("main"),
            source.name(),
            dynamics,
            source.layers().to_vec(),
        )
        .unwrap_err();
        assert_eq!(actual.to_string(), expected.to_string());
        assert!(matches!(
            actual,
            crate::document::DocumentError::Composition(fx_schema::ValidationError::MissingLayer(id))
                if id == missing_layer
        ));
    }
}
