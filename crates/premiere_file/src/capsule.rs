//! Saved capsule payload/container decoding, before graphic admission.
//!
//! Premiere stores UTF-16LE JSON, including effective style values when UI editing
//! switches are false. BinaryHash resolution uses the ordinary native graph.
//! AE text controllers bind by UUID to typed AEP properties. This module does not
//! claim resolved expression geometry, render fidelity, or ordinary clip admission.

use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Seek, SeekFrom},
    path::Path,
};

use aftereffects_file::graphic_template::{
    GraphicTemplateError, SavedGraphicNumeric, SavedGraphicTemplate, SavedGraphicText,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use thiserror::Error;

use crate::{
    format::{Element, Graph},
    schema::{
        native::{EncodedValue, Reference, SubClip, VideoFilterComponent},
        records,
    },
    ConversionError,
};

/// Decode failure preserves the responsible native/JSON/archive error.
#[derive(Debug, Error)]
pub(crate) enum CapsuleError {
    /// Native XML graph or record failure.
    #[error(transparent)]
    Native(#[from] ConversionError),
    /// Invalid base64 binary value.
    #[error("capsule base64: {0}")]
    Base64(#[from] base64::DecodeError),
    /// UTF-16LE data has unpaired surrogates.
    #[error("capsule UTF-16: {0}")]
    Utf16(#[from] std::string::FromUtf16Error),
    /// Saved JSON fields/types are not understood.
    #[error("capsule JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// Native AEP source/controller decoding failed.
    #[error(transparent)]
    Template(#[from] GraphicTemplateError),
    /// ZIP framing, compression or encryption is unsupported.
    #[error("graphic container: {0}")]
    Zip(#[from] zip::result::ZipError),
    /// Container member I/O or CRC verification failed.
    #[error("graphic container I/O: {0}")]
    Io(#[from] std::io::Error),
    /// Saved data is ambiguous or outside the currently decoded static subset.
    #[error("saved capsule: {0}")]
    Invalid(String),
}

/// Saved canvas in template pixels; placement Motion has its own sequence frame.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CapsulePoint {
    /// Horizontal template coordinate.
    pub x: f64,
    /// Vertical template coordinate.
    pub y: f64,
}

/// Effective saved single-style text override; controller flags are not its value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CapsuleTextValue {
    /// Current saved string, including Unicode and empty text.
    pub text: String,
    /// Saved PostScript font name. This does not prove host availability.
    pub font: Option<String>,
    /// Saved font size in pixels.
    pub size: Option<f64>,
    /// Whole-string All Caps flag.
    pub all_caps: Option<bool>,
    /// Unsupported faux styles remain explicit decoded facts.
    pub faux_bold: bool,
    /// Unsupported faux styles remain explicit decoded facts.
    pub faux_italic: bool,
    /// Small caps has no uniform editable equivalent here.
    pub small_caps: bool,
}

/// Current typed value bound to one saved Premiere parameter and controller UUID.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CapsuleControl {
    /// Saved match UUID; layout members refer to this exact identity.
    pub controller_uuid: String,
    /// Native parameter record whose identity, type and clock were validated.
    pub parameter: String,
    /// Effective value, independent of the declaration's default.
    pub value: CapsuleValue,
}

/// Layout controls retain their saved strings/UUID lists, not text-style defaults.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CapsuleValue {
    Text(CapsuleTextValue),
    String(String),
    Group {
        children: Vec<String>,
        expanded: bool,
    },
    Numeric(SavedGraphicNumeric),
    Unsupported {
        kind: u32,
    },
}

/// Independent saved component value. It contains no hidden replay/export bytes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavedCapsule {
    /// Native component identity, for contextual mapping diagnostics.
    pub component: String,
    /// Capsule's saved template canvas.
    pub size: CapsulePoint,
    /// Capsule's saved top-left layout offset, not occurrence Motion Position.
    pub top_left: CapsulePoint,
    /// Current controller values in saved parameter order.
    pub controls: Vec<CapsuleControl>,
    /// Parameter-local losses; strict callers still see these omissions.
    pub diagnostics: Vec<String>,
}

#[derive(Deserialize)]
struct PrivateData {
    capsuleparams: Parameters,
    framesize: Frame,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}
#[derive(Deserialize)]
struct Parameters {
    #[serde(rename = "capParams")]
    controls: Vec<WireControl>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}
#[derive(Deserialize)]
struct Frame {
    size: CapsulePoint,
    topleft: CapsulePoint,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}
#[derive(Deserialize)]
struct WireControl {
    #[serde(rename = "capPropMatchName")]
    uuid: String,
    #[serde(rename = "capPropType")]
    kind: u32,
    // Every remaining field is checked by the selected typed declaration below.
    #[serde(flatten)]
    fields: serde_json::Map<String, serde_json::Value>,
}
#[cfg(test)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextValue {
    #[serde(rename = "capPropFontEdit")]
    _font_edit: bool,
    #[serde(rename = "capPropFontFauxStyleEdit")]
    _faux_edit: bool,
    #[serde(rename = "capPropFontSizeEdit")]
    _size_edit: bool,
    #[serde(rename = "capPropTextRunCount")]
    run_count: usize,
    #[serde(rename = "fontEditValue")]
    fonts: Vec<String>,
    #[serde(rename = "fontSizeEditValue")]
    sizes: Vec<f64>,
    #[serde(rename = "fontTextRunLength")]
    lengths: Vec<usize>,
    #[serde(rename = "fontFSAllCapsValue")]
    all_caps: Vec<bool>,
    #[serde(rename = "fontFSBoldValue")]
    bold: Vec<bool>,
    #[serde(rename = "fontFSItalicValue")]
    italic: Vec<bool>,
    #[serde(rename = "fontFSSmallCapsValue")]
    small_caps: Vec<bool>,
    #[serde(rename = "textEditValue")]
    text: String,
}

impl SavedCapsule {
    /// Decode one native capsule component from the complete saved XML graph.
    ///
    /// # Errors
    /// Rejects unresolved/conflicting binary hashes, unknown payload fields,
    /// keyed/unknown controls, mixed text runs, malformed metadata and invalid
    /// group UUID references. Types 4/8 are saved strings/groups, not Source Text.
    #[cfg(test)]
    pub(crate) fn from_xml(xml: &str, component_id: &str) -> Result<Self, CapsuleError> {
        let graph = Graph::parse(xml).map_err(native_error)?;
        Self::from_graph(&graph, component_id)
    }

    pub(crate) fn from_graph(graph: &Graph<'_>, component_id: &str) -> Result<Self, CapsuleError> {
        let record = graph
            .locate(
                &Reference {
                    id: Some(component_id.into()),
                    uid: None,
                    index: None,
                },
                "capsule selection",
            )
            .map_err(native_error)?;
        let input = graph
            .decode_as::<VideoFilterComponent>(record, "capsule selection")
            .map_err(native_error)?;
        if input.value.match_name.as_deref() != Some("AE.ADBE Capsule") {
            return Err(invalid("selected component is not AE.ADBE Capsule"));
        }
        let private = record
            .element()
            .child("PremiereFilterPrivateData")
            .ok_or_else(|| invalid("capsule has no private metadata"))?;
        let encoded = EncodedValue {
            encoding: private.attribute("Encoding").unwrap_or_default().into(),
            binary_hash: private.attribute("BinaryHash").map(str::to_owned),
            value: private.text().unwrap_or_default().into(),
        };
        let private: PrivateData = decode_json(graph, &encoded, &input.identity)?;
        if [private.framesize.size.x, private.framesize.size.y]
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
            || [private.framesize.topleft.x, private.framesize.topleft.y]
                .iter()
                .any(|value| !value.is_finite())
        {
            return Err(invalid("invalid saved template frame"));
        }
        let mut diagnostics = private.extra.keys().chain(private.framesize.extra.keys()).chain(private.capsuleparams.extra.keys()).map(|field| format!("{}: optional capsule field {field:?} not mapped; known frame and controllers retained", input.identity)).collect::<Vec<_>>();
        let mut media_dependencies = media_dependencies(
            graph,
            record.element().child("MediaDependencyMap"),
            &input.identity,
            &mut diagnostics,
        );
        let params = input
            .value
            .component
            .and_then(|body| body.params)
            .ok_or_else(|| invalid("capsule has no parameter list"))?;
        if params.items.len() != private.capsuleparams.controls.len() {
            return Err(invalid(
                "capsule controls/parameters have different cardinality",
            ));
        }
        let mut uuids = BTreeSet::new();
        let mut controls = Vec::with_capacity(params.items.len());
        for (index, (reference, control)) in params
            .items
            .iter()
            .zip(private.capsuleparams.controls)
            .enumerate()
        {
            if control.uuid.is_empty() || !uuids.insert(control.uuid.clone()) {
                return Err(invalid("requires unique controller UUID bindings"));
            }
            let record = graph
                .locate(reference, &input.identity)
                .map_err(native_error)?;
            let parameter = record.identity();
            let param = record.element();
            let ids = param
                .children()
                .filter(|field| field.tag() == "ParameterID")
                .collect::<Vec<_>>();
            if ids.len() != 1 || ids[0].text() != Some(index.to_string().as_str()) {
                return Err(invalid(&format!(
                    "{parameter}: controller/parameter index binding differs"
                )));
            }
            let name = param
                .child("Name")
                .and_then(|field| field.text())
                .unwrap_or_default();
            if control
                .fields
                .get("capPropUIName")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|expected| expected != name)
            {
                diagnostics.push(format!("{parameter}: optional display-name mismatch; exact parameter index/UUID binding retained"));
            }
            for field in param.children() {
                if !matches!(
                    field.tag(),
                    "Name"
                        | "IsTimeVarying"
                        | "ParameterControlType"
                        | "ParameterID"
                        | "StartKeyframePosition"
                        | "StartKeyframeValue"
                        | "StartKeyframe"
                        | "Keyframes"
                        | "LowerBound"
                        | "UpperBound"
                        | "LowerUIBound"
                        | "UpperUIBound"
                        | "CurrentValue"
                        | "Node"
                ) {
                    diagnostics.push(format!("{parameter}: optional saved parameter field {:?} not mapped; supported override fields retained", field.tag()));
                }
            }
            let value = match control.kind {
                0 => saved_text(graph, param, &control.fields, &parameter, &mut diagnostics)?,
                4 | 8 => controls::saved_layout(
                    graph,
                    param,
                    control.kind,
                    &control.fields,
                    &parameter,
                    &mut diagnostics,
                )?,
                11 => match media_dependencies.remove(&index) {
                    Some(sub_clip) => match controls::validate_media_dependency(param) {
                        Ok(()) => {
                            diagnostics.push(format!("{parameter}: controller {}: media replacement dependency {sub_clip} is not mapped; template source/animation and supported siblings retained", control.uuid));
                            CapsuleValue::Unsupported { kind: 11 }
                        }
                        Err(reason) => {
                            diagnostics.push(format!("{parameter}: controller {}: {reason} for media replacement dependency {sub_clip}; template source/animation and supported siblings retained", control.uuid));
                            CapsuleValue::Unsupported { kind: 11 }
                        }
                    },
                    None => {
                        diagnostics.push(format!("{parameter}: controller {}: media replacement has no supported MediaDependencyMap binding; template source/animation and supported siblings retained", control.uuid));
                        CapsuleValue::Unsupported { kind: 11 }
                    }
                },
                kind => match saved_numeric(param, kind, private.framesize.size) {
                    Ok(value) => CapsuleValue::Numeric(value),
                    Err(reason) => {
                        diagnostics.push(format!("{parameter}: controller {}: {reason}; template property/animation retained", control.uuid));
                        CapsuleValue::Unsupported { kind }
                    }
                },
            };
            controls.push(CapsuleControl {
                controller_uuid: control.uuid,
                parameter,
                value,
            });
        }
        for (parameter_id, sub_clip) in media_dependencies {
            diagnostics.push(format!("{}: MediaDependencyMap ParameterID {parameter_id} targets {sub_clip} but does not identify a media replacement controller; supported controls and template content retained", input.identity));
        }
        validate_groups(&controls)?;
        Ok(Self {
            component: input.identity,
            size: private.framesize.size,
            top_left: private.framesize.topleft,
            controls,
            diagnostics,
        })
    }

    /// Resolve instance edits through exact native UUIDs; an unsupported optional
    /// edit retains the template property and its animation, with a diagnostic.
    pub(crate) fn resolve_instance(
        &self,
        template: &SavedGraphicTemplate,
    ) -> Result<
        (
            SavedGraphicTemplate,
            u32,
            Vec<SavedGraphicText>,
            Vec<String>,
        ),
        CapsuleError,
    > {
        let uuids = self
            .controls
            .iter()
            .map(|control| control.controller_uuid.as_str())
            .collect::<Vec<_>>();
        let (mut instance, composition_id, mut diagnostics) = template.instantiate(&uuids)?;
        diagnostics.extend(self.diagnostics.iter().cloned());
        let mut texts = Vec::new();
        for control in &self.controls {
            let result = match &control.value {
                CapsuleValue::Text(value) => (|| {
                    let mut text = instance.text(&control.controller_uuid)?;
                    let font = value
                        .font
                        .as_deref()
                        .unwrap_or(&text.postscript_font)
                        .to_owned();
                    text.set_value(
                        &value.text,
                        &font,
                        value.size.unwrap_or(text.document.font_size.value()),
                        value.all_caps.unwrap_or(text.document.all_caps),
                    )?;
                    texts.push(text);
                    Ok(Vec::new())
                })(),
                CapsuleValue::Numeric(value) => {
                    instance.apply_numeric(&control.controller_uuid, value)
                }
                CapsuleValue::String(_)
                | CapsuleValue::Group { .. }
                | CapsuleValue::Unsupported { .. } => continue,
            };
            match result {
                Ok(warnings) => diagnostics.extend(warnings.into_iter().map(|warning| format!("{}: controller {}: {warning}", control.parameter, control.controller_uuid))),
                Err(error) => diagnostics.push(format!("{}: controller {} override omitted: {error}; template property/animation and siblings retained", control.parameter, control.controller_uuid)),
            }
        }
        Ok((instance, composition_id, texts, diagnostics))
    }
}

mod controls;
use controls::{saved_numeric, saved_text};

/// Version whose `First` child is a saved ParameterID and whose `Second` child
/// is a replacement SubClip reference. Other versions remain optional metadata.
const MEDIA_DEPENDENCY_VERSION: &str = "1";

fn media_dependencies(
    graph: &Graph<'_>,
    map: Option<Element<'_>>,
    owner: &str,
    diagnostics: &mut Vec<String>,
) -> BTreeMap<usize, String> {
    let Some(map) = map else {
        return BTreeMap::new();
    };
    for attribute in map.attributes().filter(|attribute| *attribute != "Version") {
        diagnostics.push(format!(
            "{owner}: unsupported MediaDependencyMap attribute {attribute:?}; known entries retained"
        ));
    }
    for child in map
        .children()
        .filter(|child| child.tag() != "MediaDependency")
    {
        diagnostics.push(format!(
            "{owner}: unsupported MediaDependencyMap child {:?}; known entries retained",
            child.tag()
        ));
    }
    if map.attribute("Version") != Some(MEDIA_DEPENDENCY_VERSION) {
        diagnostics.push(format!(
            "{owner}: unsupported MediaDependencyMap version {:?}; template content retained",
            map.attribute("Version")
        ));
        return BTreeMap::new();
    }

    let mut bindings = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for (position, dependency) in map
        .children()
        .filter(|child| child.tag() == "MediaDependency")
        .enumerate()
    {
        for attribute in dependency
            .attributes()
            .filter(|attribute| !matches!(*attribute, "Version" | "Index"))
        {
            diagnostics.push(format!(
                "{owner}: MediaDependencyMap entry {position} has unsupported attribute {attribute:?}; known binding fields retained"
            ));
        }
        for child in dependency
            .children()
            .filter(|child| !matches!(child.tag(), "First" | "Second"))
        {
            diagnostics.push(format!(
                "{owner}: MediaDependencyMap entry {position} has unsupported child {:?}; known binding fields retained",
                child.tag()
            ));
        }

        let first_fields = dependency
            .children()
            .filter(|child| child.tag() == "First")
            .collect::<Vec<_>>();
        let first = match first_fields.as_slice() {
            [first] => {
                for attribute in first.attributes() {
                    diagnostics.push(format!(
                        "{owner}: MediaDependencyMap entry {position} First has unsupported attribute {attribute:?}; its saved ParameterID is retained"
                    ));
                }
                if !first.is_text_only() {
                    diagnostics.push(format!(
                        "{owner}: MediaDependencyMap entry {position} First has unsupported child content; template content retained"
                    ));
                    None
                } else {
                    match first
                        .text()
                        .map(str::trim)
                        .and_then(|value| value.parse().ok())
                    {
                        Some(first) => Some(first),
                        None => {
                            diagnostics.push(format!(
                                "{owner}: MediaDependencyMap entry {position} First is not a saved ParameterID; template content retained"
                            ));
                            None
                        }
                    }
                }
            }
            [] => {
                diagnostics.push(format!(
                    "{owner}: MediaDependencyMap entry {position} has no First saved ParameterID; template content retained"
                ));
                None
            }
            _ => {
                diagnostics.push(format!(
                    "{owner}: MediaDependencyMap entry {position} has ambiguous First saved ParameterIDs; template content retained"
                ));
                None
            }
        };

        let second_fields = dependency
            .children()
            .filter(|child| child.tag() == "Second")
            .collect::<Vec<_>>();
        let sub_clip = match second_fields.as_slice() {
            [second] => {
                for attribute in second.attributes().filter(|attribute| {
                    !matches!(
                        *attribute,
                        records::OBJECT_REF | records::OBJECT_UREF | "Index"
                    )
                }) {
                    diagnostics.push(format!(
                        "{owner}: MediaDependencyMap entry {position} Second has unsupported attribute {attribute:?}; its native reference is retained"
                    ));
                }
                for child in second.children() {
                    diagnostics.push(format!(
                        "{owner}: MediaDependencyMap entry {position} Second has unsupported child {:?}; its native reference is retained",
                        child.tag()
                    ));
                }
                let reference = second.reference();
                match graph.follow::<SubClip>(&reference, owner) {
                    Ok(sub_clip) => Some(sub_clip.identity),
                    Err(error) => {
                        diagnostics.push(format!(
                            "{owner}: MediaDependencyMap entry {position} target is not a usable SubClip: {error}; template content retained"
                        ));
                        None
                    }
                }
            }
            [] => {
                diagnostics.push(format!(
                    "{owner}: MediaDependencyMap entry {position} has no Second replacement reference; template content retained"
                ));
                None
            }
            _ => {
                diagnostics.push(format!(
                    "{owner}: MediaDependencyMap entry {position} has ambiguous Second replacement references; template content retained"
                ));
                None
            }
        };

        if dependency.attribute("Version") != Some(MEDIA_DEPENDENCY_VERSION) {
            let target = sub_clip
                .as_deref()
                .map(|sub_clip| format!(" targeting {sub_clip}"))
                .unwrap_or_default();
            diagnostics.push(format!(
                "{owner}: MediaDependencyMap entry {position}{target} has unsupported version {:?}; template content retained",
                dependency.attribute("Version")
            ));
            continue;
        }
        let (Some(first), Some(sub_clip)) = (first, sub_clip) else {
            continue;
        };
        if ambiguous.contains(&first) {
            diagnostics.push(format!(
                "{owner}: MediaDependencyMap ParameterID {first} has another ambiguous target {sub_clip}; template content retained"
            ));
            continue;
        }
        if let Some(previous) = bindings.remove(&first) {
            ambiguous.insert(first);
            diagnostics.push(format!(
                "{owner}: MediaDependencyMap ParameterID {first} ambiguously targets {previous} and {sub_clip}; template content retained"
            ));
        } else {
            bindings.insert(first, sub_clip);
        }
    }
    bindings
}

fn validate_children(children: &[String]) -> Result<(), CapsuleError> {
    let mut seen = BTreeSet::new();
    if children
        .iter()
        .any(|child| child.is_empty() || child.contains(';') || !seen.insert(child))
    {
        return Err(invalid("invalid or duplicate Group child UUID"));
    }
    Ok(())
}

fn decode_children(value: &str) -> Result<Vec<String>, CapsuleError> {
    let value = value
        .strip_suffix(';')
        .ok_or_else(|| invalid("Group UUID list is not terminated"))?;
    let children = value.split(';').map(str::to_owned).collect::<Vec<_>>();
    validate_children(&children)?;
    Ok(children)
}

fn validate_groups(controls: &[CapsuleControl]) -> Result<(), CapsuleError> {
    let values = controls
        .iter()
        .map(|control| (control.controller_uuid.as_str(), &control.value))
        .collect::<BTreeMap<_, _>>();
    for control in controls {
        let CapsuleValue::Group { children, .. } = &control.value else {
            continue;
        };
        let mut pending = children.iter().map(String::as_str).collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        while let Some(uuid) = pending.pop() {
            if uuid == control.controller_uuid {
                return Err(invalid("cyclic Group controller binding"));
            }
            let value = values
                .get(uuid)
                .ok_or_else(|| invalid(&format!("Group child {uuid:?} has no saved controller")))?;
            if seen.insert(uuid) {
                if let CapsuleValue::Group { children, .. } = value {
                    pending.extend(children.iter().map(String::as_str));
                }
            }
        }
    }
    Ok(())
}

fn native_error(error: crate::format::FormatError) -> CapsuleError {
    CapsuleError::Native(crate::error::BuildError::from(error).into())
}
fn invalid(message: &str) -> CapsuleError {
    CapsuleError::Invalid(message.into())
}
fn decode_json<T: serde::de::DeserializeOwned>(
    graph: &Graph<'_>,
    encoded: &EncodedValue,
    owner: &str,
) -> Result<T, CapsuleError> {
    Ok(serde_json::from_str(&decode_string(
        graph, encoded, owner,
    )?)?)
}
fn decode_string(
    graph: &Graph<'_>,
    encoded: &EncodedValue,
    owner: &str,
) -> Result<String, CapsuleError> {
    if encoded.encoding != "base64" {
        return Err(invalid("capsule binary encoding is not base64"));
    }
    let stored = match encoded.binary_hash.as_deref() {
        Some(hash) => graph
            .binary_value(hash, owner)
            .map_err(native_error)?
            .ok_or_else(|| invalid(&format!("{owner}: missing capsule BinaryHash {hash}")))?,
        None if !encoded.value.trim().is_empty() => &encoded.value,
        None => return Err(invalid("empty capsule binary value")),
    };
    let bytes = STANDARD.decode(
        stored
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>(),
    )?;
    if bytes.len() % 2 != 0 {
        return Err(invalid("odd UTF-16LE capsule byte length"));
    }
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Ok(String::from_utf16(&units)?)
}
#[cfg(test)]
fn validate_value(value: &TextValue) -> Result<(), CapsuleError> {
    if value.run_count != 1
        || [
            value.fonts.len(),
            value.sizes.len(),
            value.lengths.len(),
            value.all_caps.len(),
            value.bold.len(),
            value.italic.len(),
            value.small_caps.len(),
        ]
        .iter()
        .any(|length| *length != 1)
    {
        return Err(invalid("capsule requires one complete text/style run"));
    }
    let count = value.text.encode_utf16().count();
    // Native defaults include a terminal text unit; saved overrides need not.
    if !(value.lengths[0] == count || count.checked_add(1) == Some(value.lengths[0]))
        || value.fonts[0].is_empty()
        || !value.sizes[0].is_finite()
        || value.sizes[0] <= 0.0
    {
        return Err(invalid("invalid capsule text run length/font/size"));
    }
    Ok(())
}

/// Decode a single-AEP `.aegraphic` ZIP or a `.mogrt` ZIP with one nested
/// `.aegraphic`. Seek over unused media; only the selected member is expanded.
/// Members are never extracted to disk or replayed.
///
/// # Errors
/// Requires bounded single-disk ZIP32 directory/footer framing. Rejects ZIP64
/// sentinels/locators, ambiguous footers, duplicate/path-traversing members,
/// encryption, excessive expansion, recursive graphics, and missing AEPs.
#[cfg(test)]
pub(crate) fn decode_template<R: Read + Seek>(
    container: R,
) -> Result<SavedGraphicTemplate, CapsuleError> {
    let aep = aep_member(container, false)?;
    Ok(SavedGraphicTemplate::decode(&aep)?)
}

// Bound the data we allocate, not the container or unused member count. ZIP's
// metadata allocation is bounded by directory bytes; only selected AEP/graphic
// members are decompressed into memory.
const MAX_DIRECTORY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXPANDED_MEMBER_BYTES: u64 = 64 * 1024 * 1024;
const ZIP32_FOOTER_SIZE: usize = 22;
const ZIP32_FOOTER_MAGIC: &[u8; 4] = b"PK\x05\x06";

#[derive(Debug)]
struct Zip32Footer {
    members: usize,
    directory_start: u64,
    archive_offset: u64,
    comment: Vec<u8>,
}

// ZIP's filename-indexed metadata coalesces duplicates and can retry earlier
// footers. Check raw cardinality/framing before allocation, without decoding
// compression, names or extra fields a second time.
fn zip32_footer<R: Read + Seek>(reader: &mut R) -> Result<Zip32Footer, CapsuleError> {
    let length = reader.seek(SeekFrom::End(0))?;
    let tail_start = length.saturating_sub(ZIP32_FOOTER_SIZE as u64 + u64::from(u16::MAX));
    reader.seek(SeekFrom::Start(tail_start))?;
    let mut tail = vec![
        0;
        usize::try_from(length - tail_start).map_err(|_| invalid(
            "graphic container footer exceeds platform bounds"
        ))?
    ];
    reader.read_exact(&mut tail)?;
    let tail_position = tail
        .windows(4)
        .rposition(|window| window == ZIP32_FOOTER_MAGIC)
        .ok_or_else(|| invalid("graphic container has no ZIP32 footer"))?;
    let position = tail_start + tail_position as u64;
    let footer = tail
        .get(tail_position..tail_position + ZIP32_FOOTER_SIZE)
        .ok_or_else(|| invalid("truncated or ambiguous graphic container footer"))?;
    let word = |offset| u16::from_le_bytes([footer[offset], footer[offset + 1]]);
    let dword = |offset| {
        u32::from_le_bytes([
            footer[offset],
            footer[offset + 1],
            footer[offset + 2],
            footer[offset + 3],
        ])
    };
    if tail_position + ZIP32_FOOTER_SIZE + usize::from(word(20)) != tail.len() {
        return Err(invalid("graphic container footer/comment must end at EOF"));
    }
    if [word(4), word(6), word(8), word(10)].contains(&u16::MAX)
        || [dword(12), dword(16)].contains(&u32::MAX)
        || tail_position
            .checked_sub(20)
            .is_some_and(|offset| &tail[offset..offset + 4] == b"PK\x06\x07")
    {
        return Err(invalid("ZIP64 graphic containers are unsupported"));
    }
    if word(4) != 0 || word(6) != 0 || word(8) != word(10) {
        return Err(invalid(
            "multidisk or inconsistent graphic container footer",
        ));
    }
    let members = usize::from(word(10));
    let directory_size = u64::from(dword(12));
    if directory_size > MAX_DIRECTORY_BYTES {
        return Err(invalid(
            "graphic container exceeds 64 MiB of directory metadata",
        ));
    }
    let directory_start = position
        .checked_sub(directory_size)
        .ok_or_else(|| invalid("invalid graphic container directory size"))?;
    let archive_offset = directory_start
        .checked_sub(u64::from(dword(16)))
        .ok_or_else(|| invalid("invalid graphic container directory offset"))?;
    reader.seek(SeekFrom::Start(directory_start))?;
    let mut directory = vec![
        0;
        usize::try_from(directory_size).map_err(|_| invalid(
            "graphic container directory size exceeds platform bounds"
        ))?
    ];
    reader.read_exact(&mut directory)?;
    if directory
        .windows(4)
        .any(|window| window == ZIP32_FOOTER_MAGIC)
    {
        return Err(invalid("ambiguous graphic container directory/footer"));
    }
    // Only check the bounded central-directory record lengths and disk/ZIP64
    // sentinels. ZIP remains the owner of member metadata and decompression.
    let mut cursor = 0;
    for _ in 0..members {
        let header = directory
            .get(cursor..cursor + 46)
            .ok_or_else(|| invalid("truncated graphic container directory"))?;
        if &header[..4] != b"PK\x01\x02" {
            return Err(invalid("invalid graphic container directory record"));
        }
        let word = |offset| u16::from_le_bytes([header[offset], header[offset + 1]]);
        if word(34) != 0 {
            return Err(invalid("multidisk graphic container member is unsupported"));
        }
        if [20, 24, 42]
            .iter()
            .any(|offset| header[*offset..*offset + 4] == [u8::MAX; 4])
        {
            return Err(invalid("ZIP64 graphic container member is unsupported"));
        }
        cursor += 46 + usize::from(word(28)) + usize::from(word(30)) + usize::from(word(32));
        if cursor > directory.len() {
            return Err(invalid("truncated graphic container directory member"));
        }
    }
    if cursor != directory.len() {
        return Err(invalid("graphic container directory count/size mismatch"));
    }
    Ok(Zip32Footer {
        members,
        directory_start,
        archive_offset,
        comment: tail[tail_position + ZIP32_FOOTER_SIZE..].to_vec(),
    })
}

// Hide possible earlier EOCDs in compressed data/nested ZIPs only while ZIP
// constructs metadata. Afterward all member reads see the original bytes.
struct CapsuleZipReader<'a, R> {
    reader: R,
    directory_start: u64,
    metadata_only: &'a Cell<bool>,
}

impl<R: Read + Seek> Read for CapsuleZipReader<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let position = self.reader.stream_position()?;
        let count = self.reader.read(output)?;
        if self.metadata_only.get() {
            let hidden = usize::try_from(self.directory_start.saturating_sub(position))
                .unwrap_or(usize::MAX)
                .min(count);
            output[..hidden].fill(0);
        }
        Ok(count)
    }
}

impl<R: Seek> Seek for CapsuleZipReader<'_, R> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.reader.seek(position)
    }
}

fn with_archive<R: Read + Seek, T>(
    mut reader: R,
    nested: bool,
    consume: impl FnOnce(
        &mut zip::ZipArchive<CapsuleZipReader<'_, R>>,
        usize,
        bool,
    ) -> Result<T, CapsuleError>,
) -> Result<T, CapsuleError> {
    let footer = zip32_footer(&mut reader)?;
    let metadata_only = Cell::new(true);
    let mut zip = zip::ZipArchive::new(CapsuleZipReader {
        reader,
        directory_start: footer.directory_start,
        metadata_only: &metadata_only,
    })?;
    metadata_only.set(false);
    if zip.len() != footer.members {
        return Err(invalid("duplicate graphic container members"));
    }
    if zip.central_directory_start() != footer.directory_start
        || zip.offset() != footer.archive_offset
        || zip.comment() != footer.comment
    {
        return Err(invalid(
            "graphic container parser selected a different footer",
        ));
    }
    let mut names = BTreeSet::new();
    let mut target = None;
    for index in 0..zip.len() {
        let member = zip.by_index_raw(index)?;
        if member.enclosed_name().is_none() || !names.insert(member.name().to_owned()) {
            return Err(invalid("unsafe or duplicate graphic container member"));
        }
        if member.is_dir() {
            continue;
        }
        let extension = Path::new(member.name())
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default();
        let aep = extension.eq_ignore_ascii_case("aep");
        let graphic = extension.eq_ignore_ascii_case("aegraphic");
        if !aep && !graphic {
            continue;
        }
        if target.is_some() {
            return Err(invalid(
                "graphic container has ambiguous AEP/graphic targets",
            ));
        }
        if graphic && nested {
            return Err(invalid("recursive aegraphic containers are unsupported"));
        }
        if member.size() > MAX_EXPANDED_MEMBER_BYTES {
            return Err(invalid("graphic container exceeds 64 MiB expanded member"));
        }
        target = Some((index, graphic));
    }
    let (index, graphic) = target.ok_or_else(|| invalid("graphic container has no AEP"))?;
    consume(&mut zip, index, graphic)
}

fn read_member<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    index: usize,
) -> Result<Vec<u8>, CapsuleError> {
    let mut member = zip.by_index(index)?;
    let mut data = Vec::new();
    member
        .by_ref()
        .take(MAX_EXPANDED_MEMBER_BYTES + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MAX_EXPANDED_MEMBER_BYTES {
        return Err(invalid("graphic container exceeds 64 MiB actual expansion"));
    }
    Ok(data)
}

#[cfg(test)]
fn aep_member<R: Read + Seek>(reader: R, nested: bool) -> Result<Vec<u8>, CapsuleError> {
    with_archive(reader, nested, |zip, index, graphic| {
        let data = read_member(zip, index)?;
        if graphic {
            aep_member(Cursor::new(&data), true)
        } else {
            Ok(data)
        }
    })
}

pub(crate) mod picture;

#[cfg(test)]
mod tests;
