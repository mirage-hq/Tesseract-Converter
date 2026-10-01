//! Versioned AE26 panel records needed even for an empty project.
//!
//! AFsi/ARsi use 21 fixed-stride slots after a 24-byte header. The identifiers,
//! widths and flag words below are observed defaults, not arbitrary project
//! bytes. Unassigned fields remain reserved; decoded records preserve them.

use crate::rifx::RifxError;

/// Preserving codec for AE26's fixed-size project/render-panel settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelSlots([u8; 1872]);

impl PanelSlots {
    fn from_slots(sort_key: u16, slots: &[(u16, u16, u32)]) -> Self {
        let mut data = [0; 1872];
        data[..2].copy_from_slice(&sort_key.to_be_bytes());
        for (slot, (id, width, flags)) in data[24..].chunks_exact_mut(88).zip(slots) {
            slot[..2].copy_from_slice(&id.to_be_bytes());
            slot[2..4].copy_from_slice(&width.to_be_bytes());
            slot[68..72].copy_from_slice(&flags.to_be_bytes());
        }
        Self(data)
    }

    pub(crate) fn project_default() -> Self {
        Self::from_slots(
            4,
            &[
                (5, 16, 1),
                (4, 102, 1),
                (10, 21, 1),
                (6, 77, 1),
                (7, 48, 1),
                (14, 50, 1),
                (15, 83, 0),
                (16, 83, 0),
                (8, 83, 0),
                (17, 83, 1),
                (18, 83, 1),
                (19, 83, 0),
                (13, 77, 1),
                (12, 64, 1),
                (9, 250, 1),
                (11, 50, 0),
                (20, 64, 1),
            ],
        )
    }

    pub(crate) fn render_default() -> Self {
        Self::from_slots(
            0,
            &[
                (5, 16, 1),
                (6, 68, 1),
                (7, 21, 1),
                (8, 25, 1),
                (4, 98, 1),
                (9, 100, 1),
                (10, 130, 1),
                (11, 130, 1),
                (13, 90, 1),
                (12, 78, 1),
            ],
        )
    }

    /// Decodes exactly one panel record without interpreting reserved slots.
    pub fn decode(bytes: &[u8]) -> Result<Self, RifxError> {
        Ok(Self(bytes.try_into().map_err(|_| {
            RifxError::Invalid("invalid panel slots length")
        })?))
    }

    /// Writes all fields, including preserved reserved bytes.
    pub fn encode(&self) -> [u8; 1872] {
        self.0
    }
}

/// Header for an empty AE list. The element stride and layout words are
/// specific to the list kind; they must not be inferred from its empty count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyListHeader([u8; 52]);

impl EmptyListHeader {
    fn new(stride: u32, layout: [u32; 3]) -> Self {
        let mut data = [0; 52];
        data[..4].copy_from_slice(&0x00d00bee_u32.to_be_bytes());
        data[12..16].copy_from_slice(&1_u32.to_be_bytes());
        data[16..20].copy_from_slice(&stride.to_be_bytes());
        for (slot, value) in data[20..32].chunks_exact_mut(4).zip(layout) {
            slot.copy_from_slice(&value.to_be_bytes());
        }
        Self(data)
    }

    pub(crate) fn guides() -> Self {
        Self::new(16, [2, 1, 2])
    }
    pub(crate) fn render_queue() -> Self {
        Self::new(2246, [1, 1, 1])
    }

    /// Decodes the complete header, retaining any unknown fields.
    pub fn decode(bytes: &[u8]) -> Result<Self, RifxError> {
        Ok(Self(
            bytes
                .try_into()
                .map_err(|_| RifxError::Invalid("invalid lhd3 length"))?,
        ))
    }

    /// Encodes the header without dropping fields.
    pub fn encode(&self) -> [u8; 52] {
        self.0
    }
}

/// AE26 render-queue header; no queue items or render jobs are generated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderQueueHeader([u8; 20]);

impl RenderQueueHeader {
    pub(crate) fn empty_ae26() -> Self {
        let mut data = [0; 20];
        data[..2].copy_from_slice(&1_u16.to_be_bytes());
        data[2..4].copy_from_slice(&2_u16.to_be_bytes());
        // Stable versioned display defaults; their exact UI meaning is not
        // established. They are not project IDs or composition dimensions.
        data[12..14].copy_from_slice(&212_u16.to_be_bytes());
        data[14..16].copy_from_slice(&315_u16.to_be_bytes());
        data[16..20].copy_from_slice(&1_u32.to_be_bytes());
        Self(data)
    }

    /// Decodes a complete render-queue header, preserving reserved fields.
    pub fn decode(bytes: &[u8]) -> Result<Self, RifxError> {
        Ok(Self(
            bytes
                .try_into()
                .map_err(|_| RifxError::Invalid("invalid Rhed length"))?,
        ))
    }

    /// Writes all fields without normalizing decoded metadata.
    pub fn encode(&self) -> [u8; 20] {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{EmptyListHeader, PanelSlots, RenderQueueHeader};

    #[test]
    fn panel_slots_have_fixed_stride_and_zero_unused_rows() {
        let panel = PanelSlots::project_default();
        let bytes = panel.encode();
        assert_eq!(PanelSlots::decode(&bytes).unwrap(), panel);
        assert_eq!(&bytes[24..28], &[0, 5, 0, 16]);
        assert_eq!(&bytes[92..96], &1_u32.to_be_bytes());
        assert!(bytes[24 + 17 * 88..].iter().all(|b| *b == 0));
        assert!(PanelSlots::decode(&bytes[..1871]).is_err());
        let render = PanelSlots::render_default();
        assert_eq!(PanelSlots::decode(&render.encode()).unwrap(), render);
    }

    #[test]
    fn empty_list_and_queue_headers_roundtrip() {
        for header in [EmptyListHeader::guides(), EmptyListHeader::render_queue()] {
            assert_eq!(EmptyListHeader::decode(&header.encode()).unwrap(), header);
        }
        let queue = RenderQueueHeader::empty_ae26();
        assert_eq!(RenderQueueHeader::decode(&queue.encode()).unwrap(), queue);
        assert!(RenderQueueHeader::decode(&[0; 19]).is_err());
        assert!(EmptyListHeader::decode(&[0; 51]).is_err());
    }
}
