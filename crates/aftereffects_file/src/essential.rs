//! Occurrence-local decoding and application of After Effects Essential Properties.
//!
//! Layout reference: MIT py-aep e12a451c35bacd3f34a080090265f9370e66162b,
//! `parsers/essential_graphics.py` and `resolvers/essential_properties.py`.

use serde_json::Value;

use crate::{
    properties::{self, read_numeric},
    rifx::Chunk,
    structure::Layer,
};

const BY_NAME_INDEX: u64 = 0xffff_ffff;
const MEDIA_REPLACEMENT_CONTROLLER: u32 = 14;
const GROUP_CONTROLLER: u32 = 10;

/// One root-to-leaf controller path component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourcePropertyRef {
    /// Stable Adobe match name.
    pub(crate) match_name: String,
    /// Zero-based child position, or `None` for Adobe's by-name sentinel.
    pub(crate) child_index: Option<u32>,
}

/// One Essential Graphics controller declared by a source composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Controller {
    pub(crate) uuid: String,
    pub(crate) controller_type: u32,
    pub(crate) source_comp_id: Option<u32>,
    pub(crate) source_layer_id: Option<u32>,
    pub(crate) path: Vec<SourcePropertyRef>,
}

/// Native override storage resolved to its source property identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Override {
    pub(crate) source_comp_id: u32,
    pub(crate) source_layer_id: u32,
    pub(crate) value: OverrideValue,
}

/// Property storage or an alternate AV source; media controllers have no path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OverrideValue {
    Property {
        path: Vec<SourcePropertyRef>,
        /// Native storage excluding the override leaf's `tdmn` identity marker.
        chunks: Vec<Chunk>,
    },
    Media {
        source_id: u32,
    },
}

/// Stable categories which the converter can map to import diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WarningKind {
    MalformedController,
    MalformedOverride,
    UnmatchedController,
    UnsupportedGroup,
    UnsupportedSourceMetadata,
    UnresolvedSourcePath,
}

/// A non-fatal reason why one controller or override was not applied exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Warning {
    pub(crate) kind: WarningKind,
    pub(crate) message: String,
    pub(crate) affects_media: bool,
}

/// Successfully decoded values plus isolated best-effort warnings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Parsed<T> {
    pub(crate) values: Vec<T>,
    pub(crate) warnings: Vec<Warning>,
}

/// Parses controllers from the newest Essential Graphics container available.
pub(crate) fn controllers(comp_item_children: &[Chunk]) -> Parsed<Controller> {
    let mut warnings = Vec::new();
    let Some((kind, label)) = [(*b"CIF3", "CIF3"), (*b"CIF2", "CIF2"), (*b"CIFO", "CIFO")]
        .into_iter()
        .find(|(kind, _)| {
            comp_item_children
                .iter()
                .any(|chunk| chunk.list_kind() == Some(*kind))
        })
    else {
        return Parsed {
            values: Vec::new(),
            warnings,
        };
    };
    let mut containers = comp_item_children
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(kind));
    let container = containers
        .next()
        .expect("the selected controller kind was observed above");
    if containers.next().is_some() {
        warnings.push(warning(
            WarningKind::MalformedController,
            format!(
                "source composition has duplicate {label} controller containers; source controllers ignored"
            ),
        ));
        return Parsed {
            values: Vec::new(),
            warnings,
        };
    }
    let Some(container) = container.children() else {
        warnings.push(warning(
            WarningKind::MalformedController,
            format!(
                "source composition has an opaque {label} controller container; source controllers ignored"
            ),
        ));
        return Parsed {
            values: Vec::new(),
            warnings,
        };
    };

    let mut values = Vec::new();
    for (position, chunk) in container
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"CCtl"))
        .enumerate()
    {
        let Some(children) = chunk.children() else {
            warnings.push(warning(
                WarningKind::MalformedController,
                format!("controller {position} has an opaque CCtl container"),
            ));
            continue;
        };
        match parse_controller(children) {
            Ok(controller) => values.push(controller),
            Err(message) => warnings.push(warning(
                WarningKind::MalformedController,
                format!("controller {position}: {message}"),
            )),
        }
    }
    Parsed { values, warnings }
}

/// Resolves occurrence override leaves to source controller identities.
pub(crate) fn overrides(layer_content: &[Chunk], controllers: &[Controller]) -> Parsed<Override> {
    let mut warnings = Vec::new();
    let root_runs = match properties::root_runs(layer_content) {
        Ok(runs) => runs,
        Err(error) => {
            return Parsed {
                values: Vec::new(),
                warnings: vec![override_warning(
                    WarningKind::MalformedOverride,
                    format!("Essential Properties root is malformed: {error}"),
                    controllers,
                )],
            };
        }
    };
    let matching: Vec<_> = root_runs
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Layer Overrides")
        .collect();
    let [(_, root_run)] = matching.as_slice() else {
        if matching.len() > 1 {
            warnings.push(override_warning(
                WarningKind::MalformedOverride,
                "multiple ADBE Layer Overrides roots are ambiguous".into(),
                controllers,
            ));
        }
        return Parsed {
            values: Vec::new(),
            warnings,
        };
    };

    let uuids = match override_uuids(root_run) {
        Ok(uuids) => uuids,
        Err(message) => {
            return Parsed {
                values: Vec::new(),
                warnings: vec![override_warning(
                    WarningKind::MalformedOverride,
                    message,
                    controllers,
                )],
            };
        }
    };
    let values_group = match properties::unique_list(root_run, *b"tdgp") {
        Ok(group) => group,
        Err(error) => {
            return Parsed {
                values: Vec::new(),
                warnings: vec![override_warning(
                    WarningKind::MalformedOverride,
                    format!("override value group is malformed: {error}"),
                    controllers,
                )],
            };
        }
    };
    let mut slots = Vec::new();
    if let Err(error) = flatten_override_slots(values_group, &mut slots) {
        return Parsed {
            values: Vec::new(),
            warnings: vec![override_warning(
                WarningKind::MalformedOverride,
                error,
                controllers,
            )],
        };
    }
    if uuids.len() != slots.len() {
        warnings.push(override_warning(
            WarningKind::MalformedOverride,
            format!(
                "OvG2 declares {} UUIDs but the override tree contains {} preorder nodes; all overrides ignored and original source values retained",
                uuids.len(),
                slots.len()
            ),
            controllers,
        ));
        return Parsed {
            values: Vec::new(),
            warnings,
        };
    }

    let mut values = Vec::new();
    for (position, (uuid, slot)) in uuids.iter().zip(slots).enumerate() {
        let matches: Vec<_> = controllers
            .iter()
            .filter(|controller| controller.uuid == *uuid)
            .collect();
        let [controller] = matches.as_slice() else {
            warnings.push(Warning {
                kind: WarningKind::UnmatchedController,
                message: format!(
                    "override {position} ({}) matches {} controllers",
                    slot.match_name,
                    matches.len()
                ),
                affects_media: slot.match_name == "ADBE Layer Source Alternate",
            });
            continue;
        };
        if controller.controller_type == GROUP_CONTROLLER || slot.is_group {
            warnings.push(warning(
                WarningKind::UnsupportedGroup,
                format!(
                    "override {position} ({}) is a grouping controller and carries no source value",
                    slot.match_name
                ),
            ));
            continue;
        }
        let (Some(source_comp_id), Some(source_layer_id)) =
            (controller.source_comp_id, controller.source_layer_id)
        else {
            warnings.push(Warning {
                kind: WarningKind::MalformedController,
                message: format!(
                    "override {position} ({}) has no source composition/layer identity",
                    slot.match_name
                ),
                affects_media: controller.controller_type == MEDIA_REPLACEMENT_CONTROLLER,
            });
            continue;
        };
        if controller.controller_type == MEDIA_REPLACEMENT_CONTROLLER {
            let source_id = if slot.match_name == "ADBE Layer Source Alternate" {
                optional_u32(&slot.raw_chunks, *b"blsi", "alternate source ID")
            } else {
                Err(format!(
                    "unexpected media override property {}",
                    slot.match_name
                ))
            };
            match source_id {
                Ok(Some(source_id)) if source_id != 0 => values.push(Override {
                    source_comp_id,
                    source_layer_id,
                    value: OverrideValue::Media { source_id },
                }),
                Ok(_) => {} // No alternate source configured; retain the source.
                Err(message) => warnings.push(media_warning(
                    WarningKind::MalformedOverride,
                    format!("override {position}: {message}; original source retained"),
                )),
            }
            continue;
        }
        if controller.path.is_empty() {
            warnings.push(warning(
                WarningKind::MalformedController,
                format!(
                    "override {position} ({}) has no source property path",
                    slot.match_name
                ),
            ));
            continue;
        }
        if slot.raw_chunks.is_empty() {
            warnings.push(warning(
                WarningKind::MalformedOverride,
                format!(
                    "override {position} ({}) has no value storage; source value retained",
                    slot.match_name
                ),
            ));
            continue;
        }
        values.push(Override {
            source_comp_id,
            source_layer_id,
            value: OverrideValue::Property {
                path: controller.path.clone(),
                chunks: slot.raw_chunks,
            },
        });
    }
    Parsed { values, warnings }
}

/// Applies one override to an occurrence-local cloned source layer.
///
/// `Err` guarantees that the layer was not changed. `Ok` can contain warnings
/// for unsupported source metadata which was deliberately replaced along with
/// the overridden native storage.
pub(crate) fn apply(
    layer: &mut Layer,
    property_override: &Override,
) -> Result<Vec<Warning>, Warning> {
    let (path, chunks) = match &property_override.value {
        OverrideValue::Media { source_id } => {
            if layer.record.layer_type() != 0 || *source_id == 0 {
                return Err(warning(
                    WarningKind::UnresolvedSourcePath,
                    "media replacement requires an AV layer and nonzero source ID".into(),
                ));
            }
            let mut bytes = layer.record.encode();
            bytes[40..44].copy_from_slice(&source_id.to_be_bytes());
            layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes)
                .map_err(|error| warning(WarningKind::MalformedOverride, error.to_string()))?;
            return Ok(Vec::new());
        }
        OverrideValue::Property { path, chunks } => (path, chunks),
    };
    if chunks.is_empty() {
        return Err(warning(
            WarningKind::MalformedOverride,
            "override has no value storage; source value retained".into(),
        ));
    }
    let roots: Vec<_> = layer
        .content
        .iter()
        .enumerate()
        .filter(|(_, chunk)| chunk.list_kind() == Some(*b"tdgp"))
        .map(|(index, _)| index)
        .collect();
    let [root_index] = roots.as_slice() else {
        return Err(warning(
            WarningKind::UnresolvedSourcePath,
            format!("source layer has {} property roots", roots.len()),
        ));
    };
    let root = layer.content[*root_index].children_mut().ok_or_else(|| {
        warning(
            WarningKind::UnresolvedSourcePath,
            "source property root is opaque".into(),
        )
    })?;
    apply_path(root, path, None, chunks)
}

fn parse_controller(children: &[Chunk]) -> Result<Controller, String> {
    let controller_type = unique_u32(children, *b"CTyp", "controller type")?;
    let type_index = children
        .iter()
        .position(|chunk| chunk.id() == *b"CTyp")
        .ok_or_else(|| "missing controller type".to_owned())?;
    // CCtl's UUID precedes CTyp. Media controllers also store asset UUIDs and
    // a cached filename AFTER CTyp; those are not controller identities.
    let uuid = unique_direct_utf8(&children[..type_index], "controller UUID")?;
    if uuid.is_empty() {
        return Err("controller UUID is empty".into());
    }
    let cprps: Vec<_> = children
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"CPrp"))
        .collect();
    if cprps.len() > 1 {
        return Err("duplicate CPrp source references".into());
    }
    let (source_comp_id, source_layer_id, path) = if let Some(cprp) = cprps.first() {
        let children = cprp
            .children()
            .ok_or_else(|| "opaque CPrp source reference".to_owned())?;
        let source_comp_id = optional_u32(children, *b"CCId", "source composition ID")?;
        let source_layer_id = optional_u32(children, *b"CLId", "source layer ID")?;
        let paths: Vec<_> = children
            .iter()
            .filter(|chunk| chunk.id() == *b"Utf8")
            .collect();
        let path = match paths.as_slice() {
            [] => Vec::new(),
            [path] => parse_path(
                std::str::from_utf8(
                    path.data_payload()
                        .ok_or_else(|| "CPrp path Utf8 is not data".to_owned())?,
                )
                .map_err(|_| "CPrp path is not UTF-8".to_owned())?,
            )?,
            _ => return Err("duplicate CPrp path Utf8 chunks".into()),
        };
        (source_comp_id, source_layer_id, path)
    } else {
        (None, None, Vec::new())
    };
    Ok(Controller {
        uuid,
        controller_type,
        source_comp_id,
        source_layer_id,
        path,
    })
}

fn parse_path(raw: &str) -> Result<Vec<SourcePropertyRef>, String> {
    let Value::Object(nodes) =
        serde_json::from_str(raw).map_err(|error| format!("invalid CPrp path JSON: {error}"))?
    else {
        return Err("CPrp path JSON is not an object".into());
    };
    let mut ordered = Vec::with_capacity(nodes.len());
    for (position, node) in nodes {
        let position = position
            .parse::<usize>()
            .map_err(|_| "CPrp path position is not numeric".to_owned())?;
        ordered.push((position, node));
    }
    ordered.sort_unstable_by_key(|(position, _)| *position);
    let mut path = Vec::with_capacity(ordered.len());
    for (expected, (position, node)) in ordered.into_iter().enumerate() {
        if position != expected {
            return Err("CPrp path positions are not contiguous from zero".into());
        }
        let Value::Object(node) = node else {
            return Err("CPrp path node is not an object".into());
        };
        let match_name = node
            .get("matchName")
            .and_then(Value::as_str)
            .ok_or_else(|| "CPrp path node has no matchName".to_owned())?;
        if match_name.is_empty() {
            return Err("CPrp path node has an empty matchName".into());
        }
        let index = node
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| "CPrp path node has no unsigned index".to_owned())?;
        let child_index = if index == BY_NAME_INDEX {
            None
        } else {
            Some(u32::try_from(index).map_err(|_| "CPrp child index exceeds u32".to_owned())?)
        };
        path.push(SourcePropertyRef {
            match_name: match_name.to_owned(),
            child_index,
        });
    }
    Ok(path)
}

fn override_uuids(root_run: &[Chunk]) -> Result<Vec<String>, String> {
    let ovg2s: Vec<_> = root_run
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"OvG2"))
        .collect();
    let [ovg2] = ovg2s.as_slice() else {
        return if ovg2s.is_empty() {
            Ok(Vec::new())
        } else {
            Err("duplicate OvG2 metadata containers".into())
        };
    };
    let children = ovg2
        .children()
        .ok_or_else(|| "opaque OvG2 metadata container".to_owned())?;
    let mut uuids = Vec::new();
    for (position, cprp) in children
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"CPrp"))
        .enumerate()
    {
        let children = cprp
            .children()
            .ok_or_else(|| format!("override UUID {position} has an opaque CPrp"))?;
        uuids.push(unique_direct_utf8(children, "override UUID")?);
    }
    Ok(uuids)
}

#[derive(Debug)]
struct OverrideSlot {
    match_name: String,
    is_group: bool,
    raw_chunks: Vec<Chunk>,
}

fn flatten_override_slots(
    children: &[Chunk],
    result: &mut Vec<OverrideSlot>,
) -> Result<(), String> {
    let mut pending: Vec<_> = properties::runs(children)
        .map_err(|error| error.to_string())?
        .into_iter()
        .rev()
        .collect();
    while let Some((match_name, run)) = pending.pop() {
        let groups: Vec<_> = run
            .iter()
            .filter(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .collect();
        if groups.len() > 1 {
            return Err(format!("override {match_name} has duplicate tdgp storage"));
        }
        let is_group = groups.len() == 1;
        result.push(OverrideSlot {
            match_name: match_name.to_owned(),
            is_group,
            raw_chunks: if is_group { Vec::new() } else { run.to_vec() },
        });
        if let Some(group) = groups.first() {
            let nested = group
                .children()
                .ok_or_else(|| format!("override group {match_name} is opaque"))?;
            pending.extend(
                properties::runs(nested)
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .rev(),
            );
        }
    }
    Ok(())
}

#[derive(Debug)]
struct RunRange {
    name: String,
    marker: usize,
    end: usize,
}

fn run_ranges(children: &[Chunk]) -> Result<Vec<RunRange>, Warning> {
    let mut starts = Vec::new();
    for (index, chunk) in children.iter().enumerate() {
        if chunk.id() != *b"tdmn" {
            continue;
        }
        let name = decode_match_name(chunk).ok_or_else(|| {
            warning(
                WarningKind::UnresolvedSourcePath,
                "source property has malformed tdmn identity".into(),
            )
        })?;
        starts.push((index, name));
    }
    let mut result = Vec::with_capacity(starts.len());
    for (position, (marker, name)) in starts.iter().enumerate() {
        if name == "ADBE Group End" {
            continue;
        }
        result.push(RunRange {
            name: name.clone(),
            marker: *marker,
            end: starts
                .get(position + 1)
                .map_or(children.len(), |(index, _)| *index),
        });
    }
    Ok(result)
}

fn apply_path<'a>(
    children: &mut Vec<Chunk>,
    path: &'a [SourcePropertyRef],
    mut parent_name: Option<&'a str>,
    replacement: &[Chunk],
) -> Result<Vec<Warning>, Warning> {
    if path.is_empty() {
        return Err(warning(
            WarningKind::UnresolvedSourcePath,
            "source property path is empty".into(),
        ));
    }

    let mut children = children;
    for (position, head) in path.iter().enumerate() {
        let is_leaf = position + 1 == path.len();
        let ranges = run_ranges(children)?;
        let selected = match head.child_index {
            Some(index) => ranges
                .get(usize::try_from(index).map_err(|_| {
                    warning(
                        WarningKind::UnresolvedSourcePath,
                        format!("source child index {index} does not fit usize"),
                    )
                })?)
                .filter(|range| range.name == head.match_name),
            None => {
                let matches: Vec<_> = ranges
                    .iter()
                    .filter(|range| range.name == head.match_name)
                    .collect();
                match matches.as_slice() {
                    [range] => Some(*range),
                    _ => None,
                }
            }
        };

        let Some(selected) = selected else {
            if is_leaf
                && head.child_index.is_none()
                && !ranges.iter().any(|range| range.name == head.match_name)
                && is_implicit_numeric_leaf(parent_name, &head.match_name)
                && replacement
                    .iter()
                    .any(|chunk| chunk.list_kind() == Some(*b"tdbs"))
            {
                properties::unique_list(replacement, *b"tdbs")
                    .and_then(read_numeric)
                    .map_err(|error| {
                        warning(
                            WarningKind::MalformedOverride,
                            format!(
                                "{} override is not valid numeric storage: {error}; source default retained",
                                head.match_name
                            ),
                        )
                    })?;
                let insert_at = children
                    .iter()
                    .position(|chunk| decode_match_name(chunk).as_deref() == Some("ADBE Group End"))
                    .unwrap_or(children.len());
                let mut inserted = Vec::with_capacity(replacement.len() + 1);
                inserted.push(match_name_chunk(&head.match_name)?);
                inserted.extend_from_slice(replacement);
                children.splice(insert_at..insert_at, inserted);
                return Ok(Vec::new());
            }
            return Err(warning(
                WarningKind::UnresolvedSourcePath,
                match head.child_index {
                    Some(index) => format!(
                        "indexed source child {index} does not have matchName {}",
                        head.match_name
                    ),
                    None => format!(
                        "named source child {} is absent or ambiguous",
                        head.match_name
                    ),
                },
            ));
        };

        if is_leaf {
            let source_storage = &children[selected.marker + 1..selected.end];
            let warnings = source_storage_warnings(&head.match_name, source_storage);
            children.splice(
                selected.marker + 1..selected.end,
                replacement.iter().cloned(),
            );
            return Ok(warnings);
        }

        let group_positions: Vec<_> = children[selected.marker + 1..selected.end]
            .iter()
            .enumerate()
            .filter(|(_, chunk)| chunk.list_kind() == Some(*b"tdgp"))
            .map(|(offset, _)| selected.marker + 1 + offset)
            .collect();
        let [group_position] = group_positions.as_slice() else {
            return Err(warning(
                WarningKind::UnresolvedSourcePath,
                format!("source path descends through non-group {}", head.match_name),
            ));
        };
        children = children[*group_position].children_mut().ok_or_else(|| {
            warning(
                WarningKind::UnresolvedSourcePath,
                format!("source group {} is opaque", head.match_name),
            )
        })?;
        parent_name = Some(&head.match_name);
    }
    unreachable!("non-empty paths return from their leaf")
}

fn source_storage_warnings(match_name: &str, storage: &[Chunk]) -> Vec<Warning> {
    let tdbs: Vec<_> = storage
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"tdbs"))
        .collect();
    let [tdbs] = tdbs.as_slice() else {
        return Vec::new();
    };
    let Some(children) = tdbs.children() else {
        return vec![warning(
            WarningKind::UnsupportedSourceMetadata,
            format!("{match_name}: opaque numeric source storage was replaced"),
        )];
    };
    match read_numeric(children) {
        Ok(numeric) if numeric.expression_present => vec![warning(
            WarningKind::UnsupportedSourceMetadata,
            format!(
                "{match_name}: source expression metadata is not portable and was replaced by the occurrence override{}",
                if numeric.expression_enabled {
                    " (expression was enabled)"
                } else {
                    ""
                }
            ),
        )],
        Ok(_) => Vec::new(),
        Err(error) => vec![warning(
            WarningKind::UnsupportedSourceMetadata,
            format!("{match_name}: unsupported numeric source metadata was replaced: {error}"),
        )],
    }
}

/// Named leaves AE may omit when their values equal native defaults. The
/// vector names follow shapes/defaults.rs (pinned py-aep vector PropSpecs).
/// Only the supplied override storage is inserted; no values or groups are
/// invented, and indexed/ambiguous paths never reach this allowlist.
fn is_implicit_numeric_leaf(parent: Option<&str>, name: &str) -> bool {
    let Some(parent) = parent else {
        return false;
    };
    match parent {
        "ADBE Transform Group" => is_transform_leaf(name),
        "ADBE Vector Transform Group" => matches!(
            name,
            "ADBE Vector Anchor"
                | "ADBE Vector Position"
                | "ADBE Vector Scale"
                | "ADBE Vector Rotation"
                | "ADBE Vector Skew"
                | "ADBE Vector Skew Axis"
                | "ADBE Vector Group Opacity"
        ),
        "ADBE Vector Shape - Rect" => matches!(
            name,
            "ADBE Vector Rect Size"
                | "ADBE Vector Rect Position"
                | "ADBE Vector Rect Roundness"
                | "ADBE Vector Shape Direction"
        ),
        "ADBE Vector Shape - Ellipse" => matches!(
            name,
            "ADBE Vector Ellipse Size"
                | "ADBE Vector Ellipse Position"
                | "ADBE Vector Shape Direction"
        ),
        "ADBE Vector Shape - Star" => matches!(
            name,
            "ADBE Vector Star Type"
                | "ADBE Vector Star Points"
                | "ADBE Vector Star Position"
                | "ADBE Vector Star Rotation"
                | "ADBE Vector Star Inner Radius"
                | "ADBE Vector Star Outer Radius"
                | "ADBE Vector Star Inner Roundess"
                | "ADBE Vector Star Outer Roundess"
                | "ADBE Vector Shape Direction"
        ),
        "ADBE Vector Shape - Group" => name == "ADBE Vector Shape Direction",
        "ADBE Vector Graphic - Fill"
        | "ADBE Vector Graphic - G-Fill"
        | "ADBE Vector Graphic - Stroke"
        | "ADBE Vector Graphic - G-Stroke" => {
            let fill = matches!(
                parent,
                "ADBE Vector Graphic - Fill" | "ADBE Vector Graphic - G-Fill"
            );
            let gradient = matches!(
                parent,
                "ADBE Vector Graphic - G-Fill" | "ADBE Vector Graphic - G-Stroke"
            );
            matches!(
                name,
                "ADBE Vector Blend Mode" | "ADBE Vector Composite Order"
            ) || (fill && matches!(name, "ADBE Vector Fill Opacity" | "ADBE Vector Fill Rule"))
                || (!fill
                    && matches!(
                        name,
                        "ADBE Vector Stroke Opacity"
                            | "ADBE Vector Stroke Width"
                            | "ADBE Vector Stroke Line Cap"
                            | "ADBE Vector Stroke Line Join"
                            | "ADBE Vector Stroke Miter Limit"
                    ))
                || (!gradient
                    && name
                        == if fill {
                            "ADBE Vector Fill Color"
                        } else {
                            "ADBE Vector Stroke Color"
                        })
                || (gradient
                    && matches!(
                        name,
                        "ADBE Vector Grad Type"
                            | "ADBE Vector Grad Start Pt"
                            | "ADBE Vector Grad End Pt"
                            | "ADBE Vector Grad HiLite Length"
                            | "ADBE Vector Grad HiLite Angle"
                            | "ADBE Vector Grad Rotation"
                            | "ADBE Vector Grad Scale"
                    ))
        }
        "ADBE Vector Stroke Dashes" => name == "ADBE Vector Stroke Offset",
        "ADBE Vector Filter - Trim" => matches!(
            name,
            "ADBE Vector Trim Type"
                | "ADBE Vector Trim Start"
                | "ADBE Vector Trim End"
                | "ADBE Vector Trim Offset"
        ),
        "ADBE Vector Filter - RC" => name == "ADBE Vector RoundCorner Radius",
        "ADBE Vector Filter - Offset" => matches!(
            name,
            "ADBE Vector Offset Amount"
                | "ADBE Vector Offset Line Join"
                | "ADBE Vector Offset Miter Limit"
                | "ADBE Vector Offset Copies"
                | "ADBE Vector Offset Copy Offset"
        ),
        "ADBE Vector Filter - Merge" => name == "ADBE Vector Merge Type",
        _ => false,
    }
}

fn is_transform_leaf(name: &str) -> bool {
    matches!(
        name,
        "ADBE Anchor Point"
            | "ADBE Position"
            | "ADBE Position_0"
            | "ADBE Position_1"
            | "ADBE Position_2"
            | "ADBE Orientation"
            | "ADBE Rotate X"
            | "ADBE Rotate Y"
            | "ADBE Scale"
            | "ADBE Rotate Z"
            | "ADBE Opacity"
    )
}

fn match_name_chunk(name: &str) -> Result<Chunk, Warning> {
    if name.len() > 40 || name.contains('\0') {
        return Err(warning(
            WarningKind::MalformedOverride,
            format!("matchName {name:?} cannot be represented by native tdmn"),
        ));
    }
    let mut payload = vec![0; 40];
    payload[..name.len()].copy_from_slice(name.as_bytes());
    Chunk::data(*b"tdmn", payload).map_err(|error| {
        warning(
            WarningKind::MalformedOverride,
            format!("cannot construct native tdmn: {error}"),
        )
    })
}

fn decode_match_name(chunk: &Chunk) -> Option<String> {
    let bytes = chunk.data_payload()?;
    if bytes.len() != 40 {
        return None;
    }
    let end = bytes
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |index| index + 1);
    let name = std::str::from_utf8(&bytes[..end]).ok()?;
    (!name.contains('\0')).then(|| name.to_owned())
}

fn unique_direct_utf8(children: &[Chunk], field: &str) -> Result<String, String> {
    let values: Vec<_> = children
        .iter()
        .filter(|chunk| chunk.id() == *b"Utf8")
        .collect();
    let [value] = values.as_slice() else {
        return Err(format!("{field} count is {}", values.len()));
    };
    let bytes = value
        .data_payload()
        .ok_or_else(|| format!("{field} is not data"))?;
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| format!("{field} is not UTF-8"))
}

fn unique_u32(children: &[Chunk], id: [u8; 4], field: &str) -> Result<u32, String> {
    optional_u32(children, id, field)?.ok_or_else(|| format!("missing {field}"))
}

fn optional_u32(children: &[Chunk], id: [u8; 4], field: &str) -> Result<Option<u32>, String> {
    let values: Vec<_> = children.iter().filter(|chunk| chunk.id() == id).collect();
    match values.as_slice() {
        [] => Ok(None),
        [value] => {
            let bytes = value
                .data_payload()
                .ok_or_else(|| format!("{field} is not data"))?;
            let bytes: [u8; 4] = bytes
                .try_into()
                .map_err(|_| format!("{field} is not four bytes"))?;
            Ok(Some(u32::from_be_bytes(bytes)))
        }
        _ => Err(format!("duplicate {field}")),
    }
}

fn warning(kind: WarningKind, message: String) -> Warning {
    Warning {
        kind,
        message,
        affects_media: false,
    }
}

fn media_warning(kind: WarningKind, message: String) -> Warning {
    Warning {
        kind,
        message,
        affects_media: true,
    }
}

fn override_warning(kind: WarningKind, message: String, controllers: &[Controller]) -> Warning {
    Warning {
        kind,
        message,
        affects_media: controllers
            .iter()
            .any(|controller| controller.controller_type == MEDIA_REPLACEMENT_CONTROLLER),
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::{aep, properties::read_transform, structure};

    const AEP: &[u8] = include_bytes!("../tests/fixtures/essential/multiple_controllers.aep");

    #[test]
    fn native_fixture_provenance_and_projection_are_pinned() {
        assert_eq!(AEP.len(), 141_003);
        assert_eq!(
            format!("{:x}", Sha256::digest(AEP)),
            "df08145c4be5d3547b1bb5650b48be777f8663d0d88dc472f65990f0ac37c6d3"
        );
        let projection = crate::test_fixtures::read("essential/multiple_controllers.json.gz");
        assert_eq!(projection.len(), 523_479);
        assert_eq!(
            format!("{:x}", Sha256::digest(&projection)),
            "467d5e339a4686c3029ba3384c7fe9e1a4b44ee6e8cd297eeb4278c6e052bace"
        );
        let projection: Value = serde_json::from_slice(&projection).unwrap();
        let items = projection["items"].as_array().unwrap();
        let main = items.iter().find(|item| item["name"] == "main").unwrap();
        assert_eq!(main["id"], 16);
        assert_eq!(main["layers"][0]["id"], 28);
        assert_eq!(main["layers"][0]["sourceId"], 1);
        let primary = items.iter().find(|item| item["name"] == "primary").unwrap();
        assert_eq!(primary["id"], 1);
        assert_eq!(primary["motionGraphicsTemplateControllerCount"], 3);
        assert_eq!(primary["layers"][0]["id"], 15);
    }

    #[test]
    fn review_import_duplicate_newest_essential_containers_fail_closed() {
        let envelope = aep::Project::parse(AEP).unwrap();
        let original = find_controller_item(&envelope.chunks).unwrap();
        let newest = original
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"CIF3"))
            .expect("pinned native fixture must contain CIF3")
            .clone();

        for insert_at in [0, original.len()] {
            let mut duplicated = original.to_vec();
            duplicated.insert(insert_at, newest.clone());
            let parsed = controllers(&duplicated);
            assert!(
                parsed.values.is_empty(),
                "ambiguous controllers must not depend on duplicate order"
            );
            assert!(parsed.warnings.iter().any(|warning| {
                warning.kind == WarningKind::MalformedController
                    && warning.message.contains("CIF3")
                    && warning.message.contains("duplicate")
                    && warning.message.contains("source")
            }));
        }
    }

    #[test]
    fn native_transform_override_is_applied_only_to_the_clone() {
        let envelope = aep::Project::parse(AEP).unwrap();
        let comp_children = find_controller_item(&envelope.chunks).unwrap();
        let parsed_controllers = controllers(comp_children);
        assert!(
            parsed_controllers.warnings.is_empty(),
            "{:?}",
            parsed_controllers.warnings
        );
        assert_eq!(parsed_controllers.values.len(), 3);
        let opacity = parsed_controllers
            .values
            .iter()
            .find(|controller| {
                controller
                    .path
                    .last()
                    .is_some_and(|node| node.match_name == "ADBE Opacity")
            })
            .unwrap();
        assert_eq!(
            (opacity.source_comp_id, opacity.source_layer_id),
            (Some(1), Some(15))
        );

        let project = structure::read_project(AEP).unwrap();
        let occurrence = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                structure::ItemKind::Composition(comp) => {
                    comp.layers.iter().find(|layer| layer.record.id() == 28)
                }
                _ => None,
            })
            .unwrap();
        let parsed_overrides = overrides(&occurrence.content, &parsed_controllers.values);
        assert!(
            parsed_overrides.warnings.is_empty(),
            "{:?}",
            parsed_overrides.warnings
        );
        assert_eq!(parsed_overrides.values.len(), 3);
        assert!(parsed_overrides.values.iter().all(|property_override| {
            (
                property_override.source_comp_id,
                property_override.source_layer_id,
            ) == (1, 15)
        }));
        let opacity_override = parsed_overrides
            .values
            .iter()
            .find(|property_override| {
                matches!(&property_override.value, OverrideValue::Property { path, .. }
                    if path.last().is_some_and(|node| node.match_name == "ADBE Opacity"))
            })
            .unwrap();

        let source = project
            .items
            .iter()
            .find(|item| item.id == 1)
            .and_then(|item| match &item.kind {
                structure::ItemKind::Composition(comp) => {
                    comp.layers.iter().find(|layer| layer.record.id() == 15)
                }
                _ => None,
            })
            .unwrap();
        let original = source.clone();
        let mut cloned = source.clone();
        assert_eq!(transform_value(&cloned, "ADBE Opacity"), None);
        let warnings = apply(&mut cloned, opacity_override).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(transform_value(&cloned, "ADBE Opacity"), Some(vec![1.0]));
        assert_ne!(cloned.content, original.content);
        assert_eq!(transform_value(source, "ADBE Opacity"), None);
        assert_eq!(source, &original);
    }

    #[test]
    fn supplemental_uuid_slot_cardinality_mismatch_retains_source_values() {
        let envelope = aep::Project::parse(AEP).unwrap();
        let comp_children = find_controller_item(&envelope.chunks).unwrap();
        let parsed_controllers = controllers(comp_children);
        assert!(parsed_controllers.warnings.is_empty());

        let project = structure::read_project(AEP).unwrap();
        let occurrence = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                structure::ItemKind::Composition(comp) => {
                    comp.layers.iter().find(|layer| layer.record.id() == 28)
                }
                _ => None,
            })
            .unwrap();
        let control = overrides(&occurrence.content, &parsed_controllers.values);
        assert!(control.warnings.is_empty(), "{:?}", control.warnings);
        assert_eq!(control.values.len(), 3);

        // Supplemental mutation of the pinned native fixture, not independent Adobe proof.
        fn remove_one_uuid(chunks: &mut [Chunk]) -> bool {
            for chunk in chunks.iter_mut() {
                if chunk.list_kind() == Some(*b"OvG2") {
                    let children = chunk.children_mut().unwrap();
                    if let Some(position) = children
                        .iter()
                        .position(|child| child.list_kind() == Some(*b"CPrp"))
                    {
                        children.remove(position);
                        return true;
                    }
                }
                if let Some(children) = chunk.children_mut()
                    && remove_one_uuid(children)
                {
                    return true;
                }
            }
            false
        }

        let mut mismatched_content = occurrence.content.clone();
        assert!(remove_one_uuid(&mut mismatched_content));
        let mismatched = overrides(&mismatched_content, &parsed_controllers.values);
        assert!(mismatched.values.is_empty());
        assert!(mismatched.warnings.iter().any(|warning| {
            warning.kind == WarningKind::MalformedOverride
                && warning.message.contains("declares 2 UUIDs")
                && warning.message.contains("3 preorder nodes")
                && warning.message.contains("original source values retained")
                && !warning.affects_media
        }));
    }

    #[test]
    fn native_media_replacement_changes_only_the_occurrence_clone() {
        let bytes = include_bytes!("../tests/fixtures/media-replacement/media_replacement.aep");
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            "dcb61584658d6ec02fa9239c2035a9e7a0e218d367935949166a97ba0d9313e4"
        );
        let projection = crate::test_fixtures::read("media-replacement/media_replacement.json.gz");
        assert_eq!(
            format!("{:x}", Sha256::digest(&projection)),
            "c970ea7013b9b8a286aba52235fb4b0795068c0a415376c59fc6af22cca848a1"
        );
        let projection: Value = serde_json::from_slice(&projection).unwrap();
        let projected = projection["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == 30)
            .unwrap();
        assert_eq!(projected["name"], "image_with_alpha.png_sequence");
        let project = structure::read_project(bytes).unwrap();
        let structure::ItemKind::Composition(source) = &project.item(2).unwrap().kind else {
            panic!("source comp")
        };
        let structure::ItemKind::Composition(parent) = &project.item(15).unwrap().kind else {
            panic!("parent comp")
        };
        let occurrence = parent
            .layers
            .iter()
            .find(|layer| layer.record.id() == 27)
            .unwrap();
        let parsed = overrides(&occurrence.content, &source.essential_properties.values);
        assert_eq!(
            parsed.values.len(),
            1,
            "source={:?}; override={:?}",
            source.essential_properties,
            parsed.warnings
        );
        let replacement = &parsed.values[0];
        assert_eq!(
            (replacement.source_comp_id, replacement.source_layer_id),
            (2, 14)
        );
        let original = source
            .layers
            .iter()
            .find(|layer| layer.record.id() == 14)
            .unwrap();
        let mut cloned = original.clone();
        assert!(apply(&mut cloned, replacement).unwrap().is_empty());
        assert_eq!(cloned.record.source_id(), 30);
        assert_ne!(original.record.source_id(), 30);
        let mut expected = original.record.encode();
        expected[40..44].copy_from_slice(&30u32.to_be_bytes());
        assert_eq!(cloned.record.encode(), expected);
        assert_eq!(cloned.content, original.content);

        // Supplemental corruption: invalid optional storage cannot wipe the source.
        let mut corrupt = occurrence.clone();
        fn corrupt_ids(chunks: &mut [Chunk]) {
            for chunk in chunks {
                if chunk.id() == *b"blsi" {
                    *chunk = Chunk::data(*b"blsi", [0u8; 3]).unwrap();
                } else if let Some(children) = chunk.children_mut() {
                    corrupt_ids(children);
                }
            }
        }
        corrupt_ids(&mut corrupt.content);
        let malformed = overrides(&corrupt.content, &source.essential_properties.values);
        assert!(malformed.values.is_empty());
        assert!(malformed.warnings.iter().any(|warning| {
            warning.kind == WarningKind::MalformedOverride && warning.affects_media
        }));

        let mut non_av = original.clone();
        let mut bytes = non_av.record.encode();
        bytes[131] = 3;
        non_av.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
        let before = non_av.clone();
        assert!(apply(&mut non_av, replacement).is_err());
        assert_eq!(non_av, before);
    }

    #[test]
    fn stale_index_does_not_select_an_arbitrary_duplicate() {
        let storage = numeric_storage(42.0);
        let mut layer = layer_with_transform(vec![
            named_leaf("ADBE Opacity", numeric_storage(10.0)),
            named_leaf("ADBE Opacity", numeric_storage(20.0)),
        ]);
        let original = layer.clone();
        let property_override = Override {
            source_comp_id: 1,
            source_layer_id: 2,
            value: OverrideValue::Property {
                path: vec![
                    SourcePropertyRef {
                        match_name: "ADBE Transform Group".into(),
                        child_index: None,
                    },
                    SourcePropertyRef {
                        match_name: "ADBE Opacity".into(),
                        child_index: Some(0),
                    },
                ],
                chunks: storage,
            },
        };
        assert!(apply(&mut layer, &property_override).is_ok());
        assert_ne!(layer, original);

        let mut stale = original.clone();
        let mut stale_override = property_override;
        let OverrideValue::Property { path, .. } = &mut stale_override.value else {
            panic!("property")
        };
        path[1].child_index = Some(2);
        assert_eq!(
            apply(&mut stale, &stale_override).unwrap_err().kind,
            WarningKind::UnresolvedSourcePath
        );
        assert_eq!(stale, original);
    }

    #[test]
    fn missing_transform_leaf_is_inserted_but_missing_value_never_wipes_source() {
        let mut layer = layer_with_transform(Vec::new());
        let property_override = Override {
            source_comp_id: 1,
            source_layer_id: 2,
            value: OverrideValue::Property {
                path: vec![
                    SourcePropertyRef {
                        match_name: "ADBE Transform Group".into(),
                        child_index: None,
                    },
                    SourcePropertyRef {
                        match_name: "ADBE Opacity".into(),
                        child_index: None,
                    },
                ],
                chunks: numeric_storage(37.0),
            },
        };
        apply(&mut layer, &property_override).unwrap();
        assert_eq!(transform_value(&layer, "ADBE Opacity"), Some(vec![37.0]));

        let before = layer.clone();
        let mut empty = property_override;
        let OverrideValue::Property { chunks, .. } = &mut empty.value else {
            panic!("property")
        };
        chunks.clear();
        assert!(apply(&mut layer, &empty).is_err());
        assert_eq!(layer, before);
    }

    #[test]
    fn missing_vector_defaults_accept_only_named_known_leaves() {
        // Supplemental native-record construction, not an independently authored
        // AE Essential Shape-override fixture or a render-fidelity assertion.
        for (parent, name, value) in [
            ("ADBE Vector Transform Group", "ADBE Vector Rotation", 45.0),
            (
                "ADBE Vector Shape - Rect",
                "ADBE Vector Rect Roundness",
                12.0,
            ),
            (
                "ADBE Vector Graphic - Fill",
                "ADBE Vector Fill Opacity",
                37.0,
            ),
            (
                "ADBE Vector Graphic - Stroke",
                "ADBE Vector Stroke Width",
                8.0,
            ),
            (
                "ADBE Vector Stroke Dashes",
                "ADBE Vector Stroke Offset",
                5.0,
            ),
            ("ADBE Vector Filter - Trim", "ADBE Vector Trim End", 60.0),
        ] {
            let source = vec![match_name_chunk("ADBE Group End").unwrap()];
            let mut instance = source.clone();
            let path = [SourcePropertyRef {
                match_name: name.into(),
                child_index: None,
            }];
            let replacement = numeric_storage(value);
            assert!(
                apply_path(&mut instance, &path, Some(parent), &replacement)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(source.len(), 1, "source remains untouched");
            assert_eq!(decode_match_name(&instance[0]).as_deref(), Some(name));
            assert_eq!(&instance[1..instance.len() - 1], &replacement);
            assert_eq!(
                read_numeric(properties::unique_list(&replacement, *b"tdbs").unwrap())
                    .unwrap()
                    .values,
                vec![value]
            );
            assert_eq!(
                decode_match_name(instance.last().unwrap()).as_deref(),
                Some("ADBE Group End")
            );

            // A stale native index must not silently become a named insertion.
            let mut indexed = source.clone();
            let indexed_path = [SourcePropertyRef {
                match_name: name.into(),
                child_index: Some(0),
            }];
            assert!(apply_path(&mut indexed, &indexed_path, Some(parent), &replacement).is_err());
            assert_eq!(indexed, source);
        }
    }

    #[test]
    fn missing_unknown_or_wrong_parent_numeric_leaf_is_not_invented() {
        for (parent, name) in [
            ("ADBE Vector Shape - Rect", "ADBE Vector Stroke Width"),
            ("ADBE Vector Graphic - G-Fill", "ADBE Vector Fill Color"),
            ("ADBE Vector Graphic - Fill", "vendor property"),
            ("ADBE Text Properties", "ADBE Text Document"),
        ] {
            let mut children = vec![match_name_chunk("ADBE Group End").unwrap()];
            let original = children.clone();
            let path = [SourcePropertyRef {
                match_name: name.into(),
                child_index: None,
            }];
            assert!(
                apply_path(&mut children, &path, Some(parent), &numeric_storage(42.0)).is_err()
            );
            assert_eq!(children, original);
        }
    }

    #[test]
    fn malformed_numeric_override_does_not_replace_an_implicit_default() {
        let mut children = vec![match_name_chunk("ADBE Group End").unwrap()];
        let original = children.clone();
        let path = [SourcePropertyRef {
            match_name: "ADBE Vector Stroke Width".into(),
            child_index: None,
        }];
        assert_eq!(
            apply_path(
                &mut children,
                &path,
                Some("ADBE Vector Graphic - Stroke"),
                &[Chunk::list(*b"tdbs", Vec::new())],
            )
            .unwrap_err()
            .kind,
            WarningKind::MalformedOverride
        );
        assert_eq!(children, original);
    }

    #[test]
    fn ambiguous_named_default_is_not_inserted_as_a_third_property() {
        for (parent, name) in [
            ("ADBE Transform Group", "ADBE Opacity"),
            ("ADBE Vector Graphic - Stroke", "ADBE Vector Stroke Width"),
        ] {
            let mut children: Vec<_> = [
                named_leaf(name, numeric_storage(10.0)),
                named_leaf(name, numeric_storage(20.0)),
            ]
            .into_iter()
            .flatten()
            .collect();
            let original = children.clone();
            let path = [SourcePropertyRef {
                match_name: name.into(),
                child_index: None,
            }];
            assert_eq!(
                apply_path(&mut children, &path, Some(parent), &numeric_storage(42.0))
                    .unwrap_err()
                    .kind,
                WarningKind::UnresolvedSourcePath
            );
            assert_eq!(children, original);
        }
    }

    #[test]
    fn deeply_nested_override_slots_preserve_depth_first_order() {
        let mut children = named_leaf("deep-value", numeric_storage(1.0));
        for _ in 0..128 {
            children = named_leaf("group", vec![Chunk::list(*b"tdgp", children)]);
        }
        children.extend(named_leaf("root-sibling", numeric_storage(2.0)));
        let mut slots = Vec::new();
        flatten_override_slots(&children, &mut slots).unwrap();
        assert_eq!(slots.len(), 130);
        assert!(slots[..128].iter().all(|slot| slot.is_group));
        assert_eq!(slots[128].match_name, "deep-value");
        assert_eq!(slots[129].match_name, "root-sibling");
        assert!(!slots[128].raw_chunks.is_empty());
    }

    #[test]
    fn applies_a_deep_override_path_on_a_small_stack() {
        const DEPTH: usize = 2_048;
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                let mut children = named_leaf("leaf", numeric_storage(1.0));
                for _ in 0..DEPTH {
                    children = named_leaf("group", vec![Chunk::list(*b"tdgp", children)]);
                }
                let mut path = vec![
                    SourcePropertyRef {
                        match_name: "group".into(),
                        child_index: None,
                    };
                    DEPTH
                ];
                path.push(SourcePropertyRef {
                    match_name: "leaf".into(),
                    child_index: None,
                });

                apply_path(&mut children, &path, None, &numeric_storage(42.0)).unwrap();
                let mut nested = children.as_slice();
                for _ in 0..DEPTH {
                    nested = nested[1].children().unwrap();
                }
                let numeric = properties::unique_list(&nested[1..], *b"tdbs")
                    .and_then(read_numeric)
                    .unwrap();
                assert_eq!(numeric.values, vec![42.0]);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn essential_path_above_the_former_byte_and_node_limits_is_retained() {
        let long_name = "x".repeat(1024 * 1024 + 1);
        let mut nodes = serde_json::Map::new();
        for position in 0..65 {
            let match_name = if position == 0 {
                long_name.as_str()
            } else {
                "ADBE Opacity"
            };
            nodes.insert(
                position.to_string(),
                serde_json::json!({ "matchName": match_name, "index": position }),
            );
        }
        let raw = serde_json::to_string(&Value::Object(nodes)).unwrap();
        let path = parse_path(&raw).unwrap();

        assert_eq!(path.len(), 65);
        assert_eq!(path[0].match_name, long_name);
        assert_eq!(path[64].child_index, Some(64));
    }

    #[test]
    fn essential_path_still_rejects_noncontiguous_nodes_and_native_index_overflow() {
        let noncontiguous = serde_json::json!({
            "1": { "matchName": "ADBE Opacity", "index": 0 }
        })
        .to_string();
        assert_eq!(
            parse_path(&noncontiguous).unwrap_err(),
            "CPrp path positions are not contiguous from zero"
        );

        let overflow = serde_json::json!({
            "0": {
                "matchName": "ADBE Opacity",
                "index": u64::from(u32::MAX) + 1,
            }
        })
        .to_string();
        assert_eq!(
            parse_path(&overflow).unwrap_err(),
            "CPrp child index exceeds u32"
        );
    }

    fn find_controller_item(chunks: &[Chunk]) -> Option<&[Chunk]> {
        for chunk in chunks {
            let Some(children) = chunk.children() else {
                continue;
            };
            if chunk.list_kind() == Some(*b"Item") && controllers(children).values.len() == 3 {
                return Some(children);
            }
            if let Some(found) = find_controller_item(children) {
                return Some(found);
            }
        }
        None
    }

    fn transform_value(layer: &Layer, match_name: &str) -> Option<Vec<f64>> {
        read_transform(&layer.content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == match_name)
            .map(|property| property.numeric.unwrap().values)
    }

    fn layer_with_transform(children: Vec<Vec<Chunk>>) -> Layer {
        let mut root = vec![match_name_chunk("ADBE Transform Group").unwrap()];
        let mut transform: Vec<_> = children.into_iter().flatten().collect();
        transform.push(match_name_chunk("ADBE Group End").unwrap());
        root.push(Chunk::list(*b"tdgp", transform));
        root.push(match_name_chunk("ADBE Group End").unwrap());
        let record = project_layer_record();
        Layer {
            name: "source".into(),
            record,
            content: vec![Chunk::list(*b"tdgp", root)],
        }
    }

    fn project_layer_record() -> crate::schema::layer_records::LayerRecord {
        let project = structure::read_project(AEP).unwrap();
        project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                structure::ItemKind::Composition(comp) => comp.layers.first(),
                _ => None,
            })
            .unwrap()
            .record
            .clone()
    }

    fn named_leaf(name: &str, storage: Vec<Chunk>) -> Vec<Chunk> {
        std::iter::once(match_name_chunk(name).unwrap())
            .chain(storage)
            .collect()
    }

    fn numeric_storage(value: f64) -> Vec<Chunk> {
        let mut meta = vec![0; 124];
        meta[0..2].copy_from_slice(&[0xdb, 0x99]);
        meta[2..4].copy_from_slice(&1_u16.to_be_bytes());
        vec![Chunk::list(
            *b"tdbs",
            vec![
                Chunk::data(*b"tdb4", meta).unwrap(),
                Chunk::data(*b"tdsb", [0; 4]).unwrap(),
                Chunk::data(*b"cdat", value.to_be_bytes()).unwrap(),
            ],
        )]
    }
}
