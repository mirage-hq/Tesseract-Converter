//! Effect parameter and input data.
use crate::LayerId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
/// One named, animatable effect param. Doubles as the **inline schema** for a
/// [`LayerEffect::CustomShader`] param and the shape [`LayerEffect::param_schema`]
/// returns for *any* effect — one source feeding the animator's bindable set,
/// the editor's controls, and (for custom shaders) the validate surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct EffectParam {
    /// Param name — shader-local: scoped by the owning custom shader (its content
    /// hash keeps two shaders' identically-named params from colliding), it is
    /// also the editor label and the address an animator binds to.
    pub name: String,
    /// Human/agent-facing explanation of what this param controls. Surfaced to
    /// AI agents (and the editor) so a custom shader's params are
    /// self-describing. Optional on the wire (defaults to empty).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Slider lower bound / lerp-range floor.
    #[serde(default)]
    pub min: f64,
    /// Slider upper bound / lerp-range ceiling.
    #[serde(default = "effect_param_default_max")]
    pub max: f64,
    /// The value used at render time when no animator drives this param. The
    /// param itself carries no live value — an animator (when present) derives
    /// it from `min`/`max`/`default` + time at lowering; otherwise this is it.
    pub default: f64,
}

fn effect_param_default_max() -> f64 {
    1.0
}

/// Reserved parameter name that cannot be used as a caller-owned graph target.
pub const ANIMATION_TIME_PARAM: &str = "animationTime";

/// Whether `name` is a reserved custom-shader param the engine drives itself
/// (see [`ANIMATION_TIME_PARAM`]).
#[doc(hidden)]
pub fn is_reserved_effect_param(name: &str) -> bool {
    name == ANIMATION_TIME_PARAM
}

/// One declared extra input slot of a custom shader: a named `@binding(1..)`
/// texture the shader samples beyond the layer's own content (e.g. a `"map"`
/// slot for displacement).
///
/// Mirrors [`EffectParam`]: `name` is the slot's logical handle (the slot
/// `SetFxLayerEffectInputBinding` targets), `description` is agent-facing
/// context so a multi-input shader stays self-describing on later edits, and
/// `source_layer_id` is the per-instance binding (what is wired into the
/// slot) — analogous to `EffectParam::default` carrying a param's
/// per-instance value. Being a struct is also the additive home for future
/// per-input config — e.g. a sampler filter/address mode (JRB-1346) —
/// without a wire migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct EffectTextureInput {
    /// Slot name — the shader's `@binding(1..)` texture's logical name and the
    /// slot `SetFxLayerEffectInputBinding` targets. Unique within the shader.
    pub name: String,
    /// What this input provides, in plain language (e.g. "Grayscale
    /// displacement map; R→horizontal, G→vertical shift."). Surfaced to AI
    /// agents (and the editor) so the shader's inputs are self-describing.
    /// Optional on the wire (defaults to empty).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Per-instance binding: the source layer wired into this slot, or `None`
    /// when unbound (the editor's default; a preset like displacement ships
    /// declared-but-unbound). Only bound slots resolve into the render tree.
    /// `SetFxLayerEffectInputBinding` sets/clears this. Optional on the wire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_layer_id: Option<LayerId>,
}

impl<'de> Deserialize<'de> for EffectTextureInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let value = serde_json::Value::deserialize(deserializer)?;
        if value.get("hostSource").is_some() {
            return Err(D::Error::custom(
                "effect texture input `hostSource` was removed; use a sourceLayerId binding",
            ));
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            #[serde(default)]
            description: String,
            #[serde(default)]
            source_layer_id: Option<LayerId>,
        }

        let wire = Wire::deserialize(value).map_err(D::Error::custom)?;
        Ok(Self {
            name: wire.name,
            description: wire.description,
            source_layer_id: wire.source_layer_id,
        })
    }
}

impl EffectTextureInput {
    /// The slot's bound source layer, or `None` when unbound.
    #[must_use]
    pub fn source(&self) -> Option<LayerId> {
        self.source_layer_id
    }

    /// Whether a layer is wired into this slot.
    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.source_layer_id.is_some()
    }
}
