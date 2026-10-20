//! Restore same-project pseudo-effect declarations for sparse instances.
//! Only declarations are shared: authored instance values are never copied.

use std::collections::HashMap;

use crate::{properties, rifx::Chunk};

pub(super) fn restore(chunks: &mut [Chunk]) {
    let Ok(catalog) = properties::unique_list(chunks, *b"EfdG") else {
        return;
    };
    let mut definitions = HashMap::new();
    for entry in catalog {
        if entry.list_kind() != Some(*b"EfDf") {
            continue;
        }
        let Some(entry) = entry.children() else {
            continue;
        };
        let Some(name) = properties::data(entry, *b"tdmn").ok().and_then(match_name) else {
            continue;
        };
        if !name.starts_with("Pseudo/") {
            continue;
        }
        let table = properties::unique_list(entry, *b"sspc")
            .and_then(|plugin| properties::unique_list(plugin, *b"parT"))
            .ok()
            .filter(|table| valid_table(name, table));
        definitions
            .entry(name.to_owned())
            .and_modify(|definition| *definition = None)
            .or_insert_with(|| table.map(<[Chunk]>::to_vec));
    }
    let mut pending = vec![chunks];
    while let Some(chunks) = pending.pop() {
        // Effect runs pair a tdmn with their following sspc. Never match a
        // descriptor by display name or borrow defaults from another layer.
        let mut name = None;
        for chunk in chunks {
            if chunk.id() == *b"tdmn" {
                name = chunk.data_payload().and_then(match_name).map(str::to_owned);
            }
            if chunk.list_kind() == Some(*b"sspc")
                && let Some(Some(definition)) =
                    name.as_deref().and_then(|name| definitions.get(name))
                && let Some(plugin) = chunk.children_mut()
                && properties::unique_list(plugin, *b"parT").is_ok_and(<[Chunk]>::is_empty)
                && let Some(table) = plugin
                    .iter_mut()
                    .find(|chunk| chunk.list_kind() == Some(*b"parT"))
            {
                // Malformed/missing/duplicate tables keep ordinary diagnostics.
                *table = Chunk::list(*b"parT", definition.clone());
            }
            if let Some(children) = chunk.children_mut() {
                pending.push(children);
            }
        }
    }
}

fn match_name(bytes: &[u8]) -> Option<&str> {
    if bytes.len() != 40 {
        return None;
    }
    let end = bytes
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |index| index + 1);
    let name = std::str::from_utf8(&bytes[..end]).ok()?;
    (!name.contains('\0')).then_some(name)
}

fn valid_table(effect: &str, table: &[Chunk]) -> bool {
    let Ok(runs) = properties::runs(table) else {
        return false;
    };
    !runs.is_empty()
        && runs.iter().all(|(name, run)| {
            (*name == "ADBE Effect Built In Params"
                || name.strip_prefix(effect).is_some_and(|suffix| {
                    suffix.len() == 5
                        && suffix.starts_with('-')
                        && suffix[1..].bytes().all(|byte| byte.is_ascii_digit())
                }))
                && properties::data(run, *b"pard").is_ok_and(|bytes| bytes.len() == 148)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EFFECT: &str = "Pseudo/Example";

    fn named(name: &str) -> Chunk {
        let mut bytes = vec![0; 40];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        Chunk::data(*b"tdmn", bytes).unwrap()
    }

    fn declaration() -> Vec<Chunk> {
        let mut bytes = vec![0; 148];
        bytes[12..16].copy_from_slice(&10_u32.to_be_bytes());
        bytes[16..23].copy_from_slice(b"Opacity");
        bytes[56..64].copy_from_slice(&25_f64.to_be_bytes());
        vec![
            named("Pseudo/Example-0001"),
            Chunk::data(*b"pard", bytes).unwrap(),
        ]
    }

    fn instance(table: Vec<Chunk>) -> Chunk {
        Chunk::list(
            *b"tdgp",
            vec![
                named(EFFECT),
                Chunk::list(
                    *b"sspc",
                    vec![Chunk::list(*b"parT", table), Chunk::list(*b"tdgp", vec![])],
                ),
            ],
        )
    }

    fn catalog() -> Chunk {
        Chunk::list(
            *b"EfdG",
            vec![Chunk::list(
                *b"EfDf",
                vec![
                    named(EFFECT),
                    Chunk::list(*b"sspc", vec![Chunk::list(*b"parT", declaration())]),
                ],
            )],
        )
    }

    #[test]
    fn sparse_pseudo_instance_restores_project_declarations() {
        let mut chunks = vec![catalog(), instance(vec![])];
        restore(&mut chunks);
        let run = chunks[1].children().unwrap();
        let plugin = properties::unique_list(run, *b"sspc").unwrap();
        let table = properties::unique_list(plugin, *b"parT").unwrap();
        assert_eq!(table, declaration());
        assert_eq!(properties::runs(table).unwrap()[0].0, "Pseudo/Example-0001");
        // Only definitions, not another occurrence's authored values, are shared.
        assert!(
            properties::unique_list(plugin, *b"tdgp")
                .unwrap()
                .is_empty()
        );
        let content = vec![Chunk::list(
            *b"tdgp",
            vec![named("ADBE Effect Parade"), chunks[1].clone()],
        )];
        let (effects, _) = crate::effects::native::read_effects(&content, [100.0, 100.0]);
        assert_eq!(effects.len(), 1);
        assert_eq!(
            effects[0].parameters[0].numeric.as_ref().unwrap().values,
            [25.0]
        );
    }

    #[test]
    fn local_and_ambiguous_declarations_are_not_replaced() {
        let local = instance(vec![named("local")]);
        let mut chunks = vec![catalog(), local.clone()];
        restore(&mut chunks);
        assert_eq!(chunks[1], local);
        let mut catalog = catalog();
        let entries = catalog.children_mut().unwrap();
        entries.push(entries[0].clone());
        let sparse = instance(vec![]);
        let mut chunks = vec![catalog, sparse.clone()];
        restore(&mut chunks);
        assert_eq!(chunks[1], sparse);
    }
}
