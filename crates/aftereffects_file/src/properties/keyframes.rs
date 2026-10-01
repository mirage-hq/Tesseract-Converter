//! Keyframe-list layouts measured in pinned MIT py-aep ldat_chunks.py.
use super::{NumericKeyframe, NumericValueKind, PropertyError, data};
use crate::rifx::Chunk;

pub(super) fn read_keyframes(
    children: &[Chunk],
    meta: &[u8],
    dimensions: usize,
    value_kind: NumericValueKind,
    parallel_stride: Option<usize>,
) -> Result<Vec<NumericKeyframe>, PropertyError> {
    let mut lists = children.iter().filter(|c| c.list_kind() == Some(*b"list"));
    let Some(list) = lists.next() else {
        return Ok(Vec::new());
    };
    if lists.next().is_some() {
        return Err(PropertyError::Layout("duplicate keyframe list"));
    }
    let list = list
        .children()
        .ok_or(PropertyError::Layout("opaque keyframe list"))?;
    let header = data(list, *b"lhd3")?;
    if header.len() < 24 {
        return Err(PropertyError::Layout("short keyframe header"));
    }
    let count = usize::from(u16::from_be_bytes([header[10], header[11]]));
    let stride = usize::from(u16::from_be_bytes([header[18], header[19]]));
    let spatial = meta[5] & 8 != 0;
    let expected = if let Some(stride) = parallel_stride {
        stride
    } else if value_kind == NumericValueKind::Color {
        152
    } else if spatial && dimensions == 2 {
        104
    } else if dimensions == 1 {
        48
    } else if dimensions == 2 {
        88
    } else if dimensions == 3 {
        128
    } else {
        return Err(PropertyError::Layout(
            "unsupported animated numeric dimensions",
        ));
    };
    if header[23] != 4 || stride != expected {
        return Err(PropertyError::Layout("unsupported keyframe item layout"));
    }
    let bytes = data(list, *b"ldat")?;
    let required = count
        .checked_mul(stride)
        .ok_or(PropertyError::Layout("keyframe count overflow"))?;
    if bytes.len() < required || bytes.len() - required >= stride {
        return Err(PropertyError::Layout("keyframe item length"));
    }
    let timebase = u32::from_be_bytes(
        meta[12..16]
            .try_into()
            .map_err(|_| PropertyError::Layout("timebase"))?,
    );
    if count != 0 && timebase == 0 {
        return Err(PropertyError::Layout("zero keyframe timebase"));
    }
    let mut keys = Vec::with_capacity(count);
    for item in bytes[..required].chunks_exact(stride) {
        let time_units = i32::from_be_bytes(
            item[0..4]
                .try_into()
                .map_err(|_| PropertyError::Layout("key time"))?,
        );
        let time_secs = f64::from(time_units) / f64::from(timebase);
        let (values, in_speed, in_influence, out_speed, out_influence, spatial_in, spatial_out) =
            if parallel_stride.is_some() {
                let ease = doubles(&item[24..], 4)?;
                (
                    Vec::new(),
                    vec![ease[0]],
                    vec![ease[1]],
                    vec![ease[2]],
                    vec![ease[3]],
                    Vec::new(),
                    Vec::new(),
                )
            } else if spatial {
                // The 8-byte item header is followed by 8 bytes of spatial
                // flags/padding and one reserved double. Ease starts at 24,
                // values at 56 (pinned py-aep KfPosition / native Position).
                let scalars = doubles(&item[24..], 4 + 3 * dimensions)?;
                (
                    scalars[4..4 + dimensions].to_vec(),
                    vec![scalars[0]],
                    vec![scalars[1]],
                    vec![scalars[2]],
                    vec![scalars[3]],
                    scalars[4 + dimensions..4 + 2 * dimensions].to_vec(),
                    scalars[4 + 2 * dimensions..4 + 3 * dimensions].to_vec(),
                )
            } else if value_kind == NumericValueKind::Color {
                let ease = doubles(&item[24..], 4)?;
                let native = doubles(&item[56..], 4)?;
                // Like static cdat, native color keys store 0..255 ARGB, not RGBA.
                let values = [native[1], native[2], native[3], native[0]]
                    .map(|component| component / 255.0)
                    .to_vec();
                (
                    values,
                    vec![ease[0]],
                    vec![ease[1]],
                    vec![ease[2]],
                    vec![ease[3]],
                    Vec::new(),
                    Vec::new(),
                )
            } else {
                let scalars = doubles(&item[8..], 5 * dimensions)?;
                (
                    scalars[..dimensions].to_vec(),
                    scalars[dimensions..2 * dimensions].to_vec(),
                    scalars[2 * dimensions..3 * dimensions].to_vec(),
                    scalars[3 * dimensions..4 * dimensions].to_vec(),
                    scalars[4 * dimensions..].to_vec(),
                    Vec::new(),
                    Vec::new(),
                )
            };
        keys.push(NumericKeyframe {
            time_secs,
            values,
            in_interpolation: item[4],
            out_interpolation: item[5],
            in_speed,
            // Native ldat stores fractions; the shared animation helpers use
            // AE's UI percentages. This applies to numeric and parallel keys.
            in_influence: in_influence
                .into_iter()
                .map(|value| value * 100.0)
                .collect(),
            out_speed,
            out_influence: out_influence
                .into_iter()
                .map(|value| value * 100.0)
                .collect(),
            spatial_in,
            spatial_out,
        });
    }
    Ok(keys)
}

fn doubles(bytes: &[u8], count: usize) -> Result<Vec<f64>, PropertyError> {
    let bytes = bytes
        .get(..count * 8)
        .ok_or(PropertyError::Layout("short keyframe value"))?;
    let result: Vec<f64> = bytes
        .chunks_exact(8)
        .map(|b| f64::from_be_bytes(b.try_into().expect("eight-byte chunk")))
        .collect();
    if result.iter().any(|v| !v.is_finite()) {
        return Err(PropertyError::NonFinite);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_key_list_keeps_all_values_and_rejects_truncated_items() {
        const COUNT: u16 = 10_001;
        let mut meta = [0; 124];
        meta[12..16].copy_from_slice(&24_576_u32.to_be_bytes());
        let mut header = vec![0; 24];
        header[10..12].copy_from_slice(&COUNT.to_be_bytes());
        header[18..20].copy_from_slice(&48_u16.to_be_bytes());
        header[23] = 4;
        let mut bytes = vec![0; usize::from(COUNT) * 48];
        for (index, item) in bytes.chunks_exact_mut(48).enumerate() {
            item[..4].copy_from_slice(&(i32::try_from(index).unwrap() * 1024).to_be_bytes());
            item[8..16].copy_from_slice(&(index as f64).to_be_bytes());
        }
        let children = |payload| {
            vec![Chunk::list(
                *b"list",
                vec![
                    Chunk::data(*b"lhd3", header.clone()).unwrap(),
                    Chunk::data(*b"ldat", payload).unwrap(),
                ],
            )]
        };
        let keys = read_keyframes(
            &children(bytes.clone()),
            &meta,
            1,
            NumericValueKind::Continuous,
            None,
        )
        .unwrap();
        assert_eq!(keys.len(), usize::from(COUNT));
        assert_eq!(keys.last().unwrap().values, vec![10_000.0]);
        assert_eq!(keys.last().unwrap().time_secs, 10_000.0 / 24.0);
        bytes.pop();
        assert!(matches!(
            read_keyframes(
                &children(bytes),
                &meta,
                1,
                NumericValueKind::Continuous,
                None
            ),
            Err(PropertyError::Layout("keyframe item length"))
        ));
    }
}
