//! Best-effort import of AE vector contents into editable FX shape layers.

pub(super) mod defaults;
#[cfg(test)]
mod geometry_tests;
pub(crate) mod gradient;
pub(super) mod path;
mod rectangle_animation;

pub(super) mod bindings;
mod blend;
mod budget;
pub(super) use budget::OutputBudget;
mod direction;
mod dynamic_path;
pub(super) use dynamic_path::composition_origin_curve;
mod lowering;
mod modifiers;
mod native_rect;
mod native_shape;
mod program;
#[cfg(test)]
mod stroke_join_tests;

use std::collections::HashMap;

use fx_schema::animator::AnimationGraphEntry;
use fx_schema::layer::{
    BooleanOp, BooleanOperationLayer, ShapeEllipse, ShapeFillRule, ShapeFillStyle, ShapeLayer,
    ShapeLineCap, ShapeLineJoin, ShapeOffsetPaths, ShapePaint, ShapePolyStar, ShapePolyStarType,
    ShapeRoundCorners, ShapeStrokeStyle, ShapeTrimMode, ShapeTrimPaths,
};
use fx_schema::{
    Duration, LayerData as FxLayer, LayerId, NonNegativeProperty, PercentageProperty, Position,
    PropType, PropertyTarget, ShapeContent, ShapePath, ShapePathCommand, Time, TimeRangeProperty,
    Transform,
};

use crate::{
    expression_samples::{
        EvaluatedProperty, ExpressionSamples, PropertyIdentity, ShapePathSegment,
    },
    properties::{NumericProperty, PropertyError, read_numeric, root_runs, runs, unique_list},
    rifx::Chunk,
    structure::{Composition, Layer, ProjectItem},
};

use super::{
    animation::{NumericAnimationClock, NumericAnimationTarget, numeric_entries},
    animation_budget::AnimationBudget,
};

/// Resolve a native Shape leaf identity without confusing omitted named
/// defaults with indexed Contents occurrences. Indices follow the expression
/// model: the Adobe-captured Scale family, otherwise the native one-based
/// position. No pointer is dereferenced: the returned resident slice is an
/// occurrence-local lookup key for this import.
fn resolve_shape_run<'a>(layer: &'a Layer, path: &[ShapePathSegment]) -> Option<&'a [Chunk]> {
    let first = path.first()?;
    if first.index != 2 || first.match_name != "ADBE Root Vectors Group" || path.len() > 64 {
        return None;
    }
    let root = root_runs(&layer.content).ok()?;
    let matches = root
        .iter()
        .filter(|(name, _)| *name == first.match_name)
        .collect::<Vec<_>>();
    let [(_, first_run)] = matches.as_slice() else {
        return None;
    };
    let mut run = *first_run;
    let mut parent = first.match_name.as_str();
    for segment in &path[1..] {
        let group = unique_list(run, *b"tdgp").ok()?;
        let children = runs(group).ok()?;
        let matches = children
            .iter()
            .enumerate()
            .filter(|(ordinal, (name, _))| {
                *name == segment.match_name
                    && crate::properties::shape_scale_index(parent, name, *ordinal)
                        .or_else(|| u32::try_from(ordinal + 1).ok())
                        == Some(segment.index)
            })
            .collect::<Vec<_>>();
        let [(_, (_, child))] = matches.as_slice() else {
            return None;
        };
        run = child;
        parent = &segment.match_name;
    }
    Some(run)
}

#[derive(Clone, Default)]
struct Decorations {
    fills: Vec<ShapeFillStyle>,
    strokes: Vec<ShapeStrokeStyle>,
    round_corners: Option<ShapeRoundCorners>,
    offset_paths: Option<ShapeOffsetPaths>,
    trim: Option<ShapeTrimPaths>,
}

pub(super) struct ShapeImport {
    pub(super) layers: Vec<FxLayer>,
    pub(super) animations: Vec<AnimationGraphEntry>,
    pub(super) warnings: Vec<String>,
    /// Whether a committed caption lowered the layer's `Fade In+Out - frames`
    /// preset onto its paint Group, so the occurrence owner must not.
    pub(super) frame_fade_lowered: bool,
    pub(super) mapped_expressions: Vec<PropertyIdentity>,
}

struct Collector<'a> {
    includes_occurrence_pipeline: bool,
    next_id: &'a mut u64,
    animation_budget: &'a mut AnimationBudget,
    animations: Vec<AnimationGraphEntry>,
    warnings: Vec<String>,
    /// Set only after a caption output with its paint-Group fade commits.
    frame_fade_lowered: bool,
    evaluated_shapes: HashMap<usize, EvaluatedProperty>,
    /// Lowered expression identities with every FX target they must keep.
    mapped_expressions: Vec<(PropertyIdentity, Vec<(LayerId, PropType)>)>,
}

#[cfg(test)]
pub(super) fn import(
    layer: &Layer,
    occurrence: &fx_schema::GroupLayer,
    remaining_group_depth: usize,
    next_id: &mut u64,
    budget: &mut OutputBudget,
    animation_budget: &mut AnimationBudget,
) -> Result<ShapeImport, serde_json::Error> {
    import_with_control_context(
        layer,
        None,
        None,
        true,
        occurrence,
        remaining_group_depth,
        next_id,
        budget,
        animation_budget,
        None,
    )
}

#[cfg(test)]
#[allow(
    clippy::too_many_arguments,
    reason = "Preserve the existing native fixture entry point"
)]
pub(super) fn import_with_composition<'items>(
    layer: &Layer,
    composition: &Composition,
    source_items: &'items HashMap<u32, &'items ProjectItem>,
    includes_occurrence_pipeline: bool,
    occurrence: &fx_schema::GroupLayer,
    remaining_group_depth: usize,
    next_id: &mut u64,
    budget: &mut OutputBudget,
    animation_budget: &mut AnimationBudget,
) -> Result<ShapeImport, serde_json::Error> {
    import_with_control_context(
        layer,
        Some((layer, composition)),
        Some(source_items),
        includes_occurrence_pipeline,
        occurrence,
        remaining_group_depth,
        next_id,
        budget,
        animation_budget,
        None,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "Occurrence-local expression samples are separate from native controls and budgets"
)]
pub(super) fn import_with_evaluations<'items>(
    layer: &Layer,
    composition: &Composition,
    evaluations: (u32, &ExpressionSamples),
    source_items: &'items HashMap<u32, &'items ProjectItem>,
    includes_occurrence_pipeline: bool,
    occurrence: &fx_schema::GroupLayer,
    remaining_group_depth: usize,
    next_id: &mut u64,
    budget: &mut OutputBudget,
    animation_budget: &mut AnimationBudget,
) -> Result<ShapeImport, serde_json::Error> {
    import_with_control_context(
        layer,
        Some((layer, composition)),
        Some(source_items),
        includes_occurrence_pipeline,
        occurrence,
        remaining_group_depth,
        next_id,
        budget,
        animation_budget,
        Some(evaluations),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "Native source lookup is separate from destination and output budgets"
)]
fn import_with_control_context<'items>(
    layer: &Layer,
    control_context: Option<(&Layer, &Composition)>,
    source_items: Option<&'items HashMap<u32, &'items ProjectItem>>,
    includes_occurrence_pipeline: bool,
    occurrence: &fx_schema::GroupLayer,
    remaining_group_depth: usize,
    next_id: &mut u64,
    budget: &mut OutputBudget,
    animation_budget: &mut AnimationBudget,
    evaluations: Option<(u32, &ExpressionSamples)>,
) -> Result<ShapeImport, serde_json::Error> {
    let mut clock_warnings = Vec::new();
    let evaluated_shapes = evaluations
        .into_iter()
        .flat_map(|(comp_id, samples)| {
            samples.properties().iter().filter(move |sample| {
                sample.composition_id() == comp_id && sample.layer_id() == layer.record.id()
            })
        })
        .filter_map(|sample| {
            let PropertyIdentity::Shape { path } = sample.property() else {
                return None;
            };
            let run = resolve_shape_run(layer, path)?;
            // Shape contents use the layer's local clock; samples use the parent's.
            match super::animation::rebased_samples(
                sample,
                layer,
                NumericAnimationClock::source_local(),
            ) {
                Ok(rebased) => Some((run.as_ptr() as usize, rebased)),
                Err(error) => {
                    clock_warnings.push(format!(
                        "{:?}: {error}; expression samples not lowered",
                        sample.property()
                    ));
                    None
                }
            }
        })
        .collect();
    let mut collector = Collector {
        includes_occurrence_pipeline,
        next_id,
        animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
        evaluated_shapes,
        mapped_expressions: Vec::new(),
    };
    collector.warnings.append(&mut clock_warnings);
    let roots = match root_runs(&layer.content) {
        Ok(roots) => roots,
        Err(error) => {
            return Ok(ShapeImport {
                layers: Vec::new(),
                animations: Vec::new(),
                warnings: vec![format!("shape property root ignored: {error}")],
                frame_fade_lowered: false,
                mapped_expressions: Vec::new(),
            });
        }
    };
    let mut layers = Vec::new();
    for (_, run) in roots
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Root Vectors Group")
    {
        let group = match unique_list(run, *b"tdgp") {
            Ok(group) => group,
            Err(error) => {
                collector
                    .warnings
                    .push(format!("shape contents ignored: {error}"));
                continue;
            }
        };
        layers.extend(collector.collect_contents_with_sources(
            group,
            &layer.name,
            occurrence.id,
            remaining_group_depth,
            budget,
            control_context,
            source_items,
        )?);
    }
    // Lowering can roll back an entire generated group after fitting its tracks.
    // Only retained X/Y entries count as consumed capture identities.
    let mapped_expressions = collector
        .mapped_expressions
        .into_iter()
        .filter(|(_, kept)| {
            kept.iter().all(|(id, kind)| {
                collector.animations.iter().any(|entry| {
                    entry.target.as_property().is_some_and(|target| {
                        target.layer_id() == *id && target.property_type() == *kind
                    })
                })
            })
        })
        .map(|(identity, _)| identity)
        .collect();
    Ok(ShapeImport {
        layers,
        animations: collector.animations,
        warnings: collector.warnings,
        frame_fade_lowered: collector.frame_fade_lowered,
        mapped_expressions,
    })
}

impl Collector<'_> {
    #[cfg(test)]
    fn collect_contents(
        &mut self,
        children: &[Chunk],
        fallback_name: &str,
        parent: LayerId,
        remaining_group_depth: usize,
        _inherited_decorations: &Decorations,
    ) -> Vec<FxLayer> {
        self.collect_contents_with_budget(
            children,
            fallback_name,
            parent,
            remaining_group_depth,
            &mut OutputBudget::default(),
        )
        .unwrap()
    }

    #[cfg(test)]
    fn collect_contents_with_budget(
        &mut self,
        children: &[Chunk],
        fallback_name: &str,
        parent: LayerId,
        remaining_group_depth: usize,
        budget: &mut OutputBudget,
    ) -> Result<Vec<FxLayer>, serde_json::Error> {
        self.collect_contents_with_control_context(
            children,
            fallback_name,
            parent,
            remaining_group_depth,
            budget,
            None,
        )
    }

    #[cfg(test)]
    fn collect_contents_with_control_context(
        &mut self,
        children: &[Chunk],
        fallback_name: &str,
        parent: LayerId,
        remaining_group_depth: usize,
        budget: &mut OutputBudget,
        control_context: Option<(&Layer, &Composition)>,
    ) -> Result<Vec<FxLayer>, serde_json::Error> {
        self.collect_contents_with_sources(
            children,
            fallback_name,
            parent,
            remaining_group_depth,
            budget,
            control_context,
            None,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Native source lookup is separate from destination and output budgets"
    )]
    fn collect_contents_with_sources(
        &mut self,
        children: &[Chunk],
        fallback_name: &str,
        parent: LayerId,
        remaining_group_depth: usize,
        budget: &mut OutputBudget,
        control_context: Option<(&Layer, &Composition)>,
        source_items: Option<&HashMap<u32, &ProjectItem>>,
    ) -> Result<Vec<FxLayer>, serde_json::Error> {
        let program = program::Program::parse(children, remaining_group_depth);
        self.warnings.extend(program.warnings.iter().cloned());
        lowering::lower(
            self,
            &program,
            fallback_name,
            parent,
            remaining_group_depth,
            budget,
            control_context,
            source_items,
        )
    }

    #[cfg(test)]
    fn source_layer(
        &mut self,
        name: &str,
        run: &[Chunk],
        fallback_name: &str,
        parent: LayerId,
        decorations: Option<&Decorations>,
        control_context: Option<(&Layer, &Composition)>,
    ) -> Result<Option<FxLayer>, serde_json::Error> {
        self.source_layer_with_sources(
            name,
            run,
            fallback_name,
            parent,
            decorations,
            control_context,
            None,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Native source lookup is separate from destination and output budgets"
    )]
    fn source_layer_with_sources(
        &mut self,
        name: &str,
        run: &[Chunk],
        fallback_name: &str,
        parent: LayerId,
        decorations: Option<&Decorations>,
        control_context: Option<(&Layer, &Composition)>,
        source_items: Option<&HashMap<u32, &ProjectItem>>,
    ) -> Result<Option<FxLayer>, serde_json::Error> {
        let Some(id) = self.allocate() else {
            return Ok(None);
        };
        let mut content = content(empty_path(), decorations.cloned().unwrap_or_default());
        if name == "ADBE Vector Shape - Star"
            && let Ok(leaves) = property_group(run, "polystar")
        {
            self.warn_invalid_static_enum(
                &leaves,
                "ADBE Vector Star Type",
                &[1.0, 2.0],
                "Star retained",
            );
        }
        let ellipse_size = if name == "ADBE Vector Shape - Ellipse" {
            self.shape_control_numeric(run, "ADBE Vector Ellipse Size", control_context, true)
        } else {
            Ok(None)
        };
        let result = match name {
            "ADBE Vector Shape - Group" | "ADBE Mask Shape" => path::decode_first(run, [1.0, 1.0])
                .map(|(path, _)| content.path = path)
                .map_err(|error| format!("Bezier path ignored: {error}")),
            "ADBE Vector Shape - Rect" => rectangle(run)
                .map(|path| content.path = path)
                .map_err(|message| message.to_owned()),
            "ADBE Vector Shape - Ellipse" => ellipse(run)
                .map(|mut ellipse| {
                    if let Ok(Some((numeric, true))) = &ellipse_size
                        && let Some(value) = initial_scalar(numeric)
                    {
                        ellipse.size = [value, value];
                    }
                    content.ellipse = Some(ellipse);
                })
                .map_err(|message| message.to_owned()),
            "ADBE Vector Shape - Star" => poly_star(run)
                .map(|poly_star| content.poly_star = Some(poly_star))
                .map_err(|message| message.to_owned()),
            _ => Err(format!("unsupported shape source {name}")),
        };
        if let Err(message) = result {
            self.warnings.push(message);
            return Ok(None);
        }
        let mut layer = ShapeLayer {
            id,
            name: fallback_name.to_owned(),
            description: "Editable AE vector content in source-local time".into(),
            is_hidden: false,
            parent: Some(parent),
            blend_mode: Default::default(),
            track_matte: None,
            masks: Vec::new(),
            active_range: full_active_range(),
            effects: Vec::new(),
            motion_blur: false,
            transform: identity_transform(),
            shape: content,
        };
        let animation_start = self.animations.len();
        self.add_source_entries(name, run, id, control_context, source_items);
        if name != "ADBE Mask Shape" && direction::reversed(run, &mut self.warnings) {
            direction::apply(
                &mut layer,
                &mut self.animations[animation_start..],
                &mut self.warnings,
            )?;
        }
        Ok(Some(FxLayer::Shape(layer)))
    }

    fn add_transform_entries(&mut self, run: &[Chunk], target_id: LayerId) {
        let Ok(leaves) = property_group(run, "shape transform") else {
            return;
        };
        type ComponentMapping = (usize, PropType, f64);
        let mappings: &[(&str, &[ComponentMapping])] = &[
            (
                "ADBE Vector Anchor",
                &[
                    (0, PropType::AnchorPointX, 1.0),
                    (1, PropType::AnchorPointY, 1.0),
                ],
            ),
            (
                "ADBE Vector Position",
                &[(0, PropType::PositionX, 1.0), (1, PropType::PositionY, 1.0)],
            ),
            (
                "ADBE Vector Scale",
                &[(0, PropType::ScaleX, 1.0), (1, PropType::ScaleY, 1.0)],
            ),
            ("ADBE Vector Rotation", &[(0, PropType::Rotation, 1.0)]),
            ("ADBE Vector Skew", &[(0, PropType::Skew, 1.0)]),
            ("ADBE Vector Skew Axis", &[(0, PropType::SkewAxis, 1.0)]),
            ("ADBE Vector Group Opacity", &[(0, PropType::Opacity, 1.0)]),
        ];
        for (name, targets) in mappings {
            let Some(numeric) = numeric_leaf(&leaves, name, &mut self.warnings) else {
                continue;
            };
            let targets: Vec<_> = targets
                .iter()
                .map(|(component, property, scale)| {
                    NumericAnimationTarget::float(
                        PropertyTarget::layer(target_id, *property),
                        *component,
                        *scale,
                    )
                })
                .collect();
            self.add_leaf_numeric(&leaves, name, &numeric, &targets);
        }
    }

    /// Lower converter/Adobe expression samples of this exact native leaf onto
    /// the same editable targets ordinary keyed import uses; otherwise import
    /// its native keys. A partially lowered target set falls back atomically.
    fn add_leaf_numeric(
        &mut self,
        leaves: &[(&str, &[Chunk])],
        name: &str,
        numeric: &NumericProperty,
        targets: &[NumericAnimationTarget],
    ) {
        let samples = leaves
            .iter()
            .find(|(leaf_name, _)| *leaf_name == name)
            .and_then(|(_, run)| self.evaluated_shapes.get(&(run.as_ptr() as usize)))
            .cloned();
        let Some(samples) = samples else {
            self.add_numeric(name, numeric, targets);
            return;
        };
        let (entries, warnings) = super::animation::evaluated_numeric_entries(
            name,
            &samples,
            targets,
            &[],
            self.animation_budget,
        );
        if entries.len() == targets.len() {
            let kept = targets
                .iter()
                .filter_map(|target| target.property_target().as_property())
                .map(|target| (target.layer_id(), target.property_type()))
                .collect();
            self.mapped_expressions
                .push((samples.property().clone(), kept));
            self.animations.extend(entries);
        } else {
            self.add_numeric(name, numeric, targets);
        }
        self.warnings.extend(warnings);
    }

    fn add_source_entries(
        &mut self,
        name: &str,
        run: &[Chunk],
        target_id: LayerId,
        control_context: Option<(&Layer, &Composition)>,
        source_items: Option<&HashMap<u32, &ProjectItem>>,
    ) {
        if matches!(name, "ADBE Vector Shape - Group" | "ADBE Mask Shape") {
            if let Some((owner, composition)) = control_context
                && let Some(source_items) = source_items
                && let Some(result) = dynamic_path::entries(
                    owner,
                    composition,
                    source_items,
                    run,
                    PropertyTarget::layer(target_id, PropType::ShapePath),
                    NumericAnimationClock::source_local(),
                    self.animation_budget,
                )
            {
                match result {
                    Ok((entries, warnings)) => {
                        self.animations.extend(entries);
                        self.warnings.extend(warnings);
                        return;
                    }
                    Err(error) => self.warnings.push(format!(
                        "Path expression not lowered ({error}); initial editable outline retained"
                    )),
                }
            }
            let (entries, warnings) = path::entries(
                run,
                [1.0, 1.0],
                PropertyTarget::layer(target_id, PropType::ShapePath),
                NumericAnimationClock::source_local(),
                self.animation_budget,
            );
            self.animations.extend(entries);
            self.warnings.extend(warnings);
            return;
        }
        if name == "ADBE Vector Shape - Rect" {
            let (entries, warnings) = rectangle_animation::entries(run, target_id);
            self.animations.extend(entries);
            self.warnings.extend(warnings);
            return;
        }
        let Ok(leaves) = property_group(run, name) else {
            return;
        };
        let mappings: &[(&str, PropType, [usize; 2], [f64; 2])] = match name {
            "ADBE Vector Shape - Ellipse" => &[
                (
                    "ADBE Vector Ellipse Size",
                    PropType::EllipseSize,
                    [0, 1],
                    [1.0, 1.0],
                ),
                (
                    "ADBE Vector Ellipse Position",
                    PropType::EllipsePosition,
                    [0, 1],
                    [1.0, 1.0],
                ),
            ],
            "ADBE Vector Shape - Star" => &[(
                "ADBE Vector Star Position",
                PropType::PolyStarPosition,
                [0, 1],
                [1.0, 1.0],
            )],
            _ => &[],
        };
        for (property_name, property, components, scale) in mappings {
            let (numeric, linked) = if *property_name == "ADBE Vector Ellipse Size" {
                match self.shape_control_numeric(run, property_name, control_context, true) {
                    Ok(Some(value)) => value,
                    Ok(None) => continue,
                    Err(error) => {
                        self.warnings.push(format!(
                            "{property_name}: Slider control link not lowered ({error}); original expression retained"
                        ));
                        let Some(numeric) =
                            numeric_leaf(&leaves, property_name, &mut self.warnings)
                        else {
                            continue;
                        };
                        (numeric, false)
                    }
                }
            } else {
                let Some(numeric) = numeric_leaf(&leaves, property_name, &mut self.warnings) else {
                    continue;
                };
                (numeric, false)
            };
            if linked {
                self.warnings.push(format!(
                    "{property_name}: Slider control lowered to independent editable values/keys; controller edit linkage is not retained"
                ));
            }
            let target = [NumericAnimationTarget::vector2(
                PropertyTarget::layer(target_id, *property),
                *components,
                *scale,
            )];
            if linked {
                self.add_numeric(property_name, &numeric, &target);
            } else {
                self.add_leaf_numeric(&leaves, property_name, &numeric, &target);
            }
        }
        if name == "ADBE Vector Shape - Star" {
            let scalar_mappings = [
                ("ADBE Vector Star Points", PropType::PolyStarPoints),
                ("ADBE Vector Star Rotation", PropType::PolyStarRotation),
                (
                    "ADBE Vector Star Outer Radius",
                    PropType::PolyStarOuterRadius,
                ),
                (
                    "ADBE Vector Star Inner Radius",
                    PropType::PolyStarInnerRadius,
                ),
                (
                    "ADBE Vector Star Outer Roundess",
                    PropType::PolyStarOuterRoundness,
                ),
                (
                    "ADBE Vector Star Inner Roundess",
                    PropType::PolyStarInnerRoundness,
                ),
            ];
            for (property_name, property) in scalar_mappings {
                let Some(numeric) = numeric_leaf(&leaves, property_name, &mut self.warnings) else {
                    continue;
                };
                if property == PropType::PolyStarPoints
                    && (numeric.animated
                        || !numeric.keyframes.is_empty()
                        || base_component(&numeric, 0).is_some_and(|value| value.fract() != 0.0))
                {
                    self.warnings.push("Star/Polygon point counts are floored to integers after clamping to 3..1000 when generating outlines; fractional values, including between keys, are approximated. Authored values and keyframes remain editable".into());
                }
                self.add_leaf_numeric(
                    &leaves,
                    property_name,
                    &numeric,
                    &[NumericAnimationTarget::float(
                        PropertyTarget::layer(target_id, property),
                        0,
                        1.0,
                    )],
                );
            }
        }
    }

    fn shape_control_numeric(
        &mut self,
        run: &[Chunk],
        property_name: &str,
        control_context: Option<(&Layer, &Composition)>,
        repeated_vector: bool,
    ) -> Result<Option<(NumericProperty, bool)>, PropertyError> {
        let leaves = property_group(run, property_name)
            .map_err(|_| PropertyError::Layout("invalid shape property group"))?;
        let Some(numeric) = numeric_leaf(&leaves, property_name, &mut self.warnings) else {
            return Ok(None);
        };
        if !numeric.expression_enabled {
            return Ok(Some((numeric, false)));
        }
        let Some((layer, composition)) = control_context else {
            return Ok(Some((numeric, false)));
        };
        let mut matches = leaves
            .iter()
            .filter(|(candidate, _)| *candidate == property_name);
        let (_, run) = matches
            .next()
            .ok_or(PropertyError::Layout("shape property missing"))?;
        if matches.next().is_some() {
            return Err(PropertyError::Layout("ambiguous shape property"));
        }
        let property = unique_list(run, *b"tdbs")?;
        let resolved = if repeated_vector {
            super::control_links::lower_slider_repeated_vector(layer, composition, property)
        } else {
            super::control_links::lower_slider_scalar(layer, composition, property)
        }?;
        Ok(Some((resolved, true)))
    }

    fn decorations(
        &mut self,
        entries: &[(&str, &[Chunk])],
        control_context: Option<(&Layer, &Composition)>,
    ) -> Decorations {
        let mut result = decorations(entries, &mut self.warnings);
        let mut fill_index = 0;
        let mut stroke_index = 0;
        for (name, run) in entries {
            if *name == "ADBE Vector Graphic - Fill" {
                if let Ok(fill) = self.solid_fill(run, control_context)
                    && let Some(destination) = result.fills.get_mut(fill_index)
                {
                    destination.paint = fill.paint;
                    fill_index += 1;
                }
                continue;
            }
            if *name == "ADBE Vector Graphic - G-Fill" {
                fill_index += decorations(&[(*name, *run)], &mut Vec::new()).fills.len();
                continue;
            }
            if !matches!(
                *name,
                "ADBE Vector Graphic - Stroke" | "ADBE Vector Graphic - G-Stroke"
            ) {
                continue;
            }
            let count = decorations(&[(*name, *run)], &mut Vec::new()).strokes.len();
            if count == 0 {
                continue;
            }
            if *name == "ADBE Vector Graphic - Stroke" {
                match self.static_color_alias(run, "ADBE Vector Stroke Color", control_context) {
                    Ok(Some(color)) => {
                        if let Some(stroke) = result.strokes.get_mut(stroke_index) {
                            stroke.paint = ShapePaint::Solid { color };
                        }
                    }
                    Ok(None) => {}
                    Err(error) => self.warnings.push(error),
                }
            }
            if let Ok(Some((numeric, true))) =
                self.shape_control_numeric(run, "ADBE Vector Stroke Width", control_context, false)
                && let Some(width) = initial_scalar(&numeric).and_then(non_negative)
                && let Some(stroke) = result.strokes.get_mut(stroke_index)
            {
                stroke.width = width;
            }
            stroke_index += count;
        }
        result
    }

    fn solid_fill(
        &mut self,
        run: &[Chunk],
        control_context: Option<(&Layer, &Composition)>,
    ) -> Result<ShapeFillStyle, String> {
        let mut fill = solid_fill(run)?;
        if let Some(color) =
            self.static_color_alias(run, "ADBE Vector Fill Color", control_context)?
        {
            fill.paint = ShapePaint::Solid { color };
        }
        Ok(fill)
    }

    fn static_color_alias(
        &mut self,
        run: &[Chunk],
        property_name: &str,
        control_context: Option<(&Layer, &Composition)>,
    ) -> Result<Option<[f64; 4]>, String> {
        let leaves = property_group(run, "paint")?;
        let Some(numeric) = numeric_leaf(&leaves, property_name, &mut self.warnings) else {
            return Ok(None);
        };
        if !numeric.expression_enabled {
            return Ok(None);
        }
        let resolved = (|| {
            let (_, composition) =
                control_context.ok_or(PropertyError::Layout("Color Control scope missing"))?;
            let property = super::control_links::unique_run(&leaves, property_name)?;
            super::control_links::lower_static_color_control(
                unique_list(property, *b"tdbs")?,
                composition,
                None,
            )
        })();
        match resolved {
            Ok(color) => {
                self.warnings.push(format!("{property_name}: complete static sibling Color Control alias copied into editable paint; live controller linkage is lost"));
                Ok(Some(color))
            }
            Err(error) => {
                self.warnings.push(format!(
                    "{property_name}: unsupported expression retained at authored color: {error}"
                ));
                Ok(None)
            }
        }
    }

    fn add_scope_entries(
        &mut self,
        entries: &[(&str, &[Chunk])],
        decorations: &Decorations,
        layers: &[FxLayer],
        control_context: Option<(&Layer, &Composition)>,
    ) {
        let mut targets = Vec::new();
        collect_shape_target_ids(layers, &mut targets);
        if targets.is_empty() {
            return;
        }
        for (name, run) in entries {
            let Ok(leaves) = property_group(run, name) else {
                continue;
            };
            match *name {
                "ADBE Vector Graphic - Fill" => {
                    self.warn_malformed_static(
                        &leaves,
                        &["ADBE Vector Fill Rule"],
                        "NonZeroWinding retained",
                    );
                    self.warn_invalid_static_enum(
                        &leaves,
                        "ADBE Vector Fill Rule",
                        &[1.0, 2.0],
                        "NonZeroWinding retained",
                    );
                    self.add_paint_entry(
                        &leaves,
                        "ADBE Vector Fill Color",
                        PropType::FillColor,
                        &targets,
                        decorations
                            .fills
                            .first()
                            .is_some_and(|fill| matches!(fill.paint, ShapePaint::Solid { .. })),
                    );
                    self.add_scalar_scope_entry(
                        &leaves,
                        "ADBE Vector Fill Opacity",
                        PropType::Opacity,
                        1.0,
                        &targets,
                    );
                }
                "ADBE Vector Graphic - Stroke" => {
                    self.warn_malformed_static(
                        &leaves,
                        &[
                            "ADBE Vector Stroke Line Cap",
                            "ADBE Vector Stroke Line Join",
                        ],
                        "native Butt/Miter defaults retained",
                    );
                    self.warn_invalid_static_enum(
                        &leaves,
                        "ADBE Vector Stroke Line Cap",
                        &[1.0, 2.0, 3.0],
                        "Butt retained",
                    );
                    self.warn_invalid_static_enum(
                        &leaves,
                        "ADBE Vector Stroke Line Join",
                        &[1.0, 2.0, 3.0],
                        "Miter retained",
                    );
                    self.add_paint_entry(
                        &leaves,
                        "ADBE Vector Stroke Color",
                        PropType::StrokeColor,
                        &targets,
                        decorations
                            .strokes
                            .first()
                            .is_some_and(|stroke| matches!(stroke.paint, ShapePaint::Solid { .. })),
                    );
                    self.add_slider_scope_entry(
                        run,
                        &leaves,
                        "ADBE Vector Stroke Width",
                        PropType::StrokeWidth,
                        1.0,
                        &targets,
                        control_context,
                    );
                    self.add_scalar_scope_entry(
                        &leaves,
                        "ADBE Vector Stroke Miter Limit",
                        PropType::StrokeMiterLimit,
                        1.0,
                        &targets,
                    );
                    self.add_scalar_scope_entry(
                        &leaves,
                        "ADBE Vector Stroke Opacity",
                        PropType::Opacity,
                        1.0,
                        &targets,
                    );
                    self.add_stroke_join_entries(&leaves, &targets);
                    self.add_dash_entries(&leaves, &targets);
                }
                "ADBE Vector Graphic - G-Fill" | "ADBE Vector Graphic - G-Stroke" => {
                    for (property, reason) in [
                        (
                            "ADBE Vector Grad Start Pt",
                            "gradient axes have no canonical ShapePaint animation target",
                        ),
                        (
                            "ADBE Vector Grad End Pt",
                            "gradient axes have no canonical ShapePaint animation target",
                        ),
                        (
                            "ADBE Vector Grad Colors",
                            "gradient stops have no canonical ShapePaint animation target",
                        ),
                        (
                            "ADBE Vector Grad Type",
                            "gradient type has no canonical ShapePaint animation target; initial type retained",
                        ),
                        (
                            "ADBE Vector Grad HiLite Length",
                            "radial highlight has no ShapePaint target; centered static radial gradient retained",
                        ),
                        (
                            "ADBE Vector Grad HiLite Angle",
                            "radial highlight has no ShapePaint target; centered static radial gradient retained",
                        ),
                    ] {
                        self.warn_animated_unsupported(&leaves, property, reason);
                    }
                    self.warn_malformed_static(
                        &leaves,
                        &[
                            "ADBE Vector Fill Rule",
                            "ADBE Vector Stroke Line Cap",
                            "ADBE Vector Stroke Line Join",
                        ],
                        "native paint defaults retained",
                    );
                    if *name == "ADBE Vector Graphic - G-Fill" {
                        self.warn_invalid_static_enum(
                            &leaves,
                            "ADBE Vector Fill Rule",
                            &[1.0, 2.0],
                            "NonZeroWinding retained",
                        );
                    } else {
                        self.warn_invalid_static_enum(
                            &leaves,
                            "ADBE Vector Stroke Line Cap",
                            &[1.0, 2.0, 3.0],
                            "Butt retained",
                        );
                        self.warn_invalid_static_enum(
                            &leaves,
                            "ADBE Vector Stroke Line Join",
                            &[1.0, 2.0, 3.0],
                            "Miter retained",
                        );
                    }
                    let opacity = if *name == "ADBE Vector Graphic - G-Fill" {
                        "ADBE Vector Fill Opacity"
                    } else {
                        "ADBE Vector Stroke Opacity"
                    };
                    self.add_scalar_scope_entry(&leaves, opacity, PropType::Opacity, 1.0, &targets);
                    if *name == "ADBE Vector Graphic - G-Stroke" {
                        self.add_scalar_scope_entry(
                            &leaves,
                            "ADBE Vector Stroke Miter Limit",
                            PropType::StrokeMiterLimit,
                            1.0,
                            &targets,
                        );
                        self.add_slider_scope_entry(
                            run,
                            &leaves,
                            "ADBE Vector Stroke Width",
                            PropType::StrokeWidth,
                            1.0,
                            &targets,
                            control_context,
                        );
                        self.add_stroke_join_entries(&leaves, &targets);
                        self.add_dash_entries(&leaves, &targets);
                    }
                }
                "ADBE Vector Filter - RC" => self.add_scalar_scope_entry(
                    &leaves,
                    "ADBE Vector RoundCorner Radius",
                    PropType::RoundCornersRadius,
                    1.0,
                    &targets,
                ),
                "ADBE Vector Filter - Offset" => {
                    self.warn_malformed_static(
                        &leaves,
                        &[
                            "ADBE Vector Offset Line Join",
                            "ADBE Vector Offset Miter Limit",
                        ],
                        "native default retained",
                    );
                    self.warn_invalid_static_enum(
                        &leaves,
                        "ADBE Vector Offset Line Join",
                        &[1.0, 2.0, 3.0],
                        "Miter retained",
                    );
                    self.add_scalar_scope_entry(
                        &leaves,
                        "ADBE Vector Offset Amount",
                        PropType::OffsetPathsAmount,
                        1.0,
                        &targets,
                    );
                }
                "ADBE Vector Filter - Trim" => {
                    self.warn_malformed_static(
                        &leaves,
                        &["ADBE Vector Trim Type"],
                        "Simultaneously retained",
                    );
                    self.warn_invalid_static_enum(
                        &leaves,
                        "ADBE Vector Trim Type",
                        &[1.0, 2.0],
                        "Simultaneously retained",
                    );
                    for (property_name, property) in [
                        ("ADBE Vector Trim Start", PropType::TrimStart),
                        ("ADBE Vector Trim End", PropType::TrimEnd),
                        ("ADBE Vector Trim Offset", PropType::TrimOffset),
                    ] {
                        self.add_scalar_scope_entry(
                            &leaves,
                            property_name,
                            property,
                            1.0,
                            &targets,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn add_paint_entry(
        &mut self,
        leaves: &[(&str, &[Chunk])],
        name: &str,
        property: PropType,
        targets: &[LayerId],
        supported: bool,
    ) {
        let Some(numeric) = numeric_leaf(leaves, name, &mut self.warnings) else {
            return;
        };
        if numeric.keyframes.is_empty() {
            return;
        }
        if !supported {
            self.warnings.push(format!(
                "{name}: animated paint cannot target this non-solid or non-primary destination paint; static paint retained"
            ));
            return;
        }
        for target_id in targets {
            self.add_numeric(
                name,
                &numeric,
                &[NumericAnimationTarget::color(
                    PropertyTarget::layer(*target_id, property),
                    [0, 1, 2, 3],
                    [1.0; 4],
                )],
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_slider_scope_entry(
        &mut self,
        run: &[Chunk],
        leaves: &[(&str, &[Chunk])],
        name: &str,
        property: PropType,
        scale: f64,
        targets: &[LayerId],
        control_context: Option<(&Layer, &Composition)>,
    ) {
        let (numeric, linked) = match self.shape_control_numeric(run, name, control_context, false)
        {
            Ok(Some(value)) => value,
            Ok(None) => return,
            Err(error) => {
                self.warnings.push(format!(
                    "{name}: Slider control link not lowered ({error}); original expression retained"
                ));
                let Some(numeric) = numeric_leaf(leaves, name, &mut self.warnings) else {
                    return;
                };
                (numeric, false)
            }
        };
        if linked {
            self.warnings.push(format!(
                "{name}: Slider control lowered to independent editable values/keys; controller edit linkage is not retained"
            ));
        }
        for target_id in targets {
            let target = [NumericAnimationTarget::float(
                PropertyTarget::layer(*target_id, property),
                0,
                scale,
            )];
            if linked {
                self.add_numeric(name, &numeric, &target);
            } else {
                self.add_leaf_numeric(leaves, name, &numeric, &target);
            }
        }
    }

    fn add_scalar_scope_entry(
        &mut self,
        leaves: &[(&str, &[Chunk])],
        name: &str,
        property: PropType,
        scale: f64,
        targets: &[LayerId],
    ) {
        let Some(numeric) = numeric_leaf(leaves, name, &mut self.warnings) else {
            return;
        };
        for target_id in targets {
            self.add_leaf_numeric(
                leaves,
                name,
                &numeric,
                &[NumericAnimationTarget::float(
                    PropertyTarget::layer(*target_id, property),
                    0,
                    scale,
                )],
            );
        }
    }

    fn add_stroke_join_entries(&mut self, leaves: &[(&str, &[Chunk])], targets: &[LayerId]) {
        let name = "ADBE Vector Stroke Line Join";
        let Some(numeric) = numeric_leaf(leaves, name, &mut self.warnings) else {
            return;
        };
        for target_id in targets {
            self.add_numeric(
                name,
                &numeric,
                &[NumericAnimationTarget::stroke_join(PropertyTarget::layer(
                    *target_id,
                    PropType::StrokeJoin,
                ))],
            );
        }
    }

    fn add_dash_entries(&mut self, leaves: &[(&str, &[Chunk])], targets: &[LayerId]) {
        let Some((_, run)) = leaves
            .iter()
            .find(|(name, _)| *name == "ADBE Vector Stroke Dashes")
        else {
            return;
        };
        let dash_leaves = match property_group(run, "stroke dashes") {
            Ok(leaves) => leaves,
            Err(error) => {
                self.warnings.push(format!(
                    "stroke dashes are malformed: {error}; dash pattern omitted while the stroke was retained"
                ));
                return;
            }
        };
        self.add_scalar_scope_entry(
            &dash_leaves,
            "ADBE Vector Stroke Offset",
            PropType::StrokeDashOffset,
            1.0,
            targets,
        );
        for (name, run) in dash_leaves {
            if !name.starts_with("ADBE Vector Stroke Dash")
                && !name.starts_with("ADBE Vector Stroke Gap")
            {
                continue;
            }
            let value = match numeric(run, name) {
                Ok(value) => value,
                Err(error) => {
                    self.warnings.push(format!(
                        "{name}: malformed dash/gap length ({error}); that element was omitted while the stroke was retained"
                    ));
                    continue;
                }
            };
            if value.animated || value.expression_enabled || !value.keyframes.is_empty() {
                self.warnings.push(format!(
                    "{name}: animated/expression dash/gap lengths have no canonical layer-property target; the stored initial length was retained"
                ));
            }
            if match base_component(&value, 0) {
                Some(length) => !length.is_finite() || length < 0.0,
                None => true,
            } {
                self.warnings.push(format!(
                    "{name}: negative, non-finite, or missing dash/gap length omitted while the stroke was retained"
                ));
            }
        }
    }

    fn warn_animated_unsupported(&mut self, leaves: &[(&str, &[Chunk])], name: &str, reason: &str) {
        let Some((_, run)) = leaves.iter().find(|(candidate, _)| *candidate == name) else {
            return;
        };
        let animated = if name == "ADBE Vector Grad Colors" {
            unique_list(run, *b"GCst")
                .ok()
                .and_then(|wrapper| unique_list(wrapper, *b"GCky").ok())
                .is_some_and(|values| {
                    values.iter().filter(|chunk| chunk.id() == *b"Utf8").count() > 1
                })
        } else {
            match numeric(run, name) {
                Ok(value) => {
                    value.animated || value.expression_enabled || !value.keyframes.is_empty()
                }
                Err(error) => {
                    self.warnings.push(format!(
                        "{name}: malformed unsupported gradient control ({error}); stored static replacement may be incomplete"
                    ));
                    false
                }
            }
        };
        if animated {
            self.warnings.push(format!("{name}: {reason}"));
        }
    }

    fn warn_malformed_static(
        &mut self,
        leaves: &[(&str, &[Chunk])],
        names: &[&str],
        fallback: &str,
    ) {
        for name in names {
            let Some((_, run)) = leaves.iter().find(|(candidate, _)| candidate == name) else {
                continue;
            };
            if let Err(error) = numeric(run, name) {
                self.warnings.push(format!(
                    "{name}: malformed static control ({error}); {fallback}"
                ));
            }
        }
    }

    fn warn_invalid_static_enum(
        &mut self,
        leaves: &[(&str, &[Chunk])],
        name: &str,
        allowed: &[f64],
        fallback: &str,
    ) {
        let Some((_, run)) = leaves.iter().find(|(candidate, _)| *candidate == name) else {
            return;
        };
        let Ok(value) = numeric(run, name) else {
            return;
        };
        let Some(value) = base_component(&value, 0) else {
            return;
        };
        if allowed.contains(&value) {
            return;
        }
        let kind = if !value.is_finite() {
            "non-finite"
        } else if value.fract() != 0.0 {
            "fractional"
        } else {
            "unknown"
        };
        self.warnings.push(format!(
            "{name}: {kind} static enum value {value}; {fallback}"
        ));
    }

    fn add_numeric(
        &mut self,
        name: &str,
        numeric: &NumericProperty,
        targets: &[NumericAnimationTarget],
    ) {
        let (mut entries, warnings) = numeric_entries(
            name,
            numeric,
            targets,
            NumericAnimationClock::source_local(),
            self.animation_budget,
        );
        self.animations.append(&mut entries);
        self.warnings.extend(warnings);
    }

    fn id_checkpoint(&self) -> u64 {
        *self.next_id
    }

    fn restore_ids(&mut self, checkpoint: u64) {
        *self.next_id = checkpoint;
    }

    fn allocate(&mut self) -> Option<LayerId> {
        let Some(id) = super::reserve_ids(self.next_id, 1) else {
            let warning =
                "AE vector helper/paint omitted: generated layer identifier space exhausted";
            if !self.warnings.iter().any(|existing| existing == warning) {
                self.warnings.push(warning.into());
            }
            return None;
        };
        Some(LayerId::new(id))
    }
}

fn decorations(entries: &[(&str, &[Chunk])], warnings: &mut Vec<String>) -> Decorations {
    let mut result = Decorations::default();
    for (name, run) in entries {
        if matches!(
            *name,
            "ADBE Vector Graphic - Stroke" | "ADBE Vector Graphic - G-Stroke"
        ) {
            warn_stroke_taper_wave(run, warnings);
        }
        match *name {
            "ADBE Vector Graphic - Fill" => match solid_fill(run) {
                Ok(fill) => result.fills.push(fill),
                Err(message) => warnings.push(message),
            },
            "ADBE Vector Graphic - Stroke" => match solid_stroke(run) {
                Ok(stroke) => result.strokes.push(stroke),
                Err(message) => warnings.push(message),
            },
            "ADBE Vector Graphic - G-Fill" => match gradient_fill(run) {
                Ok((fill, mut paint_warnings)) => {
                    result.fills.push(fill);
                    warnings.append(&mut paint_warnings);
                }
                Err(message) => warnings.push(message),
            },
            "ADBE Vector Graphic - G-Stroke" => match gradient_stroke(run) {
                Ok((stroke, mut paint_warnings)) => {
                    result.strokes.push(stroke);
                    warnings.append(&mut paint_warnings);
                }
                Err(message) => warnings.push(message),
            },
            "ADBE Vector Filter - RC" => match scalar_leaf(run, "ADBE Vector RoundCorner Radius") {
                Ok(radius) => match non_negative(radius) {
                    Some(radius) => result.round_corners = Some(ShapeRoundCorners { radius }),
                    None => warnings.push(
                        "Round Corners radius was negative or non-finite and was omitted".into(),
                    ),
                },
                Err(message) => warnings.push(message),
            },
            "ADBE Vector Filter - Offset" => match offset_paths(run) {
                Ok(value) => result.offset_paths = Some(value),
                Err(message) => warnings.push(message),
            },
            "ADBE Vector Filter - Trim" => match trim_paths(run) {
                Ok(value) => result.trim = Some(value),
                Err(message) => warnings.push(message),
            },
            _ => {}
        }
    }
    result
}

/// AE variable-width strokes have no counterpart in FX's uniform-width paint.
/// The default native Taper/Wave groups are present on ordinary strokes too,
/// so diagnose only a nondefault control, animation or undecodable group.
fn warn_stroke_taper_wave(run: &[Chunk], warnings: &mut Vec<String>) {
    let Ok(leaves) = property_group(run, "stroke") else {
        return;
    };
    let defaults = [
        ("ADBE Vector Taper Start Width", 0.0),
        ("ADBE Vector Taper End Width", 0.0),
        ("ADBE Vector Taper StartWidthPx", 0.0),
        ("ADBE Vector Taper EndWidthPx", 0.0),
        ("ADBE Vector Taper Start Length", 0.0),
        ("ADBE Vector Taper End Length", 0.0),
        ("ADBE Vector Taper Start Ease", 0.0),
        ("ADBE Vector Taper End Ease", 0.0),
        ("ADBE Vector Taper Wave Amount", 0.0),
        ("ADBE Vector Taper Wave Phase", 0.0),
        ("ADBE Vector Taper Wave Cycles", 10.0),
        ("ADBE Vector Taper Wavelength", 100.0),
        ("ADBE Vector Taper Length Units", 1.0),
        ("ADBE Vector Taper Wave Units", 1.0),
    ];
    for (group_name, group) in leaves.iter().filter(|(name, _)| {
        matches!(
            *name,
            "ADBE Vector Stroke Taper" | "ADBE Vector Stroke Wave"
        )
    }) {
        if !crate::properties::group_enabled_or_warn(group, group_name, warnings) {
            continue;
        }
        let controls = match property_group(group, group_name) {
            Ok(controls) => controls,
            Err(error) => {
                warnings.push(format!(
                    "{group_name}: variable-width stroke controls could not be decoded ({error}); uniform FX stroke retained"
                ));
                continue;
            }
        };
        let changed: Vec<_> = controls
            .iter()
            .filter_map(|(name, property)| {
                let expected = defaults
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| *value);
                let Some(value) = numeric_from_run(property) else {
                    return Some(*name);
                };
                (value.animated
                    || value.expression_enabled
                    || !value.keyframes.is_empty()
                    || base_component(&value, 0) != expected)
                    .then_some(*name)
            })
            .collect();
        if !changed.is_empty() {
            warnings.push(format!(
                "{group_name}: AE variable-width stroke controls {} were omitted; uniform FX stroke and convertible paint siblings retained",
                changed.join(", ")
            ));
        }
    }
}

fn content(path: ShapePath, decorations: Decorations) -> ShapeContent {
    ShapeContent {
        path,
        fills: decorations.fills,
        strokes: decorations.strokes,
        round_corners: decorations.round_corners,
        offset_paths: decorations.offset_paths,
        trim: decorations.trim,
        poly_star: None,
        ellipse: None,
    }
}

fn solid_fill(run: &[Chunk]) -> Result<ShapeFillStyle, String> {
    let leaves = property_group(run, "fill")?;
    let color = vector_leaf(&leaves, "ADBE Vector Fill Color", 3)?;
    let opacity = scalar_leaf_from(&leaves, "ADBE Vector Fill Opacity")? / 100.0;
    let rule = scalar_leaf_from(&leaves, "ADBE Vector Fill Rule").unwrap_or(1.0);
    Ok(ShapeFillStyle {
        paint: ShapePaint::Solid {
            color: [
                color[0],
                color[1],
                color[2],
                color.get(3).copied().unwrap_or(1.0),
            ],
        },
        fill_rule: if rule == 2.0 {
            ShapeFillRule::EvenOdd
        } else {
            ShapeFillRule::NonZeroWinding
        },
        blend_mode: Default::default(),
        opacity: opacity.clamp(0.0, 1.0),
    })
}

fn solid_stroke(run: &[Chunk]) -> Result<ShapeStrokeStyle, String> {
    let leaves = property_group(run, "stroke")?;
    let color = vector_leaf(&leaves, "ADBE Vector Stroke Color", 3)?;
    stroke_style(
        &leaves,
        ShapePaint::Solid {
            color: [
                color[0],
                color[1],
                color[2],
                color.get(3).copied().unwrap_or(1.0),
            ],
        },
    )
}

fn gradient_fill(run: &[Chunk]) -> Result<(ShapeFillStyle, Vec<String>), String> {
    let leaves = property_group(run, "gradient fill")?;
    let (paint, warnings) = gradient_paint(&leaves)?;
    let opacity = scalar_leaf_from(&leaves, "ADBE Vector Fill Opacity")? / 100.0;
    let rule = scalar_leaf_from(&leaves, "ADBE Vector Fill Rule").unwrap_or(1.0);
    Ok((
        ShapeFillStyle {
            paint,
            fill_rule: if rule == 2.0 {
                ShapeFillRule::EvenOdd
            } else {
                ShapeFillRule::NonZeroWinding
            },
            blend_mode: Default::default(),
            opacity: opacity.clamp(0.0, 1.0),
        },
        warnings,
    ))
}

fn gradient_stroke(run: &[Chunk]) -> Result<(ShapeStrokeStyle, Vec<String>), String> {
    let leaves = property_group(run, "gradient stroke")?;
    let (paint, warnings) = gradient_paint(&leaves)?;
    Ok((stroke_style(&leaves, paint)?, warnings))
}

fn gradient_paint(leaves: &[(&str, &[Chunk])]) -> Result<(ShapePaint, Vec<String>), String> {
    let gradient_type_value = scalar_leaf_from(leaves, "ADBE Vector Grad Type")?;
    let gradient_type = if gradient_type_value == 2.0 { 2 } else { 1 };
    let start = vector_leaf(leaves, "ADBE Vector Grad Start Pt", 2)?;
    let end = vector_leaf(leaves, "ADBE Vector Grad End Pt", 2)?;
    let colors = leaves
        .iter()
        .find_map(|(name, run)| (*name == "ADBE Vector Grad Colors").then_some(*run));
    let mut decoded = gradient::decode_or_native_default(
        colors,
        gradient_type,
        [start[0], start[1]],
        [end[0], end[1]],
    )?;
    if !matches!(gradient_type_value, 1.0 | 2.0) {
        let kind = if !gradient_type_value.is_finite() {
            "non-finite"
        } else if gradient_type_value.fract() != 0.0 {
            "fractional"
        } else {
            "unknown"
        };
        decoded.warnings.push(format!(
            "ADBE Vector Grad Type: {kind} static enum value {gradient_type_value}; Linear retained"
        ));
    }
    if decoded.animated {
        decoded.warnings.push("animated AE gradient stops have no canonical FX stop-animation target; the first native stop set was retained as an editable static gradient".into());
    }
    let highlight_length =
        scalar_leaf_from(leaves, "ADBE Vector Grad HiLite Length").unwrap_or(0.0);
    let highlight_angle = scalar_leaf_from(leaves, "ADBE Vector Grad HiLite Angle").unwrap_or(0.0);
    if highlight_length != 0.0 || highlight_angle != 0.0 {
        decoded.warnings.push("AE radial-gradient highlight length/angle has no ShapePaint equivalent; the centered radial gradient was retained".into());
    }
    Ok((decoded.paint, decoded.warnings))
}

fn stroke_style(
    leaves: &[(&str, &[Chunk])],
    paint: ShapePaint,
) -> Result<ShapeStrokeStyle, String> {
    let width = non_negative(scalar_leaf_from(leaves, "ADBE Vector Stroke Width")?)
        .ok_or_else(|| "stroke width was negative or non-finite".to_owned())?;
    let opacity = scalar_leaf_from(leaves, "ADBE Vector Stroke Opacity")? / 100.0;
    let cap = scalar_leaf_from(leaves, "ADBE Vector Stroke Line Cap").unwrap_or(1.0);
    let join = scalar_leaf_from(leaves, "ADBE Vector Stroke Line Join").unwrap_or(1.0);
    let (dashes, dash_offset) = stroke_dashes(leaves);
    Ok(ShapeStrokeStyle {
        enabled: true,
        paint,
        width,
        cap: match cap {
            2.0 => ShapeLineCap::Round,
            3.0 => ShapeLineCap::Square,
            _ => ShapeLineCap::Butt,
        },
        join: match join {
            2.0 => ShapeLineJoin::Round,
            3.0 => ShapeLineJoin::Bevel,
            _ => ShapeLineJoin::Miter,
        },
        miter_limit: scalar_leaf_from(leaves, "ADBE Vector Stroke Miter Limit").unwrap_or(4.0),
        blend_mode: Default::default(),
        opacity: opacity.clamp(0.0, 1.0),
        dashes,
        dash_offset,
    })
}

fn stroke_dashes(leaves: &[(&str, &[Chunk])]) -> (Vec<NonNegativeProperty>, f64) {
    let Some((_, run)) = leaves
        .iter()
        .find(|(name, _)| *name == "ADBE Vector Stroke Dashes")
    else {
        return (Vec::new(), 0.0);
    };
    let Ok(dash_leaves) = property_group(run, "stroke dashes") else {
        return (Vec::new(), 0.0);
    };
    let mut dashes = Vec::new();
    let mut offset = 0.0;
    for (name, run) in dash_leaves {
        if name == "ADBE Vector Stroke Offset" {
            offset = numeric_from_run(run)
                .and_then(|value| base_component(&value, 0))
                .unwrap_or(0.0);
        } else if (name.starts_with("ADBE Vector Stroke Dash")
            || name.starts_with("ADBE Vector Stroke Gap"))
            && let Some(value) = numeric_from_run(run)
                .and_then(|value| base_component(&value, 0))
                .and_then(NonNegativeProperty::new)
        {
            dashes.push(value);
        }
    }
    (dashes, offset)
}

fn rectangle(run: &[Chunk]) -> Result<ShapePath, String> {
    let leaves = property_group(run, "rectangle")?;
    let size = vector_leaf(&leaves, "ADBE Vector Rect Size", 2)?;
    let position =
        vector_leaf(&leaves, "ADBE Vector Rect Position", 2).unwrap_or_else(|_| vec![0.0, 0.0]);
    let half = [size[0] / 2.0, size[1] / 2.0];
    let points = [
        [position[0] + half[0], position[1] - half[1]],
        [position[0] + half[0], position[1] + half[1]],
        [position[0] - half[0], position[1] + half[1]],
        [position[0] - half[0], position[1] - half[1]],
    ];
    let radius = scalar_leaf_from(&leaves, "ADBE Vector Rect Roundness")
        .unwrap_or(0.0)
        .max(0.0);
    let mut commands = Vec::with_capacity(5);
    for (index, point) in points.into_iter().enumerate() {
        let command = if index == 0 {
            ShapePathCommand::MoveTo {
                x: point[0],
                y: point[1],
                mirror: None,
                corner_radius: if radius > 0.0 {
                    non_negative(radius)
                } else {
                    None
                },
            }
        } else {
            ShapePathCommand::LineTo {
                x: point[0],
                y: point[1],
                mirror: None,
                corner_radius: if radius > 0.0 {
                    non_negative(radius)
                } else {
                    None
                },
            }
        };
        commands.push(command);
    }
    commands.push(ShapePathCommand::Close);
    Ok(ShapePath { commands })
}

fn ellipse(run: &[Chunk]) -> Result<ShapeEllipse, String> {
    let leaves = property_group(run, "ellipse")?;
    let size = vector_leaf(&leaves, "ADBE Vector Ellipse Size", 2)?;
    let position = vector_leaf(&leaves, "ADBE Vector Ellipse Position", 2)?;
    Ok(ShapeEllipse {
        size: [size[0], size[1]],
        position: [position[0], position[1]],
        reversed: false,
    })
}

fn poly_star(run: &[Chunk]) -> Result<ShapePolyStar, String> {
    let leaves = property_group(run, "polystar")?;
    let kind = scalar_leaf_from(&leaves, "ADBE Vector Star Type")?;
    let position = vector_leaf(&leaves, "ADBE Vector Star Position", 2)?;
    Ok(ShapePolyStar {
        star_type: if kind == 2.0 {
            ShapePolyStarType::Polygon
        } else {
            ShapePolyStarType::Star
        },
        points: scalar_leaf_from(&leaves, "ADBE Vector Star Points")?,
        position: [position[0], position[1]],
        rotation: scalar_leaf_from(&leaves, "ADBE Vector Star Rotation")?,
        outer_radius: scalar_leaf_from(&leaves, "ADBE Vector Star Outer Radius")?,
        inner_radius: scalar_leaf_from(&leaves, "ADBE Vector Star Inner Radius").unwrap_or(0.0),
        outer_roundness: scalar_leaf_from(&leaves, "ADBE Vector Star Outer Roundess")
            .unwrap_or(0.0),
        inner_roundness: scalar_leaf_from(&leaves, "ADBE Vector Star Inner Roundess")
            .unwrap_or(0.0),
        reversed: false,
    })
}

fn offset_paths(run: &[Chunk]) -> Result<ShapeOffsetPaths, String> {
    let leaves = property_group(run, "Offset Paths")?;
    let join = scalar_leaf_from(&leaves, "ADBE Vector Offset Line Join").unwrap_or(1.0);
    Ok(ShapeOffsetPaths {
        amount: scalar_leaf_from(&leaves, "ADBE Vector Offset Amount")?,
        line_join: match join {
            2.0 => ShapeLineJoin::Round,
            3.0 => ShapeLineJoin::Bevel,
            _ => ShapeLineJoin::Miter,
        },
        miter_limit: scalar_leaf_from(&leaves, "ADBE Vector Offset Miter Limit").unwrap_or(4.0),
    })
}

fn trim_paths(run: &[Chunk]) -> Result<ShapeTrimPaths, String> {
    let leaves = property_group(run, "Trim Paths")?;
    let mode = scalar_leaf_from(&leaves, "ADBE Vector Trim Type").unwrap_or(1.0);
    Ok(ShapeTrimPaths {
        start: scalar_leaf_from(&leaves, "ADBE Vector Trim Start")?,
        end: scalar_leaf_from(&leaves, "ADBE Vector Trim End")?,
        offset: scalar_leaf_from(&leaves, "ADBE Vector Trim Offset")?,
        mode: if mode == 2.0 {
            ShapeTrimMode::Individually
        } else {
            ShapeTrimMode::Simultaneously
        },
    })
}

fn decode_transform(run: &[Chunk], warnings: &mut Vec<String>) -> Transform {
    let mut transform = identity_transform();
    let Ok(leaves) = property_group(run, "shape transform") else {
        warnings.push("shape-group Transform was malformed and identity was used".into());
        return transform;
    };
    if let Ok(value) = vector_leaf(&leaves, "ADBE Vector Anchor", 2) {
        transform.anchor_point = [value[0], value[1]];
    }
    if let Ok(value) = vector_leaf(&leaves, "ADBE Vector Position", 2) {
        transform.position = Position::TwoD([value[0], value[1]]);
    }
    if let Ok(value) = vector_leaf(&leaves, "ADBE Vector Scale", 2) {
        transform.scale = [value[0], value[1]];
    }
    if let Ok(value) = scalar_leaf_from(&leaves, "ADBE Vector Rotation") {
        transform.rotation = value;
    }
    if let Ok(value) = scalar_leaf_from(&leaves, "ADBE Vector Skew") {
        transform.skew = value;
    }
    if let Ok(value) = scalar_leaf_from(&leaves, "ADBE Vector Skew Axis") {
        transform.skew_axis = value;
    }
    if let Ok(value) = scalar_leaf_from(&leaves, "ADBE Vector Group Opacity")
        && let Some(value) = PercentageProperty::new(value.clamp(0.0, 100.0))
    {
        transform.opacity = value;
    }
    transform
}

fn property_group<'a>(
    run: &'a [Chunk],
    label: &str,
) -> Result<Vec<(&'a str, &'a [Chunk])>, String> {
    if let Ok(group) = unique_list(run, *b"tdgp") {
        return runs(group).map_err(|error| format!("{label} ignored: {error}"));
    }
    let direct = runs(run).map_err(|error| format!("{label} ignored: {error}"))?;
    if direct.is_empty() {
        return Err(format!("{label} ignored: missing property LIST"));
    }
    Ok(direct)
}

fn scalar_leaf(run: &[Chunk], name: &str) -> Result<f64, String> {
    let leaves = property_group(run, name)?;
    scalar_leaf_from(&leaves, name)
}

fn scalar_leaf_from(leaves: &[(&str, &[Chunk])], name: &str) -> Result<f64, String> {
    let Some((_, run)) = leaves.iter().find(|(candidate, _)| *candidate == name) else {
        return defaults::numeric(name)
            .and_then(|values| values.first().copied())
            .ok_or_else(|| format!("{name} is absent and has no known native default"));
    };
    let value = numeric(run, name)?;
    base_component(&value, 0).ok_or_else(|| format!("{name} has no decodable base value"))
}

fn vector_leaf(
    leaves: &[(&str, &[Chunk])],
    name: &str,
    dimensions: usize,
) -> Result<Vec<f64>, String> {
    let Some((_, run)) = leaves.iter().find(|(candidate, _)| *candidate == name) else {
        return defaults::numeric(name)
            .filter(|values| values.len() >= dimensions)
            .map(<[f64]>::to_vec)
            .ok_or_else(|| format!("{name} is absent and has no known native default"));
    };
    let value = numeric(run, name)?;
    let values = if value.values.len() >= dimensions {
        value.values.clone()
    } else {
        value
            .keyframes
            .first()
            .map(|key| key.values.clone())
            .unwrap_or_default()
    };
    (values.len() >= dimensions)
        .then_some(values)
        .ok_or_else(|| format!("{name} has fewer than {dimensions} values"))
}

fn numeric(run: &[Chunk], name: &str) -> Result<NumericProperty, String> {
    numeric_from_run(run).ok_or_else(|| format!("{name} ignored: malformed numeric property"))
}

fn numeric_from_run(run: &[Chunk]) -> Option<NumericProperty> {
    let list = unique_list(run, *b"tdbs").ok()?;
    read_numeric(list).ok()
}

fn numeric_leaf(
    leaves: &[(&str, &[Chunk])],
    name: &str,
    warnings: &mut Vec<String>,
) -> Option<NumericProperty> {
    let run = leaves.iter().find(|(candidate, _)| *candidate == name)?.1;
    match numeric(run, name) {
        Ok(value) => Some(value),
        Err(message) => {
            warnings.push(message);
            None
        }
    }
}

fn initial_scalar(value: &NumericProperty) -> Option<f64> {
    value
        .values
        .first()
        .copied()
        .or_else(|| value.keyframes.first()?.values.first().copied())
        .filter(|value| value.is_finite())
}

fn base_component(value: &NumericProperty, component: usize) -> Option<f64> {
    value.values.get(component).copied().or_else(|| {
        value
            .keyframes
            .first()
            .and_then(|key| key.values.get(component))
            .copied()
    })
}

fn non_negative(value: f64) -> Option<NonNegativeProperty> {
    NonNegativeProperty::new(value)
}

fn empty_path() -> ShapePath {
    ShapePath {
        commands: Vec::new(),
    }
}

fn is_shape_source(name: &str) -> bool {
    matches!(
        name,
        "ADBE Vector Shape - Group"
            | "ADBE Vector Shape - Rect"
            | "ADBE Vector Shape - Ellipse"
            | "ADBE Vector Shape - Star"
    )
}

fn identity_transform() -> Transform {
    Transform {
        anchor_point: [0.0, 0.0],
        position: Position::TwoD([0.0, 0.0]),
        scale: [100.0, 100.0],
        rotation: 0.0,
        skew: 0.0,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0, 0.0, 0.0],
        opacity: PercentageProperty::new(100.0).expect("100 is a valid percentage"),
    }
}

fn full_active_range() -> TimeRangeProperty {
    TimeRangeProperty::new(Time::ZERO, Duration::from_secs(super::MAX_TIME_SECS))
}

fn collect_shape_target_ids(layers: &[FxLayer], output: &mut Vec<LayerId>) {
    for layer in layers {
        match layer {
            FxLayer::Shape(layer) => output.push(layer.id),
            FxLayer::BooleanOperation(layer) => output.push(layer.id),
            FxLayer::Rect(layer) => output.push(layer.id),
            FxLayer::Group(layer) => {
                for child in &layer.layers {
                    collect_shape_target_ids(std::slice::from_ref(child.data()), output);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    mod stroke_color;
    use super::*;
    use crate::structure::{ItemKind, read_project};
    use fx_schema::PropertyValue;

    #[test]
    fn shape_helpers_share_generated_id_counter_range() {
        let mut next_id = u64::MAX - 1;
        let mut budget = AnimationBudget::default();
        let mut collector = Collector {
            includes_occurrence_pipeline: true,
            next_id: &mut next_id,
            animation_budget: &mut budget,
            animations: Vec::new(),
            warnings: Vec::new(),
            frame_fade_lowered: false,
            evaluated_shapes: Default::default(),
            mapped_expressions: Vec::new(),
        };
        assert!(collector.allocate().is_some());
        assert!(collector.allocate().is_none());
        assert!(collector.allocate().is_none());
        assert_eq!(*collector.next_id, u64::MAX);
        assert_eq!(collector.warnings.len(), 1);
    }

    #[test]
    fn failed_dynamic_path_retains_initial_outline_and_static_fallback_warning() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/shapes/shape_basic.aep"
        ))
        .unwrap();
        let (owner, composition, path_run) = project
            .items
            .iter()
            .find_map(|item| {
                let ItemKind::Composition(composition) = &item.kind else {
                    return None;
                };
                composition.layers.iter().find_map(|layer| {
                    let roots = root_runs(&layer.content).ok()?;
                    let parade = roots
                        .into_iter()
                        .find(|(name, _)| *name == "ADBE Mask Parade")?;
                    let atom = runs(unique_list(parade.1, *b"tdgp").ok()?)
                        .ok()?
                        .into_iter()
                        .find(|(name, _)| *name == "ADBE Mask Atom")?;
                    let path_run = runs(unique_list(atom.1, *b"tdgp").ok()?)
                        .ok()?
                        .into_iter()
                        .find(|(name, _)| *name == "ADBE Mask Shape")?
                        .1;
                    Some((layer, composition.as_ref(), path_run))
                })
            })
            .expect("native fixture has a mask Path");
        let mut path_run = path_run.to_vec();
        fn attach_unsupported_copy(chunks: &mut [Chunk]) -> bool {
            for chunk in chunks {
                if chunk.list_kind() == Some(*b"tdbs") {
                    let body = chunk.children_mut().unwrap();
                    if let Some(index) = body.iter().position(|child| child.id() == *b"tdb4") {
                        let mut meta = body[index].data_payload().unwrap().to_vec();
                        meta[119] = 0;
                        meta[120] |= 1;
                        body[index] = test_data(b"tdb4", meta);
                        body.push(test_data(
                            b"Utf8",
                            br#"thisComp.layer("Missing Source").content("Shape 1").content("Path 1").path;"#,
                        ));
                        return true;
                    }
                }
                if let Some(children) = chunk.children_mut()
                    && attach_unsupported_copy(children)
                {
                    return true;
                }
            }
            false
        }
        assert!(attach_unsupported_copy(&mut path_run));
        let mut next_id = 10;
        let mut budget = AnimationBudget::default();
        let mut collector = Collector {
            includes_occurrence_pipeline: true,
            next_id: &mut next_id,
            animation_budget: &mut budget,
            animations: Vec::new(),
            warnings: Vec::new(),
            frame_fade_lowered: false,
            evaluated_shapes: Default::default(),
            mapped_expressions: Vec::new(),
        };
        let source_items: HashMap<u32, &ProjectItem> =
            project.items.iter().map(|item| (item.id, item)).collect();
        let layer = collector
            .source_layer_with_sources(
                "ADBE Mask Shape",
                &path_run,
                "Path",
                LayerId::new(1),
                None,
                Some((owner, composition)),
                Some(&source_items),
            )
            .unwrap()
            .expect("initial editable Path is retained");
        assert!(matches!(layer, FxLayer::Shape(shape) if !shape.shape.path.commands.is_empty()));
        assert!(collector.animations.is_empty());
        assert!(
            collector
                .warnings
                .iter()
                .any(|warning| warning.contains("Path expression not lowered"))
        );
        assert!(collector.warnings.iter().any(|warning| {
            warning.contains("Path animation omitted; initial static editable path retained")
        }));
    }

    #[test]
    fn omitted_vector_defaults_do_not_hide_malformed_present_values() {
        assert_eq!(
            scalar_leaf_from(&[], "ADBE Vector Fill Opacity").unwrap(),
            100.0
        );
        assert_eq!(
            vector_leaf(&[], "ADBE Vector Rect Size", 2).unwrap(),
            [100.0, 100.0]
        );
        assert_eq!(
            vector_leaf(&[], "ADBE Vector Grad End Pt", 2).unwrap(),
            [100.0, 0.0]
        );
        let malformed = [Chunk::data(*b"tdb4", vec![0]).unwrap()];
        let leaves = [("ADBE Vector Fill Opacity", malformed.as_slice())];
        assert!(scalar_leaf_from(&leaves, "ADBE Vector Fill Opacity").is_err());
    }

    fn test_data(id: &[u8; 4], value: impl Into<Vec<u8>>) -> Chunk {
        Chunk::data(*id, value.into()).unwrap()
    }

    fn test_list(kind: &[u8; 4], children: Vec<Chunk>) -> Chunk {
        Chunk::list(*kind, children)
    }

    fn test_match_name(value: &str) -> Chunk {
        let mut bytes = value.as_bytes().to_vec();
        bytes.resize(40, 0);
        test_data(b"tdmn", bytes)
    }

    fn test_name(value: &str) -> Chunk {
        let mut bytes = vec![0; 8 + value.len()];
        bytes[..4].copy_from_slice(b"Utf8");
        bytes[4..8].copy_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
        bytes[8..].copy_from_slice(value.as_bytes());
        test_data(b"tdsn", bytes)
    }

    fn test_numeric(values: &[f64], expression: &str) -> Chunk {
        let mut meta = vec![0; 124];
        meta[..2].copy_from_slice(&[0xdb, 0x99]);
        meta[3] = u8::try_from(values.len()).unwrap();
        test_list(
            b"tdbs",
            vec![
                test_data(b"tdb4", meta),
                test_data(b"tdsb", [0, 0, 0, 1]),
                test_data(
                    b"cdat",
                    values
                        .iter()
                        .flat_map(|value| value.to_be_bytes())
                        .collect::<Vec<_>>(),
                ),
            ]
            .into_iter()
            .chain((!expression.is_empty()).then(|| test_data(b"Utf8", expression.as_bytes())))
            .collect(),
        )
    }

    fn property_storage(chunks: &[Chunk], target: &str) -> Option<Vec<Chunk>> {
        for pair in chunks.windows(2) {
            if pair[0]
                .data_payload()
                .is_some_and(|payload| payload.starts_with(target.as_bytes()))
                && pair[1].list_kind() == Some(*b"tdbs")
            {
                return pair[1].children().map(<[Chunk]>::to_vec);
            }
        }
        chunks.iter().find_map(|chunk| {
            chunk
                .children()
                .and_then(|children| property_storage(children, target))
        })
    }

    fn replace_property(chunks: &mut [Chunk], target: &str, replacement: Chunk) -> bool {
        for index in 0..chunks.len().saturating_sub(1) {
            if chunks[index]
                .data_payload()
                .is_some_and(|payload| payload.starts_with(target.as_bytes()))
                && chunks[index + 1].list_kind() == Some(*b"tdbs")
            {
                chunks[index + 1] = replacement;
                return true;
            }
        }
        chunks.iter_mut().any(|chunk| {
            chunk
                .children_mut()
                .is_some_and(|children| replace_property(children, target, replacement.clone()))
        })
    }

    fn test_color(values: [f64; 4], expression: &str) -> Chunk {
        let mut property = test_numeric(&values, expression);
        let meta = property
            .children_mut()
            .unwrap()
            .iter_mut()
            .find(|chunk| chunk.id() == *b"tdb4")
            .unwrap();
        let mut bytes = meta.data_payload().unwrap().to_vec();
        bytes[59] = 1;
        *meta = test_data(b"tdb4", bytes);
        property
    }

    fn color_alias_probe(expression: &str, source_expression: &str) -> (Composition, Layer) {
        let (mut composition, mut owner) = probe_rectangle();
        // Supplementary minimal native chunks; the licensed original source is
        // tested separately and is not replaced by this synthetic route probe.
        owner.content = vec![test_list(
            b"tdgp",
            vec![
                test_match_name("ADBE Root Vectors Group"),
                test_list(
                    b"tdgp",
                    vec![
                        test_match_name("ADBE Vector Shape - Rect"),
                        test_list(
                            b"tdgp",
                            vec![
                                test_match_name("ADBE Vector Rect Size"),
                                test_numeric(&[1920.0, 1080.0], ""),
                            ],
                        ),
                        test_match_name("ADBE Vector Graphic - Fill"),
                        test_list(
                            b"tdgp",
                            vec![
                                test_match_name("ADBE Vector Fill Color"),
                                test_color([255.0, 255.0, 0.0, 0.0], expression),
                            ],
                        ),
                    ],
                ),
            ],
        )];
        let mut controller = owner.clone();
        controller.name = "Color Controller".into();
        controller.content = vec![test_list(
            b"tdgp",
            vec![
                test_match_name("ADBE Effect Parade"),
                test_list(
                    b"tdgp",
                    vec![
                        test_match_name("ADBE Color Control"),
                        test_list(
                            b"sspc",
                            vec![test_list(
                                b"tdgp",
                                vec![
                                    test_name("Background"),
                                    test_match_name("ADBE Color Control-0001"),
                                    test_color([255.0, 0.0, 0.0, 0.0], source_expression),
                                ],
                            )],
                        ),
                    ],
                ),
            ],
        )];
        composition.layers = vec![controller];
        (composition, owner)
    }

    #[test]
    fn static_sibling_color_control_alias_keeps_editable_rect_fill() {
        let (composition, owner) = color_alias_probe(
            "thisComp.layer(\"Color Controller\").effect(\"Background\")(\"Color\")",
            "",
        );
        let imported = import_owner(&owner, &composition);
        assert_eq!(
            imported_rect(&imported.layers).unwrap().rect.fill_color,
            [0.0, 0.0, 0.0, 1.0]
        );
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("live controller linkage is lost"))
        );
        assert!(
            imported.animations.is_empty(),
            "a static alias must not create animation or scripts"
        );
    }

    #[test]
    fn numeric_color_control_index_one_keeps_editable_rect_fill() {
        let indexed = "thisComp.layer(\"Color Controller\").effect(\"Background\")(1)";
        let (composition, owner) = color_alias_probe(indexed, "");
        let imported = import_owner(&owner, &composition);
        assert_eq!(
            imported_rect(&imported.layers).unwrap().rect.fill_color,
            [0.0, 0.0, 0.0, 1.0]
        );
        assert!(imported.animations.is_empty());

        for selector in ["0", "2", "1.0", "1 + 0", "\"1\""] {
            let expression = indexed.replace("(1)", &format!("({selector})"));
            let (composition, owner) = color_alias_probe(&expression, "");
            let imported = import_owner(&owner, &composition);
            assert_eq!(
                imported_rect(&imported.layers).unwrap().rect.fill_color,
                [1.0, 0.0, 0.0, 1.0]
            );
        }
    }

    fn pseudo_color_alias_probe(source_expression: &str) -> (Composition, Layer) {
        let (mut composition, owner) = color_alias_probe(
            "thisComp.layer(\"Color Controller\").effect(\"Background\")(\"Texts Color\")",
            "",
        );
        let mut color = test_color([255.0, 0.0, 0.0, 0.0], source_expression);
        color
            .children_mut()
            .unwrap()
            .insert(0, test_name("Texts Color"));
        composition.layers[0].content = vec![test_list(
            b"tdgp",
            vec![
                test_match_name("ADBE Effect Parade"),
                test_list(
                    b"tdgp",
                    vec![
                        test_match_name("Pseudo/NX291ee23e92k"),
                        test_list(
                            b"sspc",
                            vec![test_list(
                                b"tdgp",
                                vec![
                                    test_name("Background"),
                                    test_match_name("Pseudo/NX291ee23e92k-0001"),
                                    color,
                                    test_match_name("ADBE Effect Built In Params"),
                                    test_list(b"tdgp", Vec::new()),
                                ],
                            )],
                        ),
                    ],
                ),
            ],
        )];
        (composition, owner)
    }

    #[test]
    fn static_pseudo_color_alias_keeps_editable_rect_fill() {
        let (composition, owner) = pseudo_color_alias_probe("");
        let imported = import_owner(&owner, &composition);
        assert_eq!(
            imported_rect(&imported.layers).unwrap().rect.fill_color,
            [0.0, 0.0, 0.0, 1.0]
        );
        assert!(imported.animations.is_empty());
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("live controller linkage is lost"))
        );
    }

    #[test]
    fn ambiguous_or_non_color_pseudo_alias_keeps_authored_fill() {
        for duplicate in [false, true] {
            let (mut composition, owner) = pseudo_color_alias_probe("");
            let root = composition.layers[0].content[0].children_mut().unwrap();
            let parade = root[1].children_mut().unwrap();
            let plugin = parade[1].children_mut().unwrap();
            let body = plugin[0].children_mut().unwrap();
            if duplicate {
                let color = body[2].clone();
                body.push(test_match_name("Pseudo/NX291ee23e92k-0002"));
                body.push(color);
            } else {
                let storage = body[2].children_mut().unwrap();
                let meta = storage
                    .iter_mut()
                    .find(|chunk| chunk.id() == *b"tdb4")
                    .unwrap();
                let mut bytes = meta.data_payload().unwrap().to_vec();
                bytes[59] = 0;
                *meta = test_data(b"tdb4", bytes);
            }
            let imported = import_owner(&owner, &composition);
            assert_eq!(
                imported_rect(&imported.layers).unwrap().rect.fill_color,
                [1.0, 0.0, 0.0, 1.0]
            );
            assert!(
                imported
                    .warnings
                    .iter()
                    .any(|warning| warning.contains(if duplicate {
                        "ambiguous pseudo Color Control parameter"
                    } else {
                        "native color storage"
                    }))
            );
        }
    }

    #[test]
    fn expression_backed_pseudo_color_alias_keeps_authored_fill() {
        let (composition, owner) = pseudo_color_alias_probe("value * 0.5");
        let imported = import_owner(&owner, &composition);
        assert_eq!(
            imported_rect(&imported.layers).unwrap().rect.fill_color,
            [1.0, 0.0, 0.0, 1.0]
        );
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("static and expression-free"))
        );
    }

    #[test]
    fn tint_color_alias_uses_occurrence_composition_without_changing_named_comp_scope() {
        use sha2::{Digest, Sha256};

        // Supplementary occurrence-context regression built on a native probe.
        // The pinned Intro source remains separate native-source evidence.
        let (mut original, mut owner) = color_alias_probe("", "");
        assert!(replace_property(
            &mut original.layers[0].content,
            "ADBE Color Control-0001",
            test_color([255.0, 0.0, 0.0, 255.0], ""),
        ));
        let tint = test_list(
            b"tdgp",
            vec![
                test_match_name("ADBE Tint"),
                test_list(
                    b"sspc",
                    vec![
                        test_data(b"tdsb", [0, 0, 0, 1]),
                        test_list(
                            b"tdgp",
                            vec![
                                test_match_name("ADBE Tint-0001"),
                                test_color([255.0, 255.0, 0.0, 0.0], ""),
                                test_match_name("ADBE Tint-0002"),
                                test_color([255.0; 4], ""),
                                test_match_name("ADBE Tint-0003"),
                                test_numeric(&[100.0], ""),
                            ],
                        ),
                    ],
                ),
            ],
        );
        owner.content[0]
            .children_mut()
            .unwrap()
            .extend([test_match_name("ADBE Effect Parade"), tint]);
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/geometry/geometry_probe.aep"
        ))
        .unwrap();
        let item = project.items.iter_mut().find(|item| item.id == 14).unwrap();
        item.name = "Original color composition".into();
        item.kind = ItemKind::Composition(Box::new(original.clone()));
        let items = project.items.iter().map(|item| (item.id, item)).collect();
        let mut local = original.clone();
        assert!(replace_property(
            &mut local.layers[0].content,
            "ADBE Color Control-0001",
            test_color([255.0, 0.0, 255.0, 0.0], ""),
        ));
        let mut nonopaque = local.clone();
        assert!(replace_property(
            &mut nonopaque.layers[0].content,
            "ADBE Color Control-0001",
            test_color([127.0, 0.0, 255.0, 0.0], ""),
        ));
        let mut expressed = local.clone();
        assert!(replace_property(
            &mut expressed.layers[0].content,
            "ADBE Color Control-0001",
            test_color([255.0, 0.0, 255.0, 0.0], "[0, 1, 0, 1]"),
        ));
        for (composition, scope, expected, rejected) in [
            (&local, "thisComp", [0.0, 1.0, 0.0], false),
            (&original, "thisComp", [0.0, 0.0, 1.0], false),
            (
                &local,
                "comp(\"Original color composition\")",
                [0.0, 0.0, 1.0],
                false,
            ),
            (&nonopaque, "thisComp", [1.0, 0.0, 0.0], true),
            (&expressed, "thisComp", [1.0, 0.0, 0.0], true),
        ] {
            let expression = format!(
                "{scope}.layer(\"Color Controller\").effect(\"Background\")(\"ADBE Color Control-0001\")"
            );
            assert!(replace_property(
                &mut owner.content,
                "ADBE Tint-0001",
                test_color([255.0, 255.0, 0.0, 0.0], &expression),
            ));
            let imported = super::super::effects::import_with_context(
                super::super::effects::ImportContext {
                    evaluations: &crate::expression_samples::ExpressionSamples::default(),
                    composition_id: 14,
                    composition: Some(composition),
                    items: Some(&items),
                },
                &owner,
                [1920, 1080],
                [1920, 1080],
                &mut 10_000,
                &mut AnimationBudget::default(),
            );
            assert_eq!(imported.effects.len(), 1, "{:?}", imported.warnings);
            let effect = serde_json::to_value(&imported.effects[0]).unwrap();
            assert_eq!(effect["effect"]["type"], "tintTritone");
            for (field, value) in ["blackR", "blackG", "blackB"].into_iter().zip(expected) {
                assert_eq!(
                    effect["effect"][field], value,
                    "scope {scope}, expected {expected:?}, effect {effect}, warnings {:?}",
                    imported.warnings,
                );
            }
            assert_eq!(effect["effect"]["amount"], 100.0);
            assert!(imported.animations.is_empty());

            // Exercise the actual Converter plumbing, not only ImportContext.
            // The borrowed project map stays blue while this occurrence is green.
            let mut occurrence = project.item(14).unwrap().clone();
            let mut occurrence_comp = composition.clone();
            occurrence_comp.layers.push(owner.clone());
            occurrence.kind = ItemKind::Composition(Box::new(occurrence_comp));
            let samples = crate::expression_samples::ExpressionSamples::default();
            let mut resolver =
                |_: &super::super::MediaAssetRequest| super::super::MediaResolution::Unavailable;
            let mut converter = super::super::Converter {
                expression_samples: &samples,
                items: project.items.iter().map(|item| (item.id, item)).collect(),
                camera_normalizations: Default::default(),
                diagnostics: Vec::new(),
                next_id: 10_000,
                linked: false,
                asset_namespace: super::super::AssetNamespace::STANDALONE,
                stack: Vec::new(),
                visited_compositions: Default::default(),
                animations: Vec::new(),
                animation_budget: Default::default(),
                committed_inline_remap_bytes: 0,
                unavailable_cutouts: 0,
                overrides: Vec::new(),
                media_resolver: &mut resolver,
                assets: Vec::new(),
                shape_budget: Default::default(),
                mapped_shape_expressions: Default::default(),
                root_progress: fx_conv::Progress::default().phase("alias occurrence", "layers", 0),
            };
            let layers = converter
                .composition_layers(&occurrence, LayerId::from(9_999), 0)
                .unwrap();
            fn find_tint(value: &serde_json::Value) -> Option<&serde_json::Value> {
                match value {
                    serde_json::Value::Object(fields) => {
                        if fields.get("type").and_then(serde_json::Value::as_str)
                            == Some("tintTritone")
                        {
                            Some(value)
                        } else {
                            fields.values().find_map(find_tint)
                        }
                    }
                    serde_json::Value::Array(values) => values.iter().find_map(find_tint),
                    _ => None,
                }
            }
            let converted = serde_json::to_value(&layers).unwrap();
            let tint = find_tint(&converted).expect("Converter retains editable Tint");
            for (field, value) in ["blackR", "blackG", "blackB"].into_iter().zip(expected) {
                assert_eq!(tint[field], value, "Converter scope {scope}");
            }
            assert_eq!(
                imported
                    .warnings
                    .iter()
                    .any(|warning| { warning.contains("static Color Control alias not lowered") }),
                rejected,
                "{:?}",
                imported.warnings,
            );
        }

        // Synthetic sidecar samples retain precedence over the local alias.
        let source = include_bytes!("../../tests/fixtures/geometry/geometry_probe.aep");
        let sidecar = serde_json::json!({
            "version": 2,
            "source_sha256": format!("{:x}", Sha256::digest(source)),
            "sample_interval_ms": 1,
            "capture_scope": {"mode": "selected_composition", "root_composition_id": 14},
            "properties": [{
                "composition_id": 14,
                "layer_id": owner.record.id(),
                "property": {"kind": "effect", "index": 1, "match_name": "ADBE Tint-0001"},
                "start_ms": 0,
                "sample_times_seconds": [0.0, 0.001],
                "values": [[1.0, 1.0, 0.0, 1.0], [1.0, 1.0, 0.0, 1.0]],
            }],
            "errors": [],
        });
        let samples = crate::expression_samples::ExpressionSamples::from_json_for_source(
            &serde_json::to_vec(&sidecar).unwrap(),
            source,
        )
        .unwrap();
        let sampled = super::super::effects::import_with_context(
            super::super::effects::ImportContext {
                evaluations: &samples,
                composition_id: 14,
                composition: Some(&local),
                items: Some(&items),
            },
            &owner,
            [1920, 1080],
            [1920, 1080],
            &mut 10_000,
            &mut AnimationBudget::default(),
        );
        let effect = serde_json::to_value(&sampled.effects[0]).unwrap();
        // The authored red remains the base value; captured yellow is carried
        // by editable animation keys, not copied into the static effect fields.
        assert_eq!(effect["effect"]["blackR"], 1.0);
        assert_eq!(effect["effect"]["blackG"], 0.0);
        assert_eq!(effect["effect"]["blackB"], 0.0);
        let fx_schema::EffectData::Identified { id, .. } = sampled.effects[0].data() else {
            panic!("imported Tint has an editable effect identity");
        };
        for (field, expected) in [("blackR", 1.0), ("blackG", 1.0), ("blackB", 0.0)] {
            let target = fx_schema::PropertyTarget::effect_param(*id, field);
            let entry = sampled
                .animations
                .iter()
                .find(|entry| entry.target == target)
                .expect("captured color has editable channel keys");
            let track = entry
                .animator
                .keyframe_track()
                .expect("captured color keyframes");
            assert!(!track.keyframes().is_empty());
            for key in track.keyframes() {
                assert_eq!(key.value(), &fx_schema::PropertyValue::Float(expected));
            }
        }
        assert!(
            !sampled
                .warnings
                .iter()
                .any(|warning| { warning.contains("static Color Control alias copied") })
        );
    }

    #[test]
    fn static_cross_comp_color_control_alias_keeps_editable_effect_fill() {
        let (source_composition, mut owner) = color_alias_probe("", "");
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/geometry/geometry_probe.aep"
        ))
        .unwrap();
        let mut source = project.item(14).unwrap().clone();
        source.id = 99_999;
        source.name = "Color Source".into();
        source.kind = ItemKind::Composition(Box::new(source_composition));
        project.items.push(source);
        let effect = test_list(
            b"tdgp",
            vec![
                test_match_name("ADBE Fill"),
                test_list(
                    b"sspc",
                    vec![
                        test_data(b"tdsb", [0, 0, 0, 1]),
                        test_list(
                            b"tdgp",
                            vec![
                                test_match_name("ADBE Fill-0002"),
                                test_color([255.0, 255.0, 0.0, 0.0], ""),
                            ],
                        ),
                    ],
                ),
            ],
        );
        owner
            .content
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap()
            .extend([test_match_name("ADBE Effect Parade"), effect]);
        assert!(replace_property(
            &mut owner.content,
            "ADBE Fill-0002",
            test_color(
                [255.0, 255.0, 0.0, 0.0],
                "comp(\"Color Source\").layer(\"Color Controller\").effect(\"Background\")(\"ADBE Color Control-0001\")",
            )
        ));
        let ItemKind::Composition(composition) = &mut project
            .items
            .iter_mut()
            .find(|item| item.id == 14)
            .unwrap()
            .kind
        else {
            panic!("probe composition")
        };
        composition.layers = vec![owner.clone()];
        let ItemKind::Composition(composition) = &project.item(14).unwrap().kind else {
            panic!("probe composition")
        };
        let items = project.items.iter().map(|item| (item.id, item)).collect();
        let imported = super::super::effects::import_with_context(
            super::super::effects::ImportContext {
                evaluations: &crate::expression_samples::ExpressionSamples::default(),
                composition_id: 14,
                composition: Some(composition),
                items: Some(&items),
            },
            &owner,
            [1920, 1080],
            [1920, 1080],
            &mut 10_000,
            &mut AnimationBudget::default(),
        );
        let effect = serde_json::to_value(&imported.effects[0]).unwrap();
        assert_eq!(effect["effect"]["blackR"], 0.0);
        assert_eq!(effect["effect"]["blackG"], 0.0);
        assert_eq!(effect["effect"]["blackB"], 0.0);
        assert_eq!(effect["effect"]["whiteR"], 0.0);
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("live controller linkage is lost")),
            "{:?}",
            imported.warnings
        );
        assert!(imported.animations.is_empty());
    }

    #[test]
    fn unsupported_sibling_color_control_alias_keeps_authored_fill_and_diagnostic() {
        let direct = "thisComp.layer(\"Color Controller\").effect(\"Background\")(\"Color\")";
        for (expression, source_expression, duplicate) in [
            (format!("{direct} + [1,0,0,0]"), "", false),
            (direct.into(), "value", false),
            (direct.into(), "", true),
        ] {
            let (mut composition, owner) = color_alias_probe(&expression, source_expression);
            if duplicate {
                composition.layers.push(composition.layers[0].clone());
            }
            let imported = import_owner(&owner, &composition);
            assert_eq!(
                imported_rect(&imported.layers).unwrap().rect.fill_color,
                [1.0, 0.0, 0.0, 1.0]
            );
            assert!(
                imported
                    .warnings
                    .iter()
                    .any(|warning| warning
                        .contains("unsupported expression retained at authored color")),
                "{:?}",
                imported.warnings
            );
        }
    }

    /// One Slider Control effect named `label` whose value leaf is `storage`.
    fn slider_effect(label: &str, mut storage: Vec<Chunk>) -> Vec<Chunk> {
        storage.retain(|chunk| chunk.id() != *b"tdsn");
        storage.push(test_name("Slider"));
        vec![
            test_match_name("ADBE Slider Control"),
            test_list(
                b"sspc",
                vec![test_list(
                    b"tdgp",
                    vec![
                        test_name(label),
                        test_match_name("ADBE Slider Control-0001"),
                        test_list(b"tdbs", storage),
                    ],
                )],
            ),
        ]
    }

    fn slider_controller(mut layer: Layer, scalar: Vec<Chunk>) -> Layer {
        layer.name = "Shape Controller".into();
        layer.content = vec![test_list(
            b"tdgp",
            vec![
                test_match_name("ADBE Effect Parade"),
                test_list(b"tdgp", slider_effect("Size", scalar)),
            ],
        )];
        layer
    }

    fn import_controlled_shape(
        project: &mut crate::structure::StructuralProject,
        composition_id: u32,
        property: &str,
        replacement: Chunk,
        scalar: &[Chunk],
    ) -> ShapeImport {
        let composition = project
            .items
            .iter_mut()
            .find_map(|item| match &mut item.kind {
                ItemKind::Composition(composition) if item.id == composition_id => {
                    Some(composition)
                }
                _ => None,
            })
            .unwrap();
        let owner_index = composition
            .layers
            .iter_mut()
            .position(|layer| replace_property(&mut layer.content, property, replacement.clone()))
            .unwrap();
        let controller =
            slider_controller(composition.layers[owner_index].clone(), scalar.to_vec());
        composition.layers.push(controller);
        let parent =
            super::super::group(LayerId::new(1), "source".into(), None, full_active_range());
        import_with_composition(
            &composition.layers[owner_index],
            composition,
            &HashMap::new(),
            true,
            &parent,
            8,
            &mut 10_000,
            &mut OutputBudget::default(),
            &mut AnimationBudget::default(),
        )
        .unwrap()
    }

    fn stored_shape(layers: &[fx_schema::Layer]) -> Option<&ShapeLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            FxLayer::Shape(shape)
                if shape.shape.ellipse.is_some() || !shape.shape.strokes.is_empty() =>
            {
                Some(shape)
            }
            FxLayer::Group(group) => stored_shape(&group.layers),
            _ => None,
        })
    }

    fn imported_shape(layers: &[FxLayer]) -> Option<&ShapeLayer> {
        layers.iter().find_map(|layer| match layer {
            FxLayer::Shape(shape)
                if shape.shape.ellipse.is_some() || !shape.shape.strokes.is_empty() =>
            {
                Some(shape)
            }
            FxLayer::Group(group) => stored_shape(&group.layers),
            _ => None,
        })
    }

    fn stored_stroke_width(layers: &[fx_schema::Layer]) -> Option<f64> {
        layers.iter().find_map(|layer| match layer.data() {
            FxLayer::Shape(shape) => shape
                .shape
                .strokes
                .first()
                .map(|stroke| stroke.width.value()),
            FxLayer::Rect(rect) if rect.rect.stroke_enabled => Some(rect.rect.stroke_width.value()),
            FxLayer::Group(group) => stored_stroke_width(&group.layers),
            _ => None,
        })
    }

    fn imported_stroke_width(layers: &[FxLayer]) -> Option<f64> {
        layers.iter().find_map(|layer| match layer {
            FxLayer::Shape(shape) => shape
                .shape
                .strokes
                .first()
                .map(|stroke| stroke.width.value()),
            FxLayer::Rect(rect) if rect.rect.stroke_enabled => Some(rect.rect.stroke_width.value()),
            FxLayer::Group(group) => stored_stroke_width(&group.layers),
            _ => None,
        })
    }

    #[test]
    fn linked_stroke_width_does_not_change_sibling_paint() {
        let project = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/properties/property_1D_opacity.aep"
        ))
        .unwrap();
        let mut composition = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(c) => Some(c.clone()),
                _ => None,
            })
            .unwrap();
        let owner = composition.layers[0].clone();
        let scalar = test_numeric(&[25.0], "");
        composition.layers.push(slider_controller(
            owner.clone(),
            scalar.children().unwrap().to_vec(),
        ));
        let linked = vec![test_list(
            b"tdgp",
            vec![
                test_match_name("ADBE Vector Stroke Width"),
                test_numeric(
                    &[5.0],
                    "thisComp.layer('Shape Controller').effect('Size')('Slider')",
                ),
            ],
        )];
        let ordinary = vec![test_list(
            b"tdgp",
            vec![
                test_match_name("ADBE Vector Stroke Width"),
                test_numeric(&[9.0], ""),
            ],
        )];
        let mut next_id = 1;
        let mut budget = AnimationBudget::default();
        let mut collector = Collector {
            includes_occurrence_pipeline: true,
            next_id: &mut next_id,
            animation_budget: &mut budget,
            animations: Vec::new(),
            warnings: Vec::new(),
            frame_fade_lowered: false,
            evaluated_shapes: Default::default(),
            mapped_expressions: Vec::new(),
        };
        let result = collector.decorations(
            &[
                ("ADBE Vector Graphic - Stroke", &linked),
                ("ADBE Vector Graphic - Stroke", &ordinary),
            ],
            Some((&owner, &composition)),
        );
        assert_eq!(result.strokes.len(), 2);
        assert_eq!(result.strokes[0].width, non_negative(25.0).unwrap());
        assert_eq!(result.strokes[1].width, non_negative(9.0).unwrap());
    }

    #[test]
    fn ordinary_non_square_ellipse_keeps_independent_dimensions() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/shapes/import_shape_control_animation.aep"
        ))
        .unwrap();
        let composition = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) if item.id == 167 => Some(composition),
                _ => None,
            })
            .unwrap();
        let layer = composition
            .layers
            .iter()
            .find(|layer| property_storage(&layer.content, "ADBE Vector Ellipse Size").is_some())
            .unwrap();
        let parent =
            super::super::group(LayerId::new(1), "source".into(), None, full_active_range());
        let imported = import_with_composition(
            layer,
            composition,
            &HashMap::new(),
            true,
            &parent,
            8,
            &mut 10_000,
            &mut OutputBudget::default(),
            &mut AnimationBudget::default(),
        )
        .unwrap();
        assert_eq!(
            imported_shape(&imported.layers)
                .unwrap()
                .shape
                .ellipse
                .as_ref()
                .unwrap()
                .size,
            [200.0, 120.0]
        );
    }

    #[test]
    fn sibling_slider_drives_editable_ellipse_size_and_stroke_width() {
        let bytes =
            include_bytes!("../../tests/fixtures/shapes/import_shape_control_animation.aep");
        let mut project = read_project(bytes).unwrap();
        let scalar = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) if item.id == 47 => composition
                    .layers
                    .iter()
                    .find_map(|layer| property_storage(&layer.content, "ADBE Vector Stroke Width")),
                _ => None,
            })
            .unwrap();
        let vector_expression = "temp = thisComp.layer(\"Shape Controller\").effect(\"Size\")(\"Slider\");\r[temp, temp]";
        let ellipse = import_controlled_shape(
            &mut project,
            167,
            "ADBE Vector Ellipse Size",
            test_numeric(&[500.0, 500.0], vector_expression),
            &scalar,
        );
        let ellipse_shape = imported_shape(&ellipse.layers).unwrap();
        assert_eq!(
            ellipse_shape.shape.ellipse.as_ref().unwrap().size,
            [4.0, 4.0],
            "{:?}",
            ellipse.warnings
        );
        assert!(
            ellipse.animations.iter().any(|entry| {
                entry.target.as_property().is_some_and(|target| {
                    target.property_type() == PropType::EllipseSize
                        && entry.animator.keyframe_track().is_some_and(|track| {
                            let keys = track.keyframes();
                            keys.first().is_some_and(|key| {
                                key.value() == &PropertyValue::Vector2([4.0, 4.0])
                            }) && keys.last().is_some_and(|key| {
                                key.value() == &PropertyValue::Vector2([30.0, 30.0])
                            })
                        })
                })
            }),
            "targets={:?} warnings={:?}",
            ellipse
                .animations
                .iter()
                .map(|entry| &entry.target)
                .collect::<Vec<_>>(),
            ellipse.warnings
        );
        assert!(ellipse.warnings.iter().any(|warning| {
            warning.contains("ADBE Vector Ellipse Size")
                && warning.contains("controller edit linkage is not retained")
        }));

        let mut project = read_project(bytes).unwrap();
        let stroke = import_controlled_shape(
            &mut project,
            47,
            "ADBE Vector Stroke Width",
            test_numeric(
                &[2.0],
                "thisComp.layer(\"Shape Controller\").effect(\"Size\")(\"Slider\")",
            ),
            &scalar,
        );
        assert_eq!(imported_stroke_width(&stroke.layers), Some(4.0));
        assert!(stroke.animations.iter().any(|entry| {
            entry.target.as_property().is_some_and(|target| {
                target.property_type() == PropType::StrokeWidth
                    && entry.animator.keyframe_track().is_some_and(|track| {
                        let keys = track.keyframes();
                        keys.first()
                            .is_some_and(|key| key.value() == &PropertyValue::Float(4.0))
                            && keys
                                .last()
                                .is_some_and(|key| key.value() == &PropertyValue::Float(30.0))
                    })
            })
        }));
        assert!(stroke.warnings.iter().any(|warning| {
            warning.contains("ADBE Vector Stroke Width")
                && warning.contains("controller edit linkage is not retained")
        }));
    }

    const SLIDER_RECT_SIZE: &str =
        "[effect(\"Width\")(\"Slider\"), effect(\"Height\")(\"Slider\")]";
    const SLIDER_RECT_POSITION: &str = "[effect(\"X\")(\"Slider\"), effect(\"Y\")(\"Slider\")]";
    const SLIDER_RECT_ROUNDNESS: &str = "effect(\"Radius\")(\"Slider\")";

    /// A static Slider value leaf, expression-driven unless `expression` is empty.
    fn slider_value(value: f64, expression: &str) -> Vec<Chunk> {
        test_numeric(&[value], expression)
            .children()
            .unwrap()
            .to_vec()
    }

    /// Keyed scalar storage from a pinned native property of `fixture`.
    fn native_keys(fixture: &[u8], layer_id: u32, property: &str) -> Vec<Chunk> {
        read_project(fixture)
            .unwrap()
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) => composition
                    .layers
                    .iter()
                    .find(|layer| layer.record.id() == layer_id)
                    .and_then(|layer| property_storage(&layer.content, property)),
                _ => None,
            })
            .expect("pinned native keyed scalar")
    }

    /// Bezier keys with nonzero temporal speeds (0.15 to 1.75 over two seconds).
    fn native_bezier_keys() -> Vec<Chunk> {
        native_keys(
            include_bytes!(
                "../../tests/fixtures/pr4442_native/sources/timing_time_remap_bezier.aep"
            ),
            30,
            "ADBE Time Remapping",
        )
    }

    /// Linear keys from 0 to 1 over five seconds.
    fn native_linear_keys() -> Vec<Chunk> {
        native_keys(
            include_bytes!("../../tests/fixtures/properties/property_1D_opacity.aep"),
            15,
            "ADBE Opacity",
        )
    }

    /// Adds `leaf` to the first property group named `group` in `chunks`.
    fn push_leaf(chunks: &mut [Chunk], group: &str, leaf: Vec<Chunk>) -> bool {
        for index in 0..chunks.len().saturating_sub(1) {
            if chunks[index]
                .data_payload()
                .is_some_and(|payload| payload.starts_with(group.as_bytes()))
                && chunks[index + 1].list_kind() == Some(*b"tdgp")
            {
                chunks[index + 1].children_mut().unwrap().extend(leaf);
                return true;
            }
        }
        chunks.iter_mut().any(|chunk| {
            chunk
                .children_mut()
                .is_some_and(|children| push_leaf(children, group, leaf.clone()))
        })
    }

    /// geometry_probe comp 14 without its layers, and its native stroked
    /// Rectangle, layer 40.
    fn probe_rectangle() -> (Composition, Layer) {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/geometry/geometry_probe.aep"
        ))
        .unwrap();
        let ItemKind::Composition(composition) = &project.item(14).unwrap().kind else {
            panic!("probe composition")
        };
        let mut composition = composition.as_ref().clone();
        let rectangle = composition
            .layers
            .iter()
            .find(|layer| layer.record.id() == 40)
            .unwrap()
            .clone();
        composition.layers.clear();
        (composition, rectangle)
    }

    /// Derived storage, not an Adobe-authored Rectangle oracle: `rectangle`
    /// renamed, its Size and added Position and Roundness leaves driven by
    /// `size`, [`SLIDER_RECT_POSITION`] and `roundness`, and `sliders` in a new
    /// Effect Parade.
    fn slider_rect(
        rectangle: &Layer,
        name: &str,
        size: &str,
        roundness: &str,
        sliders: Vec<Chunk>,
    ) -> Layer {
        let mut layer = rectangle.clone();
        layer.name = name.into();
        assert!(replace_property(
            &mut layer.content,
            "ADBE Vector Rect Size",
            test_numeric(&[100.0, 50.0], size),
        ));
        assert!(push_leaf(
            &mut layer.content,
            "ADBE Vector Shape - Rect",
            vec![
                test_match_name("ADBE Vector Rect Position"),
                test_numeric(&[0.0, 0.0], SLIDER_RECT_POSITION),
                test_match_name("ADBE Vector Rect Roundness"),
                test_numeric(&[0.0], roundness),
            ],
        ));
        layer
            .content
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .and_then(Chunk::children_mut)
            .unwrap()
            .extend([
                test_match_name("ADBE Effect Parade"),
                test_list(b"tdgp", sliders),
            ]);
        layer
    }

    /// Width and Height Sliders, static X 10, Y -20 and Radius 8 Sliders, and
    /// `extra` effects.
    fn rect_sliders(width: Vec<Chunk>, height: Vec<Chunk>, extra: Vec<Chunk>) -> Vec<Chunk> {
        let mut sliders = [
            slider_effect("Width", width),
            slider_effect("Height", height),
            slider_effect("X", slider_value(10.0, "")),
            slider_effect("Y", slider_value(-20.0, "")),
            slider_effect("Radius", slider_value(8.0, "")),
        ]
        .concat();
        sliders.extend(extra);
        sliders
    }

    /// The stock Rigged Box 3.0 Rectangle formulas, one statement per piece.
    const RIGGED_BOX_SIZE: &str = concat!(
        "e = effect(\"Rigged Box\");",
        "xSize = e(1);",
        "ySize = e(2);",
        "[xSize,ySize];",
    );
    const RIGGED_BOX_POSITION: &str = concat!(
        "e = effect(\"Rigged Box\");",
        "size = thisProperty.propertyGroup(1).size;",
        "xAnchor = e(4) / -200;",
        "yAnchor = e(5) / -200;",
        "value + [xAnchor*size[0],yAnchor*size[1]];",
    );
    const RIGGED_BOX_ROUNDNESS: &str = concat!("e = effect(\"Rigged Box\");", "e(3);");

    /// A stock Rigged Box pseudo effect: X Size `width`, Y Size 50, Roundness
    /// 8 and zero anchors in its native control slots.
    fn rigged_box_effect(width: Vec<Chunk>) -> Vec<Chunk> {
        let mut controls = vec![test_name("Rigged Box")];
        for (slot, storage) in [
            (1, width),
            (2, slider_value(50.0, "")),
            (3, slider_value(8.0, "")),
            (4, slider_value(0.0, "")),
            (5, slider_value(0.0, "")),
        ] {
            controls.push(test_match_name(&format!("Pseudo/PS Rigged Box-000{slot}")));
            controls.push(test_list(b"tdbs", storage));
        }
        vec![
            test_match_name("Pseudo/PS Rigged Box"),
            test_list(b"sspc", vec![test_list(b"tdgp", controls)]),
        ]
    }

    fn import_owner(layer: &Layer, composition: &Composition) -> ShapeImport {
        let parent =
            super::super::group(LayerId::new(1), "source".into(), None, full_active_range());
        import_with_composition(
            layer,
            composition,
            &HashMap::new(),
            true,
            &parent,
            8,
            &mut 10_000,
            &mut OutputBudget::default(),
            &mut AnimationBudget::default(),
        )
        .unwrap()
    }

    fn imported_rect(layers: &[FxLayer]) -> Option<&fx_schema::RectLayer> {
        layers.iter().find_map(|layer| match layer {
            FxLayer::Rect(rect) => Some(rect),
            FxLayer::Group(group) => stored_rect(&group.layers),
            _ => None,
        })
    }

    fn stored_rect(layers: &[fx_schema::Layer]) -> Option<&fx_schema::RectLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            FxLayer::Rect(rect) => Some(rect),
            FxLayer::Group(group) => stored_rect(&group.layers),
            _ => None,
        })
    }

    /// A fresh document import of the probe composition holding only `layer`.
    fn import_probe_document(layer: Layer) -> crate::structure_document::StructuralConversion {
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/geometry/geometry_probe.aep"
        ))
        .unwrap();
        let probe = project.items.iter_mut().find(|item| item.id == 14).unwrap();
        let ItemKind::Composition(composition) = &mut probe.kind else {
            panic!("probe composition")
        };
        composition.layers = vec![layer];
        crate::structure_document::to_structural_fx_document(&project, Some(14)).unwrap()
    }

    /// The fresh native layer holding the Rectangle of `imported` after the
    /// existing export, which must not omit it for differing X/Y easings.
    fn exported_rect_layer(
        imported: &crate::structure_document::StructuralConversion,
        case: impl std::fmt::Debug,
    ) -> Layer {
        let output = crate::export_document::to_aep(&imported.document).unwrap();
        assert!(
            output
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("Spatial X/Y easing differs")),
            "{case:?}: {:?}",
            output.diagnostics
        );
        read_project(&output.bytes)
            .unwrap()
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) => composition
                    .layers
                    .iter()
                    .find(|layer| {
                        property_storage(&layer.content, "ADBE Vector Rect Size").is_some()
                    })
                    .cloned(),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{case:?}: Rectangle omitted: {:?}", output.diagnostics))
    }

    /// Key values of the first native `property` leaf in `chunks`.
    fn native_key_values(chunks: &[Chunk], property: &str) -> Vec<Vec<f64>> {
        let storage = property_storage(chunks, property)
            .unwrap_or_else(|| panic!("native {property} missing"));
        read_numeric(&storage)
            .unwrap()
            .keyframes
            .into_iter()
            .map(|key| key.values)
            .collect()
    }

    #[test]
    fn complete_slider_references_drive_a_typed_rect_and_its_sibling_copy() {
        // Derived, not an Adobe-authored Rectangle oracle: the native
        // Rectangle and the Bezier Width keys come from separate pinned sources.
        let width = native_bezier_keys();
        let native = read_numeric(&width).unwrap();
        let [first, last] = [&native.keyframes[0], &native.keyframes[1]];
        assert_eq!(native.keyframes.len(), 2);
        let (mut composition, rectangle) = probe_rectangle();
        let fixed = |value| slider_value(value, "");
        composition.layers = vec![
            slider_rect(
                &rectangle,
                "Box",
                SLIDER_RECT_SIZE,
                SLIDER_RECT_ROUNDNESS,
                rect_sliders(width, fixed(50.0), Vec::new()),
            ),
            // A same-clock sibling copy aliases the box's Width Slider.
            slider_rect(
                &rectangle,
                "Box Shadow",
                SLIDER_RECT_SIZE,
                SLIDER_RECT_ROUNDNESS,
                rect_sliders(
                    slider_value(1.0, "thisComp.layer(\"Box\").effect(\"Width\")(\"Slider\")"),
                    fixed(50.0),
                    Vec::new(),
                ),
            ),
        ];
        // AE's temporal ease of the pixel segment: no Scale unit conversion.
        let duration = last.time_secs - first.time_secs;
        let delta = last.values[0] - first.values[0];
        let x1 = first.out_influence[0] / 100.0;
        let x2 = 1.0 - last.in_influence[0] / 100.0;
        let ease = fx_schema::PropertyKeyframeEasing::CubicBezier {
            x1,
            y1: first.out_speed[0] * duration / delta * x1,
            x2,
            y2: 1.0 - last.in_speed[0] * duration / delta * (1.0 - x2),
        };
        let linear = fx_schema::PropertyKeyframeEasing::Linear;
        let [start, end] = [first, last].map(|key| (key.time_secs * 1000.0).round() as i64);
        for layer in &composition.layers {
            let imported = import_owner(layer, &composition);
            let rect = imported_rect(&imported.layers)
                .unwrap_or_else(|| panic!("{}: {:?}", layer.name, imported.warnings));
            assert_eq!(rect.rect.size, [first.values[0], 50.0]);
            assert_eq!(rect.rect.roundness, 8.0);
            assert_eq!(rect.transform.anchor_point, [first.values[0] / 2.0, 25.0]);
            assert_eq!(rect.transform.position, Position::TwoD([10.0, -20.0]));
            for (property, keys) in [
                (
                    PropType::RectSize,
                    [
                        (
                            start,
                            PropertyValue::Vector2([first.values[0], 50.0]),
                            linear,
                        ),
                        (end, PropertyValue::Vector2([last.values[0], 50.0]), ease),
                    ],
                ),
                (
                    PropType::AnchorPointX,
                    [
                        (start, PropertyValue::Float(first.values[0] / 2.0), linear),
                        (end, PropertyValue::Float(last.values[0] / 2.0), ease),
                    ],
                ),
            ] {
                let track = imported
                    .animations
                    .iter()
                    .find(|entry| entry.target == PropertyTarget::layer(rect.id, property))
                    .and_then(|entry| entry.animator.keyframe_track())
                    .unwrap_or_else(|| panic!("{}: {property:?} keys", layer.name));
                let actual: Vec<_> = track
                    .keyframes()
                    .iter()
                    .map(|key| {
                        (
                            key.layer_time().as_millis(),
                            key.value().clone(),
                            key.easing(),
                        )
                    })
                    .collect();
                assert_eq!(actual, keys, "{}: {property:?}", layer.name);
            }
            // The unchanging Height keeps only its static half-size anchor.
            assert!(imported.animations.iter().all(|entry| {
                !entry.animator.is_js_script()
                    && [
                        PropType::AnchorPointY,
                        PropType::RectRoundness,
                        PropType::PositionX,
                        PropType::PositionY,
                    ]
                    .into_iter()
                    .all(|static_property| {
                        entry.target != PropertyTarget::layer(rect.id, static_property)
                    })
            }));
            assert!(
                imported.warnings.iter().any(|warning| {
                    warning.contains("complete Slider references lowered")
                        && warning.contains("live controller linkage is not retained")
                }),
                "{:?}",
                imported.warnings
            );
        }
    }

    #[test]
    fn slider_rect_anchor_keys_follow_only_moving_size_axes_through_export() {
        // Derived fresh import of the probe Rectangle, then the existing
        // exporter: structural evidence only, not Adobe opening or rendering.
        let (_, rectangle) = probe_rectangle();
        let fixed = |value| slider_value(value, "");
        for (width, height, moving) in [
            (native_bezier_keys(), fixed(50.0), [true, false]),
            (fixed(100.0), native_bezier_keys(), [false, true]),
            (native_bezier_keys(), native_bezier_keys(), [true, true]),
        ] {
            let imported = import_probe_document(slider_rect(
                &rectangle,
                "Box",
                SLIDER_RECT_SIZE,
                SLIDER_RECT_ROUNDNESS,
                rect_sliders(width, height, Vec::new()),
            ));
            let composition = imported.document.composition();
            let rect = stored_rect(composition.layers())
                .unwrap_or_else(|| panic!("{moving:?}: {:?}", imported.diagnostics));
            let keys = |property| {
                composition
                    .dynamics()
                    .entries()
                    .iter()
                    .find(|entry| entry.target == PropertyTarget::layer(rect.id, property))
                    .and_then(|entry| entry.animator.keyframe_track())
                    .map(|track| {
                        track
                            .keyframes()
                            .iter()
                            .map(|key| key.value().clone())
                            .collect::<Vec<_>>()
                    })
            };
            let sizes: Vec<[f64; 2]> = keys(PropType::RectSize)
                .expect("typed Size keys")
                .into_iter()
                .map(|value| match value {
                    PropertyValue::Vector2(size) => size,
                    other => panic!("Size key {other:?}"),
                })
                .collect();
            assert_eq!(sizes.len(), 2, "{moving:?}");

            // The fresh native Rectangle keeps its Size keys and moving center.
            let native = exported_rect_layer(&imported, moving);
            assert_eq!(
                native_key_values(&native.content, "ADBE Vector Rect Size"),
                sizes.iter().map(|size| size.to_vec()).collect::<Vec<_>>()
            );
            assert_eq!(
                native_key_values(&native.content, "ADBE Vector Rect Position"),
                sizes
                    .iter()
                    .map(|size| size.map(|extent| extent / 2.0).to_vec())
                    .collect::<Vec<_>>()
            );

            // An unchanging axis keeps only its static half-size anchor.
            assert_eq!(
                rect.transform.anchor_point,
                sizes[0].map(|extent| extent / 2.0)
            );
            for (axis, property) in [PropType::AnchorPointX, PropType::AnchorPointY]
                .into_iter()
                .enumerate()
            {
                let expected = moving[axis].then(|| {
                    sizes
                        .iter()
                        .map(|size| PropertyValue::Float(size[axis] / 2.0))
                        .collect::<Vec<_>>()
                });
                assert_eq!(keys(property), expected, "{moving:?}: {property:?}");
            }
        }
    }

    #[test]
    fn slider_linked_rect_position_and_roundness_keep_pixel_keys_through_export() {
        // Derived fresh import, then the existing exporter (structural evidence
        // only): Slider-linked Position axes beside an unlinked static native
        // Size and a Slider-linked, keyed Roundness.
        let (_, rectangle) = probe_rectangle();
        let fixed = |value| slider_value(value, "");
        let slider = read_numeric(&native_bezier_keys()).unwrap().keyframes;
        let radius = native_linear_keys();
        let radius_keys = read_numeric(&radius).unwrap().keyframes;
        let radii: Vec<f64> = radius_keys.iter().map(|key| key.values[0]).collect();
        let base = [10.0, -20.0];
        let millis =
            |key: &crate::properties::NumericKeyframe| (key.time_secs * 1000.0).round() as i64;
        // AE's temporal ease of the Slider pixel segment: no Scale factor.
        let [first, last] = [&slider[0], &slider[1]];
        let duration = last.time_secs - first.time_secs;
        let delta = last.values[0] - first.values[0];
        let x1 = first.out_influence[0] / 100.0;
        let x2 = 1.0 - last.in_influence[0] / 100.0;
        let ease = fx_schema::PropertyKeyframeEasing::CubicBezier {
            x1,
            y1: first.out_speed[0] * duration / delta * x1,
            x2,
            y2: 1.0 - last.in_speed[0] * duration / delta * (1.0 - x2),
        };
        let linear = fx_schema::PropertyKeyframeEasing::Linear;
        let close = |left: f64, right: f64| (left - right).abs() < 1e-9;
        for (x, y, moving) in [
            (native_bezier_keys(), native_bezier_keys(), [true, true]),
            (native_bezier_keys(), fixed(base[1]), [true, false]),
            (fixed(base[0]), native_bezier_keys(), [false, true]),
        ] {
            let imported = import_probe_document(slider_rect(
                &rectangle,
                "Box",
                "",
                SLIDER_RECT_ROUNDNESS,
                [
                    slider_effect("X", x),
                    slider_effect("Y", y),
                    slider_effect("Radius", radius.clone()),
                ]
                .concat(),
            ));
            let composition = imported.document.composition();
            let rect = stored_rect(composition.layers())
                .unwrap_or_else(|| panic!("{moving:?}: {:?}", imported.diagnostics));
            // Each axis in Slider pixels at the native key times: the Slider's
            // keys, or its static value.
            let [xs, ys] = [0, 1].map(|axis| {
                slider
                    .iter()
                    .map(|key| {
                        if moving[axis] {
                            key.values[0]
                        } else {
                            base[axis]
                        }
                    })
                    .collect::<Vec<_>>()
            });
            assert_eq!(rect.rect.size, [100.0, 50.0]);
            assert_eq!(rect.transform.anchor_point, [50.0, 25.0]);
            assert_eq!(rect.transform.position, Position::TwoD([xs[0], ys[0]]));
            assert_eq!(rect.rect.roundness, radii[0]);
            let keys = |property| {
                composition
                    .dynamics()
                    .entries()
                    .iter()
                    .find(|entry| entry.target == PropertyTarget::layer(rect.id, property))
                    .and_then(|entry| entry.animator.keyframe_track())
                    .map(|track| {
                        track
                            .keyframes()
                            .iter()
                            .map(|key| {
                                (
                                    key.layer_time().as_millis(),
                                    key.value().clone(),
                                    key.easing(),
                                )
                            })
                            .collect::<Vec<_>>()
                    })
            };
            // Slider pixels at the native key times with the Slider's ease; an
            // unchanging axis never moves.
            for (axis, property) in [PropType::PositionX, PropType::PositionY]
                .into_iter()
                .enumerate()
            {
                let values = [&xs, &ys][axis];
                let track = keys(property);
                if moving[axis] {
                    let expected = vec![
                        (millis(first), PropertyValue::Float(values[0]), linear),
                        (millis(last), PropertyValue::Float(values[1]), ease),
                    ];
                    assert_eq!(track, Some(expected), "{moving:?}: {property:?}");
                } else {
                    assert!(
                        track.is_none_or(|track| {
                            track
                                .iter()
                                .all(|(_, value, _)| *value == PropertyValue::Float(base[axis]))
                        }),
                        "{moving:?}: {property:?}"
                    );
                }
            }
            let expected_radii = radius_keys
                .iter()
                .map(|key| (millis(key), PropertyValue::Float(key.values[0]), linear))
                .collect();
            assert_eq!(keys(PropType::RectRoundness), Some(expected_radii));
            // The static native Size emits no Size or center-anchor keys.
            for property in [
                PropType::RectSize,
                PropType::AnchorPointX,
                PropType::AnchorPointY,
            ] {
                assert_eq!(keys(property), None, "{moving:?}: {property:?}");
            }
            assert!(
                imported.diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .message
                        .contains("complete Slider references lowered")
                }),
                "{:?}",
                imported.diagnostics
            );

            // The fresh native Rectangle keeps the Slider path, shifted by the
            // one origin offset of a generated precomposition's roots. Two
            // moving axes stay one spatial path; one moving axis becomes
            // separated Position followers, which keep each axis's own ease.
            let native = exported_rect_layer(&imported, moving);
            let native_transform = crate::properties::read_transform(&native.content).unwrap();
            let leaf = |name: &str| {
                native_transform
                    .iter()
                    .find(|leaf| leaf.match_name == name)
                    .and_then(|leaf| leaf.numeric.as_ref().ok())
                    .unwrap_or_else(|| panic!("{moving:?}: native {name}"))
            };
            let separated = moving != [true, true];
            assert_eq!(
                leaf("ADBE Position").dimensions_separated,
                separated,
                "{moving:?}"
            );
            for (axis, values) in [&xs, &ys].into_iter().enumerate() {
                let (native_keys, component) = if separated {
                    let follower = ["ADBE Position_0", "ADBE Position_1"][axis];
                    (&leaf(follower).keyframes, 0)
                } else {
                    (&leaf("ADBE Position").keyframes, axis)
                };
                let origin = native_keys[0].values[component] - values[0];
                assert!(
                    native_keys.len() == slider.len()
                        && native_keys.iter().zip(&slider).zip(values).all(
                            |((key, source), value)| {
                                key.time_secs == source.time_secs
                                    && close(key.values[component], value + origin)
                            }
                        ),
                    "{moving:?}: axis {axis}: {native_keys:?}"
                );
                // Both axes use the same Slider keys, so one spatial path's
                // speed is the magnitude of their equal speeds.
                let speed = |value: f64| if separated { value } else { value.hypot(value) };
                let [from, to] = [&native_keys[0], &native_keys[1]];
                assert!(
                    if moving[axis] {
                        from.out_interpolation == 2
                            && to.in_interpolation == 2
                            && close(from.out_speed[0], speed(first.out_speed[0]))
                            && close(to.in_speed[0], speed(last.in_speed[0]))
                            && close(from.out_influence[0], first.out_influence[0])
                            && close(to.in_influence[0], last.in_influence[0])
                    } else {
                        native_keys
                            .iter()
                            .all(|key| key.in_speed[0] == 0.0 && key.out_speed[0] == 0.0)
                    },
                    "{moving:?}: axis {axis}: {native_keys:?}"
                );
            }
            assert_eq!(
                native_key_values(&native.content, "ADBE Vector Rect Roundness"),
                radii.iter().map(|radius| vec![*radius]).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn slider_rect_references_outside_the_complete_form_keep_the_static_outline() {
        // Derived rejections; each keeps the diagnosed static-outline fallback.
        let (composition, rectangle) = probe_rectangle();
        let fixed = |value| slider_value(value, "");
        let alias = |layer: &str| {
            slider_value(
                1.0,
                &format!("thisComp.layer(\"{layer}\").effect(\"Width\")(\"Slider\")"),
            )
        };
        let rect = |name: &str, size: &str, roundness: &str, sliders| {
            slider_rect(&rectangle, name, size, roundness, sliders)
        };
        let boxed = |size: &str, roundness: &str, sliders| rect("Box", size, roundness, sliders);
        let plain = || rect_sliders(fixed(100.0), fixed(50.0), Vec::new());
        let mut late = rect("Late Box", SLIDER_RECT_SIZE, SLIDER_RECT_ROUNDNESS, plain());
        let mut record = late.record.encode();
        record[12..16].copy_from_slice(&1_i32.to_be_bytes());
        late.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
        let shadow = |target: &str| {
            rect(
                "Box Shadow",
                SLIDER_RECT_SIZE,
                SLIDER_RECT_ROUNDNESS,
                rect_sliders(alias(target), fixed(50.0), Vec::new()),
            )
        };
        let cases = [
            (
                vec![boxed(
                    &format!("{SLIDER_RECT_SIZE}.value"),
                    SLIDER_RECT_ROUNDNESS,
                    plain(),
                )],
                "not a complete Slider reference expression",
            ),
            (
                vec![boxed(
                    "[effect(\"Width\")(\"Slider\") + 1, effect(\"Height\")(\"Slider\")]",
                    SLIDER_RECT_ROUNDNESS,
                    plain(),
                )],
                "not a complete Slider reference expression",
            ),
            (
                vec![boxed(
                    "effect(\"Width\")(\"Slider\")",
                    SLIDER_RECT_ROUNDNESS,
                    plain(),
                )],
                "not a complete Slider reference expression",
            ),
            (
                vec![boxed(
                    "[effect(\"Width\")(\"Slider\"), effect(\"Height\")(\"Slider\"), effect(\"Width\")(\"Slider\")]",
                    SLIDER_RECT_ROUNDNESS,
                    plain(),
                )],
                "not a complete Slider reference expression",
            ),
            (
                vec![boxed(
                    SLIDER_RECT_SIZE,
                    "thisLayer.effect(\"Radius\")(\"Slider\")",
                    plain(),
                )],
                "not a complete Slider reference expression",
            ),
            (
                vec![boxed(
                    SLIDER_RECT_SIZE,
                    "effect(\"Missing\")(\"Slider\")",
                    plain(),
                )],
                "Slider not found",
            ),
            (
                vec![boxed(
                    SLIDER_RECT_SIZE,
                    SLIDER_RECT_ROUNDNESS,
                    rect_sliders(
                        fixed(100.0),
                        fixed(50.0),
                        slider_effect("Width", fixed(90.0)),
                    ),
                )],
                "ambiguous or non-Slider effect",
            ),
            (
                vec![boxed(
                    SLIDER_RECT_SIZE,
                    SLIDER_RECT_ROUNDNESS,
                    rect_sliders(
                        fixed(100.0),
                        slider_value(50.0, "effect(\"Height\")(\"Slider\")"),
                        Vec::new(),
                    ),
                )],
                "cyclic Slider alias",
            ),
            (
                vec![boxed(
                    SLIDER_RECT_SIZE,
                    SLIDER_RECT_ROUNDNESS,
                    rect_sliders(native_bezier_keys(), native_linear_keys(), Vec::new()),
                )],
                "incompatible key times",
            ),
            (
                vec![shadow("Late Box"), late],
                "identical positive source clocks",
            ),
            (
                vec![
                    shadow("Middle"),
                    rect(
                        "Middle",
                        SLIDER_RECT_SIZE,
                        SLIDER_RECT_ROUNDNESS,
                        rect_sliders(alias("Box"), fixed(50.0), Vec::new()),
                    ),
                    boxed(SLIDER_RECT_SIZE, SLIDER_RECT_ROUNDNESS, plain()),
                ],
                "not a pure alias",
            ),
        ];
        for (layers, reason) in cases {
            let mut composition = composition.clone();
            composition.layers = layers;
            let imported = import_owner(&composition.layers[0], &composition);
            assert!(imported_rect(&imported.layers).is_none(), "{reason}");
            assert!(
                imported.warnings.iter().any(|warning| {
                    warning.contains("typed Rect mapping rejected") && warning.contains(reason)
                }),
                "{reason}: {:?}",
                imported.warnings
            );
        }
    }

    #[test]
    fn resolved_slider_rect_geometry_uses_the_typed_rect_guards() {
        // Derived: validity and the gradient-axis guard read the resolved
        // curves, because an expression-backed native leaf looks static.
        let (composition, rectangle) = probe_rectangle();
        let fixed = |value| slider_value(value, "");
        let boxed = |width, height| {
            let mut composition = composition.clone();
            composition.layers = vec![slider_rect(
                &rectangle,
                "Box",
                SLIDER_RECT_SIZE,
                SLIDER_RECT_ROUNDNESS,
                rect_sliders(width, height, Vec::new()),
            )];
            composition
        };
        // Zero start with a later positive key stays a typed Rect.
        let zero_start = boxed(native_linear_keys(), fixed(50.0));
        let imported = import_owner(&zero_start.layers[0], &zero_start);
        let rect = imported_rect(&imported.layers)
            .unwrap_or_else(|| panic!("zero-start width: {:?}", imported.warnings));
        assert_eq!(rect.rect.size, [0.0, 50.0]);
        for invalid in [
            boxed(fixed(-5.0), fixed(50.0)),
            boxed(fixed(f64::NAN), fixed(50.0)),
        ] {
            let imported = import_owner(&invalid.layers[0], &invalid);
            assert!(imported_rect(&imported.layers).is_none());
            assert!(imported.warnings.iter().any(|warning| {
                warning.contains("ADBE Vector Rect Size animation")
                    && warning.contains("initial outline retained")
            }));
        }

        let gradient =
            read_project(include_bytes!("../../tests/fixtures/shapes/gradient.aep")).unwrap();
        let ItemKind::Composition(gradient_composition) = &gradient.item(1).unwrap().kind else {
            panic!("pinned gradient composition")
        };
        fn gradient_fill(chunks: &[Chunk]) -> Option<Vec<Chunk>> {
            if let Ok(entries) = runs(chunks)
                && let Some((_, run)) = entries
                    .into_iter()
                    .find(|(name, _)| *name == "ADBE Vector Graphic - G-Fill")
            {
                return Some(run.to_vec());
            }
            chunks
                .iter()
                .filter_map(Chunk::children)
                .find_map(gradient_fill)
        }
        let fill = gradient_fill(&gradient_composition.layers[0].content)
            .expect("pinned native Gradient Fill");
        // The stock Rigged Box profile resolves through the same guard.
        let rigged = |width| {
            let mut owner = slider_rect(
                &rectangle,
                "Box",
                RIGGED_BOX_SIZE,
                RIGGED_BOX_ROUNDNESS,
                rigged_box_effect(width),
            );
            assert!(replace_property(
                &mut owner.content,
                "ADBE Vector Rect Position",
                test_numeric(&[0.0, 0.0], RIGGED_BOX_POSITION),
            ));
            let mut composition = composition.clone();
            composition.layers = vec![owner];
            composition
        };
        let sliders = "complete Slider references lowered";
        let stock = "bounded Rigged Box controls lowered";
        for (composition, profile, admitted) in [
            (boxed(native_bezier_keys(), fixed(50.0)), sliders, false),
            (boxed(fixed(100.0), fixed(50.0)), sliders, true),
            (rigged(native_bezier_keys()), stock, false),
            (rigged(fixed(100.0)), stock, true),
        ] {
            let owner = &composition.layers[0];
            let (_, root) = root_runs(&owner.content)
                .unwrap()
                .into_iter()
                .find(|(name, _)| *name == "ADBE Root Vectors Group")
                .unwrap();
            let mut program = program::Program::parse(unique_list(root, *b"tdgp").unwrap(), 8);
            program.paints[0].operation = program::NativeRun {
                name: "ADBE Vector Graphic - G-Fill",
                chunks: &fill,
            };
            let mut next_id = 10;
            let mut animation_budget = AnimationBudget::default();
            let mut collector = Collector {
                includes_occurrence_pipeline: true,
                next_id: &mut next_id,
                animation_budget: &mut animation_budget,
                animations: Vec::new(),
                warnings: Vec::new(),
                frame_fade_lowered: false,
                evaluated_shapes: Default::default(),
                mapped_expressions: Vec::new(),
            };
            let lowered = native_rect::lower_with_context(
                &mut collector,
                &program,
                "gradient Box",
                LayerId::new(1),
                &mut OutputBudget::default(),
                Some((owner, &composition)),
            )
            .unwrap();
            assert_eq!(lowered.is_some(), admitted, "{:?}", collector.warnings);
            // Both profiles resolve; only the guard rejects animated geometry.
            assert!(
                !collector
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("typed Rect mapping rejected")),
                "{:?}",
                collector.warnings
            );
            if let Some(layers) = lowered {
                let rect = imported_rect(&layers).unwrap();
                assert_eq!(rect.rect.size, [100.0, 50.0]);
                assert!(matches!(
                    rect.rect.fill_paint,
                    Some(ShapePaint::Gradient { .. })
                ));
                assert!(
                    collector
                        .warnings
                        .iter()
                        .any(|warning| warning.contains(profile)),
                    "{profile}: {:?}",
                    collector.warnings
                );
            }
        }
    }

    #[test]
    fn native_gradient_import_keeps_editable_gradient_paints() {
        let project = read_project(include_bytes!("../../tests/fixtures/shapes/gradient.aep"))
            .expect("pinned native gradient fixture parses");
        let layer = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) => composition.layers.first(),
                _ => None,
            })
            .expect("fixture has a shape layer");
        let parent =
            super::super::group(LayerId::new(1), "source".into(), None, full_active_range());
        let mut output_budget = OutputBudget::default();
        let mut animation_budget = AnimationBudget::default();
        let imported = import(
            layer,
            &parent,
            8,
            &mut 10_000,
            &mut output_budget,
            &mut animation_budget,
        )
        .unwrap();
        assert!(
            imported.layers.iter().any(layer_has_gradient),
            "warnings: {:?}",
            imported.warnings
        );
        assert!(
            imported
                .warnings
                .iter()
                .all(|warning| !warning.contains("variable-width stroke controls")),
            "native default Taper/Wave controls must not be reported as a loss: {:?}",
            imported.warnings
        );
    }

    #[test]
    fn nondefault_stroke_taper_is_diagnosed_without_dropping_the_paint() {
        fn group(name: &str, children: Vec<Chunk>) -> Vec<Chunk> {
            let mut marker = name.as_bytes().to_vec();
            marker.resize(40, 0);
            vec![
                Chunk::data(*b"tdmn", marker).unwrap(),
                Chunk::list(*b"tdgp", children),
            ]
        }
        let mut marker = b"ADBE Vector Taper Start Width".to_vec();
        marker.resize(40, 0);
        let mut metadata = vec![0; 124];
        metadata[..2].copy_from_slice(&[0xdb, 0x99]);
        metadata[3] = 1;
        let width = vec![
            Chunk::data(*b"tdmn", marker).unwrap(),
            Chunk::list(
                *b"tdbs",
                vec![
                    Chunk::data(*b"tdb4", metadata).unwrap(),
                    Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
                    Chunk::data(*b"cdat", 25.0_f64.to_be_bytes().to_vec()).unwrap(),
                ],
            ),
        ];
        let taper = group("ADBE Vector Stroke Taper", width);
        let stroke = group("ADBE Vector Graphic - Stroke", taper);
        let mut warnings = Vec::new();
        warn_stroke_taper_wave(&stroke, &mut warnings);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("ADBE Vector Taper Start Width"));
        assert!(warnings[0].contains("uniform FX stroke"));
    }

    fn layer_has_gradient(layer: &FxLayer) -> bool {
        match layer {
            FxLayer::Shape(layer) => {
                layer
                    .shape
                    .fills
                    .iter()
                    .any(|fill| matches!(fill.paint, ShapePaint::Gradient { .. }))
                    || layer
                        .shape
                        .strokes
                        .iter()
                        .any(|stroke| matches!(stroke.paint, ShapePaint::Gradient { .. }))
            }
            FxLayer::Group(layer) => layer
                .layers
                .iter()
                .any(|child| layer_has_gradient(child.data())),
            FxLayer::BooleanOperation(layer) => {
                layer
                    .fills
                    .iter()
                    .any(|fill| matches!(fill.paint, ShapePaint::Gradient { .. }))
                    || layer
                        .strokes
                        .iter()
                        .any(|stroke| matches!(stroke.paint, ShapePaint::Gradient { .. }))
            }
            _ => false,
        }
    }

    #[test]
    fn dash_gradient_and_static_fallback_losses_are_diagnosed() {
        fn keyed_scalar(name: &str) -> Vec<Chunk> {
            fn find(chunks: &[Chunk]) -> Option<Vec<Chunk>> {
                for (_, run) in crate::properties::runs(chunks).ok()? {
                    if crate::properties::unique_list(run, *b"tdbs")
                        .and_then(crate::properties::read_numeric)
                        .is_ok_and(|value| !value.keyframes.is_empty())
                    {
                        return Some(run.to_vec());
                    }
                }
                chunks.iter().filter_map(Chunk::children).find_map(find)
            }
            let project = read_project(include_bytes!(
                "../../tests/fixtures/properties/property_rotation.aep"
            ))
            .unwrap();
            let storage = project
                .items
                .iter()
                .find_map(|item| match &item.kind {
                    ItemKind::Composition(comp) => {
                        comp.layers.iter().find_map(|layer| find(&layer.content))
                    }
                    _ => None,
                })
                .expect("native scalar key record");
            let mut marker = name.as_bytes().to_vec();
            marker.resize(40, 0);
            let mut run = vec![Chunk::data(*b"tdmn", marker).unwrap()];
            run.extend(storage);
            run
        }
        fn named_group(name: &str, children: Vec<Chunk>) -> Vec<Chunk> {
            let mut marker = name.as_bytes().to_vec();
            marker.resize(40, 0);
            vec![
                Chunk::data(*b"tdmn", marker).unwrap(),
                Chunk::list(*b"tdgp", children),
            ]
        }

        let malformed = [Chunk::data(*b"tdb4", vec![0]).unwrap()];
        let malformed_dash = [("ADBE Vector Stroke Dashes", malformed.as_slice())];
        let mut next_id = 10;
        let mut animation_budget = AnimationBudget::default();
        let mut collector = Collector {
            includes_occurrence_pipeline: true,
            next_id: &mut next_id,
            animation_budget: &mut animation_budget,
            animations: Vec::new(),
            warnings: Vec::new(),
            frame_fade_lowered: false,
            evaluated_shapes: Default::default(),
            mapped_expressions: Vec::new(),
        };
        collector.add_dash_entries(&malformed_dash, &[LayerId::new(2)]);

        let dash_group = named_group(
            "ADBE Vector Stroke Dashes",
            keyed_scalar("ADBE Vector Stroke Dash 1"),
        );
        let dash_entries = crate::properties::runs(&dash_group).unwrap();
        collector.add_dash_entries(&dash_entries, &[LayerId::new(2)]);

        let gradient_storage: Vec<_> = [
            keyed_scalar("ADBE Vector Grad Start Pt"),
            keyed_scalar("ADBE Vector Grad Type"),
            keyed_scalar("ADBE Vector Grad HiLite Length"),
            keyed_scalar("ADBE Vector Grad HiLite Angle"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let gradient_entries = crate::properties::runs(&gradient_storage).unwrap();
        for name in [
            "ADBE Vector Grad Start Pt",
            "ADBE Vector Grad Type",
            "ADBE Vector Grad HiLite Length",
            "ADBE Vector Grad HiLite Angle",
        ] {
            collector.warn_animated_unsupported(
                &gradient_entries,
                name,
                "unsupported gradient motion retained statically",
            );
        }
        let malformed_static = [("ADBE Vector Trim Type", malformed.as_slice())];
        collector.warn_malformed_static(
            &malformed_static,
            &["ADBE Vector Trim Type"],
            "Simultaneously retained",
        );

        for expected in [
            "stroke dashes are malformed",
            "animated/expression dash/gap lengths",
            "ADBE Vector Grad Start Pt",
            "ADBE Vector Grad Type",
            "ADBE Vector Grad HiLite Length",
            "ADBE Vector Grad HiLite Angle",
            "ADBE Vector Trim Type: malformed static control",
        ] {
            assert!(
                collector
                    .warnings
                    .iter()
                    .any(|warning| warning.contains(expected)),
                "missing diagnostic {expected}: {:?}",
                collector.warnings
            );
        }
    }

    #[test]
    fn zero_remaining_depth_omits_nested_shape_groups_contextually() {
        let project = read_project(include_bytes!("../../tests/fixtures/shapes/gradient.aep"))
            .expect("pinned native gradient fixture parses");
        let layer = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) => composition.layers.first(),
                _ => None,
            })
            .expect("fixture has a shape layer");
        let parent =
            super::super::group(LayerId::new(1), "source".into(), None, full_active_range());
        let mut output_budget = OutputBudget::default();
        let mut animation_budget = AnimationBudget::default();
        let imported = import(
            layer,
            &parent,
            0,
            &mut 2,
            &mut output_budget,
            &mut animation_budget,
        )
        .unwrap();
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("depth budget"))
        );
    }
}
