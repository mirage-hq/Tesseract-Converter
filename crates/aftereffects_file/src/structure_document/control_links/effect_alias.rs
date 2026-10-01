//! Pure same-effect scalar aliases, such as a Mosaic Vertical Blocks control
//! whose enabled expression is exactly `effect("Mosaic")("Horizontal Blocks")`.
//!
//! Resolution uses native identity only: the referenced name must uniquely be
//! the destination's own Effect Parade occurrence under AE's current instance
//! name, and the referenced parameter must identify one of its explicit
//! controls. The caller lowers that control's existing decoded values/keys.
//! No expression code runs and live linkage is not retained.

use super::{
    Reference, display_name, effect_instance_name, expression, finished, named_reference,
    unique_run,
};
use crate::{
    properties::{self, PropertyError},
    rifx::Chunk,
};

/// See [`super::resolve_same_effect_alias`].
pub(super) fn resolve(
    content: &[Chunk],
    effect_index: usize,
    parameter: &str,
) -> Option<Result<String, PropertyError>> {
    let instances = parade(content).ok()?;
    let (_, run) = instances.get(effect_index.checked_sub(1)?)?;
    let controls = controls(run).ok()?;
    let leaf = controls
        .iter()
        .find(|(name, _)| *name == parameter)
        .and_then(|(_, run)| properties::unique_list(run, *b"tdbs").ok())?;
    let link = pure_reference(expression(leaf).ok()?)?;
    Some(follow(&instances, effect_index, &controls, parameter, link))
}

fn parade(content: &[Chunk]) -> Result<Vec<(&str, &[Chunk])>, PropertyError> {
    let root = properties::root_runs(content)?;
    let parade = unique_run(&root, "ADBE Effect Parade")?;
    properties::runs(properties::unique_list(parade, *b"tdgp")?)
}

fn plugin(run: &[Chunk]) -> Result<(&[Chunk], &[Chunk]), PropertyError> {
    let descriptor = properties::unique_list(run, *b"sspc")?;
    Ok((descriptor, properties::unique_list(descriptor, *b"tdgp")?))
}

fn controls(run: &[Chunk]) -> Result<Vec<(&str, &[Chunk])>, PropertyError> {
    properties::runs(plugin(run)?.1)
}

/// A complete expression consisting of one `effect(name)(parameter)` reference.
fn pure_reference(text: &str) -> Option<Reference<'_>> {
    let mut rest = text;
    let link = named_reference(&mut rest)?;
    finished(rest).then_some(link)
}

fn follow<'a>(
    instances: &[(&str, &'a [Chunk])],
    effect_index: usize,
    controls: &[(&'a str, &'a [Chunk])],
    destination: &str,
    mut link: Reference<'a>,
) -> Result<String, PropertyError> {
    let mut visited = vec![destination];
    loop {
        let mut named = Vec::new();
        for (position, (_, run)) in instances.iter().enumerate() {
            let (descriptor, body) = plugin(run)?;
            let name = effect_instance_name(descriptor, body).ok_or(PropertyError::Layout(
                "an Effect Parade occurrence name is unreadable, so the reference is ambiguous",
            ))?;
            if name == link.effect {
                named.push(position + 1);
            }
        }
        match named.as_slice() {
            [] => return Err(PropertyError::Layout("referenced effect not found")),
            [index] if *index == effect_index => {}
            [_] => {
                return Err(PropertyError::Layout(
                    "references another effect occurrence; only same-effect aliases are lowered",
                ));
            }
            _ => return Err(PropertyError::Layout("referenced effect name is ambiguous")),
        }
        let mut matches = controls.iter().filter_map(|(name, run)| {
            if name.ends_with("-0000") || *name == "ADBE Effect Built In Params" {
                return None;
            }
            let leaf = properties::unique_list(run, *b"tdbs").ok()?;
            (*name == link.parameter || display_name(leaf) == Some(link.parameter))
                .then_some((*name, leaf))
        });
        let (name, leaf) = matches.next().ok_or(PropertyError::Layout(
            "referenced effect parameter not found",
        ))?;
        if matches.next().is_some() {
            return Err(PropertyError::Layout(
                "referenced effect parameter name is ambiguous",
            ));
        }
        if visited.contains(&name) {
            return Err(PropertyError::Layout("cyclic same-effect alias"));
        }
        visited.push(name);
        if !properties::read_numeric(leaf)?.expression_enabled {
            return Ok(name.to_owned());
        }
        link = pure_reference(expression(leaf)?).ok_or(PropertyError::Layout(
            "referenced parameter has an expression that is not a pure same-effect alias",
        ))?;
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{data, list, numeric};
    use super::resolve;
    use crate::{properties::PropertyError, rifx::Chunk};

    fn utf8(id: &[u8; 4], value: &str) -> Chunk {
        let mut bytes = b"Utf8".to_vec();
        bytes.extend(u32::try_from(value.len()).unwrap().to_be_bytes());
        bytes.extend(value.as_bytes());
        data(id, bytes)
    }

    /// Synthetic storage-level effect occurrence; the public-derived native
    /// Mosaic regressions live in `structure_document::tests`.
    fn effect(
        match_name: &str,
        instance: &str,
        plugin: &str,
        controls: &[(&str, &str, Option<&str>)],
    ) -> Vec<Chunk> {
        let mut body = vec![utf8(b"tdsn", instance)];
        for (parameter, label, expression) in controls {
            body.push(data(b"tdmn", parameter.as_bytes()));
            let mut leaf = numeric(&[4.0], *expression);
            if let Some(children) = leaf.children_mut() {
                children.retain(|chunk| chunk.id() != *b"tdsn");
                children.push(utf8(b"tdsn", label));
            }
            body.push(leaf);
        }
        vec![
            data(b"tdmn", match_name.as_bytes()),
            list(b"sspc", vec![utf8(b"fnam", plugin), list(b"tdgp", body)]),
        ]
    }

    fn content(effects: Vec<Vec<Chunk>>) -> Vec<Chunk> {
        vec![list(
            b"tdgp",
            vec![
                data(b"tdmn", b"ADBE Effect Parade"),
                list(b"tdgp", effects.concat()),
            ],
        )]
    }

    fn mosaic(instance: &str, vertical: &str, horizontal: Option<&str>) -> Vec<Chunk> {
        effect(
            "ADBE Mosaic",
            instance,
            "Mosaic",
            &[
                ("ADBE Mosaic-0001", "Horizontal Blocks", horizontal),
                ("ADBE Mosaic-0002", "Vertical Blocks", Some(vertical)),
            ],
        )
    }

    const ALIAS: &str = "effect(\"Mosaic\")(\"Horizontal Blocks\")";

    fn error(result: Option<Result<String, PropertyError>>) -> String {
        match result {
            Some(Err(error)) => error.to_string(),
            other => panic!("expected a diagnosed alias, got {other:?}"),
        }
    }

    #[test]
    fn placeholder_instances_resolve_by_plugin_name_and_renames_by_instance_name() {
        let placeholder = content(vec![mosaic("-_0_/-", ALIAS, None)]);
        assert_eq!(
            resolve(&placeholder, 1, "ADBE Mosaic-0002"),
            Some(Ok("ADBE Mosaic-0001".into()))
        );
        let renamed = content(vec![mosaic("Pixelate", ALIAS, None)]);
        assert!(error(resolve(&renamed, 1, "ADBE Mosaic-0002")).contains("not found"));
        let explicit = content(vec![mosaic(
            "Pixelate",
            "effect('Pixelate')('ADBE Mosaic-0001');",
            None,
        )]);
        assert_eq!(
            resolve(&explicit, 1, "ADBE Mosaic-0002"),
            Some(Ok("ADBE Mosaic-0001".into()))
        );
    }

    #[test]
    fn references_follow_same_effect_chains_and_reject_cycles() {
        // Horizontal itself is a pure alias of a third control, which is static.
        let chain = content(vec![effect(
            "ADBE Mosaic",
            "-_0_/-",
            "Mosaic",
            &[
                (
                    "ADBE Mosaic-0001",
                    "Horizontal Blocks",
                    Some("effect(\"Mosaic\")(\"Sharp Colors\")"),
                ),
                ("ADBE Mosaic-0002", "Vertical Blocks", Some(ALIAS)),
                ("ADBE Mosaic-0003", "Sharp Colors", None),
            ],
        )]);
        assert_eq!(
            resolve(&chain, 1, "ADBE Mosaic-0002"),
            Some(Ok("ADBE Mosaic-0003".into()))
        );
        let this = content(vec![mosaic(
            "-_0_/-",
            "effect(\"Mosaic\")(\"Vertical Blocks\")",
            None,
        )]);
        assert!(error(resolve(&this, 1, "ADBE Mosaic-0002")).contains("cyclic"));
        let pair = content(vec![mosaic(
            "-_0_/-",
            ALIAS,
            Some("effect(\"Mosaic\")(\"Vertical Blocks\")"),
        )]);
        assert!(error(resolve(&pair, 1, "ADBE Mosaic-0002")).contains("cyclic"));
        let impure = content(vec![mosaic(
            "-_0_/-",
            ALIAS,
            Some("effect(\"Mosaic\")(\"Sharp Colors\") + 1"),
        )]);
        assert!(error(resolve(&impure, 1, "ADBE Mosaic-0002")).contains("not a pure"));
    }

    #[test]
    fn ambiguous_or_foreign_identities_are_diagnosed() {
        let duplicate_names = content(vec![
            mosaic("-_0_/-", ALIAS, None),
            mosaic(
                "-_0_/-",
                "effect(\"Mosaic 2\")(\"Horizontal Blocks\")",
                None,
            ),
        ]);
        assert!(error(resolve(&duplicate_names, 1, "ADBE Mosaic-0002")).contains("ambiguous"));
        let foreign = content(vec![
            mosaic("-_0_/-", "effect(\"Other\")(\"Horizontal Blocks\")", None),
            mosaic("Other", "effect(\"Other\")(\"Horizontal Blocks\")", None),
        ]);
        assert!(error(resolve(&foreign, 1, "ADBE Mosaic-0002")).contains("another effect"));
        assert_eq!(
            resolve(&foreign, 2, "ADBE Mosaic-0002"),
            Some(Ok("ADBE Mosaic-0001".into()))
        );
        let duplicate_labels = content(vec![effect(
            "ADBE Mosaic",
            "-_0_/-",
            "Mosaic",
            &[
                ("ADBE Mosaic-0001", "Blocks", None),
                (
                    "ADBE Mosaic-0002",
                    "Vertical Blocks",
                    Some("effect(\"Mosaic\")(\"Blocks\")"),
                ),
                ("ADBE Mosaic-0003", "Blocks", None),
            ],
        )]);
        assert!(error(resolve(&duplicate_labels, 1, "ADBE Mosaic-0002")).contains("ambiguous"));
    }

    #[test]
    fn non_reference_expressions_are_not_candidates() {
        for text in [
            "effect(\"Mosaic\")(\"Horizontal Blocks\") * 2",
            "effect(\"Mosaic\")(\"Horizontal Blocks\").valueAtTime(0)",
            "thisLayer.effect(\"Mosaic\")(\"Horizontal Blocks\")",
            "effect(\"Mosaic\")(1)",
            "wiggle(2, 3)",
        ] {
            let source = content(vec![mosaic("-_0_/-", text, None)]);
            assert_eq!(resolve(&source, 1, "ADBE Mosaic-0002"), None, "{text}");
        }
        let static_destination = content(vec![mosaic("-_0_/-", ALIAS, None)]);
        assert_eq!(resolve(&static_destination, 1, "ADBE Mosaic-0001"), None);
        assert_eq!(resolve(&static_destination, 2, "ADBE Mosaic-0002"), None);
    }

    #[test]
    fn chained_index_references_are_not_pure_aliases() {
        // Index 1 names a Slider value only in Slider links, not here.
        let chained = content(vec![mosaic("-_0_/-", ALIAS, Some("effect(\"Mosaic\")(1)"))]);
        assert!(error(resolve(&chained, 1, "ADBE Mosaic-0002")).contains("not a pure"));
    }
}
