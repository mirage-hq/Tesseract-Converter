//! Native Text numeric control envelopes, with independently authored defaults.
//!
//! Only descriptor/bounds metadata is retained. Every visible value and key list
//! comes from the editable input; source text, selectors and owners are not replayed.
use std::collections::BTreeMap;
use std::sync::LazyLock;

use crate::rifx::{Chunk, Rifx};

use super::AepWriteError;
use super::keyframes::PropertyClock;

const INVALID: AepWriteError =
    AepWriteError::Invalid("native Text control reference layout changed");
const SOURCES: [&[u8]; 4] = [
    include_bytes!("../../tests/fixtures/text/import_text_selector_controls.aep"),
    include_bytes!("../../tests/fixtures/text/import_text_animator_channels.aep"),
    include_bytes!("../../tests/fixtures/text/import_text_path_options.aep"),
    include_bytes!("../../tests/fixtures/text/import_text_additional_controls.aep"),
];

static PROFILES: LazyLock<Result<BTreeMap<Vec<u8>, Chunk>, AepWriteError>> = LazyLock::new(|| {
    let mut profiles = BTreeMap::new();
    for source in SOURCES {
        let native = Rifx::parse_with(source, |kind| kind == *b"btdk")?;
        collect(native.chunks(), &mut profiles);
    }
    Ok(profiles)
});

fn collect(children: &[Chunk], profiles: &mut BTreeMap<Vec<u8>, Chunk>) {
    for pair in children.windows(2) {
        if pair[0].id() != *b"tdmn" || pair[1].list_kind() != Some(*b"tdbs") {
            continue;
        }
        let Some(name) = pair[0].data_payload() else {
            continue;
        };
        let name = name.split(|byte| *byte == 0).next().unwrap_or_default();
        if name.starts_with(b"ADBE Text ")
            && pair[1]
                .children()
                .is_some_and(|children| children.iter().any(|child| child.id() == *b"cdat"))
        {
            profiles.insert(name.to_vec(), pair[1].clone());
        }
    }
    for child in children {
        if let Some(children) = child.children() {
            collect(children, profiles);
        }
    }
}

pub(super) fn normalize(group: &mut Chunk, clock: PropertyClock) -> Result<(), AepWriteError> {
    let profiles = PROFILES.as_ref().map_err(|_| INVALID)?;
    let Some(children) = group.children_mut() else {
        return Ok(());
    };
    for index in 0..children.len().saturating_sub(1) {
        if children[index].id() != *b"tdmn" || children[index + 1].list_kind() != Some(*b"tdbs") {
            continue;
        }
        let name = children[index].data_payload().ok_or(INVALID)?;
        let name = name.split(|byte| *byte == 0).next().unwrap_or_default();
        let Some(profile) = profiles.get(name) else {
            continue;
        };
        let generated = children[index + 1].children().ok_or(INVALID)?;
        let value = generated
            .iter()
            .find(|child| child.id() == *b"cdat" || child.list_kind() == Some(*b"list"))
            .ok_or(INVALID)?;
        let animated = value.list_kind() == Some(*b"list");
        let mut native = profile.clone();
        let metadata = native.children_mut().ok_or(INVALID)?;
        // Value/key events precede native bound records; appending after bounds
        // makes Adobe reject the otherwise correct descriptor as missing data.
        *metadata
            .iter_mut()
            .find(|child| child.id() == *b"cdat")
            .ok_or(INVALID)? = value.clone();
        let descriptor = metadata
            .iter_mut()
            .find(|child| child.id() == *b"tdb4")
            .ok_or(INVALID)?;
        let mut bytes: [u8; 124] = descriptor
            .data_payload()
            .ok_or(INVALID)?
            .try_into()
            .map_err(|_| INVALID)?;
        bytes[12..16].copy_from_slice(&clock.ticks().to_be_bytes());
        if animated {
            // Native Text profiles use scalar/spatial/color selection bits 1/15/7
            // for a constant, and 0/14/6 for keys. Preserve the native value type.
            let selection = u16::from_be_bytes([bytes[4], bytes[5]])
                .checked_sub(1)
                .ok_or(INVALID)?;
            bytes[4..6].copy_from_slice(&selection.to_be_bytes());
            bytes[68] = 1;
        }
        *descriptor = Chunk::data(*b"tdb4", bytes.to_vec())?;
        children[index + 1] = native;
    }
    for child in children {
        normalize(child, clock)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::views::{self, ValueKind};
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn native_text_selector_envelope_retains_required_bounds_and_authored_values() {
        assert_eq!(
            format!("{:x}", Sha256::digest(SOURCES[0])),
            "7aedddf83a6a2f7afa849d3206d48dab35b6c43b5b66067bc268e1d39821f9b6"
        );
        let clock = PropertyClock::for_rate(crate::timing::FrameRate::new(30.0).unwrap()).unwrap();
        for name in [
            "ADBE Text Selector Mode",
            "ADBE Text Wiggly Max Amount",
            "ADBE Text Wiggly Min Amount",
            "ADBE Text Temporal Freq",
            "ADBE Text Wiggly Random Seed",
        ] {
            let generated =
                views::property_with_clock(ValueKind::Scalar, &[7.0], None, None, clock).unwrap();
            let value = generated
                .children()
                .unwrap()
                .iter()
                .find(|child| child.id() == *b"cdat")
                .unwrap()
                .clone();
            let mut group = views::group(1, "", vec![(name, generated)]).unwrap();
            normalize(&mut group, clock).unwrap();
            let native = &group.children().unwrap()[3];
            let children = native.children().unwrap();
            assert_eq!(
                children
                    .iter()
                    .find(|child| child.id() == *b"cdat")
                    .unwrap(),
                &value
            );
            let profile = &PROFILES.as_ref().unwrap()[name.as_bytes()];
            assert_eq!(
                children.iter().map(Chunk::id).collect::<Vec<_>>(),
                profile
                    .children()
                    .unwrap()
                    .iter()
                    .map(Chunk::id)
                    .collect::<Vec<_>>(),
                "native event/bounds order must be retained"
            );
            for id in [*b"tdsb", *b"tdum", *b"tduM"] {
                assert_eq!(
                    children.iter().find(|child| child.id() == id),
                    profile
                        .children()
                        .unwrap()
                        .iter()
                        .find(|child| child.id() == id)
                );
            }
            let mut descriptor = profile
                .children()
                .unwrap()
                .iter()
                .find(|child| child.id() == *b"tdb4")
                .unwrap()
                .data_payload()
                .unwrap()
                .to_vec();
            descriptor[12..16].copy_from_slice(&clock.ticks().to_be_bytes());
            assert_eq!(
                children
                    .iter()
                    .find(|child| child.id() == *b"tdb4")
                    .unwrap()
                    .data_payload()
                    .unwrap(),
                descriptor
            );
        }
    }
}
