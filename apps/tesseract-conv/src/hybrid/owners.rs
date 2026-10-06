//! Locate typed animation targets in their source root scope.
use std::collections::BTreeMap;

use anyhow::{ensure, Context};
use fx_schema::effect::EffectData;
use fx_schema::{EffectId, FxItemId, Layer, LayerData, LayerId, PropertyTarget};

#[derive(Default)]
pub(super) struct Owners {
    layers: BTreeMap<LayerId, usize>,
    effects: BTreeMap<EffectId, (usize, LayerId)>,
    items: BTreeMap<FxItemId, (usize, LayerId)>,
}

impl Owners {
    pub(super) fn new(roots: &[Layer]) -> anyhow::Result<Self> {
        let mut owners = Self::default();
        for (root, layer) in roots.iter().enumerate() {
            owners.add(layer, root, 0)?;
        }
        Ok(owners)
    }

    fn add(&mut self, layer: &Layer, root: usize, depth: usize) -> anyhow::Result<()> {
        ensure!(depth < 64, "hybrid source hierarchy exceeds 64 levels");
        ensure!(
            self.layers.insert(layer.id(), root).is_none(),
            "duplicate source layer ID"
        );
        for effect in layer.effects() {
            if let EffectData::Identified { id, .. } = effect.data() {
                ensure!(
                    self.effects.insert(*id, (root, layer.id())).is_none(),
                    "duplicate effect ID"
                );
            }
        }
        for effect in layer.effects() {
            // Stored migration metadata retains the historical animation address.
            if let Some(id) = effect
                .wire_value()
                .get("legacySource")
                .filter(|source| {
                    source.get("kind").and_then(serde_json::Value::as_str) == Some("layerStyle")
                })
                .and_then(|source| source.get("itemId"))
                .and_then(serde_json::Value::as_u64)
            {
                self.item(FxItemId::new(id), root, layer.id())?;
            }
        }
        for mask in masks(layer) {
            self.item(mask.id, root, layer.id())?;
        }
        if let LayerData::Text(text) = layer.data() {
            for animator in &text.animators {
                self.item(animator.id, root, layer.id())?;
                for selector in &animator.selectors {
                    self.item(selector.id, root, layer.id())?;
                }
                for selector in &animator.wiggly_selectors {
                    self.item(selector.id, root, layer.id())?;
                }
            }
            if let Some(options) = &text.path_options {
                self.item(options.id, root, layer.id())?;
            }
            if let Some(options) = &text.anchor_options {
                self.item(options.id, root, layer.id())?;
            }
            if let Some(axes) = &text.source_text.font_variations {
                self.item(axes.id(), root, layer.id())?;
            }
        }
        for child in layer.child_layers().into_iter().flatten() {
            self.add(child, root, depth + 1)?;
        }
        Ok(())
    }

    fn item(&mut self, id: FxItemId, root: usize, layer: LayerId) -> anyhow::Result<()> {
        ensure!(self.items.len() < 65_536, "hybrid FX-item limit exceeded");
        ensure!(
            self.items.insert(id, (root, layer)).is_none(),
            "duplicate FX-item ID"
        );
        Ok(())
    }

    pub(super) fn layer(&self, id: LayerId) -> anyhow::Result<usize> {
        self.layers
            .get(&id)
            .copied()
            .context("missing source layer owner")
    }

    pub(super) fn target(&self, target: &PropertyTarget) -> anyhow::Result<usize> {
        match target {
            PropertyTarget::LayerProperty(p) => self.layer(p.layer_id()),
            PropertyTarget::EffectProperty(_) | PropertyTarget::FxItemProperty(_) => {
                self.layer(self.target_layer(target)?)
            }
        }
    }

    /// The layer that owns `target`: a layer property's layer, or the layer
    /// that holds the addressed effect or FX item.
    pub(super) fn target_layer(&self, target: &PropertyTarget) -> anyhow::Result<LayerId> {
        match target {
            PropertyTarget::LayerProperty(p) => Ok(p.layer_id()),
            PropertyTarget::EffectProperty(p) => self
                .effects
                .get(&p.effect_id())
                .map(|(_, layer)| *layer)
                .context("missing effect owner"),
            PropertyTarget::FxItemProperty(p) => self
                .items
                .get(&p.item_id())
                .map(|(_, layer)| *layer)
                .context("missing FX-item owner"),
        }
    }
}

#[cfg(test)]
mod tests;

pub(super) fn masks(layer: &Layer) -> &[fx_schema::layer::PathMask] {
    match layer.data() {
        LayerData::Media(v) => &v.masks,
        LayerData::Video(v) => &v.masks,
        LayerData::Image(v) => &v.masks,
        LayerData::Text(v) => &v.masks,
        LayerData::Rect(v) => &v.masks,
        LayerData::Shape(v) => &v.masks,
        LayerData::Group(v) => &v.masks,
        LayerData::BooleanOperation(v) => &v.masks,
        LayerData::Adjustment(v) => &v.masks,
        _ => &[],
    }
}
