//! Owned ICC binding for explicitly requested fresh native HDR exports.

use crate::{
    aep::Project,
    rifx::{Chunk, RifxError},
    structure::HdrProfileBinding,
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
        match set_32bpc(&mut project.chunks[index])? {
            Some(SettingsRecord::Compact) => compact = true,
            Some(SettingsRecord::Expanded) => expanded = true,
            None => {}
        }
        match id {
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

/// The project-settings record a chunk holds ([`set_32bpc`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsRecord {
    /// `nhed`.
    Compact,
    /// `nnhd`.
    Expanded,
}

/// Rewrite a compact (`nhed`) or expanded (`nnhd`) project-settings chunk
/// at 32 bpc, the depth of AE 26's independently native-saved HDR and OCIO
/// projects; `None` leaves any other chunk unchanged.
pub(crate) fn set_32bpc(chunk: &mut Chunk) -> Result<Option<SettingsRecord>, AepWriteError> {
    use crate::schema::project_settings::{CompactProjectSettings, ExpandedProjectSettings};
    let id = chunk.id();
    let (data, record) = match id {
        [b'n', b'h', b'e', b'd'] => {
            let mut data = CompactProjectSettings::decode(payload(chunk)?)?.encode();
            data[15] = 2;
            (data.to_vec(), SettingsRecord::Compact)
        }
        [b'n', b'n', b'h', b'd'] => {
            let mut data = ExpandedProjectSettings::decode(payload(chunk)?)?.encode();
            data[24] = 2;
            (data.to_vec(), SettingsRecord::Expanded)
        }
        _ => return Ok(None),
    };
    *chunk = Chunk::data(id, data)?;
    Ok(Some(record))
}

fn payload(chunk: &Chunk) -> Result<&[u8], RifxError> {
    chunk
        .data_payload()
        .ok_or(RifxError::Invalid("invalid project settings record"))
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
    if let Some(context) = crate::ocio::import_context(project) {
        return context;
    }
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
    let Some(index) = project
        .chunks
        .iter()
        .position(|chunk| chunk.id() == *b"PwCs")
    else {
        return pre_declaration_context(project);
    };
    let value = project
        .chunks
        .get(index + 1)
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
    if let Some(name) = adobe_sdr_built_in(&value) {
        if linear_blending {
            return unsupported(
                "linear blending in an SDR working space has no FX compositing equivalent",
            );
        }
        return ProjectColorContext::LegacyDeclared {
            declaration: format!(
                "AE built-in \"{name}\" SDR working profile, identified by its exact AE 26.5 ICC payload; AE's display and output-module profile conversions are not modeled"
            ),
        };
    }
    let binding = owned_transfer(project)
        .map(|transfer| (transfer, HdrProfileBinding::OwnedIcc))
        .or_else(|| adobe_built_in(&value));
    match binding {
        Some((transfer, binding)) => ProjectColorContext::Hdr {
            transfer,
            linear_blending,
            binding,
        },
        None if profile_name_is_rec2100(&value) => unsupported(
            "native built-in Rec.2100 working profile is not bound: only AE 26.5's exact \"Rec.2100 PQ\" ICC payload is pinned; other Rec.2100 PQ/HLG variants (W100/W203/W300/W10000, HLG, HLG Scene) have no pinned native payload, and a profile name alone is not a binding",
        ),
        None => unsupported(
            "working profile is neither an exact admitted owned PQ/HLG ICC payload nor a pinned AE 26.5 built-in (sRGB IEC61966-2.1, Rec.709 Gamma 2.4, Rec.2100 PQ) payload; other built-in and OCIO domains remain unbound",
        ),
    }
}

/// The earliest project format whose natively saved fixtures carry the
/// `pcms`/`PwCs`/`pdvc` colour-management declarations (After Effects 25.6).
const FIRST_DECLARING_FORMAT: u8 = 96;

/// Projects saved before the `PwCs` declaration existed name their working
/// space only by `cpid`, the 16-byte ICC profile ID of the working profile
/// (all `0xFF` when the working space is "None"); `LIST CPPl` pools the
/// referenced ICC payloads. An After Effects 17.0 project (format 93)
/// declares `cpid` none and AE 26.5 reopens it with working space "None",
/// 8 bpc, no linearization and no linear blending: the legacy interpretation.
fn pre_declaration_context(project: &Project) -> crate::structure::ProjectColorContext {
    use crate::structure::ProjectColorContext;
    let unreadable = |reason| ProjectColorContext::Unsupported {
        profile: "unreadable working declaration".into(),
        reason,
    };
    let format = single_payload(project, *b"head")
        .and_then(|head| crate::schema::HeadRecord::decode(head).ok())
        .map(|head| head.format_version());
    let declares_management = project
        .chunks
        .iter()
        .any(|chunk| matches!(&chunk.id(), b"pcms" | b"pdvc"));
    if declares_management || format.is_none_or(|format| format >= FIRST_DECLARING_FORMAT) {
        return unreadable("missing or malformed native working-profile declaration");
    }
    let Some(cpid) = single_payload(project, *b"cpid").filter(|cpid| cpid.len() == 16) else {
        return unreadable(
            "older-format project has no single 16-byte cpid working-profile identity",
        );
    };
    let (Some(linearized), Some(linear_blending)) = (
        domain_flag(project, *b"lnrp"),
        domain_flag(project, *b"lnrb"),
    ) else {
        return unreadable("malformed or conflicting native linearization/blending declarations");
    };
    if cpid.iter().all(|byte| *byte == 0xFF) {
        if linearized || linear_blending {
            return unreadable(
                "older-format project without a working space declares linearization or linear blending",
            );
        }
        return ProjectColorContext::LegacyDeclared {
            declaration: format!(
                "project format {} predates the PwCs working-profile declaration; its cpid declares no working space (After Effects \"None\")",
                format.unwrap_or_default()
            ),
        };
    }
    ProjectColorContext::Unsupported {
        profile: pooled_profile_description(project, cpid)
            .unwrap_or_else(|| "unpooled older-format working profile".into()),
        reason: "older-format ICC working-space declaration (cpid) has no measured native fixture; only the \"None\" working space of pre-PwCs projects is admitted",
    }
}

fn single_payload(project: &Project, id: [u8; 4]) -> Option<&[u8]> {
    let mut found = project.chunks.iter().filter(|chunk| chunk.id() == id);
    let payload = found.next()?.data_payload()?;
    found.next().is_none().then_some(payload)
}

/// The ICC `desc` text of the `LIST CPPl` profile whose ICC profile ID equals
/// `cpid`, used only to name the profile in a diagnostic.
fn pooled_profile_description(project: &Project, cpid: &[u8]) -> Option<String> {
    let pool = project
        .chunks
        .iter()
        .find(|chunk| chunk.list_kind() == Some(*b"CPPl"))?;
    let icc = pool
        .children()?
        .iter()
        .filter(|chunk| chunk.id() == *b"pprf")
        .filter_map(Chunk::data_payload)
        .find(|icc| icc.get(84..100) == Some(cpid))?;
    icc_description(icc)
}

/// Reads the first record of an ICC v4 `mluc` (or v2 `desc`) description tag.
fn icc_description(icc: &[u8]) -> Option<String> {
    let be32 = |offset: usize| {
        icc.get(offset..offset + 4)
            .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap_or_default()) as usize)
    };
    let count = be32(128)?;
    let (offset, length) = (0..count.min(64)).find_map(|tag| {
        let entry = 132 + 12 * tag;
        (icc.get(entry..entry + 4)? == b"desc").then_some((be32(entry + 4)?, be32(entry + 8)?))
    })?;
    let tag = icc.get(offset..offset.checked_add(length)?)?;
    let text: String = match tag.get(0..4)? {
        b"mluc" => {
            let at = |offset: usize| {
                tag.get(offset..offset + 4)
                    .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap_or_default()) as usize)
            };
            let (size, start) = (at(20)?, at(24)?);
            let units: Vec<u16> = tag
                .get(start..start.checked_add(size)?)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_be_bytes(*pair))
                .collect();
            String::from_utf16(&units).ok()?
        }
        b"desc" => {
            let size = u32::from_be_bytes(tag.get(8..12)?.try_into().ok()?) as usize;
            String::from_utf8_lossy(tag.get(12..12 + size)?).into_owned()
        }
        _ => return None,
    };
    let text: String = text.trim_end_matches('\0').chars().take(160).collect();
    (!text.is_empty()).then_some(text)
}

/// AE 26.5x89's built-in "Rec.2100 PQ" working profile as natively saved in
/// `color_ae_rec2100_pq_project.aep`: `baseProfileType` 2 with the embedded
/// Adobe ICC (BT.2020 primaries, PQ transfer, `lumi` 100 cd/m²).
const ADOBE_REC2100_PQ_NAME: &str = "Rec.2100 PQ";
const ADOBE_REC2100_PQ_SHA256: &str =
    "e333f8242594ac5c29a9b68ca9b934baa5e06662b4882ee0fae02564ef10bee0";

/// Bind a built-in profile only by its exact pinned name and ICC payload.
fn adobe_built_in(value: &serde_json::Value) -> Option<(HdrTransfer, HdrProfileBinding)> {
    use sha2::{Digest, Sha256};

    if value["baseProfileType"].as_u64()? != 2
        || value["baseColorProfile"]["colorProfileName"].as_str()? != ADOBE_REC2100_PQ_NAME
    {
        return None;
    }
    let data = STANDARD
        .decode(value["baseColorProfile"]["colorProfileData"].as_str()?)
        .ok()?;
    (format!("{:x}", Sha256::digest(&data)) == ADOBE_REC2100_PQ_SHA256)
        .then_some((HdrTransfer::Pq, HdrProfileBinding::AdobeRec2100Pq))
}

/// AE 26.5x89's built-in SDR working profiles as natively saved (8 bpc,
/// non-linearized) in `color_ae_working_srgb.aep` and
/// `color_ae_working_rec709_gamma24.aep`: `baseProfileType` 2 with the
/// embedded Adobe ICC. Their point colours are working-space code values that
/// AE renders unchanged, the legacy numeric interpretation.
const ADOBE_SDR_WORKING_PROFILES: [(&str, &str); 2] = [
    (
        "sRGB IEC61966-2.1",
        "2b3aa1645779a9e634744faf9b01e9102b0c9b88fd6deced7934df86b949af7e",
    ),
    (
        "Rec.709 Gamma 2.4",
        "d6e44b64516d08fa2e3032878d03de0135c87433863df04d2ea0770f6adcb6c7",
    ),
];

/// Admit a built-in SDR profile only by its exact pinned name and ICC payload.
fn adobe_sdr_built_in(value: &serde_json::Value) -> Option<&'static str> {
    use sha2::{Digest, Sha256};

    if value["baseProfileType"].as_u64()? != 2 {
        return None;
    }
    let name = value["baseColorProfile"]["colorProfileName"].as_str()?;
    let data = STANDARD
        .decode(value["baseColorProfile"]["colorProfileData"].as_str()?)
        .ok()?;
    let sha256 = format!("{:x}", Sha256::digest(&data));
    ADOBE_SDR_WORKING_PROFILES
        .iter()
        .find(|(pinned, digest)| *pinned == name && *digest == sha256)
        .map(|(pinned, _)| *pinned)
}

/// Name-only recognition, used solely to choose a more specific diagnostic.
fn profile_name_is_rec2100(value: &serde_json::Value) -> bool {
    value["baseColorProfile"]["colorProfileName"]
        .as_str()
        .is_some_and(|name| name.to_ascii_lowercase().contains("2100"))
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

/// An in-house AE 26.5 fixture (working space "None") rewritten into the
/// pre-`PwCs` layout: the `pcms`/`PwCs`/`pdvc` declarations and their
/// `Utf8` values removed, the `head` format lowered to 93 and the producer
/// word (`head` and `svap`) set to After Effects 17.0 build 26.
#[cfg(test)]
pub(crate) fn pre_declaration_project() -> Project {
    let mut project = Project::parse(include_bytes!(
        "../tests/fixtures/color_ae_channel_mixer.aep"
    ))
    .unwrap();
    // Major 17 = high part 2 (bits 26..30) * 8 + low part 1 (bits 19..21).
    let producer: u32 = (2 << 26) | (1 << 19) | 26;
    let mut chunks = Vec::new();
    let mut skip_value = false;
    for chunk in project.chunks {
        if std::mem::take(&mut skip_value) && chunk.id() == *b"Utf8" {
            continue;
        }
        if matches!(&chunk.id(), b"pcms" | b"PwCs" | b"pdvc") {
            skip_value = true;
            continue;
        }
        if chunk.id() == *b"head" {
            let mut head = chunk.data_payload().unwrap().to_vec();
            head[1] = 93;
            head[4..8].copy_from_slice(&producer.to_be_bytes());
            chunks.push(Chunk::data(*b"head", head).unwrap());
            continue;
        }
        if chunk.id() == *b"svap" {
            chunks.push(Chunk::data(*b"svap", producer.to_be_bytes().to_vec()).unwrap());
            continue;
        }
        chunks.push(chunk);
    }
    project.chunks = chunks;
    project
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project_with_profile(transfer: HdrTransfer, icc: &[u8]) -> Project {
        let spec = crate::writer::CompositionSpec {
            name: "HDR labels".into(),
            width: 320,
            height: 180,
            duration_frames: 24,
        };
        let mut bytes = crate::writer::write_empty_composition(&spec).unwrap();
        configure_project(&mut bytes, transfer).unwrap();
        let mut project = Project::parse(&bytes).unwrap();
        let index = project
            .chunks
            .iter()
            .position(|chunk| chunk.id() == *b"PwCs")
            .unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(project.chunks[index + 1].data_payload().unwrap()).unwrap();
        value["baseColorProfile"]["colorProfileData"] = STANDARD.encode(icc).into();
        project.chunks[index + 1] =
            Chunk::data(*b"Utf8", serde_json::to_vec(&value).unwrap()).unwrap();
        project
    }

    #[test]
    fn native_built_in_rec2100_profiles_stay_unbound_with_a_specific_reason() {
        let mut project = project_with_profile(HdrTransfer::Pq, profile(HdrTransfer::Pq).1);
        let index = project
            .chunks
            .iter()
            .position(|chunk| chunk.id() == *b"PwCs")
            .unwrap();
        for (kind, name) in [(1, "Rec. 2100 PQ"), (3, "ITU-R BT.2100 HLG")] {
            let value = serde_json::json!({
                "baseProfileType": kind,
                "baseColorProfile": {"colorProfileName": name, "colorProfileData": STANDARD.encode(b"not owned")}
            });
            project.chunks[index + 1] =
                Chunk::data(*b"Utf8", serde_json::to_vec(&value).unwrap()).unwrap();
            let crate::structure::ProjectColorContext::Unsupported { profile, reason } =
                import_context(&project)
            else {
                panic!("built-in {name} must remain unbound")
            };
            assert_eq!(profile, name);
            assert!(reason.contains("built-in Rec.2100"), "{reason}");
        }
    }

    const NATIVE_REC2100_PQ: &[u8] =
        include_bytes!("../tests/fixtures/color_ae_rec2100_pq_project.aep");

    fn native_working_value(project: &Project) -> serde_json::Value {
        let index = project
            .chunks
            .iter()
            .position(|chunk| chunk.id() == *b"PwCs")
            .unwrap();
        serde_json::from_slice(project.chunks[index + 1].data_payload().unwrap()).unwrap()
    }

    fn with_working_value(project: &Project, value: &serde_json::Value) -> Project {
        let mut project = project.clone();
        let index = project
            .chunks
            .iter()
            .position(|chunk| chunk.id() == *b"PwCs")
            .unwrap();
        project.chunks[index + 1] =
            Chunk::data(*b"Utf8", serde_json::to_vec(value).unwrap()).unwrap();
        project
    }

    #[test]
    fn native_ae_rec2100_pq_project_binds_the_built_in_pq_profile() {
        use sha2::{Digest, Sha256};
        assert_eq!(
            format!("{:x}", Sha256::digest(NATIVE_REC2100_PQ)),
            "1be8348f26fdca009d01fce9f9efac4f26d840ae942dfcbc8f2305caa42cb465"
        );
        let project = Project::parse(NATIVE_REC2100_PQ).unwrap();
        let value = native_working_value(&project);
        assert_eq!(value["baseProfileType"], 2);
        assert_eq!(value["baseColorProfile"]["colorProfileName"], "Rec.2100 PQ");
        assert_eq!(owned_transfer(&project), None);
        assert_eq!(
            import_context(&project),
            crate::structure::ProjectColorContext::Hdr {
                transfer: HdrTransfer::Pq,
                linear_blending: false,
                binding: HdrProfileBinding::AdobeRec2100Pq,
            }
        );
        assert_eq!(
            HdrProfileBinding::AdobeRec2100Pq.graphics_white_nits(),
            100.0
        );

        // The ICC payload, not the label, is the identity: an unpinned W-variant
        // name or a changed payload stays unbound with the specific reason.
        let mut renamed = value.clone();
        renamed["baseColorProfile"]["colorProfileName"] = "Rec.2100 PQ W203".into();
        let mut changed = value.clone();
        changed["baseColorProfile"]["colorProfileData"] = STANDARD.encode(b"other").into();
        let mut hlg = value;
        hlg["baseColorProfile"]["colorProfileName"] = "Rec.2100 HLG W203".into();
        for candidate in [renamed, changed, hlg] {
            let crate::structure::ProjectColorContext::Unsupported { reason, .. } =
                import_context(&with_working_value(&project, &candidate))
            else {
                panic!("unpinned built-in profile must stay unbound: {candidate}")
            };
            assert!(reason.contains("built-in Rec.2100"), "{reason}");
        }
        // Linearized working spaces stay unsupported even for the pinned profile.
        let mut linearized = project.clone();
        linearized
            .chunks
            .push(Chunk::data(*b"lnrp", vec![1]).unwrap());
        assert!(matches!(
            import_context(&linearized),
            crate::structure::ProjectColorContext::Unsupported { .. }
        ));
    }

    #[test]
    fn pre_declaration_projects_without_a_working_space_are_admitted_as_legacy() {
        use crate::structure::ProjectColorContext;
        let project = pre_declaration_project();
        assert!(!project.chunks.iter().any(|c| c.id() == *b"PwCs"));
        let ProjectColorContext::LegacyDeclared { declaration } = import_context(&project) else {
            panic!("{:?}", import_context(&project))
        };
        assert_eq!(
            declaration,
            "project format 93 predates the PwCs working-profile declaration; its cpid declares no working space (After Effects \"None\")"
        );

        let unreadable = |project: &Project| match import_context(project) {
            ProjectColorContext::Unsupported { profile, reason } => {
                assert_eq!(profile, "unreadable working declaration");
                reason
            }
            other => panic!("{other:?}"),
        };
        // A declaring-format project that lost its PwCs stays malformed.
        let mut declaring = project.clone();
        let head = declaring
            .chunks
            .iter()
            .position(|c| c.id() == *b"head")
            .unwrap();
        let mut bytes = declaring.chunks[head].data_payload().unwrap().to_vec();
        bytes[1] = FIRST_DECLARING_FORMAT;
        declaring.chunks[head] = Chunk::data(*b"head", bytes).unwrap();
        assert!(unreadable(&declaring).contains("missing or malformed"));
        // So does a partial modern declaration family.
        let mut partial = project.clone();
        partial.chunks.push(Chunk::data(*b"pcms", vec![1]).unwrap());
        assert!(unreadable(&partial).contains("missing or malformed"));
        // Linearization without a working space is not guessed.
        let mut linearized = project.clone();
        linearized
            .chunks
            .push(Chunk::data(*b"lnrp", vec![1]).unwrap());
        assert!(unreadable(&linearized).contains("linearization"));
        // No working-profile identity at all.
        let mut anonymous = project.clone();
        anonymous.chunks.retain(|c| c.id() != *b"cpid");
        assert!(unreadable(&anonymous).contains("cpid"));
    }

    #[test]
    fn pre_declaration_icc_working_spaces_stay_unsupported_and_name_the_pooled_profile() {
        use crate::structure::ProjectColorContext;
        let icc = profile(HdrTransfer::Pq).1;
        let mut project = pre_declaration_project();
        let cpid = project
            .chunks
            .iter()
            .position(|c| c.id() == *b"cpid")
            .unwrap();
        project.chunks[cpid] = Chunk::data(*b"cpid", icc[84..100].to_vec()).unwrap();
        let pool = project
            .chunks
            .iter()
            .position(|c| c.list_kind() == Some(*b"CPPl"))
            .unwrap();
        project.chunks[pool] =
            Chunk::list(*b"CPPl", vec![Chunk::data(*b"pprf", icc.to_vec()).unwrap()]);
        let ProjectColorContext::Unsupported {
            profile: name,
            reason,
        } = import_context(&project)
        else {
            panic!("an older ICC working space must stay unsupported")
        };
        assert_eq!(name, profile(HdrTransfer::Pq).0);
        assert!(
            reason.contains("older-format ICC working-space declaration"),
            "{reason}"
        );

        project.chunks[pool] = Chunk::list(*b"CPPl", Vec::new());
        let ProjectColorContext::Unsupported { profile: name, .. } = import_context(&project)
        else {
            panic!("an unpooled older ICC working space must stay unsupported")
        };
        assert_eq!(name, "unpooled older-format working profile");
    }

    #[test]
    fn native_ae_sdr_working_spaces_are_admitted_as_declared_legacy() {
        use crate::structure::ProjectColorContext;
        use sha2::{Digest, Sha256};
        for (bytes, sha256, name) in [
            (
                include_bytes!("../tests/fixtures/color_ae_working_srgb.aep").as_slice(),
                "296bc5b3c80fe420b7da83c6d2850a5e118ccdd9794e3a497707b5268055b577",
                "sRGB IEC61966-2.1",
            ),
            (
                include_bytes!("../tests/fixtures/color_ae_working_rec709_gamma24.aep").as_slice(),
                "587b86ae00f15c6c351d0eb9bb62052425fcbf3da356f3d7808cfac79221c9d5",
                "Rec.709 Gamma 2.4",
            ),
        ] {
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), sha256);
            let project = Project::parse(bytes).unwrap();
            assert_eq!(
                import_context(&project),
                ProjectColorContext::LegacyDeclared {
                    declaration: format!(
                        "AE built-in \"{name}\" SDR working profile, identified by its exact AE 26.5 ICC payload; AE's display and output-module profile conversions are not modeled"
                    ),
                }
            );
            // The ICC payload, not the label, is the identity.
            let value = native_working_value(&project);
            let mut changed = value.clone();
            changed["baseColorProfile"]["colorProfileData"] = STANDARD.encode(b"other").into();
            let mut swapped = value;
            swapped["baseColorProfile"]["colorProfileName"] = "Rec.709 Gamma 2.2".into();
            for candidate in [changed, swapped] {
                assert!(
                    matches!(
                        import_context(&with_working_value(&project, &candidate)),
                        ProjectColorContext::Unsupported { .. }
                    ),
                    "{candidate}"
                );
            }
            // Linearized or linear-blending SDR working spaces stay unsupported.
            for switch in [*b"lnrp", *b"lnrb"] {
                let mut variant = project.clone();
                variant.chunks.push(Chunk::data(switch, vec![1]).unwrap());
                assert!(matches!(
                    import_context(&variant),
                    ProjectColorContext::Unsupported { .. }
                ));
            }
        }
    }

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
                    linear_blending: false,
                    binding: HdrProfileBinding::OwnedIcc,
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
                    linear_blending: true,
                    binding: HdrProfileBinding::OwnedIcc,
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
