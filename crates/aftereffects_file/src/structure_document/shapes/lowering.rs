//! Lower native paint ownership, rather than broadcasting scope decorations.
use super::program::{Draw, GeometryId, GeometryKind, Program, ScopeId};
use super::*;
use fx_schema::{PropertyValue, layer::RectLayer};

#[derive(Clone)]
enum Producer {
    Shape(ShapeLayer),
    Rect(RectLayer),
}

impl Producer {
    fn id(&self) -> LayerId {
        match self {
            Self::Shape(layer) => layer.id,
            Self::Rect(layer) => layer.id,
        }
    }

    fn shape(&self) -> Option<&ShapeLayer> {
        match self {
            Self::Shape(layer) => Some(layer),
            Self::Rect(_) => None,
        }
    }

    fn layer(&self) -> FxLayer {
        match self {
            Self::Shape(layer) => FxLayer::Shape(layer.clone()),
            Self::Rect(layer) => FxLayer::Rect(layer.clone()),
        }
    }
}

struct Lowering<'a, 'b, 'c, 'd> {
    collector: &'a mut Collector<'b>,
    program: &'a Program<'c>,
    name: &'a str,
    control_context: Option<(&'d Layer, &'d Composition)>,
    sources: Vec<Option<Producer>>,
    controls: Vec<Option<fx_schema::GroupLayer>>,
    modifiers: Vec<modifiers::Control<'c>>,
    remaining_depth: usize,
    budget: &'a mut OutputBudget,
}

#[allow(
    clippy::too_many_arguments,
    reason = "Preserve separate native source, destination, and budget contexts"
)]
pub(super) fn lower(
    collector: &mut Collector<'_>,
    program: &Program<'_>,
    name: &str,
    parent: LayerId,
    remaining_depth: usize,
    budget: &mut OutputBudget,
    control_context: Option<(&Layer, &Composition)>,
    source_items: Option<&HashMap<u32, &ProjectItem>>,
) -> Result<Vec<FxLayer>, serde_json::Error> {
    if let Some(rect) = super::native_rect::lower_with_context(
        collector,
        program,
        name,
        parent,
        budget,
        control_context,
    )? {
        return Ok(rect);
    }
    if let Some(shape) = super::native_shape::lower_with_context(
        collector,
        program,
        name,
        parent,
        budget,
        control_context,
        source_items,
    )? {
        return Ok(shape);
    }
    let modifiers = modifiers::controls(collector, program, parent, budget, control_context)?;
    let mut lowering = Lowering {
        collector,
        program,
        name,
        control_context,
        sources: vec![None; program.geometry.len()],
        controls: vec![None; program.scopes.len()],
        modifiers,
        remaining_depth,
        budget,
    };
    let mut helpers = Vec::new();
    'controls: for (index, scope) in program.scopes.iter().enumerate().skip(1) {
        let id_checkpoint = lowering.collector.id_checkpoint();
        let animation_checkpoint = lowering.collector.animation_budget.checkpoint();
        let Some(id) = lowering.collector.allocate() else {
            break;
        };
        let mut control = super::super::group(
            id,
            format!("{name}: vector group controls {index}"),
            Some(parent),
            full_active_range(),
        );
        control.is_hidden = true;
        if let Some(operation) = scope.operation {
            control.blend_mode =
                blend::from_run(operation.chunks, &mut lowering.collector.warnings);
        }
        let source_start = lowering.collector.animations.len();
        if let Some(run) = scope.transform {
            control.transform = decode_transform(run, &mut lowering.collector.warnings);
            lowering.collector.add_transform_entries(run, id);
        }
        for (property, value) in transform_values(&control.transform) {
            match bindings::constant(
                &mut lowering.collector.animations,
                PropertyTarget::layer(id, property),
                &PropertyValue::Float(value),
                source_start,
                lowering.collector.animation_budget,
            ) {
                Ok(true) => {}
                Ok(false) => {
                    lowering.collector.animations.truncate(source_start);
                    lowering
                        .collector
                        .animation_budget
                        .rollback(animation_checkpoint);
                    lowering.collector.restore_ids(id_checkpoint);
                    lowering.collector.warnings.push(format!(
                        "vector group control {index} omitted at the generated-animation allowance"
                    ));
                    continue 'controls;
                }
                Err(error) => {
                    lowering.collector.animations.truncate(source_start);
                    lowering
                        .collector
                        .animation_budget
                        .rollback(animation_checkpoint);
                    lowering.collector.restore_ids(id_checkpoint);
                    return Err(error);
                }
            }
        }
        let helper = FxLayer::Group(control.clone());
        if !lowering.reserve(&helper, source_start, animation_checkpoint) {
            lowering.collector.restore_ids(id_checkpoint);
            continue;
        }
        lowering.controls[index] = Some(control);
        helpers.push(helper);
    }
    'sources: for (index, geometry) in program.geometry.iter().enumerate() {
        let GeometryKind::Source(source) = &geometry.kind else {
            continue;
        };
        let source_start = lowering.collector.animations.len();
        let animation_checkpoint = lowering.collector.animation_budget.checkpoint();
        let id_checkpoint = lowering.collector.id_checkpoint();
        let fallback_name = format!("{name}: geometry {index}");
        let boolean_owner = bounded_boolean_owner(program, GeometryId(index));
        let eligible_rect = boolean_owner == Some(geometry.owner) && geometry.modifiers.is_empty();
        if source.name == "ADBE Vector Shape - Rect" && boolean_owner.is_some() && !eligible_rect {
            lowering.collector.warnings.push(format!(
                "{fallback_name}: Rectangle Boolean operand crosses a vector scope or an intermediate modifier; typed Rect transport is unsupported, so the existing diagnosed static-outline fallback is retained"
            ));
        }
        let reversed_rect = eligible_rect
            && source.name == "ADBE Vector Shape - Rect"
            && super::direction::reversed(source.chunks, &mut lowering.collector.warnings);
        if reversed_rect {
            lowering.collector.warnings.push(format!(
                "{fallback_name}: reversed Rectangle winding cannot use the typed Rect Boolean producer; the reversed static editable Shape fallback is retained"
            ));
        }
        let producer = if eligible_rect && !reversed_rect {
            super::native_rect::producer(lowering.collector, source, &fallback_name, parent)
                .map(Producer::Rect)
        } else {
            None
        };
        let producer = if let Some(producer) = producer {
            producer
        } else {
            let source_layer = match lowering.collector.source_layer_with_sources(
                source.name,
                source.chunks,
                &fallback_name,
                parent,
                None,
                lowering.control_context,
                source_items,
            ) {
                Ok(source_layer) => source_layer,
                Err(error) => {
                    lowering.collector.animations.truncate(source_start);
                    lowering
                        .collector
                        .animation_budget
                        .rollback(animation_checkpoint);
                    lowering.collector.restore_ids(id_checkpoint);
                    return Err(error);
                }
            };
            let Some(FxLayer::Shape(mut shape)) = source_layer else {
                lowering.collector.animations.truncate(source_start);
                lowering
                    .collector
                    .animation_budget
                    .rollback(animation_checkpoint);
                lowering.collector.restore_ids(id_checkpoint);
                continue;
            };
            shape.is_hidden = true;
            match bindings::outline(
                &mut lowering.collector.animations,
                &shape,
                source_start,
                lowering.collector.animation_budget,
            ) {
                Ok(true) => {}
                Ok(false) => {
                    lowering.collector.animations.truncate(source_start);
                    lowering
                        .collector
                        .animation_budget
                        .rollback(animation_checkpoint);
                    lowering.collector.restore_ids(id_checkpoint);
                    lowering.collector.warnings.push(format!(
                        "{fallback_name}: native geometry controls omitted at the generated-animation allowance"
                    ));
                    continue 'sources;
                }
                Err(error) => {
                    lowering.collector.animations.truncate(source_start);
                    lowering
                        .collector
                        .animation_budget
                        .rollback(animation_checkpoint);
                    lowering.collector.restore_ids(id_checkpoint);
                    return Err(error);
                }
            }
            Producer::Shape(shape)
        };
        let helper = producer.layer();
        if !lowering.reserve(&helper, source_start, animation_checkpoint) {
            lowering.collector.restore_ids(id_checkpoint);
            continue;
        }
        lowering.sources[index] = Some(producer);
        helpers.push(helper);
    }
    let mut layers = lowering.scope(ScopeId(0), parent)?;
    layers.extend(helpers);
    layers.extend(modifiers::layers(lowering.modifiers));
    Ok(layers)
}

fn bounded_boolean_owner(program: &Program<'_>, source_id: GeometryId) -> Option<ScopeId> {
    program
        .geometry
        .iter()
        .find_map(|geometry| match &geometry.kind {
            GeometryKind::Merge {
                operation,
                operands,
            } if operands.contains(&source_id)
                && scalar_leaf(operation.chunks, "ADBE Vector Merge Type")
                    .is_ok_and(|mode| matches!(mode, 2.0 | 3.0 | 4.0 | 5.0)) =>
            {
                Some(geometry.owner)
            }
            _ => None,
        })
}

fn transform_values(transform: &Transform) -> [(PropType, f64); 10] {
    let position = match transform.position {
        Position::TwoD(value) => [value[0], value[1]],
        Position::ThreeD(value) => [value[0], value[1]],
    };
    [
        (PropType::AnchorPointX, transform.anchor_point[0]),
        (PropType::AnchorPointY, transform.anchor_point[1]),
        (PropType::PositionX, position[0]),
        (PropType::PositionY, position[1]),
        (PropType::ScaleX, transform.scale[0]),
        (PropType::ScaleY, transform.scale[1]),
        (PropType::Rotation, transform.rotation),
        (PropType::Skew, transform.skew),
        (PropType::SkewAxis, transform.skew_axis),
        (PropType::Opacity, transform.opacity.value()),
    ]
}

impl Lowering<'_, '_, '_, '_> {
    fn reserve<T: serde::Serialize>(
        &mut self,
        value: &T,
        animation_start: usize,
        animation_checkpoint: super::super::animation_budget::AnimationCheckpoint,
    ) -> bool {
        if self
            .budget
            .reserve(&(value, &self.collector.animations[animation_start..]))
        {
            return true;
        }
        self.collector.animations.truncate(animation_start);
        self.collector
            .animation_budget
            .rollback(animation_checkpoint);
        self.collector.warnings.push(budget::EXHAUSTED.into());
        false
    }

    fn rollback(
        &mut self,
        animation_start: usize,
        animation_checkpoint: super::super::animation_budget::AnimationCheckpoint,
        budget_checkpoint: usize,
        id_checkpoint: u64,
    ) {
        self.collector.animations.truncate(animation_start);
        self.collector
            .animation_budget
            .rollback(animation_checkpoint);
        self.budget.restore(budget_checkpoint);
        self.collector.restore_ids(id_checkpoint);
    }

    fn scope(
        &mut self,
        owner: ScopeId,
        parent: LayerId,
    ) -> Result<Vec<FxLayer>, serde_json::Error> {
        let scope = &self.program.scopes[owner.0];
        let draws = scope.paint_order(|id| {
            blend::composite_order(
                self.program.paints[id.0].operation.chunks,
                &mut self.collector.warnings,
            )
        });
        let mut result = Vec::new();
        'draws: for draw in draws {
            match draw {
                Draw::Group(child) => {
                    let Some(control) = self.controls[child.0].clone() else {
                        continue;
                    };
                    let animation_start = self.collector.animations.len();
                    let animation_checkpoint = self.collector.animation_budget.checkpoint();
                    let budget_checkpoint = self.budget.checkpoint();
                    let id_checkpoint = self.collector.id_checkpoint();
                    let Some(id) = self.collector.allocate() else {
                        break;
                    };
                    let mut group = super::super::group(
                        id,
                        format!("{}: vector group {}", self.name, child.0),
                        Some(parent),
                        full_active_range(),
                    );
                    group.is_hidden = !self.program.scopes[child.0].enabled;
                    group.transform = control.transform;
                    group.blend_mode = control.blend_mode;
                    for (property, value) in transform_values(&control.transform) {
                        match bindings::mirror(
                            &mut self.collector.animations,
                            PropertyTarget::layer(id, property),
                            PropertyTarget::layer(control.id, property),
                            PropertyValue::Float(value),
                            &mut self.collector.warnings,
                            self.collector.animation_budget,
                        ) {
                            Ok(true) => {}
                            Ok(false) => {
                                self.rollback(
                                    animation_start,
                                    animation_checkpoint,
                                    budget_checkpoint,
                                    id_checkpoint,
                                );
                                self.collector.warnings.push(
                                    "vector group omitted because required mirrored controls exceeded the generated-animation allowance"
                                        .into(),
                                );
                                continue 'draws;
                            }
                            Err(error) => {
                                self.rollback(
                                    animation_start,
                                    animation_checkpoint,
                                    budget_checkpoint,
                                    id_checkpoint,
                                );
                                return Err(error);
                            }
                        }
                    }
                    if self.remaining_depth == 0 {
                        self.rollback(
                            animation_start,
                            animation_checkpoint,
                            budget_checkpoint,
                            id_checkpoint,
                        );
                        self.collector
                            .warnings
                            .push("vector group omitted at remaining output depth budget".into());
                        continue;
                    }
                    self.remaining_depth -= 1;
                    let children = self.scope(child, id);
                    self.remaining_depth += 1;
                    let children = match children {
                        Ok(children) => children,
                        Err(error) => {
                            self.rollback(
                                animation_start,
                                animation_checkpoint,
                                budget_checkpoint,
                                id_checkpoint,
                            );
                            return Err(error);
                        }
                    };
                    group.layers = match super::super::stored_layers(children) {
                        Ok(layers) => layers,
                        Err(error) => {
                            self.rollback(
                                animation_start,
                                animation_checkpoint,
                                budget_checkpoint,
                                id_checkpoint,
                            );
                            return Err(error);
                        }
                    };
                    if self.reserve(&group, animation_start, animation_checkpoint) {
                        result.push(FxLayer::Group(group));
                    } else {
                        self.budget.restore(budget_checkpoint);
                        self.collector.restore_ids(id_checkpoint);
                    }
                }
                Draw::Paint(id) => {
                    let paint = &self.program.paints[id.0];
                    debug_assert_eq!(paint.owner, owner);
                    let entries = [(paint.operation.name, paint.operation.chunks)];
                    let mut style = self.collector.decorations(&entries, self.control_context);
                    if style.fills.is_empty() && style.strokes.is_empty() {
                        continue;
                    }
                    let opacity = if let Some(fill) = style.fills.first_mut() {
                        let opacity = fill.opacity;
                        fill.opacity = 1.0;
                        opacity
                    } else if let Some(stroke) = style.strokes.first_mut() {
                        let opacity = stroke.opacity;
                        stroke.opacity = 1.0;
                        opacity
                    } else {
                        continue;
                    };
                    let blend_mode =
                        blend::from_run(paint.operation.chunks, &mut self.collector.warnings);
                    let opacity = PercentageProperty::new((opacity * 100.0).clamp(0.0, 100.0))
                        .unwrap_or_else(|| identity_transform().opacity);
                    if let Some(layer) = self.geometry(
                        &paint.geometry,
                        owner,
                        parent,
                        &style,
                        Some((opacity, blend_mode, !paint.enabled)),
                    )? {
                        let animation_start = self.collector.animations.len();
                        let animation_checkpoint = self.collector.animation_budget.checkpoint();
                        self.collector.add_scope_entries(
                            &entries,
                            &style,
                            std::slice::from_ref(&layer),
                            self.control_context,
                        );
                        if !self.reserve(&(), animation_start, animation_checkpoint) {
                            self.collector.warnings.push("paint animation omitted at output budget; already budgeted static paint retained".into());
                        }
                        result.push(layer);
                    }
                }
            }
        }
        Ok(result)
    }

    fn flatten_appends(&mut self, geometry: &[GeometryId], all_merges: bool) -> Vec<GeometryId> {
        let mut pending: Vec<_> = geometry.iter().rev().copied().collect();
        let mut result = Vec::new();
        let mut visited = 0usize;
        while let Some(id) = pending.pop() {
            visited += 1;
            if visited > 100_000 {
                self.collector.warnings.push(
                    "compound geometry exceeded the traversal budget; remaining operands omitted"
                        .into(),
                );
                break;
            }
            let node = &self.program.geometry[id.0];
            if let GeometryKind::Merge {
                operation,
                operands,
            } = &node.kind
            {
                let mode = scalar_leaf(operation.chunks, "ADBE Vector Merge Type").unwrap_or(1.0);
                if all_merges || !matches!(mode, 2.0 | 3.0 | 4.0 | 5.0) {
                    if !all_merges && mode != 1.0 {
                        self.collector.warnings.push(format!("unknown Merge Paths mode {mode}; operands appended without Boolean operation"));
                    }
                    if !node.modifiers.is_empty() && geometry != [id] {
                        self.collector.warnings.push("intermediate Append modifiers require a resolved-outline stage; those modifiers omitted".into());
                    }
                    pending.extend(operands.iter().rev().copied());
                    continue;
                }
            }
            result.push(id);
        }
        result
    }

    fn needs_round_stages(&self, geometry: &[GeometryId], owner: ScopeId) -> bool {
        let Some(first) = geometry.first() else {
            return false;
        };
        let expected = &self.program.geometry[first.0].modifiers;
        let mut any = false;
        let mut complex = expected.len() != 1
            || expected
                .first()
                .is_some_and(|modifier| modifier.owner != owner);
        for id in geometry {
            let Some(source) = self.sources[id.0].as_ref().and_then(Producer::shape) else {
                return false;
            };
            let modifiers = &self.program.geometry[id.0].modifiers;
            if modifiers
                .iter()
                .any(|modifier| modifier.operation.name != "ADBE Vector Filter - RC")
            {
                return false;
            }
            any |= !modifiers.is_empty();
            complex |= modifiers.len() != expected.len()
                || !modifiers
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| modifiers::same(a, b));
            complex |= geometry.len() > 1
                && (source.shape.ellipse.is_some()
                    || source.shape.poly_star.is_some()
                    || source
                        .shape
                        .path
                        .commands
                        .iter()
                        .any(|command| matches!(command, ShapePathCommand::CubicTo { .. })));
        }
        any && complex
    }

    fn outline_input(
        &mut self,
        id: GeometryId,
        owner: ScopeId,
        stage_rounds: bool,
    ) -> Option<bindings::OutlineInput> {
        self.sources[id.0].as_ref()?;
        let mut scope = self.program.geometry[id.0].owner;
        let mut transforms = Vec::new();
        let mut rounds = Vec::new();
        loop {
            if stage_rounds {
                rounds.push(
                    self.program.geometry[id.0]
                        .modifiers
                        .iter()
                        .filter(|modifier| modifier.owner == scope)
                        .filter_map(|modifier| modifiers::control_id(&self.modifiers, modifier))
                        .collect(),
                );
            }
            if scope == owner {
                break;
            }
            let crossed_scope = &self.program.scopes[scope.0];
            if !super::native_rect::identity_group(
                Some(crossed_scope),
                &mut self.collector.warnings,
            ) {
                transforms.push(self.controls[scope.0].as_ref()?.id);
            }
            scope = crossed_scope.parent?;
        }
        Some(bindings::OutlineInput { transforms, rounds })
    }

    fn geometry(
        &mut self,
        geometry: &[GeometryId],
        owner: ScopeId,
        parent: LayerId,
        style: &Decorations,
        presentation: Option<(PercentageProperty, fx_schema::BlendMode, bool)>,
    ) -> Result<Option<FxLayer>, serde_json::Error> {
        let animation_start = self.collector.animations.len();
        let animation_checkpoint = self.collector.animation_budget.checkpoint();
        let budget_checkpoint = self.budget.checkpoint();
        let id_checkpoint = self.collector.id_checkpoint();
        let flattened = self.flatten_appends(geometry, false);
        let stage_rounds = flattened == geometry && self.needs_round_stages(&flattened, owner);
        let mut layer =
            match self.geometry_without_modifiers(&flattened, owner, parent, style, stage_rounds) {
                Ok(Some(layer)) => layer,
                Ok(None) => {
                    self.rollback(
                        animation_start,
                        animation_checkpoint,
                        budget_checkpoint,
                        id_checkpoint,
                    );
                    return Ok(None);
                }
                Err(error) => {
                    self.rollback(
                        animation_start,
                        animation_checkpoint,
                        budget_checkpoint,
                        id_checkpoint,
                    );
                    return Err(error);
                }
            };
        if stage_rounds {
            self.collector.warnings.push("Round-only intermediate stages cannot stay editable as native FX operator tracks; static per-point rounding is retained when representable and missing motion is diagnosed".into());
        } else {
            modifiers::apply(
                self.collector,
                self.program,
                &self.modifiers,
                &flattened,
                owner,
                &mut layer,
            );
        }
        if flattened != geometry {
            modifiers::apply(
                self.collector,
                self.program,
                &self.modifiers,
                geometry,
                owner,
                &mut layer,
            );
        }
        if let Some((opacity, blend_mode, is_hidden)) = presentation {
            match &mut layer {
                FxLayer::Shape(shape) => {
                    shape.transform.opacity = opacity;
                    shape.blend_mode = blend_mode;
                    shape.is_hidden = is_hidden;
                }
                FxLayer::BooleanOperation(shape) => {
                    shape.transform.opacity = opacity;
                    shape.blend_mode = blend_mode;
                    shape.is_hidden = is_hidden;
                }
                _ => unreachable!("geometry lowers to shape or boolean"),
            }
        }
        if self.reserve(&layer, animation_start, animation_checkpoint) {
            Ok(Some(layer))
        } else {
            self.budget.restore(budget_checkpoint);
            self.collector.restore_ids(id_checkpoint);
            Ok(None)
        }
    }

    fn geometry_without_modifiers(
        &mut self,
        geometry: &[GeometryId],
        owner: ScopeId,
        parent: LayerId,
        style: &Decorations,
        stage_rounds: bool,
    ) -> Result<Option<FxLayer>, serde_json::Error> {
        if geometry.is_empty() {
            return Ok(None);
        }
        if geometry.len() == 1 {
            let node = &self.program.geometry[geometry[0].0];
            if let GeometryKind::Merge {
                operation,
                operands,
            } = &node.kind
            {
                let mode = scalar_leaf(operation.chunks, "ADBE Vector Merge Type").unwrap_or(1.0);
                if mode == 1.0 {
                    return self.geometry(operands, owner, parent, style, None);
                }
                let op = match mode {
                    2.0 => BooleanOp::Union,
                    3.0 => BooleanOp::Subtract,
                    4.0 => BooleanOp::Intersect,
                    5.0 => BooleanOp::Exclude,
                    _ => {
                        self.collector.warnings.push(format!("unknown Merge Paths mode {mode}; operands retained without Boolean operation"));
                        return self.geometry(operands, owner, parent, style, None);
                    }
                };
                if self.remaining_depth == 0 {
                    self.collector.warnings.push("Merge Paths exceeded the remaining Boolean depth budget; operands appended without the Boolean operation".into());
                    let flattened = self.flatten_appends(operands, true);
                    return self.geometry(&flattened, owner, parent, style, None);
                }
                let Some(id) = self.collector.allocate() else {
                    return Ok(None);
                };
                let mut layers = Vec::with_capacity(operands.len());
                let mut missing_operand = operands.is_empty();
                self.remaining_depth -= 1;
                for &operand in operands {
                    match self.geometry(&[operand], owner, id, &Decorations::default(), None) {
                        Ok(Some(layer)) => layers.push(layer),
                        Ok(None) => {
                            missing_operand = true;
                            break;
                        }
                        Err(error) => {
                            self.remaining_depth += 1;
                            return Err(error);
                        }
                    }
                }
                self.remaining_depth += 1;
                if missing_operand || layers.len() != operands.len() {
                    self.collector.warnings.push(format!(
                        "{}: Merge Paths mode {mode} omitted because every native operand is required; independent paints and sibling groups were retained",
                        self.name
                    ));
                    return Ok(None);
                }
                // Native first operand is the subject; FX uses bottom-most.
                if op == BooleanOp::Subtract {
                    layers.reverse();
                }
                return Ok(Some(FxLayer::BooleanOperation(BooleanOperationLayer {
                    id,
                    name: self.name.into(),
                    description: "AE Boolean geometry with shared source bindings".into(),
                    is_hidden: false,
                    parent: Some(parent),
                    blend_mode: Default::default(),
                    track_matte: None,
                    masks: Vec::new(),
                    active_range: full_active_range(),
                    effects: Vec::new(),
                    motion_blur: false,
                    transform: identity_transform(),
                    op,
                    layers: super::super::stored_layers(layers)?,
                    fills: style.fills.clone(),
                    strokes: style.strokes.clone(),
                    trim: style.trim,
                })));
            }
        }
        if let [source_id] = geometry
            && let Some(input) = self.outline_input(*source_id, owner, stage_rounds)
            && input.rounds.iter().all(Vec::is_empty)
            && let Some(source) = self.sources[source_id.0].as_ref()
            && let Some(controls) = input
                .transforms
                .iter()
                .map(|id| {
                    self.controls
                        .iter()
                        .flatten()
                        .find(|control| control.id == *id)
                })
                .collect::<Option<Vec<_>>>()
            && let Some(transport) =
                bindings::shared_outline_transform(&controls, &self.collector.animations)
        {
            let Some(id) = self.collector.allocate() else {
                return Ok(None);
            };
            let source_id = source.id();
            let layer = match source {
                Producer::Shape(source) => {
                    let mut shape = source.clone();
                    shape.id = id;
                    shape.name = self.name.into();
                    shape.description = "AE paint on native editable geometry".into();
                    shape.parent = Some(parent);
                    shape.is_hidden = false;
                    shape.shape.fills = style.fills.clone();
                    shape.shape.strokes = style.strokes.clone();
                    shape.shape.round_corners = style.round_corners.clone();
                    shape.shape.offset_paths = style.offset_paths;
                    shape.shape.trim = style.trim;
                    if let Some(transform) = transport.transform {
                        shape.transform = transform;
                        shape.transform.opacity = identity_transform().opacity;
                        let detail = if transport.keyframes_only {
                            "nested static translation parents composed with retained child transform values"
                        } else {
                            "one vector-group geometry transform normalized onto its paint"
                        };
                        self.collector.warnings.push(format!(
                            "{}: {detail}; original group name/control and live shared-source edit identity are not retained",
                            self.name
                        ));
                    }
                    FxLayer::Shape(shape)
                }
                Producer::Rect(source) => {
                    if transport.transform.is_some()
                        || !style.fills.is_empty()
                        || !style.strokes.is_empty()
                        || style.round_corners.is_some()
                        || style.offset_paths.is_some()
                        || style.trim.is_some()
                    {
                        self.collector.warnings.push(format!(
                            "{}: native Rectangle producer cannot cross a vector-group or own paint/modifiers as a Boolean operand; static Shape fallback required",
                            self.name
                        ));
                        return Ok(None);
                    }
                    let mut rect = source.clone();
                    rect.id = id;
                    rect.name = self.name.into();
                    rect.description = "Native editable Rectangle Boolean operand".into();
                    rect.parent = Some(parent);
                    rect.is_hidden = false;
                    FxLayer::Rect(rect)
                }
            };
            // Copy native typed keys onto the visible paint rather than
            // emitting a JS dependency on the hidden producer. Each source is
            // measured and reserved before its animator/value clone.
            let source_entry_count = self.collector.animations.len();
            let (animations, animation_budget) = (
                &mut self.collector.animations,
                &mut self.collector.animation_budget,
            );
            let candidates = (0..source_entry_count).filter_map(|source_index| {
                let entry = &animations[source_index];
                let property = entry.target.as_property()?;
                let source_property = property.layer_id() == source_id;
                let transform_property = transport.producer.is_some_and(|producer| {
                    property.layer_id() == producer && property.property_type() != PropType::Opacity
                });
                if !(source_property || transform_property)
                    || !entry.dependencies.is_empty()
                    || entry.animator.is_js_script()
                    || (transport.keyframes_only
                        && !matches!(
                            entry.animator.data(),
                            fx_schema::animator::AnimatorData::Keyframes { .. }
                        ))
                {
                    return None;
                }
                Some((
                    &entry.animator,
                    PropertyTarget::layer(id, property.property_type()),
                ))
            });
            match bindings::copy_animators_atomically(candidates, animation_budget) {
                Ok(Some(copied)) => {
                    let copied_count = copied.len();
                    animations.extend(copied);
                    if transport.keyframes_only && copied_count > 0 {
                        self.collector.warnings.push(format!(
                            "{}: {copied_count} retained native keyframe tracks copied to the composed paint after the complete batch committed; copies are independent of hidden controls",
                            self.name
                        ));
                    }
                }
                Ok(None) => self.collector.warnings.push(format!(
                    "{}: native geometry animation copy exceeded the generated-animation allowance; the complete copied control batch was omitted and static native geometry retained",
                    self.name
                )),
                Err(error) => self.collector.warnings.push(format!(
                    "{}: native geometry animation copy failed: {error}; the complete copied control batch was omitted and static native geometry retained",
                    self.name
                )),
            }
            self.collector.warnings.push(format!(
                "{}: native source geometry copied into independently editable paint; shared geometry edits across paints cannot remain linked without FX reference animators",
                self.name
            ));
            return Ok(Some(layer));
        }
        let mut inputs = Vec::new();
        let mut initial = Vec::new();
        for &id in geometry {
            if let Some(mut input) = self.outline_input(id, owner, stage_rounds) {
                let Some(source) = self.sources[id.0].as_ref() else {
                    return Ok(None);
                };
                let Some(source) = source.shape() else {
                    self.collector.warnings.push(
                        "native Rectangle producer reached an Append/resolved-outline stage; typed Rect retained only for bounded same-scope Boolean operands and this contour was omitted"
                            .into(),
                    );
                    continue;
                };
                if self.collector.animations.iter().any(|entry| {
                    entry.target == PropertyTarget::layer(source.id, PropType::ShapePath)
                        && matches!(
                            entry.animator.data(),
                            fx_schema::animator::AnimatorData::Keyframes { .. }
                        )
                }) {
                    self.collector.warnings.push(format!(
                        "{}: animated compound Path fusion is unsupported; paint omitted instead of freezing authored motion",
                        source.name
                    ));
                    return Ok(None);
                }
                if source.shape.path.commands.is_empty() {
                    self.collector.warnings.push(format!(
                        "{}: parametric geometry in compound paint cannot be converted into a linked native FX outline without generated scripts; unsupported contour omitted",
                        source.name
                    ));
                    continue;
                }
                let mut commands = source.shape.path.commands.clone();
                let round_ids: Vec<_> = input.rounds.iter().flatten().copied().collect();
                if !round_ids.is_empty() {
                    if let [round_id] = round_ids.as_slice()
                        && let Some(radius) = modifiers::round_radius(&self.modifiers, *round_id)
                        && commands.iter().all(|command| {
                            matches!(
                                command,
                                ShapePathCommand::MoveTo {
                                    corner_radius: None,
                                    ..
                                } | ShapePathCommand::LineTo {
                                    corner_radius: None,
                                    ..
                                } | ShapePathCommand::Close
                            )
                        })
                    {
                        for command in &mut commands {
                            match command {
                                ShapePathCommand::MoveTo { corner_radius, .. }
                                | ShapePathCommand::LineTo { corner_radius, .. } => {
                                    *corner_radius = Some(radius);
                                }
                                _ => {}
                            }
                        }
                        self.collector.warnings.push(format!(
                            "{}: intermediate Round Corners radius {} copied into static per-point geometry; native radius animation and shared-edit identity are not preserved",
                            radius.value(),
                            self.name
                        ));
                    } else {
                        self.collector.warnings.push(format!(
                            "{}: intermediate Round Corners stage cannot be mapped without a resolved-outline operator; source contour retained unrounded",
                            self.name
                        ));
                    }
                    input.rounds.clear();
                }
                initial.extend(commands);
                inputs.push(input);
            } else {
                self.collector.warnings.push("compound paint cannot append a resolved Boolean outline through the current ShapePath dependency API; unsupported operand omitted".into());
            }
        }
        if inputs.is_empty() {
            return Ok(None);
        }
        let Some(id) = self.collector.allocate() else {
            return Ok(None);
        };
        let path = ShapePath { commands: initial };
        let animation = match bindings::append(id, &inputs, &path, self.collector.animation_budget)
        {
            Ok(Some(animation)) => animation,
            Ok(None) => {
                self.collector.warnings.push(
                    "static compound Path omitted at the generated-animation allowance".into(),
                );
                return Ok(None);
            }
            Err(error) => {
                self.collector.warnings.push(error);
                return Ok(None);
            }
        };
        self.collector.animations.push(animation);
        self.collector.warnings.push(format!(
            "{}: compound paint contours are static editable copies; shared source edits and native path motion cannot stay linked without FX reference/Path-key animators",
            self.name
        ));
        Ok(Some(FxLayer::Shape(ShapeLayer {
            id,
            name: self.name.into(),
            description: "One AE paint over shared editable geometry".into(),
            is_hidden: false,
            parent: Some(parent),
            blend_mode: Default::default(),
            track_matte: None,
            masks: Vec::new(),
            active_range: full_active_range(),
            effects: Vec::new(),
            motion_blur: false,
            transform: identity_transform(),
            shape: content(path, style.clone()),
        })))
    }
}
