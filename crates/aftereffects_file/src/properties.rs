//! Bounded decoding of the ordinary numeric leaves in an AE Transform group.
//!
//! Layout reference: MIT py-aep e12a451c35bacd3f34a080090265f9370e66162b,
//! binary/property_chunks.py and parsers/utils.py (Fortiche production).
//! Original bytes remain in `structure::Layer::content`; this is not a writer.

use crate::rifx::Chunk;

mod keyframes;
use keyframes::read_keyframes;

/// A property could not be interpreted safely within the supported numeric layout.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PropertyError {
    /// A required child is absent or duplicated, or has an unsupported layout.
    #[error("unsupported or malformed property: {0}")]
    Layout(&'static str),
    /// Non-finite values cannot be published in editable FX JSON.
    #[error("non-finite numeric property value")]
    NonFinite,
}

/// One AE key in layer-local seconds; temporal speeds are in source units per second.
#[derive(Clone, Debug, PartialEq)]
pub struct NumericKeyframe {
    pub time_secs: f64,
    pub values: Vec<f64>,
    pub in_interpolation: u8,
    pub out_interpolation: u8,
    pub in_speed: Vec<f64>,
    pub in_influence: Vec<f64>,
    pub out_speed: Vec<f64>,
    pub out_influence: Vec<f64>,
    pub spatial_in: Vec<f64>,
    pub spatial_out: Vec<f64>,
}

/// Binary value representation used by one numeric property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericValueKind {
    /// Scalar/vector doubles in source units.
    Continuous,
    /// A discrete numeric value stored as a double.
    Integer,
    /// A color exposed as normalized RGBA, regardless of native static ordering.
    Color,
}

/// Numeric storage and evaluation flags, not an evaluated animation.
#[derive(Clone, Debug, PartialEq)]
pub struct NumericProperty {
    /// First `dimensions` big-endian cdat doubles, in source storage units.
    /// Empty when an animated leaf has no static cdat record.
    pub values: Vec<f64>,
    /// Keyframes are present; `values` must not be mistaken for a static value.
    pub animated: bool,
    /// An enabled expression must be evaluated before claiming a static value.
    pub expression_enabled: bool,
    /// A disabled expression is retained only in the original source chunks.
    pub expression_present: bool,
    /// Position's value is supplied by its separation followers instead.
    pub dimensions_separated: bool,
    /// Native authored keys; empty if no supported keyframe payload is present.
    pub keyframes: Vec<NumericKeyframe>,
    /// Source value classification after bounded native decoding.
    pub(crate) value_kind: NumericValueKind,
}

/// A recognized Transform leaf, including an explicit decoding failure.
#[derive(Clone, Debug, PartialEq)]
pub struct TransformProperty {
    /// Stable Adobe match name; independent of translated display names.
    pub match_name: String,
    /// Decoded storage, or the reason this leaf cannot be used.
    pub numeric: Result<NumericProperty, PropertyError>,
}

/// Returns named property runs at the layer root, without discarding unknown groups.
/// The returned slices borrow the source chunks and retain native encounter order.
pub(crate) fn root_runs(content: &[Chunk]) -> Result<Vec<(&str, &[Chunk])>, PropertyError> {
    let roots: Vec<_> = content
        .iter()
        .filter(|c| c.list_kind() == Some(*b"tdgp"))
        .collect();
    match roots.as_slice() {
        [] => Ok(Vec::new()),
        [root] => runs(
            root.children()
                .ok_or(PropertyError::Layout("opaque property root"))?,
        ),
        _ => Err(PropertyError::Layout("duplicate layer property roots")),
    }
}

/// Reads only the layer's own Transform group, never effects or nested shapes.
/// Missing leaves remain absent: choosing source-dependent defaults belongs to
/// conversion, not the binary decoder. Duplicate group identities are rejected.
pub fn read_transform(content: &[Chunk]) -> Result<Vec<TransformProperty>, PropertyError> {
    let root_runs = root_runs(content)?;
    let mut groups = root_runs
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Transform Group");
    let Some((_, run)) = groups.next() else {
        return Ok(Vec::new());
    };
    if groups.next().is_some() {
        return Err(PropertyError::Layout("duplicate Transform groups"));
    }
    let group = unique_list(run, *b"tdgp")?;
    let leaves = runs(group)?;
    let mut result: Vec<TransformProperty> = Vec::new();
    for (name, run) in leaves {
        if !matches!(
            name,
            "ADBE Anchor Point"
                | "ADBE Position"
                | "ADBE Position_0"
                | "ADBE Position_1"
                | "ADBE Position_2"
                | "ADBE Orientation"
                | "ADBE Rotate X"
                | "ADBE Rotate Y"
                | "ADBE Scale"
                | "ADBE Rotate Z"
                | "ADBE Opacity"
        ) {
            continue;
        }
        if let Some(previous) = result.iter_mut().find(|p| p.match_name == name) {
            previous.numeric = Err(PropertyError::Layout("duplicate Transform leaf"));
            continue;
        }
        let numeric = if name == "ADBE Orientation" {
            read_orientation(run)
        } else {
            unique_list(run, *b"tdbs").and_then(read_numeric)
        };
        result.push(TransformProperty {
            match_name: name.to_owned(),
            numeric,
        });
    }
    Ok(result)
}

/// Reads AE's static source-relative Anchor representation without applying
/// source dimensions. Adobe-authored Solid source edits use descriptor marker
/// `2.0` with fractional `cdat` values; ordinary pixel Anchors use a different
/// representation. The caller must restrict scaling to a known-dimension source.
pub(crate) fn read_static_source_relative_anchor(
    content: &[Chunk],
) -> Result<Option<[f64; 2]>, PropertyError> {
    let root_runs = root_runs(content)?;
    let mut groups = root_runs
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Transform Group");
    let Some((_, run)) = groups.next() else {
        return Ok(None);
    };
    if groups.next().is_some() {
        return Err(PropertyError::Layout("duplicate Transform groups"));
    }
    let leaves = runs(unique_list(run, *b"tdgp")?)?;
    let mut anchors = leaves
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Anchor Point");
    let Some((_, run)) = anchors.next() else {
        return Ok(None);
    };
    if anchors.next().is_some() {
        return Err(PropertyError::Layout("duplicate Transform leaf"));
    }
    let property = unique_list(run, *b"tdbs")?;
    let meta = data(property, *b"tdb4")?;
    if meta.len() != 124 || meta[..2] != [0xdb, 0x99] {
        return Err(PropertyError::Layout("tdb4 layout"));
    }
    // Solid anchors are source-relative even when this descriptor's unit
    // field is 1.0 (ordinary square-pixel sources), not only 2.0.
    let numeric = read_numeric(property)?;
    if numeric.animated || numeric.expression_enabled {
        return Ok(None);
    }
    let [x, y, ..] = numeric.values.as_slice() else {
        return Err(PropertyError::Layout("source-relative Anchor dimensions"));
    };
    Ok(Some([*x, *y]))
}

pub(crate) fn runs(children: &[Chunk]) -> Result<Vec<(&str, &[Chunk])>, PropertyError> {
    let starts: Vec<_> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.id() == *b"tdmn")
        .map(|(i, _)| i)
        .collect();
    let mut result = Vec::with_capacity(starts.len());
    for (n, &i) in starts.iter().enumerate() {
        let bytes = children[i]
            .data_payload()
            .ok_or(PropertyError::Layout("match-name shape"))?;
        if bytes.len() != 40 {
            return Err(PropertyError::Layout("match-name length"));
        }
        let end = bytes.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
        let name = std::str::from_utf8(&bytes[..end])
            .map_err(|_| PropertyError::Layout("match-name encoding"))?;
        if name.contains('\0') {
            return Err(PropertyError::Layout("embedded match-name terminator"));
        }
        if name != "ADBE Group End" {
            result.push((
                name,
                &children[i + 1..starts.get(n + 1).copied().unwrap_or(children.len())],
            ));
        }
    }
    Ok(result)
}

pub(crate) fn unique_list(children: &[Chunk], kind: [u8; 4]) -> Result<&[Chunk], PropertyError> {
    let mut lists = children.iter().filter(|c| c.list_kind() == Some(kind));
    let first = lists
        .next()
        .and_then(Chunk::children)
        .ok_or(PropertyError::Layout("missing property LIST"))?;
    if lists.next().is_some() {
        return Err(PropertyError::Layout("duplicate property LIST"));
    }
    Ok(first)
}

pub(crate) fn data(children: &[Chunk], id: [u8; 4]) -> Result<&[u8], PropertyError> {
    let mut chunks = children.iter().filter(|c| c.id() == id);
    let first = chunks
        .next()
        .and_then(Chunk::data_payload)
        .ok_or(PropertyError::Layout("missing numeric record"))?;
    if chunks.next().is_some() {
        return Err(PropertyError::Layout("duplicate numeric record"));
    }
    Ok(first)
}

/// Group eyeball state lives directly in tdgp/tdsb, unlike leaf metadata in tdbs.
/// Missing flags mean enabled; never borrow a descendant's flags.
pub(crate) fn group_enabled(run: &[Chunk]) -> Result<bool, PropertyError> {
    let group = unique_list(run, *b"tdgp")?;
    if !group.iter().any(|chunk| chunk.id() == *b"tdsb") {
        return Ok(true);
    }
    let flags = data(group, *b"tdsb")?;
    if flags.len() != 4 {
        return Err(PropertyError::Layout("group enable flags length"));
    }
    Ok(flags[3] & 1 != 0)
}

/// Best-effort group toggle decoding with an explicit malformed-record diagnostic.
pub(crate) fn group_enabled_or_warn(run: &[Chunk], name: &str, warnings: &mut Vec<String>) -> bool {
    match group_enabled(run) {
        Ok(enabled) => enabled,
        Err(error) => {
            warnings.push(format!(
                "{name} enable flag malformed: {error}; enabled retained"
            ));
            true
        }
    }
}

/// Decode a numeric property's `tdbs` children without changing native values.
/// This also supports external structural inspectors of freshly exported files.
pub fn read_numeric(children: &[Chunk]) -> Result<NumericProperty, PropertyError> {
    read_numeric_with_layout(children, false, false, false)
}

/// AE plugin Point controls use the numeric-vector key layout but mark their
/// two components with the integer flag. The values themselves are doubles,
/// including fractional coordinates, so they remain continuous FX values.
pub(crate) fn read_effect_point(children: &[Chunk]) -> Result<NumericProperty, PropertyError> {
    read_numeric_with_layout(children, false, false, true)
}

fn read_orientation(run: &[Chunk]) -> Result<NumericProperty, PropertyError> {
    let wrapper = unique_list(run, *b"otst")?;
    let inner = unique_list(wrapper, *b"tdbs")?;
    let mut property = read_numeric_with_layout(inner, true, true, false)?;
    if let Some(bytes) = inner
        .iter()
        .find(|chunk| chunk.id() == *b"cdat")
        .and_then(Chunk::data_payload)
    {
        if bytes.len() % 8 != 0 {
            return Err(PropertyError::Layout("truncated orientation value"));
        }
        property.values = decode_doubles(bytes, (bytes.len() / 8).min(3), true)?;
        property.values.resize(3, 0.0);
    }
    if property.keyframes.is_empty() {
        return Ok(property);
    }
    let values = unique_list(wrapper, *b"otky")?;
    let records: Vec<_> = values
        .iter()
        .filter(|chunk| chunk.id() == *b"otda")
        .map(|chunk| {
            let bytes = chunk
                .data_payload()
                .ok_or(PropertyError::Layout("opaque orientation key value"))?;
            decode_doubles(bytes, 3, false)
        })
        .collect::<Result<_, _>>()?;
    if records.len() != property.keyframes.len() {
        return Err(PropertyError::Layout(
            "orientation key/value count mismatch",
        ));
    }
    for (key, values) in property.keyframes.iter_mut().zip(records) {
        key.values = values;
    }
    Ok(property)
}

fn read_numeric_with_layout(
    children: &[Chunk],
    little_endian_static: bool,
    orientation: bool,
    effect_point: bool,
) -> Result<NumericProperty, PropertyError> {
    let meta = data(children, *b"tdb4")?;
    let flags = data(children, *b"tdsb")?;
    if meta.len() != 124 || meta[..2] != [0xdb, 0x99] || flags.len() != 4 {
        return Err(PropertyError::Layout("tdb4/tdsb layout"));
    }
    let dimensions = usize::from(u16::from_be_bytes([meta[2], meta[3]]));
    if !(1..=4).contains(&dimensions) || (!orientation && meta[57] & 1 != 0) {
        return Err(PropertyError::Layout("not a supported numeric vector"));
    }
    let color = meta[59] & 1 != 0;
    let integer = meta[59] & 4 != 0;
    if (color && (integer || dimensions != 4))
        || (integer && dimensions != 1 && !(effect_point && dimensions == 2 && meta[59] == 4))
        || (effect_point && (dimensions != 2 || meta[59] != 4))
    {
        return Err(PropertyError::Layout("invalid numeric type/dimensions"));
    }
    let value_kind = if color {
        NumericValueKind::Color
    } else if integer && !effect_point {
        NumericValueKind::Integer
    } else {
        NumericValueKind::Continuous
    };
    let animated = meta[68] != 0 || children.iter().any(|c| c.list_kind() == Some(*b"list"));
    let bytes = if animated && !children.iter().any(|c| c.id() == *b"cdat") {
        &[][..]
    } else {
        data(children, *b"cdat")?
    };
    if (!bytes.is_empty() || !animated) && (bytes.len() < dimensions * 8 || bytes.len() % 8 != 0) {
        return Err(PropertyError::Layout("truncated numeric value"));
    }
    let mut values = if bytes.is_empty() {
        Vec::new()
    } else {
        decode_doubles(bytes, dimensions, little_endian_static)?
    };
    if color && !values.is_empty() {
        values = [values[1], values[2], values[3], values[0]]
            .map(|component| component / 255.0)
            .to_vec();
    }
    let expression_present = meta[120] & 1 != 0
        || children
            .iter()
            .any(|chunk| matches!(&chunk.id(), b"Utf8" | b"expr"));
    let keyframes = read_keyframes(
        children,
        meta,
        dimensions,
        value_kind,
        orientation.then_some(80),
    )?;
    Ok(NumericProperty {
        values,
        animated,
        expression_enabled: expression_present && meta[119] & 1 == 0,
        expression_present,
        dimensions_separated: flags[2] & 8 != 0,
        keyframes,
        value_kind,
    })
}

/// Path values live in sibling `omks` records; their 64-byte key headers carry
/// timing and dimensionless ease only, unlike ordinary numeric properties.
pub(crate) fn read_path_metadata(children: &[Chunk]) -> Result<NumericProperty, PropertyError> {
    let meta = data(children, *b"tdb4")?;
    let flags = data(children, *b"tdsb")?;
    if meta.len() != 124 || meta[..2] != [0xdb, 0x99] || flags.len() != 4 {
        return Err(PropertyError::Layout("path tdb4/tdsb layout"));
    }
    let expression_present = meta[120] & 1 != 0
        || children
            .iter()
            .any(|chunk| matches!(&chunk.id(), b"Utf8" | b"expr"));
    Ok(NumericProperty {
        values: Vec::new(),
        animated: meta[68] != 0 || children.iter().any(|c| c.list_kind() == Some(*b"list")),
        expression_enabled: expression_present && meta[119] & 1 == 0,
        expression_present,
        dimensions_separated: false,
        keyframes: read_keyframes(children, meta, 0, NumericValueKind::Continuous, Some(64))?,
        value_kind: NumericValueKind::Continuous,
    })
}

fn decode_doubles(
    bytes: &[u8],
    dimensions: usize,
    little_endian: bool,
) -> Result<Vec<f64>, PropertyError> {
    let bytes = bytes
        .get(..dimensions * 8)
        .ok_or(PropertyError::Layout("truncated numeric value"))?;
    let values: Vec<_> = bytes
        .chunks_exact(8)
        .map(|bytes| {
            let bytes: [u8; 8] = bytes.try_into().expect("eight-byte chunk");
            if little_endian {
                f64::from_le_bytes(bytes)
            } else {
                f64::from_be_bytes(bytes)
            }
        })
        .collect();
    if values.iter().any(|value| !value.is_finite()) {
        return Err(PropertyError::NonFinite);
    }
    Ok(values)
}

#[cfg(test)]
mod tests;
