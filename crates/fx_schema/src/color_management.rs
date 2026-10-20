//! Document-owned ACES/legacy color-management policy, rendered by the ACES pipeline.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Validated root policy; `legacyDisplay` by default, or a complete ACES policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct ColorManagement {
    working_space: WorkingSpace,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    output_transform: Option<OutputTransform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    compositing: Option<Compositing>,
}

/// The supported working space, not an SDR output constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum WorkingSpace {
    /// All existing rendering, including HDR and extended authored colors.
    LegacyDisplay,
    /// Opt-in ACES: ACEScct grading, ACEScg compositing and ACES 2.0 output.
    Acescct,
}

/// Pinned ACES output-transform generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "project_types.d.ts")]
pub enum OutputTransform {
    #[serde(rename = "aces2.0")]
    Aces2_0,
}

/// Default compositing-domain rules for an ACES document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum Compositing {
    FootageOnly,
    WholeProgram,
    PerClip,
}

/// Complete, validated ACES policy returned by the document accessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcesPolicy {
    /// Pinned output-transform generation.
    pub output_transform: OutputTransform,
    /// Document default compositing preset.
    pub compositing: Compositing,
}

/// Explicit FX Group or video/image compositing-domain override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum RenderDomain {
    Scene,
    Display,
}

impl<'de> Deserialize<'de> for RenderDomain {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("scene") => Ok(Self::Scene),
            Some("display") => Ok(Self::Display),
            _ => Err(serde::de::Error::custom(
                ColorManagementError::Unsupported {
                    field: "renderDomain".into(),
                    value,
                },
            )),
        }
    }
}

impl Default for ColorManagement {
    fn default() -> Self {
        Self {
            working_space: WorkingSpace::LegacyDisplay,
            output_transform: None,
            compositing: None,
        }
    }
}

/// Errors are shared by all document readers, before resources or rendering.
#[derive(Debug, thiserror::Error)]
pub enum ColorManagementError {
    /// A future semantic policy must not silently render as legacy.
    #[error("unsupported color management {field} {value}, requires a newer Jerboa")]
    Unsupported { field: String, value: Value },
    /// Corrupt or incomplete input is not a missing policy.
    #[error("invalid colorManagement: {0}")]
    Invalid(String),
    /// Trees inherit their document policy and cannot override it.
    #[error("colorManagement is document-scoped; nested compositions inherit it ({0})")]
    Misplaced(String),
}

impl ColorManagement {
    /// Construct a complete ACES policy; no partially valid state is exposed.
    pub fn new_aces(output_transform: OutputTransform, compositing: Compositing) -> Self {
        Self {
            working_space: WorkingSpace::Acescct,
            output_transform: Some(output_transform),
            compositing: Some(compositing),
        }
    }

    /// Document working-space identity.
    pub fn working_space(&self) -> WorkingSpace {
        self.working_space
    }

    /// Complete ACES controls, absent for legacy documents.
    pub fn aces(&self) -> Option<AcesPolicy> {
        Some(AcesPolicy {
            output_transform: self.output_transform?,
            compositing: self.compositing?,
        })
    }

    /// Whether existing rendering semantics apply unchanged.
    pub fn is_legacy(&self) -> bool {
        self.working_space == WorkingSpace::LegacyDisplay
    }

    /// Reject unsupported rendering before any resource or GPU work. Every
    /// validated policy renders: ACES documents are lowered to
    /// scene-color-domain groups, and content the renderer cannot place in
    /// one fails that render with a typed error, never as legacy (ENG-1948).
    pub fn render_preflight(&self) -> Result<(), ColorManagementError> {
        Ok(())
    }
    /// Validate an explicitly supplied policy object, including every sibling.
    pub fn from_value(value: Value) -> Result<Self, ColorManagementError> {
        let object = value
            .as_object()
            .ok_or_else(|| ColorManagementError::Invalid("expected object".into()))?;
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "version" | "workingSpace" | "outputTransform" | "compositing"
            ) {
                return Err(ColorManagementError::Unsupported {
                    field: key.clone(),
                    value: object[key].clone(),
                });
            }
        }
        // The inert version field is ignored regardless of its value.
        // Omission is the canonical shape for new documents.
        let working_space = object
            .get("workingSpace")
            .ok_or_else(|| ColorManagementError::Invalid("missing workingSpace".into()))?;
        let name = working_space
            .as_str()
            .ok_or_else(|| ColorManagementError::Invalid("workingSpace must be a string".into()))?;
        match name {
            "legacyDisplay" => {
                for field in ["outputTransform", "compositing"] {
                    if let Some(value) = object.get(field) {
                        return Err(ColorManagementError::Unsupported {
                            field: field.into(),
                            value: value.clone(),
                        });
                    }
                }
                Ok(Self::default())
            }
            "acescct" => {
                let required = |field: &str| -> Result<&str, ColorManagementError> {
                    object
                        .get(field)
                        .ok_or_else(|| ColorManagementError::Invalid(format!("missing {field}")))?
                        .as_str()
                        .ok_or_else(|| {
                            ColorManagementError::Invalid(format!("{field} must be a string"))
                        })
                };
                let output_transform = match required("outputTransform")? {
                    "aces2.0" => OutputTransform::Aces2_0,
                    _ => {
                        return Err(ColorManagementError::Unsupported {
                            field: "outputTransform".into(),
                            value: object["outputTransform"].clone(),
                        })
                    }
                };
                let compositing = match required("compositing")? {
                    "footageOnly" => Compositing::FootageOnly,
                    "wholeProgram" => Compositing::WholeProgram,
                    "perClip" => Compositing::PerClip,
                    _ => {
                        return Err(ColorManagementError::Unsupported {
                            field: "compositing".into(),
                            value: object["compositing"].clone(),
                        })
                    }
                };
                Ok(Self::new_aces(output_transform, compositing))
            }
            _ => Err(ColorManagementError::Unsupported {
                field: "workingSpace".into(),
                value: working_space.clone(),
            }),
        }
    }
}

impl<'de> Deserialize<'de> for ColorManagement {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_value(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Non-fatal domain override ignored by an enclosing scene group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderDomainDiagnostic {
    /// Structural scope of the ineffective override.
    pub path: String,
}

impl std::fmt::Display for RenderDomainDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "renderDomain=display has no effect inside a scene group ({})",
            self.path
        )
    }
}

/// Validate domain nesting without assuming a standalone composition's root policy.
pub fn validate_render_domain_nesting(
    composition: &Value,
) -> Result<Vec<RenderDomainDiagnostic>, ColorManagementError> {
    let mut diagnostics = Vec::new();
    validate_domains(composition, false, "composition", &mut diagnostics)?;
    for diagnostic in &diagnostics {
        log::warn!("{diagnostic}");
    }
    Ok(diagnostics)
}

/// Validate a composition where it meets its owning document policy.
/// Standalone trees are provisional; root readers and writers must call this.
pub fn validate_render_domains(
    _policy: &ColorManagement,
    composition: &Value,
) -> Result<Vec<RenderDomainDiagnostic>, ColorManagementError> {
    // Legacy preserves overrides but ignores them; structural validity is universal.
    validate_render_domain_nesting(composition)
}

fn validate_domains(
    value: &Value,
    inside_scene: bool,
    path: &str,
    diagnostics: &mut Vec<RenderDomainDiagnostic>,
) -> Result<(), ColorManagementError> {
    let mut scene = inside_scene;
    if let Some(domain) = value.get("renderDomain") {
        let domain = match domain.as_str() {
            Some("scene") => RenderDomain::Scene,
            Some("display") => RenderDomain::Display,
            _ => {
                return Err(ColorManagementError::Unsupported {
                    field: format!("renderDomain at {path}"),
                    value: domain.clone(),
                })
            }
        };
        if inside_scene && domain == RenderDomain::Display {
            diagnostics.push(RenderDomainDiagnostic { path: path.into() });
        }
        scene |= domain == RenderDomain::Scene;
    }
    if let Some(layers) = value.get("layers").and_then(Value::as_array) {
        for (index, layer) in layers.iter().enumerate() {
            validate_domains(
                layer,
                scene,
                &format!("{path}.layers[{index}]"),
                diagnostics,
            )?;
            if let Some(content) = layer.get("content") {
                validate_domains(
                    content,
                    scene,
                    &format!("{path}.layers[{index}].content"),
                    diagnostics,
                )?;
            }
        }
    }
    Ok(())
}

/// Reject a policy at one structural scope without inspecting extension data.
pub fn reject_scoped_color_management(
    value: &Value,
    path: &str,
) -> Result<(), ColorManagementError> {
    if value.get("colorManagement").is_some() {
        return Err(ColorManagementError::Misplaced(path.into()));
    }
    Ok(())
}

/// Reject only actual composition/layer scopes, never arbitrary extension metadata.
pub fn reject_nested_color_management(composition: &Value) -> Result<(), ColorManagementError> {
    fn scope(value: &Value, path: &str) -> Result<(), ColorManagementError> {
        reject_scoped_color_management(value, path)?;
        if let Some(layers) = value.get("layers").and_then(Value::as_array) {
            for (index, layer) in layers.iter().enumerate() {
                scope(layer, &format!("{path}.layers[{index}]"))?;
                if let Some(content) = layer.get("content") {
                    scope(content, &format!("{path}.layers[{index}].content"))?;
                }
            }
        }
        Ok(())
    }
    scope(composition, "composition")
}
