//! Native Text Path links address mask identities, not cached popup ordinals.
use super::*;
use crate::properties::data;

pub(super) fn read(
    group: &[Chunk],
    source_content: &[Chunk],
) -> Result<NumericProperties, PropertyError> {
    Ok(runs(group)?
        .into_iter()
        .map(|(name, run)| {
            let property = unique_list(run, *b"tdbs").and_then(|storage| {
                let mut numeric = read_numeric(storage)?;
                if name == "ADBE Text Path" && storage.iter().any(|chunk| chunk.id() == *b"tdli") {
                    if numeric.animated
                        || numeric.expression_enabled
                        || numeric.dimensions_separated
                    {
                        return Err(PropertyError::Layout("dynamic Text Path mask reference"));
                    }
                    if numeric.values.len() != 1 {
                        return Err(PropertyError::Layout("nonscalar Text Path mask reference"));
                    }
                    let reference = data(storage, *b"tdli")?;
                    let reference: [u8; 4] = reference
                        .try_into()
                        .map_err(|_| PropertyError::Layout("malformed Text Path mask reference"))?;
                    let ordinal = mask_ordinal(source_content, u32::from_be_bytes(reference))?;
                    // Existing text lowering consumes ordinals; resolve the native
                    // identity once without changing shared mask-guide indexing.
                    numeric.values = vec![f64::from(ordinal)];
                }
                Ok(numeric)
            });
            (name.to_owned(), property)
        })
        .collect())
}

fn mask_ordinal(content: &[Chunk], reference: u32) -> Result<u32, PropertyError> {
    let roots = root_runs(content)?;
    let mut parades = roots
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Mask Parade");
    let Some((_, parade)) = parades.next() else {
        return Err(PropertyError::Layout("Text Path references an absent mask"));
    };
    if parades.next().is_some() {
        return Err(PropertyError::Layout("ambiguous Text Path mask parade"));
    }
    let masks = runs(unique_list(parade, *b"tdgp")?)?;
    let mut selected = None;
    for (index, (_, mask)) in masks
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Mask Atom")
        .enumerate()
    {
        let info = data(mask, *b"mkif")?;
        if info.len() != 48 {
            return Err(PropertyError::Layout("malformed Text Path mask identity"));
        }
        let identity = u32::from_be_bytes(info[8..12].try_into().expect("checked mkif length"));
        if identity == reference {
            if selected.is_some() {
                return Err(PropertyError::Layout("ambiguous Text Path mask identity"));
            }
            selected = Some(u32::try_from(index + 1).map_err(|_| {
                PropertyError::Layout("Text Path mask ordinal exceeds native range")
            })?);
        }
    }
    selected.ok_or(PropertyError::Layout("Text Path references an absent mask"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> Chunk {
        let mut bytes = value.as_bytes().to_vec();
        bytes.resize(40, 0);
        Chunk::data(*b"tdmn", bytes).unwrap()
    }

    fn masks(identities: &[u32]) -> Vec<Chunk> {
        let mut atoms = Vec::new();
        for identity in identities {
            let mut info = vec![0; 48];
            info[8..12].copy_from_slice(&identity.to_be_bytes());
            atoms.extend([
                name("ADBE Mask Atom"),
                Chunk::data(*b"mkif", info).unwrap(),
                Chunk::list(*b"tdgp", Vec::new()),
            ]);
        }
        vec![Chunk::list(
            *b"tdgp",
            vec![name("ADBE Mask Parade"), Chunk::list(*b"tdgp", atoms)],
        )]
    }

    fn property(reference: Option<Vec<u8>>, animated: bool) -> Vec<Chunk> {
        let mut flags = vec![0; 124];
        flags[..2].copy_from_slice(&[0xdb, 0x99]);
        flags[3] = 1;
        flags[6..8].copy_from_slice(&1_u16.to_be_bytes());
        flags[12..16].copy_from_slice(&1000_u32.to_be_bytes());
        flags[68] = u8::from(animated);
        let mut storage = vec![
            Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
            Chunk::data(*b"tdb4", flags).unwrap(),
            Chunk::data(*b"cdat", 2.0_f64.to_be_bytes().to_vec()).unwrap(),
        ];
        if let Some(reference) = reference {
            storage.push(Chunk::data(*b"tdli", reference).unwrap());
        }
        vec![name("ADBE Text Path"), Chunk::list(*b"tdbs", storage)]
    }

    #[test]
    fn native_mask_identity_is_not_a_popup_ordinal() {
        let source = masks(&[17, 2]);
        assert_eq!(mask_ordinal(&source, 17).unwrap(), 1);
        assert_eq!(mask_ordinal(&source, 2).unwrap(), 2);
        let values = read(
            &property(Some(17_u32.to_be_bytes().to_vec()), false),
            &source,
        )
        .unwrap();
        assert_eq!(values[0].1.as_ref().unwrap().values, [1.0]);
    }

    #[test]
    fn absent_native_reference_preserves_legacy_numeric_selection() {
        let values = read(&property(None, false), &[]).unwrap();
        assert_eq!(values[0].1.as_ref().unwrap().values, [2.0]);
    }

    #[test]
    fn explicit_bad_reference_never_falls_back_to_numeric_cache() {
        let source = masks(&[1, 2]);
        for reference in [vec![0, 1], 99_u32.to_be_bytes().to_vec()] {
            assert!(
                read(&property(Some(reference), false), &source).unwrap()[0]
                    .1
                    .is_err()
            );
        }
        assert!(mask_ordinal(&masks(&[2, 2]), 2).is_err());
        let mut group = property(Some(1_u32.to_be_bytes().to_vec()), false);
        group[1]
            .children_mut()
            .unwrap()
            .push(Chunk::data(*b"tdli", 2_u32.to_be_bytes().to_vec()).unwrap());
        assert!(read(&group, &source).unwrap()[0].1.is_err());
    }

    #[test]
    fn reference_does_not_override_malformed_numeric_shape() {
        let mut group = property(Some(1_u32.to_be_bytes().to_vec()), false);
        let storage = group[1].children_mut().unwrap();
        let mut metadata = storage[1].data_payload().unwrap().to_vec();
        metadata[3] = 2;
        storage[1] = Chunk::data(*b"tdb4", metadata).unwrap();
        storage[2] = Chunk::data(*b"cdat", vec![0; 16]).unwrap();
        let values = read(&group, &masks(&[1])).unwrap();
        assert!(matches!(
            &values[0].1,
            Err(PropertyError::Layout("nonscalar Text Path mask reference"))
        ));
    }

    #[test]
    fn enabled_reference_expression_is_rejected_but_disabled_expression_is_static() {
        for disabled in [false, true] {
            let mut group = property(Some(1_u32.to_be_bytes().to_vec()), false);
            let metadata = &mut group[1].children_mut().unwrap()[1];
            let mut bytes = metadata.data_payload().unwrap().to_vec();
            bytes[120] = 1;
            bytes[119] = u8::from(disabled);
            *metadata = Chunk::data(*b"tdb4", bytes).unwrap();
            let values = read(&group, &masks(&[1])).unwrap();
            if disabled {
                assert_eq!(values[0].1.as_ref().unwrap().values, [1.0]);
            } else {
                assert!(matches!(
                    &values[0].1,
                    Err(PropertyError::Layout("dynamic Text Path mask reference"))
                ));
            }
        }
    }

    #[test]
    fn dynamic_native_reference_is_not_a_static_selection() {
        let values = read(
            &property(Some(1_u32.to_be_bytes().to_vec()), true),
            &masks(&[1]),
        )
        .unwrap();
        assert!(matches!(
            &values[0].1,
            Err(PropertyError::Layout("dynamic Text Path mask reference"))
        ));
    }
}
