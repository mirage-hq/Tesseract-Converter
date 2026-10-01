//! Source-declared legacy centered Geometry2 for bounded inverse-matte stages.
//! Modern Point defaults and explicit point overrides require a separate mapping.
use super::{control_links::unique_run, self_inverse_matte::static_values};
use crate::{
    properties,
    rifx::Chunk,
    structure::{ItemKind, Layer, ProjectItem},
};
use std::collections::HashSet;

fn descriptors(layer: &Layer) -> Result<Vec<&[Chunk]>, String> {
    let root = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    if !root.iter().any(|(n, _)| *n == "ADBE Effect Parade") {
        return Ok(Vec::new());
    }
    let parade = unique_run(&root, "ADBE Effect Parade").map_err(|e| e.to_string())?;
    let effects =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    effects
        .iter()
        .filter(|(name, _)| *name == "ADBE Geometry2")
        .map(|(_, effect)| properties::unique_list(effect, *b"sspc").map_err(|e| e.to_string()))
        .collect()
}
pub(super) fn descriptor(layer: &Layer) -> Result<Option<&[Chunk]>, String> {
    let descriptors = descriptors(layer)?;
    match descriptors.as_slice() {
        [] => Ok(None),
        [only] => Ok(Some(only)),
        _ => Err("ambiguous named control".into()),
    }
}
fn legacy_defaults(table: &[Chunk]) -> Result<(), String> {
    let rows = properties::runs(table).map_err(|e| e.to_string())?;
    let profile = [
        ("0000", 0, 0),
        ("0001", 6, 50 << 16),
        ("0002", 6, 50 << 16),
        ("0011", 4, 1),
        ("0003", 2, 100 << 16),
        ("0004", 2, 100 << 16),
        ("0005", 2, 0),
        ("0006", 3, 0),
        ("0007", 3, 0),
        ("0008", 2, 100 << 16),
        ("0009", 4, 1),
        ("0010", 2, 0),
        ("0012", 7, 1),
        ("built-in", 9, 0),
    ];
    if rows.len() != profile.len() {
        return Err("requires complete legacy Transform declarations".into());
    }
    let mut seen = HashSet::new();
    for (name, run) in &rows {
        if !seen.insert(*name) {
            return Err("duplicate Transform declaration".into());
        }
        let _ = properties::data(run, *b"pard").map_err(|e| e.to_string())?;
    }
    for (suffix, kind, value) in profile {
        let name = if suffix == "built-in" {
            "ADBE Effect Built In Params".into()
        } else {
            format!("ADBE Geometry2-{suffix}")
        };
        let run = unique_run(&rows, &name).map_err(|e| e.to_string())?;
        let p = properties::data(run, *b"pard").map_err(|e| e.to_string())?;
        if p.len() != 148
            || p[12..16] != (kind as u32).to_be_bytes()
            || p[56..60] != (value as u32).to_be_bytes()
        {
            return Err("Transform declaration is outside legacy center profile".into());
        }
        // PF_Point defaults are percentage slots68/72, distinct from value slots56/60.
        if kind == 6
            && (p[60..64] != (value as u32).to_be_bytes()
                || p[64..68] != [0; 4]
                || p[68..72] != (value as u32).to_be_bytes()
                || p[72..76] != (value as u32).to_be_bytes())
        {
            return Err("unsupported legacy Point declaration".into());
        }
    }
    Ok(())
}
pub(super) fn rotation(
    layer: &Layer,
    items: &std::collections::HashMap<u32, &ProjectItem>,
) -> Result<bool, String> {
    let Some(descriptor) = descriptor(layer)? else {
        return Ok(false);
    };
    let mut warnings = Vec::new();
    let enabled = properties::group_enabled_or_warn(descriptor, "ADBE Geometry2", &mut warnings);
    if !warnings.is_empty() {
        return Err(warnings.join("; "));
    }
    if !enabled {
        return Ok(false);
    }
    let own = properties::unique_list(descriptor, *b"parT").map_err(|e| e.to_string())?;
    if own.is_empty() {
        let mut found = false;
        for item in items.values() {
            if let ItemKind::Composition(comp) = &item.kind {
                for candidate in &comp.layers {
                    for d in descriptors(candidate)? {
                        let table =
                            properties::unique_list(d, *b"parT").map_err(|e| e.to_string())?;
                        if !table.is_empty() {
                            legacy_defaults(table)?;
                            found = true;
                        }
                    }
                }
            }
        }
        if !found {
            return Err("no unambiguous same-project legacy Transform defaults".into());
        }
    } else {
        legacy_defaults(own)?;
    }
    let controls =
        properties::runs(properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for (name, run) in controls {
        if !seen.insert(name) {
            return Err("duplicate Transform control".into());
        }
        match name {
            "ADBE Geometry2-0000" => static_values(run, &[0.])?,
            "ADBE Geometry2-0007" => static_values(run, &[180.])?,
            "ADBE Effect Built In Params" => {
                if properties::runs(
                    properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?
                .iter()
                .any(|(n, _)| *n != "ADBE Group End")
                {
                    return Err("nonempty Transform compositing options".into());
                }
            }
            "ADBE Group End" => {}
            _ => {
                return Err(
                    "only static180 rotation with absent point overrides is supported".into(),
                );
            }
        }
    }
    if !seen.contains("ADBE Geometry2-0007") {
        return Err("requires explicit180 rotation".into());
    }
    Ok(true)
}
