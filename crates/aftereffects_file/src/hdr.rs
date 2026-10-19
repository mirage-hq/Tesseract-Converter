//! Owned ICC binding for explicitly requested fresh native HDR exports.

use crate::{
    aep::Project,
    rifx::{Chunk, RifxError},
    writer::AepWriteError,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use fx_conv::HdrTransfer;

pub(crate) fn profile(transfer: HdrTransfer) -> (&'static str, &'static [u8]) {
    match transfer {
        HdrTransfer::Pq => (
            "Jerboa BT.2100 PQ BT.2020 D65 W203 v2",
            include_bytes!("../../../profiles/jerboa-pq.icc"),
        ),
        HdrTransfer::Hlg => (
            "Jerboa BT.2100 HLG BT.2020 D65 W203 v2",
            include_bytes!("../../../profiles/jerboa-hlg.icc"),
        ),
    }
}

/// Fresh generated projects only. Embedded owned ICC bytes are interpretation,
/// not a substitute for converting the generated native point-colour values.
pub(crate) fn configure_project(
    bytes: &mut Vec<u8>,
    transfer: HdrTransfer,
) -> Result<(), AepWriteError> {
    let mut project = Project::parse(bytes)?;
    let (name, icc) = profile(transfer);
    let working = serde_json::to_vec(&serde_json::json!({
        "baseProfileType":2,
        "baseColorProfile":{"colorProfileName":name,"colorProfileData":STANDARD.encode(icc)}
    }))
    .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
    let mut compact = false;
    let mut expanded = false;
    let mut working_slot = false;
    for index in 0..project.chunks.len() {
        let id = project.chunks[index].id();
        match id {
            [b'n', b'h', b'e', b'd'] => {
                let mut data = crate::schema::project_settings::CompactProjectSettings::decode(
                    payload(&project.chunks[index])?,
                )?
                .encode();
                data[15] = 2; // Independently native-saved AE26 32bpc compact code.
                project.chunks[index] = Chunk::data(id, data.to_vec())?;
                compact = true;
            }
            [b'n', b'n', b'h', b'd'] => {
                let mut data = crate::schema::project_settings::ExpandedProjectSettings::decode(
                    payload(&project.chunks[index])?,
                )?
                .encode();
                data[24] = 2; // Independently native-saved AE26 32bpc expanded code.
                project.chunks[index] = Chunk::data(id, data.to_vec())?;
                expanded = true;
            }
            [b'c', b'p', b'i', b'd'] => {
                // Deterministic owned-profile identity, never a specimen UUID.
                project.chunks[index] = Chunk::data(id, icc[84..100].to_vec())?;
            }
            [b'P', b'w', b'C', b's'] => {
                let next = project
                    .chunks
                    .get_mut(index + 1)
                    .ok_or(AepWriteError::Invalid(
                        "missing native working-profile slot",
                    ))?;
                if next.id() != *b"Utf8" {
                    return Err(AepWriteError::Invalid(
                        "invalid native working-profile slot",
                    ));
                }
                *next = Chunk::data(*b"Utf8", working.clone())?;
                working_slot = true;
            }
            _ => {}
        }
    }
    if !(compact && expanded && working_slot) {
        return Err(AepWriteError::Invalid(
            "fresh HDR project settings are incomplete",
        ));
    }
    *bytes = project.encode()?;
    Ok(())
}

fn payload(chunk: &Chunk) -> Result<&[u8], RifxError> {
    chunk
        .data_payload()
        .ok_or(RifxError::Invalid("invalid HDR project settings record"))
}

/// Only identify an exact owned-profile payload, not a potentially misleading
/// label. Built-in/native OCIO profiles need separate measured bindings.
pub(crate) fn owned_transfer(project: &Project) -> Option<HdrTransfer> {
    if project
        .chunks
        .iter()
        .filter(|chunk| chunk.id() == *b"PwCs")
        .count()
        != 1
    {
        return None;
    }
    let index = project
        .chunks
        .iter()
        .position(|chunk| chunk.id() == *b"PwCs")?;
    let chunk = project.chunks.get(index + 1)?;
    if chunk.id() != *b"Utf8" {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(chunk.data_payload()?).ok()?;
    if value["baseProfileType"].as_u64()? != 2 {
        return None;
    }
    let data = STANDARD
        .decode(value["baseColorProfile"]["colorProfileData"].as_str()?)
        .ok()?;
    [HdrTransfer::Pq, HdrTransfer::Hlg]
        .into_iter()
        .find(|transfer| profile(*transfer).1 == data)
}

/// AE26 domain switches were independently set/save/reopened with native getters:
/// `lnrp=1` is linearized working space, `lnrb=1` is linear blending. Missing
/// switches mean false; malformed/duplicated declarations are not guessed.
pub(crate) fn import_context(project: &Project) -> crate::structure::ProjectColorContext {
    use crate::structure::ProjectColorContext;
    if project
        .chunks
        .iter()
        .filter(|chunk| chunk.id() == *b"PwCs")
        .count()
        > 1
    {
        return ProjectColorContext::Unsupported {
            profile: "multiple working declarations".into(),
            reason: "duplicate PwCs working profiles are ambiguous; no first-profile admission",
        };
    }
    let index = project
        .chunks
        .iter()
        .position(|chunk| chunk.id() == *b"PwCs");
    let value = index
        .and_then(|index| project.chunks.get(index + 1))
        .filter(|chunk| chunk.id() == *b"Utf8")
        .and_then(Chunk::data_payload)
        .and_then(|data| serde_json::from_slice::<serde_json::Value>(data).ok());
    let Some(value) = value else {
        return ProjectColorContext::Unsupported {
            profile: "unreadable working declaration".into(),
            reason: "missing or malformed native working-profile declaration",
        };
    };
    if value.as_object().is_some_and(|value| value.is_empty()) {
        return ProjectColorContext::Legacy;
    }
    let profile = value["baseColorProfile"]["colorProfileName"]
        .as_str()
        .unwrap_or("unnamed working profile")
        .chars()
        .take(160)
        .collect();
    let unsupported = |reason| ProjectColorContext::Unsupported { profile, reason };
    let (Some(linearized), Some(linear_blending)) = (
        domain_flag(project, *b"lnrp"),
        domain_flag(project, *b"lnrb"),
    ) else {
        return unsupported("malformed or conflicting native linearization/blending declarations");
    };
    if linearized {
        return unsupported(
            "linearized working-space point coordinates have not been independently calibrated",
        );
    }
    match owned_transfer(project) {
        Some(transfer) => ProjectColorContext::Hdr {
            transfer,
            linear_blending,
        },
        None => unsupported(
            "working profile is not an exact admitted owned PQ/HLG ICC payload; native built-in and OCIO domains remain unbound",
        ),
    }
}

fn domain_flag(project: &Project, id: [u8; 4]) -> Option<bool> {
    let mut found = project.chunks.iter().filter(|chunk| chunk.id() == id);
    let value = match found.next() {
        None => false,
        Some(chunk) => match chunk.data_payload()? {
            [0] => false,
            [1] => true,
            _ => return None,
        },
    };
    found.next().is_none().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_hdr_binding_embeds_owned_bytes_and_32bpc_mirrors() {
        for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
            let spec = crate::writer::CompositionSpec {
                name: "HDR".into(),
                width: 320,
                height: 180,
                duration_frames: 24,
            };
            let mut bytes = crate::writer::write_empty_composition(&spec).unwrap();
            configure_project(&mut bytes, transfer).unwrap();
            let project = Project::parse(&bytes).unwrap();
            assert_eq!(owned_transfer(&project), Some(transfer));
            assert_eq!(
                import_context(&project),
                crate::structure::ProjectColorContext::Hdr {
                    transfer,
                    linear_blending: false
                }
            );
            let mut duplicate = project.clone();
            let index = project
                .chunks
                .iter()
                .position(|chunk| chunk.id() == *b"PwCs")
                .unwrap();
            duplicate
                .chunks
                .extend_from_slice(&project.chunks[index..=index + 1]);
            assert_eq!(owned_transfer(&duplicate), None);
            assert!(matches!(
                import_context(&duplicate),
                crate::structure::ProjectColorContext::Unsupported { .. }
            ));
            let mut conflicting = duplicate;
            let mut other_bytes = crate::writer::write_empty_composition(&spec).unwrap();
            configure_project(
                &mut other_bytes,
                match transfer {
                    HdrTransfer::Pq => HdrTransfer::Hlg,
                    HdrTransfer::Hlg => HdrTransfer::Pq,
                },
            )
            .unwrap();
            let other = Project::parse(&other_bytes).unwrap();
            let other_index = other
                .chunks
                .iter()
                .position(|chunk| chunk.id() == *b"PwCs")
                .unwrap();
            *conflicting.chunks.last_mut().unwrap() = other.chunks[other_index + 1].clone();
            assert_eq!(owned_transfer(&conflicting), None);
            assert!(matches!(
                import_context(&conflicting),
                crate::structure::ProjectColorContext::Unsupported { .. }
            ));
            let find = |id| {
                project
                    .chunks
                    .iter()
                    .find(|c| c.id() == id)
                    .unwrap()
                    .data_payload()
                    .unwrap()
            };
            assert_eq!(find(*b"nhed")[15], 2);
            assert_eq!(find(*b"nnhd")[24], 2);
            assert_eq!(find(*b"cpid"), &profile(transfer).1[84..100]);
            let mut linearized = project.clone();
            linearized
                .chunks
                .push(Chunk::data(*b"lnrp", vec![1]).unwrap());
            assert!(matches!(
                import_context(&linearized),
                crate::structure::ProjectColorContext::Unsupported { .. }
            ));
            let mut blending = project.clone();
            blending
                .chunks
                .push(Chunk::data(*b"lnrb", vec![1]).unwrap());
            assert_eq!(
                import_context(&blending),
                crate::structure::ProjectColorContext::Hdr {
                    transfer,
                    linear_blending: true
                }
            );
            blending
                .chunks
                .push(Chunk::data(*b"lnrb", vec![0]).unwrap());
            assert!(matches!(
                import_context(&blending),
                crate::structure::ProjectColorContext::Unsupported { .. }
            ));
        }
    }
}
