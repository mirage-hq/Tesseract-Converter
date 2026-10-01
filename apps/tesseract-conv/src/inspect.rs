//! Read-only, bounded AEP project-panel inventory. No media paths are opened.

use aftereffects_file::{
    aep,
    schema::HeadRecord,
    structure::{self, FootageSourceKind, ItemKind, StructuralProject},
};
use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Serialize)]
struct Source {
    sha256: String,
    bytes: usize,
    format_version: u8,
    producer_version_word: u32,
}

#[derive(Serialize)]
struct DirectLayer {
    id: u32,
    name: String,
    source_id: u32,
}

#[derive(Serialize)]
struct Composition {
    id: u32,
    name: String,
    parent_folder: Option<u32>,
    width: u16,
    height: u16,
    frame_rate: f64,
    duration_secs: f64,
    direct_layer_count: usize,
    direct_layers: Vec<DirectLayer>,
    // Distinct timeline layers across this composition and every reachable precomp.
    // Each composition is traversed once, including in cyclic graphs.
    reachable_unique_layer_count: usize,
    reachable_composition_ids: Vec<u32>,
    reachable_source_ids: Vec<u32>,
    unresolved_source_ids: Vec<u32>,
}

#[derive(Serialize)]
struct Media {
    id: u32,
    name: String,
    parent_folder: Option<u32>,
    main_source: &'static str,
    proxy_source: Option<&'static str>,
    authored_path: Option<String>,
    missing_at_save: Option<bool>,
    current_status: &'static str,
    metadata_error: Option<String>,
}

#[derive(Serialize)]
struct Inspection {
    schema_version: u8,
    source: Source,
    compositions: Vec<Composition>,
    media: Vec<Media>,
    /// Source-only inventories not reliably exposed by the structural reader.
    font_inventory: &'static str,
    effect_inventory: &'static str,
    expression_inventory: &'static str,
}

fn source_kind(kind: FootageSourceKind) -> &'static str {
    match kind {
        FootageSourceKind::File => "file",
        FootageSourceKind::Solid => "solid",
        FootageSourceKind::Placeholder => "placeholder",
        FootageSourceKind::Unknown => "unknown",
    }
}

fn compositions(project: &StructuralProject) -> Result<Vec<Composition>> {
    let by_id: HashMap<_, _> = project.items.iter().map(|item| (item.id, item)).collect();
    let mut result = Vec::new();
    for item in &project.items {
        let ItemKind::Composition(comp) = &item.kind else {
            continue;
        };
        let mut visited = HashSet::new();
        let mut stack = vec![item.id];
        let mut reachable_composition_ids = Vec::new();
        let mut reachable_source_ids = HashSet::new();
        let mut unresolved_source_ids = HashSet::new();
        let mut reachable_unique_layer_count = 0_usize;
        // Visit each reachable composition once per root, including cycles.
        // The output includes every root's closure; no cumulative visit quota.
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(target) = by_id.get(&id) else {
                continue;
            };
            let ItemKind::Composition(target_comp) = &target.kind else {
                continue;
            };
            reachable_composition_ids.push(id);
            reachable_unique_layer_count = reachable_unique_layer_count
                .checked_add(target_comp.layers.len())
                .ok_or_else(|| anyhow::anyhow!("reachable layer count overflow"))?;
            for layer in &target_comp.layers {
                let source_id = layer.record.source_id();
                if source_id == 0 {
                    continue;
                }
                reachable_source_ids.insert(source_id);
                match by_id.get(&source_id) {
                    Some(source) if matches!(source.kind, ItemKind::Composition(_)) => {
                        stack.push(source_id)
                    }
                    Some(_) => {}
                    None => {
                        unresolved_source_ids.insert(source_id);
                    }
                }
            }
        }
        reachable_composition_ids.sort_unstable();
        let mut reachable_source_ids: Vec<_> = reachable_source_ids.into_iter().collect();
        reachable_source_ids.sort_unstable();
        let mut unresolved_source_ids: Vec<_> = unresolved_source_ids.into_iter().collect();
        unresolved_source_ids.sort_unstable();
        result.push(Composition {
            id: item.id,
            name: item.name.clone(),
            parent_folder: item.parent_folder,
            width: comp.width,
            height: comp.height,
            frame_rate: comp.frame_rate,
            duration_secs: comp.duration_secs,
            direct_layer_count: comp.layers.len(),
            direct_layers: comp
                .layers
                .iter()
                .map(|layer| DirectLayer {
                    id: layer.record.id(),
                    name: layer.name.to_string(),
                    source_id: layer.record.source_id(),
                })
                .collect(),
            reachable_unique_layer_count,
            reachable_composition_ids,
            reachable_source_ids,
            unresolved_source_ids,
        });
    }
    Ok(result)
}

fn inventory(bytes: &[u8]) -> Result<Inspection> {
    let project = structure::read_project(bytes).context("read AEP structure")?;
    let envelope = aep::Project::parse(bytes).context("read AEP header")?;
    let head = envelope
        .chunks
        .iter()
        .find(|chunk| chunk.id() == *b"head")
        .and_then(|chunk| chunk.data_payload())
        .context("AEP header is missing")?;
    let producer_version_word = HeadRecord::decode(head)
        .context("decode AEP header")?
        .producer_version_word();
    let media = project
        .items
        .iter()
        .filter_map(|item| {
            let footage = item.footage?;
            let (authored_path, missing_at_save, metadata_error) = match item.media.as_ref() {
                Some(Ok(descriptor)) => (
                    Some(descriptor.authored_path.clone()),
                    Some(descriptor.missing_at_save),
                    None,
                ),
                Some(Err(error)) => (None, None, Some(error.to_string())),
                None => (None, None, None),
            };
            Some(Media {
                id: item.id,
                name: item.name.clone(),
                parent_folder: item.parent_folder,
                main_source: source_kind(footage.main_source),
                proxy_source: footage.proxy_source.map(source_kind),
                authored_path,
                missing_at_save,
                current_status: "not_checked",
                metadata_error,
            })
        })
        .collect();
    Ok(Inspection {
        schema_version: 1,
        source: Source {
            sha256: format!("{:x}", Sha256::digest(bytes)),
            bytes: bytes.len(),
            format_version: project.format_version,
            producer_version_word,
        },
        compositions: compositions(&project)?,
        media,
        font_inventory: "unavailable",
        effect_inventory: "unavailable",
        expression_inventory: "unavailable",
    })
}

// Names are the last column so arbitrarily long (including non-ASCII) labels do
// not move the other columns. Every header, rule and row uses measured widths.
pub(super) fn table<const N: usize>(headers: [&str; N], rows: Vec<[String; N]>) -> Vec<String> {
    let widths: [usize; N] = std::array::from_fn(|column| {
        headers[column].chars().count().max(
            rows.iter()
                .map(|row| row[column].chars().count())
                .max()
                .unwrap_or(0),
        )
    });
    let format_row = |columns: [&str; N]| {
        let mut line = String::new();
        for (index, value) in columns.into_iter().enumerate() {
            if index != 0 {
                line.push_str("  ");
            }
            line.push_str(value);
            if index + 1 < N {
                line.push_str(&" ".repeat(widths[index] - value.chars().count()));
            }
        }
        line.trim_end().to_owned()
    };
    let mut lines = Vec::with_capacity(rows.len() + 2);
    lines.push(format_row(headers));
    lines.push(widths.map(|width| "-".repeat(width)).join("  "));
    lines.extend(
        rows.iter()
            .map(|row| format_row(row.each_ref().map(String::as_str))),
    );
    lines
}

pub(super) fn summary(
    path: &Path,
    selected: Option<u32>,
    media_ready: &HashMap<u32, bool>,
) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read AEP {}", path.display()))?;
    let inventory = inventory(&bytes)?;
    let mut lines = vec![
        format!(
            "File: {}",
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
        ),
        format!("Compositions: {}", inventory.compositions.len()),
        String::new(),
    ];
    let mut rows = Vec::new();
    let mut unresolved = Vec::new();
    for comp in inventory
        .compositions
        .iter()
        .filter(|comp| selected.is_none_or(|id| comp.id == id))
    {
        let status = match media_ready.get(&comp.id) {
            Some(true) => "OK",
            Some(false) => "ISSUES",
            None => "UNKNOWN",
        };
        rows.push([
            comp.id.to_string(),
            status.to_owned(),
            format!("{}x{}", comp.width, comp.height),
            format!("{:.2}", comp.frame_rate),
            format!("{:.2}s", comp.duration_secs),
            comp.direct_layer_count.to_string(),
            comp.reachable_unique_layer_count.to_string(),
            comp.name.clone(),
        ]);
        if !comp.unresolved_source_ids.is_empty() {
            unresolved.push(format!(
                "Unresolved sources for composition {}: {:?}",
                comp.id, comp.unresolved_source_ids
            ));
        }
    }
    lines.extend(table(
        [
            "ID",
            "MEDIA CHECK",
            "SIZE",
            "FPS",
            "LENGTH",
            "DIRECT",
            "WITH PRECOMPS",
            "NAME",
        ],
        rows,
    ));
    if !unresolved.is_empty() {
        lines.push(String::new());
        lines.extend(unresolved);
    }
    Ok(lines.join("\n"))
}

pub(super) fn inspect(path: &Path, json: bool) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read AEP {}", path.display()))?;
    let inventory = inventory(&bytes)?;
    if json {
        return Ok(serde_json::to_string(&inventory)?);
    }
    let mut lines = vec![format!(
        "AEP sha256={} bytes={} format_version={} producer_version_word={}",
        inventory.source.sha256,
        inventory.source.bytes,
        inventory.source.format_version,
        inventory.source.producer_version_word
    )];
    for comp in inventory.compositions {
        lines.push(format!(
            "comp id={} name={:?} parent_folder={:?} {}x{} fps={} duration_secs={} direct_layers={} reachable_unique_layers={} reachable_comps={:?} unresolved_sources={:?}",
            comp.id, comp.name, comp.parent_folder, comp.width, comp.height,
            comp.frame_rate, comp.duration_secs, comp.direct_layer_count,
            comp.reachable_unique_layer_count, comp.reachable_composition_ids, comp.unresolved_source_ids
        ));
    }
    for media in inventory.media {
        lines.push(format!(
            "media id={} name={:?} main_source={} authored_path={:?} missing_at_save={:?} current_status={} metadata_error={:?}",
            media.id, media.name, media.main_source, media.authored_path,
            media.missing_at_save, media.current_status, media.metadata_error
        ));
    }
    lines.push("font/effect/expression inventory: unavailable (not scanned)".to_owned());
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_measures_headers_rules_and_values_from_the_same_widths() {
        let lines = table(
            ["ID", "STATUS", "FPS", "NAME"],
            vec![
                ["1000", "ISSUES", "24.00", "日本語"].map(str::to_owned),
                ["12345678901", "OK", "120.00", "Long name"].map(str::to_owned),
            ],
        );
        assert_eq!(lines[0], "ID           STATUS  FPS     NAME");
        assert_eq!(lines[1], "-----------  ------  ------  ---------");
        assert_eq!(lines[2], "1000         ISSUES  24.00   日本語");
        assert_eq!(lines[3], "12345678901  OK      120.00  Long name");
    }

    #[test]
    fn composition_inspection_preserves_roots_past_the_former_visit_quota() {
        let bytes =
            include_bytes!("../../../crates/aftereffects_file/tests/fixtures/layers/folder.aep");
        let mut project = structure::read_project(bytes).expect("pinned source envelope");
        let template = project
            .items
            .iter()
            .find(|item| matches!(item.kind, ItemKind::Composition(_)))
            .unwrap()
            .clone();
        // Supplementary synthetic count regression, not Adobe feature proof.
        let count = 200_001;
        project.items = (1..=count)
            .map(|id| {
                let mut item = template.clone();
                item.id = id;
                item
            })
            .collect();
        let complete = compositions(&project).unwrap();
        assert_eq!(complete.len(), usize::try_from(count).unwrap());
        assert_eq!(complete.last().unwrap().id, count);
    }

    #[test]
    fn composition_inspection_still_terminates_on_a_source_cycle() {
        let bytes = include_bytes!(
            "../../../crates/aftereffects_file/tests/fixtures/properties/property_1D_opacity.aep"
        );
        let mut project = structure::read_project(bytes).unwrap();
        let item = project
            .items
            .iter_mut()
            .find(|item| matches!(item.kind, ItemKind::Composition(_)))
            .unwrap();
        let id = item.id;
        let ItemKind::Composition(comp) = &mut item.kind else {
            unreachable!()
        };
        let layer = &mut comp.layers[0];
        let mut record = layer.record.encode();
        record[40..44].copy_from_slice(&id.to_be_bytes());
        layer.record =
            aftereffects_file::schema::layer_records::LayerRecord::decode(&record).unwrap();
        let complete = compositions(&project).unwrap();
        let root = complete.iter().find(|root| root.id == id).unwrap();
        assert_eq!(root.reachable_composition_ids, vec![id]);
        assert_eq!(root.reachable_unique_layer_count, 1);
    }
}
