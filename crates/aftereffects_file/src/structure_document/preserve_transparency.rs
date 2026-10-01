//! Source-stack analysis for AE's Preserve Underlying Transparency switch.

use std::collections::BTreeSet;

use crate::structure::{Composition, Layer};

use super::compositing;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProviderSelection {
    pub(super) indices: Vec<usize>,
}

pub(super) fn providers(
    composition: &Composition,
    target_index: usize,
    solo: bool,
) -> Result<ProviderSelection, String> {
    let target = composition.layers.get(target_index).ok_or_else(|| {
        "preserve-transparency target index is outside the composition".to_owned()
    })?;
    if !target.record.flags().preserve_transparency {
        return Ok(ProviderSelection {
            indices: Vec::new(),
        });
    }
    if target.record.flags().adjustment_layer {
        return Err("the target is an Adjustment layer".into());
    }
    if compositing::matte_source(composition, target_index)?.is_some() {
        return Err("the target also owns a native track matte".into());
    }
    let (start, end) = active_range(target)
        .ok_or_else(|| "the target has an invalid native active range".to_owned())?;
    let target_range = (start.max(0.0), end.min(composition.duration_secs));
    if !composition.duration_secs.is_finite() || target_range.1 <= target_range.0 {
        return Err("the target has no active range inside the composition".into());
    }
    let mut indices = BTreeSet::new();
    for (index, layer) in composition.layers[..target_index].iter().enumerate() {
        let flags = layer.record.flags();
        // A preserve-alpha layer cannot expand the accumulated underlying alpha,
        // so excluding it keeps the reusable provider set exact for consecutive
        // preserve-alpha siblings.
        if flags.preserve_transparency
            || !flags.enabled
            || flags.guide_layer
            || flags.null_layer
            || (solo && !flags.solo)
        {
            continue;
        }
        let range = active_range(layer).ok_or_else(|| {
            format!(
                "underlying layer {} has an invalid native active range",
                layer.record.id()
            )
        })?;
        if !overlaps(target_range, range) {
            continue;
        }
        if flags.adjustment_layer {
            return Err(format!(
                "underlying adjustment layer {} has stack-dependent alpha",
                layer.record.id()
            ));
        }
        if !matches!(layer.record.blend_mode(), 0 | 2) {
            return Err(format!(
                "underlying layer {} uses blend mode {} whose alpha accumulation is not proven",
                layer.record.id(),
                layer.record.blend_mode()
            ));
        }
        indices.insert(index);
    }
    if indices.is_empty() {
        return Err("no overlapping ordinary underlying paint layers were found".into());
    }

    // Explicit/legacy matte sources may be disabled ordinary paint, but they
    // still contribute to the selected layer's alpha. Include their complete
    // dependency chain; the caller reapplies composition mattes inside the
    // independent provider group.
    let mut pending: Vec<_> = indices.iter().copied().collect();
    while let Some(index) = pending.pop() {
        let Some((source, _)) = compositing::matte_source(composition, index)? else {
            continue;
        };
        let layer = &composition.layers[source];
        let flags = layer.record.flags();
        if flags.adjustment_layer || flags.preserve_transparency {
            return Err(format!(
                "matte dependency layer {} has stack-dependent alpha",
                layer.record.id()
            ));
        }
        if !matches!(layer.record.blend_mode(), 0 | 2) {
            return Err(format!(
                "matte dependency layer {} uses blend mode {} whose alpha accumulation is not proven",
                layer.record.id(),
                layer.record.blend_mode()
            ));
        }
        if indices.insert(source) {
            pending.push(source);
        }
    }
    Ok(ProviderSelection {
        indices: indices.into_iter().collect(),
    })
}

fn active_range(layer: &Layer) -> Option<(f64, f64)> {
    let record = &layer.record;
    let start = record.start_time()?;
    let input = record.in_point()?;
    let output = record.out_point()?;
    let stretch = record.stretch()?;
    // Match Converter::timing: native source bounds must ascend, even when
    // negative stretch reverses their order in composition time.
    if input >= output || stretch == 0.0 {
        return None;
    }
    let input = start + input * stretch;
    let output = start + output * stretch;
    if !input.is_finite() || !output.is_finite() {
        return None;
    }
    let range = (input.min(output), input.max(output));
    (range.1 > range.0 && range.1 <= super::MAX_TIME_SECS).then_some(range)
}

fn overlaps(left: (f64, f64), right: (f64, f64)) -> bool {
    left.0 < right.1 && right.0 < left.1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        schema::layer_records::LayerRecord,
        structure::{ItemKind, read_project},
    };

    // Synthetic clocks isolate interval selection, not Adobe render fidelity.
    fn timed_layer(id: u32, start: i32, input: i32, output: i32, stretch: i32) -> Layer {
        let mut bytes = vec![0; 164];
        bytes[..4].copy_from_slice(&id.to_be_bytes());
        for (offset, value) in [(8, stretch), (12, start), (20, input), (28, output)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        for offset in [16, 24, 32, 108] {
            bytes[offset..offset + 4].copy_from_slice(&1_u32.to_be_bytes());
        }
        bytes[39] = 1;
        bytes[99] = 2;
        bytes[103] = u8::from(id == 2);
        Layer {
            name: "interval probe".into(),
            record: LayerRecord::decode(&bytes).unwrap(),
            content: vec![],
        }
    }

    fn selection(provider: Layer, target: Layer) -> Result<ProviderSelection, String> {
        let project =
            read_project(include_bytes!("../../tests/fixtures/ae26_one_comp.aep")).unwrap();
        let mut comp = project
            .items
            .into_iter()
            .find_map(|item| match item.kind {
                ItemKind::Composition(comp) => Some(comp),
                _ => None,
            })
            .unwrap();
        comp.duration_secs = 20.0;
        comp.layers = vec![provider, target];
        providers(&comp, 1, false)
    }

    #[test]
    fn review_audit_preserve_transparency_uses_composition_clocks() {
        let target = timed_layer(2, 10, 0, 2, 1);
        assert_eq!(
            selection(timed_layer(1, 0, 10, 12, 1), target.clone())
                .unwrap()
                .indices,
            vec![0]
        );
        assert!(selection(timed_layer(1, 0, 0, 2, 1), target.clone()).is_err());
        assert_eq!(
            selection(timed_layer(1, 14, 1, 2, -2), target)
                .unwrap()
                .indices,
            vec![0]
        );
    }

    #[test]
    fn review_audit_preserve_transparency_ignores_overlap_outside_composition() {
        assert!(selection(timed_layer(1, 30, 0, 2, 1), timed_layer(2, 30, 0, 2, 1)).is_err());
        assert!(selection(timed_layer(1, 10, 0, 2, 0), timed_layer(2, 10, 0, 2, 1)).is_err());
    }

    #[test]
    fn provider_selection_retains_more_than_128_layers() {
        let project =
            read_project(include_bytes!("../../tests/fixtures/ae26_one_comp.aep")).unwrap();
        let mut comp = project
            .items
            .into_iter()
            .find_map(|item| match item.kind {
                ItemKind::Composition(comp) => Some(comp),
                _ => None,
            })
            .unwrap();
        comp.duration_secs = 20.0;
        comp.layers = (3..=131)
            .map(|id| timed_layer(id, 0, 0, 2, 1))
            .chain(std::iter::once(timed_layer(2, 0, 0, 2, 1)))
            .collect();
        let selection = providers(&comp, 129, false).unwrap();
        assert_eq!(selection.indices, (0..129).collect::<Vec<_>>());
    }

    #[test]
    fn active_ranges_use_half_open_overlap() {
        assert!(overlaps((2.0, 3.0), (2.5, 4.0)));
        assert!(overlaps((2.0, 3.0), (1.0, 2.5)));
        assert!(!overlaps((2.0, 3.0), (3.0, 4.0)));
        assert!(!overlaps((2.0, 3.0), (1.0, 2.0)));
    }
}
