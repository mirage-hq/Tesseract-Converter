//! Source-only composition marker clocks, not persisted FX marker metadata.
//! Marker keys use the observed 16-byte native list envelope; values are references
//! to marker records, not numeric property doubles.
use crate::{
    properties::{data, root_runs, unique_list},
    rifx::Chunk,
};

pub(super) fn times(children: &[Chunk]) -> Result<Vec<f64>, String> {
    decode(children).map_err(|error| error.to_string())
}

fn decode(children: &[Chunk]) -> Result<Vec<f64>, crate::properties::PropertyError> {
    use crate::properties::PropertyError;
    let mut sections = children
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"SecL"));
    let Some(section) = sections.next() else {
        return Ok(Vec::new());
    };
    if sections.next().is_some() {
        return Err(PropertyError::Layout(
            "duplicate composition marker section",
        ));
    }
    let content = section
        .children()
        .ok_or(PropertyError::Layout("opaque marker section"))?;
    let runs = root_runs(content)?;
    let mut markers = runs.iter().filter(|(name, _)| *name == "ADBE Marker");
    let Some(marker) = markers.next() else {
        return Ok(Vec::new());
    };
    if markers.next().is_some() {
        return Err(PropertyError::Layout("duplicate marker property"));
    }
    let wrapper = unique_list(marker.1, *b"mrst")?;
    let property = unique_list(wrapper, *b"tdbs")?;
    let meta = data(property, *b"tdb4")?;
    let list = unique_list(property, *b"list")?;
    let header = data(list, *b"lhd3")?;
    if meta.len() < 16 || header.len() < 24 || header[18..20] != [0, 16] || header[23] != 4 {
        return Err(PropertyError::Layout("unsupported marker key layout"));
    }
    let count = usize::from(u16::from_be_bytes([header[10], header[11]]));
    let bytes = data(list, *b"ldat")?;
    let timebase = u32::from_be_bytes(meta[12..16].try_into().expect("validated metadata length"));
    if timebase == 0 || bytes.len() != count * 16 {
        return Err(PropertyError::Layout(
            "invalid marker key length or timebase",
        ));
    }
    let mut times = Vec::with_capacity(count);
    for item in bytes.as_chunks::<16>().0.iter() {
        let ticks = i32::from_be_bytes(item[..4].try_into().expect("fixed marker key stride"));
        let time = f64::from(ticks) / f64::from(timebase);
        if times.last().is_some_and(|previous| *previous >= time) {
            return Err(PropertyError::Layout("unordered composition markers"));
        }
        times.push(time);
    }
    Ok(times)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_clock_list_preserves_times_and_rejects_truncation() {
        let make = |truncate: bool| {
            let mut name = b"ADBE Marker".to_vec();
            name.resize(40, 0);
            let mut meta = vec![0; 124];
            meta[12..16].copy_from_slice(&30720_u32.to_be_bytes());
            let mut header = vec![0; 24];
            header[10..12].copy_from_slice(&2_u16.to_be_bytes());
            header[18..20].copy_from_slice(&16_u16.to_be_bytes());
            header[23] = 4;
            let mut keys = vec![0; 32];
            keys[..4].copy_from_slice(&61440_i32.to_be_bytes());
            keys[16..20].copy_from_slice(&92160_i32.to_be_bytes());
            if truncate {
                keys.pop();
            }
            vec![Chunk::list(
                *b"SecL",
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
                                            Chunk::data(*b"ldat", keys).unwrap(),
                                        ],
                                    ),
                                ],
                            )],
                        ),
                    ],
                )],
            )]
        };
        assert_eq!(times(&make(false)).unwrap(), [2.0, 3.0]);
        assert!(times(&make(true)).is_err());
        assert!(times(&[]).unwrap().is_empty());
    }
}
