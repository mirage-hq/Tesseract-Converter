//! Alpha transfer operators consume their source and mask the lower stack.
//! This is not a source-over blend, nor a matte on the operator's own paint.

use super::*;

fn full_span(record: &crate::schema::layer_records::LayerRecord, duration: f64) -> bool {
    let (Some(start), Some(input), Some(output), Some(stretch)) = (
        record.start_time(),
        record.in_point(),
        record.out_point(),
        record.stretch(),
    ) else {
        return false;
    };
    let first = start + input * stretch;
    let last = start + output * stretch;
    first.is_finite()
        && last.is_finite()
        && duration.is_finite()
        && duration > 0.0
        && first.min(last) <= 0.0
        && first.max(last) >= duration
}

fn depth(layer: &FxLayer) -> usize {
    1 + layer
        .child_layers()
        .unwrap_or_default()
        .iter()
        .map(|child| depth(child.data()))
        .max()
        .unwrap_or(0)
}

fn reparent(layer: &mut FxLayer, parent: LayerId) {
    match layer {
        FxLayer::Group(layer) => layer.parent = Some(parent),
        FxLayer::Adjustment(layer) => layer.parent = Some(parent),
        _ => unreachable!("alpha stack admission accepts only Group/Adjustment siblings"),
    }
}

impl Converter<'_> {
    pub(super) fn apply_alpha_stack(
        &mut self,
        context: &LayerContext<'_>,
        sources: &[(usize, LayerId)],
        layers: &mut Vec<FxLayer>,
    ) -> Result<(), DocumentError> {
        // Bottom-up preserves each operator's exact accumulated backdrop. The
        // original IDs and animation targets survive identity reparenting.
        for &(source_index, source_id) in sources.iter().rev() {
            let native = &context.comp.layers[source_index];
            let flags = native.record.flags();
            if !paints_visuals(flags, context.solo) {
                continue;
            }
            let Some(index) = layers.iter().position(|layer| layer.id() == source_id) else {
                self.warn(Limitation::BlendMode, Some(context.comp_id), Some(native.record.id()),
                    "alpha stack operator was restructured by another lowering; original fallback retained".into());
                continue;
            };
            // Native helper kinds and sources explicitly retained as placeholders
            // do not provide independent raster alpha. Do not infer this from
            // opacity or empty children: a valid empty precomp is a raster source.
            let reason = if flags.null_layer {
                Some("non-rendering Null source has no raster alpha; lower content retained")
            } else if !matches!(native.record.layer_type(), 0 | 3 | 4) {
                Some(
                    "non-rendering layer kind has no supported raster alpha; lower content retained",
                )
            } else if self.diagnostics.iter().any(|diagnostic| {
                diagnostic.limitation == Limitation::Placeholder
                    && diagnostic.composition_id == Some(context.comp_id)
                    && diagnostic.layer_id == Some(native.record.id())
            }) {
                Some("placeholder source has no reliable raster alpha; lower content retained")
            } else if flags.adjustment_layer {
                Some(
                    "Adjustment source depends on the backdrop rather than independent source alpha",
                )
            } else if flags.preserve_transparency {
                Some("Preserve Underlying Transparency source has backdrop-dependent alpha")
            } else if !full_span(&native.record, context.comp.duration_secs) {
                Some("partial-span operator requires unmasked intervals outside its active range")
            } else if layers[index..]
                .iter()
                .any(|layer| !matches!(layer, FxLayer::Group(_) | FxLayer::Adjustment(_)))
            {
                Some("lowering introduced a non-stack sibling")
            } else if layers[index..].iter().map(depth).max().unwrap_or(0) + context.depth + 1
                >= MAX_GROUP_DEPTH
            {
                Some("identity backdrop wrapper would exceed the nesting limit")
            } else {
                None
            };
            if let Some(reason) = reason {
                self.warn(Limitation::BlendMode, Some(context.comp_id), Some(native.record.id()),
                    format!("alpha stack transfer mode {} not lowered: {reason}; Normal fallback retained", native.record.blend_mode()));
                continue;
            }
            let id = self.allocate_id()?;
            let mut wrapper = group(
                id,
                "Alpha stack".into(),
                Some(context.parent),
                TimeRangeProperty::new(
                    Time::ZERO,
                    self.duration(context.comp_id, context.comp.duration_secs),
                ),
            );
            wrapper.description = format!(
                "Native alpha stack comp={} layer={} mode={}; original editable source masks only accumulated lower siblings",
                context.comp_id,
                native.record.id(),
                native.record.blend_mode()
            );
            wrapper.track_matte = Some(fx_schema::TrackMatte {
                layer: source_id,
                mode: if native.record.blend_mode() == 17 {
                    fx_schema::TrackMatteType::Alpha
                } else {
                    fx_schema::TrackMatteType::AlphaInverted
                },
            });
            for mut child in layers.drain(index..) {
                reparent(&mut child, id);
                wrapper.layers.push(fx_schema::Layer::from_data(&child)?);
            }
            layers.push(FxLayer::Group(wrapper));
            self.warn(Limitation::BlendMode, Some(context.comp_id), Some(native.record.id()),
                format!("alpha stack transfer mode {} lowered to editable {} matte over accumulated lower siblings; source paint is consumed, source keys/IDs and upper siblings retained; independent native alpha/render proof remains unverified", native.record.blend_mode(), if native.record.blend_mode() == 17 { "Alpha" } else { "AlphaInverted" }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_project(code: u8, partial: bool) -> StructuralProject {
        let mut project = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/compositing/trackMatteType.aep"
        ))
        .unwrap();
        let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
        let ItemKind::Composition(comp) = &mut item.kind else {
            panic!("composition");
        };
        for layer in &mut comp.layers {
            let mut bytes = layer.record.encode();
            bytes[107] = 0; // Remove the fixture's native matte edge.
            if bytes.len() >= 164 {
                bytes[160..164].fill(0);
            }
            bytes[39] |= 1; // Both ordinary participants visibly enabled.
            layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
        }
        let mut bytes = comp.layers[0].record.encode();
        bytes[99] = code;
        if partial {
            bytes[28..32].copy_from_slice(&1_i32.to_be_bytes());
        }
        comp.layers[0].record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
        project
    }

    fn synthetic_transfer(code: u8, partial: bool) -> StructuralConversion {
        to_structural_fx_document(&synthetic_project(code, partial), Some(1)).unwrap()
    }

    fn change_operator(project: &mut StructuralProject, change: impl FnOnce(&mut Vec<u8>)) {
        let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
        let ItemKind::Composition(comp) = &mut item.kind else {
            panic!("composition");
        };
        let mut bytes = comp.layers[0].record.encode();
        change(&mut bytes);
        comp.layers[0].record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
    }

    fn assert_nonrendering_operator_is_skipped(project: &StructuralProject, reason: &str) {
        let result = to_structural_fx_document(project, Some(1)).unwrap();
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.limitation == Limitation::BlendMode
                && diagnostic.message.contains(reason)
                && diagnostic.message.contains("not lowered")
        }));
        let mut normal = project.clone();
        change_operator(&mut normal, |bytes| bytes[99] = 0);
        let expected = to_structural_fx_document(&normal, Some(1)).unwrap();
        assert_eq!(
            result.document.to_json_vec().unwrap(),
            expected.document.to_json_vec().unwrap(),
            "non-rendering carrier must not mask or restructure any lower content"
        );
    }

    #[test]
    fn alpha_stack_eligibility_null_preserves_the_lower_stack() {
        for code in [17, 19] {
            let mut project = synthetic_project(code, false);
            change_operator(&mut project, |bytes| bytes[38] |= 1 << 7);
            assert_nonrendering_operator_is_skipped(&project, "non-rendering Null");
        }
    }

    #[test]
    fn alpha_stack_eligibility_placeholders_preserve_the_lower_stack() {
        for code in [17, 19] {
            let mut project = synthetic_project(code, false);
            change_operator(&mut project, |bytes| bytes[131] = 5);
            assert_nonrendering_operator_is_skipped(&project, "non-rendering layer kind");
            let mut missing = synthetic_project(code, false);
            change_operator(&mut missing, |bytes| {
                bytes[40..44].copy_from_slice(&u32::MAX.to_be_bytes());
            });
            assert_nonrendering_operator_is_skipped(&missing, "placeholder source");
        }
    }

    #[test]
    fn alpha_stack_eligibility_empty_raster_precomp_remains_supported() {
        let mut project = synthetic_project(17, false);
        let root = project.items.iter().find(|item| item.id == 1).unwrap();
        let ItemKind::Composition(comp) = &root.kind else {
            panic!("composition");
        };
        let source_id = comp.layers[0].record.source_id();
        let mut empty = comp.clone();
        empty.layers.clear();
        let source = project
            .items
            .iter_mut()
            .find(|item| item.id == source_id)
            .unwrap();
        source.kind = ItemKind::Composition(empty);
        let result = to_structural_fx_document(&project, Some(1)).unwrap();
        let value = String::from_utf8(result.document.to_json_vec().unwrap()).unwrap();
        assert!(
            value.contains("Native alpha stack"),
            "raster source eligibility is not an alpha/nonempty-content test"
        );
    }

    #[test]
    fn alpha_stack_ordinary_alpha_and_inverse_consume_only_lower_siblings() {
        for (code, mode) in [
            (17, fx_schema::TrackMatteType::Alpha),
            (19, fx_schema::TrackMatteType::AlphaInverted),
        ] {
            let result = synthetic_transfer(code, false);
            let root = result.document.composition().layers()[0].data();
            let siblings = root.child_layers().unwrap();
            let wrapper = siblings
                .iter()
                .find_map(|layer| match layer.data() {
                    FxLayer::Group(group) if group.description.contains("alpha stack") => {
                        Some(group)
                    }
                    _ => None,
                })
                .expect("alpha stack wrapper");
            let matte = wrapper.track_matte.as_ref().unwrap();
            assert_eq!(matte.mode, mode);
            assert!(wrapper.layers.iter().any(|layer| layer.id() == matte.layer));
            assert!(wrapper.layers.len() >= 2);
            assert!(
                wrapper
                    .layers
                    .iter()
                    .all(|layer| layer.data().parent_id() == Some(wrapper.id))
            );
            result.document.to_json_vec().unwrap();
        }
    }

    #[test]
    fn alpha_stack_partial_span_and_luma_keep_explicit_fallbacks() {
        for (code, partial, reason) in [
            (17, true, "partial-span"),
            (18, false, "no existing FX blend equivalent"),
        ] {
            let result = synthetic_transfer(code, partial);
            assert!(
                result
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.limitation == Limitation::BlendMode
                        && diagnostic.message.contains(reason))
            );
            let value: serde_json::Value =
                serde_json::from_slice(&result.document.to_json_vec().unwrap()).unwrap();
            assert!(!value.to_string().contains("Native alpha stack"));
        }
    }

    #[test]
    fn alpha_stack_full_span_uses_composition_clock() {
        let mut bytes = vec![0_u8; 164];
        for (offset, numerator, denominator) in
            [(12, 0_i32, 1000_u32), (20, 0, 1000), (28, 4000, 1000)]
        {
            bytes[offset..offset + 4].copy_from_slice(&numerator.to_be_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&denominator.to_be_bytes());
        }
        // Native stretch numerator/denominator are noncontiguous fields.
        bytes[8..12].copy_from_slice(&1_i32.to_be_bytes());
        bytes[108..112].copy_from_slice(&1_u32.to_be_bytes());
        let record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
        assert!(full_span(&record, 4.0));
        assert!(!full_span(&record, 4.1));
        assert!(!full_span(&record, f64::NAN));
    }
}
