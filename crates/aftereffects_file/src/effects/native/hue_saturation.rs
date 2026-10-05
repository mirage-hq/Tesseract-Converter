//! Static Hue/Saturation render controls live in the arbitrary channel record.
//! Numeric UI declarations may cache a selected channel rather than Master.
use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError},
    rifx::Chunk,
};

pub(super) const CHANNEL_RANGE: &str = "ADBE HUE SATURATION-0003";
const RANGES: [[i32; 4]; 6] = [
    [315, 345, 15, 45],
    [15, 45, 75, 105],
    [75, 105, 135, 165],
    [135, 165, 195, 225],
    [195, 225, 255, 285],
    [255, 285, 315, 345],
];

#[derive(Debug)]
pub(super) struct Master([i32; 3]);

impl Master {
    pub(super) fn numeric(&self, name: &str) -> Option<NumericProperty> {
        let index = match name {
            "ADBE HUE SATURATION-0004" => 0,
            "ADBE HUE SATURATION-0005" => 1,
            "ADBE HUE SATURATION-0006" => 2,
            _ => return None,
        };
        Some(NumericProperty {
            values: vec![f64::from(self.0[index])],
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
    let mut matches = explicit.iter().filter(|(name, _)| *name == CHANNEL_RANGE);
    let Some((_, run)) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(PropertyError::Layout("duplicate channel-range controls"));
    }
    let leaf = properties::unique_list(run, *b"tdbs")?;
    let metadata = properties::read_path_metadata(leaf)?;
    if metadata.animated || metadata.expression_enabled || !metadata.keyframes.is_empty() {
        return Err(PropertyError::Layout(
            "animated/expression channel range unsupported",
        ));
    }
    let meta = properties::data(leaf, *b"tdb4")?;
    if meta[2..4] != [0, 1] || meta[57] != 1 || meta[59] != 8 {
        return Err(PropertyError::Layout("unknown channel-range property type"));
    }
    let arbitrary = properties::unique_list(run, *b"aRbs")?;
    if arbitrary.len() != 1 {
        return Err(PropertyError::Layout("ambiguous channel-range records"));
    }
    let bytes = properties::data(arbitrary, *b"aRbp")?;
    if bytes.len() != 180 {
        return Err(PropertyError::Layout("unknown channel-range record size"));
    }
    let values: Vec<_> = bytes
        .chunks_exact(4)
        .map(|bytes| i32::from_be_bytes(bytes.try_into().expect("four-byte integer chunk")))
        .collect();
    let master = [values[0], values[1], values[2]];
    // Master Hue is an angle, not a signed-180-degree slider. Bound its
    // integral packed state by the signed 16:16 UI contract used by the writer,
    // while retaining multi-turn angles rather than silently truncating them.
    if !(-32_768..=32_767).contains(&master[0]) {
        return Err(PropertyError::Layout(
            "Master hue exceeds native 16:16 range",
        ));
    }
    if master[1..]
        .iter()
        .any(|value| !(-100..=100).contains(value))
    {
        return Err(PropertyError::Layout("invalid Master saturation/lightness"));
    }
    for (index, channel) in values[3..].chunks_exact(7).enumerate() {
        if channel[..4].iter().any(|value| !(0..360).contains(value))
            || channel[5..]
                .iter()
                .any(|value| !(-100..=100).contains(value))
        {
            return Err(PropertyError::Layout("invalid channel-range controls"));
        }
        if channel[..4] != RANGES[index] || channel[4..] != [0, 0, 0] {
            warnings.push(format!(
                "ADBE HUE SATURATION: packed channel record {} {channel:?} omitted; only Master H/S/L retained",
                index + 1
            ));
        }
    }
    if metadata.expression_present {
        warnings.push(
            "ADBE HUE SATURATION: disabled expression omitted; stored static packed Master retained"
                .to_owned(),
        );
    }
    Ok(Some(Master(master)))
}
