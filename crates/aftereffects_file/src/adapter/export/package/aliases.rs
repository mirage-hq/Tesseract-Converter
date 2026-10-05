//! Final-package identity binding for fresh file-footage aliases only.

use std::{collections::BTreeSet, path::Path};

use crate::{
    AepConversionError,
    aep::Project,
    rifx::Chunk,
    writer::{AepWriteError, footage::RelativeMediaPath},
};

/// Bind before generation hashing/publication, retaining native relocation hints.
/// Check and public staging never call this: no private staging path is embedded.
pub(in crate::adapter::export) fn bind_published_media_aliases(
    bytes: &mut Vec<u8>,
    emitted: &BTreeSet<RelativeMediaPath>,
    destination: &Path,
) -> Result<(), AepConversionError> {
    if !destination.is_absolute() {
        return Err(AepConversionError::Output(
            "media alias destination must be absolute",
        ));
    }
    if emitted.is_empty() {
        return Ok(());
    }
    let mut project = Project::parse(bytes).map_err(AepWriteError::from)?;
    let mut bound = BTreeSet::new();
    let mut pending = project.chunks.iter_mut().collect::<Vec<_>>();
    while let Some(chunk) = pending.pop() {
        let is_source = chunk.list_kind() == Some(*b"Pin ");
        let Some(children) = chunk.children_mut() else {
            continue;
        };
        if !is_source {
            pending.extend(children.iter_mut());
            continue;
        }
        for alias in children
            .iter_mut()
            .filter(|child| child.list_kind() == Some(*b"Als2"))
        {
            let payloads = alias
                .children_mut()
                .ok_or(AepConversionError::Output("generated alias is not a list"))?;
            let mut records = payloads.iter_mut().filter(|child| child.id() == *b"alas");
            let record = records.next().ok_or(AepConversionError::Output(
                "generated alias payload is missing",
            ))?;
            if records.next().is_some() {
                return Err(AepConversionError::Output(
                    "duplicate generated alias payload",
                ));
            }
            bound.insert(bind_alias(record, emitted, destination)?);
        }
    }
    if &bound != emitted {
        return Err(AepConversionError::Output(
            "generated aliases do not match emitted media",
        ));
    }
    *bytes = project.encode().map_err(AepWriteError::from)?;
    Ok(())
}

fn bind_alias(
    chunk: &mut Chunk,
    emitted: &BTreeSet<RelativeMediaPath>,
    destination: &Path,
) -> Result<RelativeMediaPath, AepConversionError> {
    let payload = chunk.data_payload().ok_or(AepConversionError::Output(
        "generated alias payload is not data",
    ))?;
    let mut alias: serde_json::Value = serde_json::from_slice(payload)
        .map_err(|_| AepConversionError::Output("generated alias payload is not JSON"))?;
    let fullpath = alias.get_mut("fullpath").ok_or(AepConversionError::Output(
        "generated alias fullpath is missing",
    ))?;
    let relative = fullpath
        .as_str()
        .and_then(|path| path.strip_prefix("./"))
        .ok_or(AepConversionError::Output(
            "generated alias is not package-relative",
        ))?;
    let path = RelativeMediaPath::new(relative)?;
    if !emitted.contains(&path) {
        return Err(AepConversionError::Output(
            "generated alias references unretained media",
        ));
    }
    let absolute = destination.join(path.as_str());
    *fullpath = serde_json::Value::String(
        absolute
            .to_str()
            .ok_or(AepConversionError::Output(
                "native media alias path is not UTF-8",
            ))?
            .to_owned(),
    );
    let payload = serde_json::to_vec(&alias)
        .map_err(|_| AepConversionError::Output("cannot encode generated alias JSON"))?;
    *chunk = Chunk::data(*b"alas", payload).map_err(AepWriteError::from)?;
    Ok(path)
}
