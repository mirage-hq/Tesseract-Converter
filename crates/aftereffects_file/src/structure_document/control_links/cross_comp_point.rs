//! Static native Point bindings to Position3D under the user-approved Z=0 policy.

use crate::{
    properties::{self, NumericProperty, PropertyError},
    structure::Layer,
};

use super::{cross_comp, expression, finished, point_control, quoted, token, unique_run};

#[derive(Debug, PartialEq)]
struct Link<'a> {
    composition: &'a str,
    layer: &'a str,
    effect: &'a str,
}

fn parse(mut text: &str) -> Option<Link<'_>> {
    token(&mut text, "comp(")?;
    let composition = quoted(&mut text)?;
    token(&mut text, ").layer(")?;
    let layer = quoted(&mut text)?;
    token(&mut text, ").effect(")?;
    let effect = quoted(&mut text)?;
    token(&mut text, ")(")?;
    if quoted(&mut text)? != "ADBE Point Control-0001" {
        return None;
    }
    token(&mut text, ")")?;
    finished(text).then_some(Link {
        composition,
        layer,
        effect,
    })
}

pub(super) fn lower(
    layer: &Layer,
    context: cross_comp::Context<'_>,
    base: &NumericProperty,
) -> Option<Result<NumericProperty, PropertyError>> {
    let roots = properties::root_runs(&layer.content).ok()?;
    let transform = unique_run(&roots, "ADBE Transform Group").ok()?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp").ok()?).ok()?;
    let position =
        properties::unique_list(unique_run(&leaves, "ADBE Position").ok()?, *b"tdbs").ok()?;
    let link = parse(expression(position).ok()?)?;
    Some((|| {
        cross_comp::validate_destination(base, 3)?;
        if !layer.record.flags().three_d_layer || base.animated || !base.keyframes.is_empty() {
            return Err(PropertyError::Layout(
                "Point2D policy requires a static unkeyed native Position3D receiver",
            ));
        }
        let source = cross_comp::source(
            context,
            cross_comp::Reference {
                composition: link.composition,
                layer: link.layer,
                member: cross_comp::Member::Position,
            },
        )?;
        if source.context.composition_id == context.composition_id
            && source.layer.record.id() == layer.record.id()
        {
            return Err(PropertyError::Layout("self Point Control Position binding"));
        }
        // The source item establishes unique identity. For a reference back to
        // this composition, its occurrence-local overrides remain authoritative.
        let (composition, producer) = if source.context.composition_id == context.composition_id {
            let mut producers = context
                .composition
                .layers
                .iter()
                .filter(|producer| producer.name.as_ref() == link.layer);
            let producer = producers.next().ok_or(PropertyError::Layout(
                "occurrence-local Point Control layer missing",
            ))?;
            if producers.next().is_some() || producer.record.id() != source.layer.record.id() {
                return Err(PropertyError::Layout(
                    "ambiguous occurrence-local Point Control identity",
                ));
            }
            (context.composition, producer)
        } else {
            (source.context.composition, source.layer)
        };
        let [x, y] =
            point_control::selected_with_sparse_explicit(composition, producer, link.effect)?;
        let mut lowered = base.clone();
        lowered.values = vec![x, y, 0.0];
        lowered.expression_enabled = false;
        lowered.expression_present = false;
        Ok(lowered)
    })())
}

#[cfg(test)]
mod tests;
