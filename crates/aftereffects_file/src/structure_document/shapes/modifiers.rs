//! Shared native modifier controls, distinct from each consuming paint.
use super::program::{GeometryId, Modifier, Program, ScopeId};
use super::*;

pub(super) struct Control<'a> {
    native: Modifier<'a>,
    layer: ShapeLayer,
}

pub(super) fn same(a: &Modifier<'_>, b: &Modifier<'_>) -> bool {
    a.owner == b.owner && std::ptr::eq(a.operation.chunks, b.operation.chunks)
}

pub(super) fn control_id(controls: &[Control<'_>], modifier: &Modifier<'_>) -> Option<LayerId> {
    controls
        .iter()
        .find(|control| same(&control.native, modifier))
        .map(|control| control.layer.id)
}

pub(super) fn round_radius(controls: &[Control<'_>], id: LayerId) -> Option<NonNegativeProperty> {
    controls
        .iter()
        .find(|control| control.layer.id == id)?
        .layer
        .shape
        .round_corners
        .as_ref()
        .map(|round| round.radius)
}

fn values(style: &ShapeContent) -> Vec<(PropType, f64)> {
    let mut result = Vec::new();
    if let Some(value) = &style.round_corners {
        result.push((PropType::RoundCornersRadius, value.radius.value()));
    }
    if let Some(value) = &style.offset_paths {
        result.push((PropType::OffsetPathsAmount, value.amount));
    }
    if let Some(value) = &style.trim {
        result.extend([
            (PropType::TrimStart, value.start),
            (PropType::TrimEnd, value.end),
            (PropType::TrimOffset, value.offset),
        ]);
    }
    result
}

pub(super) fn controls<'a>(
    collector: &mut Collector<'_>,
    program: &Program<'a>,
    parent: LayerId,
    budget: &mut OutputBudget,
    control_context: Option<(&Layer, &Composition)>,
) -> Result<Vec<Control<'a>>, serde_json::Error> {
    let mut controls: Vec<Control<'a>> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    'modifiers: for modifier in program.geometry.iter().flat_map(|node| &node.modifiers) {
        // A native operation is shared by all affected geometry references.
        // Deduplicate unsupported operations too, so one omission has one report.
        let key = (
            modifier.owner.0,
            std::ptr::from_ref(modifier.operation.chunks),
        );
        if !seen.insert(key) {
            continue;
        }
        let entries = [(modifier.operation.name, modifier.operation.chunks)];
        let style = collector.decorations(&entries, control_context);
        if style.round_corners.is_none() && style.offset_paths.is_none() && style.trim.is_none() {
            collector.warnings.push(format!(
                "{} has no canonical modifier representation; source outline retained",
                modifier.operation.name
            ));
            continue;
        }
        let id_checkpoint = collector.id_checkpoint();
        let animation_checkpoint = collector.animation_budget.checkpoint();
        let Some(id) = collector.allocate() else {
            break;
        };
        let layer = ShapeLayer {
            id,
            name: format!("AE {} controls", modifier.operation.name),
            description: "Shared source-local modifier controls".into(),
            is_hidden: true,
            parent: Some(parent),
            blend_mode: Default::default(),
            track_matte: None,
            masks: Vec::new(),
            active_range: full_active_range(),
            effects: Vec::new(),
            motion_blur: false,
            transform: identity_transform(),
            shape: content(
                ShapePath {
                    commands: Vec::new(),
                },
                style.clone(),
            ),
        };
        let source_start = collector.animations.len();
        collector.add_scope_entries(
            &entries,
            &style,
            &[FxLayer::Shape(layer.clone())],
            control_context,
        );
        for (property, value) in values(&layer.shape) {
            match bindings::constant(
                &mut collector.animations,
                PropertyTarget::layer(id, property),
                &fx_schema::PropertyValue::Float(value),
                source_start,
                collector.animation_budget,
            ) {
                Ok(true) => {}
                Ok(false) => {
                    collector.animations.truncate(source_start);
                    collector.animation_budget.rollback(animation_checkpoint);
                    collector.restore_ids(id_checkpoint);
                    collector.warnings.push(format!(
                        "{} control omitted at the generated-animation allowance",
                        modifier.operation.name
                    ));
                    continue 'modifiers;
                }
                Err(error) => {
                    collector.animations.truncate(source_start);
                    collector.animation_budget.rollback(animation_checkpoint);
                    collector.restore_ids(id_checkpoint);
                    return Err(error);
                }
            }
        }
        if !budget.reserve(&(&layer, &collector.animations[source_start..])) {
            collector.animations.truncate(source_start);
            collector.animation_budget.rollback(animation_checkpoint);
            collector.restore_ids(id_checkpoint);
            collector.warnings.push(budget::EXHAUSTED.into());
            continue;
        }
        controls.push(Control {
            native: *modifier,
            layer,
        });
    }
    Ok(controls)
}

pub(super) fn layers(controls: Vec<Control<'_>>) -> impl Iterator<Item = FxLayer> {
    controls
        .into_iter()
        .map(|control| FxLayer::Shape(control.layer))
}

pub(super) fn apply(
    collector: &mut Collector<'_>,
    program: &Program<'_>,
    controls: &[Control<'_>],
    geometry: &[GeometryId],
    owner: ScopeId,
    layer: &mut FxLayer,
) {
    let Some(first) = geometry.first() else {
        return;
    };
    let modifiers = &program.geometry[first.0].modifiers;
    if !geometry.iter().all(|id| {
        let other = &program.geometry[id.0].modifiers;
        other.len() == modifiers.len() && other.iter().zip(modifiers).all(|(a, b)| same(a, b))
    }) {
        collector.warnings.push("partially covered compound paint cannot apply a modifier to unrelated geometry; modifiers omitted from this paint".into());
        return;
    }
    let mut previous = 0;
    for modifier in modifiers {
        let rank = match modifier.operation.name {
            "ADBE Vector Filter - RC" => 1,
            "ADBE Vector Filter - Offset" => 2,
            "ADBE Vector Filter - Trim" => 3,
            _ => 0,
        };
        if modifier.owner != owner || rank == 0 || rank <= previous {
            collector.warnings.push(format!("{} cannot preserve its scope/repeated operation/order in the fixed FX modifier pipeline; omitted from this paint", modifier.operation.name));
            continue;
        }
        previous = rank;
        let Some(control) = controls
            .iter()
            .find(|control| same(&control.native, modifier))
        else {
            continue;
        };
        if control
            .layer
            .shape
            .trim
            .is_some_and(|trim| trim.mode == ShapeTrimMode::Individually)
        {
            let affected: Vec<_> = program
                .geometry
                .iter()
                .enumerate()
                .filter(|(_, node)| node.modifiers.iter().any(|other| same(other, modifier)))
                .map(|(index, _)| GeometryId(index))
                .collect();
            if affected != geometry {
                collector.warnings.push("Individual Trim requires the complete native path set; prefix paint trim omitted rather than changing total length".into());
                continue;
            }
        }
        // Existing fields can come from a pre-Append stage. Adding an earlier
        // pipeline operation would reorder it even when the kinds differ.
        let conflicts_with_prior_stage = match &*layer {
            FxLayer::Shape(shape) => {
                shape.shape.trim.is_some()
                    || (rank <= 2 && shape.shape.offset_paths.is_some())
                    || (rank == 1 && shape.shape.round_corners.is_some())
            }
            FxLayer::BooleanOperation(shape) => shape.trim.is_some(),
            _ => false,
        };
        if conflicts_with_prior_stage {
            collector.warnings.push("modifier before and after Append requires separate resolved-outline stages; later modifier omitted".into());
            continue;
        }
        let target = match layer {
            FxLayer::Shape(shape) => {
                if let Some(value) = &control.layer.shape.round_corners {
                    shape.shape.round_corners = Some(value.clone());
                }
                if let Some(value) = control.layer.shape.offset_paths {
                    shape.shape.offset_paths = Some(value);
                }
                if let Some(value) = control.layer.shape.trim {
                    shape.shape.trim = Some(value);
                }
                shape.id
            }
            FxLayer::BooleanOperation(shape) if rank == 3 => {
                shape.trim = control.layer.shape.trim;
                shape.id
            }
            _ => {
                collector.warnings.push("post-Boolean Round Corners/Offset Paths has no canonical resolved-outline target; modifier omitted".into());
                continue;
            }
        };
        for (property, value) in values(&control.layer.shape) {
            match bindings::mirror(
                &mut collector.animations,
                PropertyTarget::layer(target, property),
                PropertyTarget::layer(control.layer.id, property),
                fx_schema::PropertyValue::Float(value),
                &mut collector.warnings,
                collector.animation_budget,
            ) {
                Ok(true) => {}
                Ok(false) => collector.warnings.push(format!(
                    "{} native modifier animation omitted at the generated-animation allowance; static value retained",
                    modifier.operation.name
                )),
                Err(error) => collector.warnings.push(format!(
                    "{} native modifier animation omitted: {error}",
                    modifier.operation.name
                )),
            }
        }
    }
}
