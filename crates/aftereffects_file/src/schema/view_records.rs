//! AE 2026 pseudo-layer and static camera-property records.
//!
//! The reserved bytes are preserved on decode; constructors set observed AE26
//! defaults individually. This is deliberately not a schema for arbitrary AE
//! versions or animated/keyframed properties.

use crate::{
    rifx::RifxError,
    schema::{
        RecordError,
        layout::{I32Field, RecordImage, U32Field},
    },
    timing::Duration24,
};

fn fixed<const N: usize>(bytes: &[u8], error: &'static str) -> Result<[u8; N], RifxError> {
    bytes.try_into().map_err(|_| RifxError::Invalid(error))
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

const VIEW_ID: U32Field<164> = U32Field::new(0);
const VIEW_OUT_POINT: I32Field<164> = I32Field::new(28);

/// AE26's 164-byte pseudo-layer metadata (`ldta`). Uninterpreted fields remain
/// intact when decoded; the constructor selects explicit observed defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewLayerRecord(RecordImage<164>);

impl ViewLayerRecord {
    /// Reads an exact AE26 pseudo-layer record, preserving reserved bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        Ok(Self(RecordImage::decode(bytes, "view ldta")?))
    }

    /// Constructs one canonical working-view or marker pseudo-layer.
    /// IDs must be allocated consistently with the enclosing project.
    pub(crate) fn new(
        id: u32,
        name: &str,
        duration: Duration24,
        marker: bool,
        side: bool,
    ) -> Result<Self, RecordError> {
        if name.len() > 32 || name.contains('\0') {
            return Err(RecordError::Invalid("invalid view layer name"));
        }
        let mut image = RecordImage::zeroed();
        VIEW_ID.set(&mut image, id);
        VIEW_OUT_POINT.set(&mut image, duration.signed_ticks());
        let bytes = image.bytes_mut();
        put_u32(bytes, 8, 1); // Time-stretch numerator.
        put_u32(bytes, 16, 600); // Zero start-time denominator.
        for offset in [24, 32] {
            put_u32(bytes, offset, 24_576);
        }
        // Offset 28 is the out-point numerator. In-point (offset 20) is zero;
        // offsets 24 and 32 are the respective in/out-point denominators.
        if marker {
            bytes[5] = 2; // Marker pseudo-layer quality, distinct from layer kind.
            bytes[39] = 0x87;
            bytes[59] = 1;
            bytes[61] = 8;
            bytes[99] = 2;
            bytes[131] = 4;
        } else {
            bytes[38] = 0x44;
            bytes[39] = 1;
            bytes[61] = 4;
            bytes[131] = 2;
            bytes[151] = 1;
            // AE26 default viewer optical parameter, not a canvas dimension.
            bytes[152..160].copy_from_slice(&102.0472440944882_f64.to_be_bytes());
        }
        bytes[64..64 + name.len()].copy_from_slice(name.as_bytes());
        put_u32(bytes, 108, 1); // Time-stretch denominator.
        bytes[144] = 1;
        if side {
            bytes[143] = 1;
        }
        Ok(Self(image))
    }

    /// Project-wide pseudo-layer identifier.
    pub fn id(&self) -> u32 {
        VIEW_ID.get(&self.0)
    }

    /// Replaces the project-wide identifier without modifying other fields.
    pub fn set_id(&mut self, id: u32) {
        VIEW_ID.set(&mut self.0, id);
    }

    /// Signed out-point numerator; equal to duration ticks for canonical zero in-point.
    pub fn out_point_ticks(&self) -> i32 {
        VIEW_OUT_POINT.get(&self.0)
    }

    /// Writes the complete record, including reserved fields.
    pub fn encode(&self) -> [u8; 164] {
        self.0.encode()
    }
}

/// AE26 static-property descriptor (`tdb4`). Numeric defaults are typed;
/// unknown flags are confined to constructor defaults or preserved on decode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticPropertyRecord([u8; 124]);

impl StaticPropertyRecord {
    /// Reads an exact property descriptor without normalizing unknown bits.
    pub fn decode(bytes: &[u8]) -> Result<Self, RifxError> {
        Ok(Self(fixed(bytes, "invalid tdb4 length")?))
    }

    /// Descriptor fields at offsets 2, 4, 6 and 8 are AE26 version-specific;
    /// offsets 56 and 60 hold an observed subtype and secondary flag.
    pub fn new(
        components: u16,
        selection: u16,
        variant: u16,
        flags: u32,
        mode: u32,
        subtype: u8,
        special: bool,
    ) -> Self {
        let mut data = [0; 124];
        data[0..2].copy_from_slice(&0xdb99_u16.to_be_bytes());
        data[2..4].copy_from_slice(&components.to_be_bytes());
        data[4..6].copy_from_slice(&selection.to_be_bytes());
        data[6..8].copy_from_slice(&variant.to_be_bytes());
        put_u32(&mut data, 8, flags);
        put_u32(&mut data, 12, 24_576);
        // Shared AE26 descriptor defaults, observed in 127 static properties.
        // These slots are not yet interpreted as a general property schema.
        data[16..24].copy_from_slice(&0.0001_f64.to_be_bytes());
        for offset in [24, 32, 40, 48] {
            data[offset..offset + 8].copy_from_slice(&1.0_f64.to_be_bytes());
        }
        put_u32(&mut data, 56, mode);
        data[60] = subtype;
        if special {
            data[76] = 3;
            data[77] = 3;
            data[79] = 1;
        }
        Self(data)
    }

    /// AE26 color/orientation descriptors carry this initialized-value flag
    /// independently of the spatial tangent-mode bytes at offsets 76 and 77.
    pub(crate) fn set_initialized(&mut self) {
        self.0[79] = 1;
    }

    /// Writes the complete property descriptor.
    pub fn encode(&self) -> [u8; 124] {
        self.0
    }
}

/// Static property payload (`cdat`): finite big-endian f64 slots. The property
/// recipe determines which slots hold values versus tangent/default metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct StaticPropertyValues(Vec<f64>);

impl StaticPropertyValues {
    /// Reads a bounded sequence of finite double-precision values.
    pub fn decode(bytes: &[u8]) -> Result<Self, RifxError> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(8) || bytes.len() > 256 {
            return Err(RifxError::Invalid("invalid static cdat length"));
        }
        let values = bytes
            .chunks_exact(8)
            .map(|part| {
                f64::from_be_bytes(part.try_into().expect("chunks_exact yields eight bytes"))
            })
            .collect::<Vec<_>>();
        Self::new(values)
    }

    /// Validates a finite payload of 1..=32 numeric slots.
    pub fn new(values: Vec<f64>) -> Result<Self, RifxError> {
        if values.is_empty() || values.len() > 32 || values.iter().any(|value| !value.is_finite()) {
            return Err(RifxError::Invalid("invalid static cdat values"));
        }
        Ok(Self(values))
    }

    /// All slots, including static tangent/default values after the value.
    pub fn values(&self) -> &[f64] {
        &self.0
    }

    /// Writes each slot as a big-endian f64.
    pub fn encode(&self) -> Vec<u8> {
        self.0
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{StaticPropertyRecord, StaticPropertyValues, ViewLayerRecord};
    use crate::timing::Duration24;

    #[test]
    fn view_timing_and_static_property_roundtrip() {
        let layer =
            ViewLayerRecord::new(5, "Top", Duration24::from_frames(48).unwrap(), false, true)
                .unwrap();
        assert_eq!(layer.id(), 5);
        assert_eq!(layer.out_point_ticks(), 49_152);
        assert_eq!(ViewLayerRecord::decode(&layer.encode()).unwrap(), layer);
        let prop = StaticPropertyRecord::new(3, 15, 3, u32::MAX, 8, 9, true);
        assert_eq!(StaticPropertyRecord::decode(&prop.encode()).unwrap(), prop);
        let values = StaticPropertyValues::new(vec![960.0, 540.0, -5000.0, 0.0, 0.0]).unwrap();
        assert_eq!(
            StaticPropertyValues::decode(&values.encode()).unwrap(),
            values
        );
        assert!(StaticPropertyValues::decode(&[0; 7]).is_err());
        assert!(ViewLayerRecord::decode(&[0; 163]).is_err());
        let mut encoded = [0xa5; 164];
        encoded[0..4].copy_from_slice(&5_u32.to_be_bytes());
        let mut decoded = ViewLayerRecord::decode(&encoded).unwrap();
        decoded.set_id(9);
        let edited = decoded.encode();
        assert_eq!(&edited[0..4], &9_u32.to_be_bytes());
        assert_eq!(&edited[4..], &encoded[4..]);
        assert!(
            ViewLayerRecord::new(5, "Top", Duration24::from_frames(1).unwrap(), false, true,)
                .is_ok()
        );
    }
}
