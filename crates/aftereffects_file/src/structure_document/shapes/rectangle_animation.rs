//! Native Rect handles eligible parameter animation. Compound Rectangle
//! outlines cannot carry typed Path tracks in the current FX schema.

use super::{numeric_leaf, property_group};
use crate::rifx::Chunk;
use fx_schema::LayerId;
use fx_schema::animator::AnimationGraphEntry;

pub(super) fn entries(run: &[Chunk], _target: LayerId) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    let mut warnings = Vec::new();
    let leaves = match property_group(run, "Rectangle Path") {
        Ok(leaves) => leaves,
        Err(message) => return (Vec::new(), vec![message]),
    };
    for name in [
        "ADBE Vector Rect Size",
        "ADBE Vector Rect Position",
        "ADBE Vector Rect Roundness",
    ] {
        if numeric_leaf(&leaves, name, &mut warnings).is_some_and(|numeric| {
            numeric.animated || !numeric.keyframes.is_empty() || numeric.expression_enabled
        }) {
            warnings.push(format!(
                "{name} animation in compound Rectangle cannot be mapped to editable FX Path keys; initial outline retained without motion"
            ));
        }
    }
    (Vec::new(), warnings)
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::structure::{ItemKind, read_project};
    use crate::structure_document::animation_budget::AnimationBudget;

    fn name(value: &str) -> Chunk {
        let mut bytes = value.as_bytes().to_vec();
        bytes.resize(40, 0);
        Chunk::data(*b"tdmn", bytes).unwrap()
    }

    fn find_position(chunks: &[Chunk]) -> Option<Vec<Chunk>> {
        if let Ok(entries) = runs(chunks) {
            for (name, run) in entries {
                if name == "ADBE Position"
                    && numeric_from_run(run).is_some_and(|value| !value.keyframes.is_empty())
                {
                    return Some(run.to_vec());
                }
            }
        }
        chunks
            .iter()
            .filter_map(Chunk::children)
            .find_map(find_position)
    }

    #[test]
    fn compound_rectangle_size_keys_warn_without_generating_js() {
        // Supplemental native-record relabeling, NOT a native Rectangle oracle.
        let project = read_project(include_bytes!(
            "../../../tests/fixtures/properties/property_2D_position.aep"
        ))
        .unwrap();
        let position = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(comp) => comp
                    .layers
                    .iter()
                    .find_map(|layer| find_position(&layer.content)),
                _ => None,
            })
            .unwrap();
        let mut size = vec![name("ADBE Vector Rect Size")];
        size.extend(position.into_iter().filter(|chunk| chunk.id() != *b"tdmn"));
        let run = vec![Chunk::list(*b"tdgp", size)];
        assert!(
            path::is_dynamic(&run),
            "keyed rectangle must not be fused as static"
        );
        let mut id = 77;
        let mut animation_budget = AnimationBudget::default();
        let mut collector = Collector {
            includes_occurrence_pipeline: true,
            next_id: &mut id,
            animation_budget: &mut animation_budget,
            animations: Vec::new(),
            warnings: Vec::new(),
            frame_fade_lowered: false,
            evaluated_shapes: Default::default(),
            mapped_expressions: Vec::new(),
        };
        let shape = collector
            .source_layer(
                "ADBE Vector Shape - Rect",
                &run,
                "Rectangle",
                LayerId::new(1),
                Some(&Decorations::default()),
                None,
            )
            .unwrap();
        assert!(
            collector.animations.is_empty(),
            "no ShapePath script is permitted"
        );
        assert!(collector.warnings.iter().any(|message| {
            message.contains("ADBE Vector Rect Size animation")
                && message.contains("initial outline")
        }));
        let mut root = crate::structure_document::group(
            LayerId::new(1),
            "root".into(),
            None,
            full_active_range(),
        );
        root.layers = crate::structure_document::stored_layers(vec![shape.unwrap()]).unwrap();
        fx_schema::FXComposition::try_from_parts(
            fx_schema::CompositionId::new("rectangle"),
            "Rectangle",
            fx_schema::AnimationGraph::from_entries(collector.animations).unwrap(),
            crate::structure_document::stored_layers(vec![FxLayer::Group(root)]).unwrap(),
        )
        .unwrap();
    }
}
