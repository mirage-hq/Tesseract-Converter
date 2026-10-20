//! Layer marker times and comments for numeric expression evaluation.

use super::MarkerModel;
use crate::{properties, rifx::Chunk, structure::Layer};

pub(super) fn read(layer: &Layer) -> MarkerModel {
    let decoded = times(&layer.content).and_then(|times| {
        let start = layer
            .record
            .start_time()
            .ok_or(properties::PropertyError::Layout("marker owner start"))?;
        let stretch = layer
            .record
            .stretch()
            .ok_or(properties::PropertyError::Layout("marker owner stretch"))?;
        // Marker API times are composition times, not source-local property times.
        if stretch <= 0.0 {
            return Err(properties::PropertyError::Layout(
                "nonpositive marker owner stretch",
            ));
        }
        Ok(times
            .into_iter()
            .map(|time| start + time * stretch)
            .collect::<Vec<_>>())
    });
    match decoded {
        Ok(times) => {
            let (names, names_error) = match names(&layer.content, times.len()) {
                Ok(names) => (names, None),
                Err(error) => (Vec::new(), Some(error.to_string())),
            };
            MarkerModel {
                times,
                names,
                names_error,
                error: None,
            }
        }
        Err(error) => MarkerModel {
            times: Vec::new(),
            names: Vec::new(),
            names_error: None,
            error: Some(error.to_string()),
        },
    }
}

fn names(content: &[Chunk], count: usize) -> Result<Vec<String>, properties::PropertyError> {
    use properties::{PropertyError, data, unique_list};
    if count == 0 {
        return Ok(Vec::new());
    }
    let runs = properties::root_runs(content)?;
    let (_, run) = runs
        .iter()
        .find(|(name, _)| *name == "ADBE Marker")
        .ok_or(PropertyError::Layout("missing marker property"))?;
    let records = unique_list(unique_list(run, *b"mrst")?, *b"mrky")?;
    if records.len() != count {
        return Err(PropertyError::Layout("marker value count"));
    }
    records
        .iter()
        .map(|record| {
            if record.list_kind() != Some(*b"Nmrd") {
                return Err(PropertyError::Layout("unsupported marker value"));
            }
            let children = record
                .children()
                .ok_or(PropertyError::Layout("opaque marker value"))?;
            let header = data(children, *b"NmHd")?;
            if header.len() != 20 {
                return Err(PropertyError::Layout("marker value header"));
            }
            let strings: Vec<_> = children
                .iter()
                .filter(|chunk| chunk.id() == *b"Utf8")
                .collect();
            if strings.len() != 5 {
                return Err(PropertyError::Layout("marker value strings"));
            }
            let comment = strings[0]
                .data_payload()
                .ok_or(PropertyError::Layout("opaque marker comment"))?;
            let comment = std::str::from_utf8(comment)
                .map_err(|_| PropertyError::Layout("marker comment UTF-8"))?
                .trim_end_matches('\0');
            if comment.contains('\0') {
                return Err(PropertyError::Layout("embedded marker comment terminator"));
            }
            Ok(comment.to_owned())
        })
        .collect()
}

fn times(content: &[Chunk]) -> Result<Vec<f64>, properties::PropertyError> {
    use properties::{PropertyError, data, unique_list};
    let runs = properties::root_runs(content)?;
    let mut markers = runs.iter().filter(|(name, _)| *name == "ADBE Marker");
    let Some((_, run)) = markers.next() else {
        return Ok(Vec::new());
    };
    if markers.next().is_some() {
        return Err(PropertyError::Layout("duplicate marker property"));
    }
    let marker = unique_list(run, *b"mrst")?;
    let leaf = unique_list(marker, *b"tdbs")?;
    let meta = data(leaf, *b"tdb4")?;
    if meta.len() != 124 || meta[..8] != [0xdb, 0x99, 0, 1, 0, 0, 0, 0] {
        return Err(PropertyError::Layout("unsupported marker descriptor"));
    }
    let timebase = u32::from_be_bytes([meta[12], meta[13], meta[14], meta[15]]);
    if timebase == 0 {
        return Err(PropertyError::Layout("zero marker timebase"));
    }
    let list = unique_list(leaf, *b"list")?;
    let header = data(list, *b"lhd3")?;
    if header.len() != 52
        || header[..4] != [0, 0xd0, 0x0b, 0xee]
        || header[18..20] != [0, 16]
        || header[23] != 4
    {
        return Err(PropertyError::Layout("unsupported marker key layout"));
    }
    let count = usize::from(u16::from_be_bytes([header[10], header[11]]));
    let bytes = data(list, *b"ldat")?;
    if bytes.len() != count * 16 {
        return Err(PropertyError::Layout("marker key length"));
    }
    let mut times = Vec::with_capacity(count);
    for item in bytes.as_chunks::<16>().0.iter() {
        let time = f64::from(i32::from_be_bytes([item[0], item[1], item[2], item[3]]))
            / f64::from(timebase);
        if times.last().is_some_and(|last| *last >= time) {
            return Err(PropertyError::Layout("unordered marker keys"));
        }
        times.push(time);
    }
    Ok(times)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn content() -> Vec<Chunk> {
        let mut meta = vec![0; 124];
        meta[..8].copy_from_slice(&[0xdb, 0x99, 0, 1, 0, 0, 0, 0]);
        meta[12..16].copy_from_slice(&24000_u32.to_be_bytes());
        let mut header = vec![0; 52];
        header[..4].copy_from_slice(&[0, 0xd0, 0x0b, 0xee]);
        header[11] = 2;
        header[19] = 16;
        header[23] = 4;
        let mut bytes = vec![0; 32];
        bytes[..4].copy_from_slice(&(-12000_i32).to_be_bytes());
        bytes[16..20].copy_from_slice(&48000_i32.to_be_bytes());
        let mut name = b"ADBE Marker".to_vec();
        name.resize(40, 0);
        vec![Chunk::list(
            *b"tdgp",
            vec![
                Chunk::data(*b"tdmn", name).unwrap(),
                Chunk::list(
                    *b"mrst",
                    vec![Chunk::list(
                        *b"tdbs",
                        vec![
                            Chunk::data(*b"tdb4", meta).unwrap(),
                            Chunk::list(
                                *b"list",
                                vec![
                                    Chunk::data(*b"lhd3", header).unwrap(),
                                    Chunk::data(*b"ldat", bytes).unwrap(),
                                ],
                            ),
                        ],
                    )],
                ),
            ],
        )]
    }
    #[test]
    fn marker_comments_follow_key_order_and_reject_ambiguous_layouts() {
        let mut chunks = content();
        let root = chunks[0].children_mut().unwrap();
        let wrapper = root
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"mrst"))
            .unwrap()
            .children_mut()
            .unwrap();
        let records = ["second", "first"].map(|name| {
            let mut fields = vec![Chunk::data(*b"NmHd", vec![0; 20]).unwrap()];
            for text in [name, "", "", "", ""] {
                fields.push(Chunk::data(*b"Utf8", text.as_bytes()).unwrap());
            }
            Chunk::list(*b"Nmrd", fields)
        });
        wrapper.push(Chunk::list(*b"mrky", records.to_vec()));
        assert_eq!(names(&chunks, 2).unwrap(), ["second", "first"]);
        assert!(names(&chunks, 1).is_err());
        assert_eq!(times(&chunks).unwrap(), [-0.5, 2.0]);
    }

    #[test]
    fn marker_times_use_signed_native_units() {
        assert_eq!(times(&content()).unwrap(), [-0.5, 2.0]);
    }
}
