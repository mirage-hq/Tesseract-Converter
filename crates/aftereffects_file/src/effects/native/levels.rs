//! Easy Levels stores render controls in its arbitrary Histogram, not UI caches.
//! Only the observed static version-1 layout is decoded; no custom keys are guessed.
use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError},
    rifx::Chunk,
};

pub(super) const HISTOGRAM: &str = "ADBE Easy Levels2-0002";
const HEADER: [u8; 8] = [0, 15, 16, 167, 0, 0, 0, 1];
const IDENTITY: [f64; 5] = [0., 1., 1., 0., 1.];

#[derive(Debug)]
pub(super) struct Master([f64; 5]);

impl Master {
    pub(super) fn numeric(&self, name: &str) -> Option<NumericProperty> {
        let index = match name {
            "ADBE Easy Levels2-0003" => 0,
            "ADBE Easy Levels2-0004" => 1,
            "ADBE Easy Levels2-0005" => 2,
            "ADBE Easy Levels2-0006" => 3,
            "ADBE Easy Levels2-0007" => 4,
            _ => return None,
        };
        Some(NumericProperty {
            values: vec![self.0[index]],
            animated: false,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: Vec::new(),
            value_kind: NumericValueKind::Continuous,
        })
    }
}

pub(super) fn read(
    explicit: &[(&str, &[Chunk])],
    warnings: &mut Vec<String>,
) -> Result<Option<Master>, PropertyError> {
    let mut matches = explicit.iter().filter(|(name, _)| *name == HISTOGRAM);
    let Some((_, run)) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(PropertyError::Layout("duplicate Histogram controls"));
    }
    let leaf = properties::unique_list(run, *b"tdbs")?;
    // Arbitrary properties share the path property's metadata envelope, but not
    // its value/key layout. Only the static envelope is usable here.
    let metadata = properties::read_path_metadata(leaf)?;
    if metadata.animated || metadata.expression_present || !metadata.keyframes.is_empty() {
        return Err(PropertyError::Layout(
            "animated/expression Histogram unsupported",
        ));
    }
    let meta = properties::data(leaf, *b"tdb4")?;
    if meta.len() < 60 || meta[2..4] != [0, 1] || meta[57] != 1 || meta[59] != 8 {
        return Err(PropertyError::Layout("unknown Histogram property type"));
    }
    let arbitrary = properties::unique_list(run, *b"aRbs")?;
    if arbitrary.len() != 1 {
        return Err(PropertyError::Layout(
            "ambiguous Histogram arbitrary records",
        ));
    }
    let bytes = properties::data(arbitrary, *b"aRbp")?;
    if bytes.len() != 108 || bytes[..8] != HEADER {
        return Err(PropertyError::Layout(
            "unknown Histogram size/magic/version",
        ));
    }
    let mut channels = [[0.; 5]; 5];
    for (channel, bytes) in channels.iter_mut().zip(bytes[8..].chunks_exact(20)) {
        for (value, bytes) in channel.iter_mut().zip(bytes.chunks_exact(4)) {
            *value = f64::from(f32::from_be_bytes(
                bytes.try_into().expect("four-byte float chunk"),
            ));
        }
        if channel.iter().any(|value| !value.is_finite())
            || [0, 1, 3, 4]
                .into_iter()
                .any(|i| !(0.0..=1.0).contains(&channel[i]))
            || channel[0] >= channel[1]
            || channel[2] <= 0.0
        {
            return Err(PropertyError::Layout("unsupported Histogram channel range"));
        }
    }
    for (index, channel) in channels.iter().enumerate().skip(1) {
        if *channel != IDENTITY {
            warnings.push(format!(
                "ADBE Easy Levels2: packed channel record {} {channel:?} omitted; only master RGB Levels retained",
                index + 1
            ));
        }
    }
    Ok(Some(Master(channels[0])))
}
