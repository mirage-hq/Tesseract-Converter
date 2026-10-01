//! Fixed-size binary records shared by the AEP reader and writer.
//!
//! Unknown fields remain byte-for-byte intact when an existing record is
//! modified. Field offsets are explicit; version-specific records require a
//! separate schema rather than permissive truncation.

use thiserror::Error;

use crate::{rifx::RifxError, timing::Duration24};

pub mod layer_records;
mod layout;
pub mod panel_records;
pub mod project_settings;
pub mod view_records;

/// A fixed-layout record or a value used to construct one is invalid.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RecordError {
    /// The payload does not match the versioned record layout exactly.
    #[error("invalid {record} length: expected {expected}, got {actual}")]
    Length {
        /// Wire record name.
        record: &'static str,
        /// Required size in bytes.
        expected: usize,
        /// Observed size in bytes.
        actual: usize,
    },
    /// A field cannot represent the requested value.
    #[error("invalid record field: {0}")]
    Invalid(&'static str),
}

fn fixed<const N: usize>(bytes: &[u8], label: &'static str) -> Result<[u8; N], RifxError> {
    bytes.try_into().map_err(|_| RifxError::Invalid(label))
}

fn u16_at<const N: usize>(data: &[u8; N], at: usize) -> u16 {
    u16::from_be_bytes([data[at], data[at + 1]])
}

fn u32_at<const N: usize>(data: &[u8; N], at: usize) -> u32 {
    u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

fn put_u16<const N: usize>(data: &mut [u8; N], at: usize, value: u16) {
    data[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn put_u32<const N: usize>(data: &mut [u8; N], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// A 20-byte project header (`head`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadRecord([u8; 20]);

impl HeadRecord {
    /// Constructs an AE26 project header with a fresh save revision.
    /// The producer field is a version bitfield, not a project identity.
    pub(crate) fn empty_ae26(next_item_id: u32) -> Self {
        let mut data = [0; 20];
        data[1] = 97;
        put_u16(&mut data, 2, 10); // Observed AE26 header subtype.
        // AE26.5 build89 on macOS: split major version, platform, minor,
        // reserved producer bit, release bit, and build number respectively.
        let producer = (3 << 26) | (14 << 22) | (2 << 19) | (5 << 15) | (1 << 10) | (1 << 9) | 89;
        put_u32(&mut data, 4, producer);
        data[8] = 0x80; // AE26 default header flag.
        put_u32(&mut data, 12, next_item_id);
        put_u16(&mut data, 18, 1); // Fresh save revision.
        Self(data)
    }

    /// Decodes exactly one project header.
    pub fn decode(bytes: &[u8]) -> Result<Self, RifxError> {
        Ok(Self(fixed(bytes, "invalid head length")?))
    }

    /// The binary format revision (97 for AE 2026).
    pub fn format_version(&self) -> u8 {
        self.0[1]
    }

    /// The packed producer-version word, also stored in the root `svap` record.
    pub fn producer_version_word(&self) -> u32 {
        u32_at(&self.0, 4)
    }

    /// The next project-wide item identifier.
    pub fn next_item_id(&self) -> u32 {
        u32_at(&self.0, 12)
    }

    /// Encodes the header, retaining reserved fields.
    pub fn encode(&self) -> [u8; 20] {
        self.0
    }
}

/// An 84-byte AE project-item record (`idta`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemRecord([u8; 84]);

impl ItemRecord {
    /// Fresh synthetic footage item; source-specific data lives in its `Pin `.
    /// The canonical flags match AE26 native solid items without source stamps.
    pub(crate) fn solid_ae26(id: u32) -> Result<Self, RifxError> {
        if id == 0 {
            return Err(RifxError::Invalid("zero solid item ID"));
        }
        let mut data = [0; 84];
        put_u16(&mut data, 0, 7);
        put_u32(&mut data, 16, id);
        put_u32(&mut data, 20, 0x30);
        data[58] = 1;
        Ok(Self(data))
    }

    /// Constructs an unparented composition item for the AE26 empty-project
    /// subset. The unexplained stable versioned defaults are not IDs.
    pub(crate) fn empty_ae26_composition(id: u32) -> Result<Self, RifxError> {
        if id == 0 {
            return Err(RifxError::Invalid("zero composition item ID"));
        }
        let mut data = [0; 84];
        put_u16(&mut data, 0, 4);
        put_u32(&mut data, 16, id);
        put_u32(&mut data, 20, 32); // AE26 composition item default.
        put_u16(&mut data, 58, 0x0f00); // AE26 item flags, observed across empty cases.
        // Do not reuse the oracle's unexplained trailing 4-byte stamp (offset 80).
        Ok(Self(data))
    }

    /// Decodes exactly one item record.
    pub fn decode(bytes: &[u8]) -> Result<Self, RifxError> {
        Ok(Self(fixed(bytes, "invalid idta length")?))
    }

    /// The item type; 1 is a folder, 4 is a composition, and 7 is footage.
    pub fn item_type(&self) -> u16 {
        u16_at(&self.0, 0)
    }

    /// The project-local identifier used by layers and parent references.
    pub fn id(&self) -> u32 {
        u32_at(&self.0, 16)
    }

    /// Sets the project-local identifier without modifying unknown fields.
    pub fn set_id(&mut self, id: u32) {
        put_u32(&mut self.0, 16, id);
    }

    /// Encodes this item record, retaining unknown fields.
    pub fn encode(&self) -> [u8; 84] {
        self.0
    }
}

/// The observed 204-byte composition record (`cdta`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionRecord(layout::RecordImage<204>);

const COMP_RESOLUTION_X: layout::U16Field<204> = layout::U16Field::new(0);
const COMP_RESOLUTION_Y: layout::U16Field<204> = layout::U16Field::new(2);
const COMP_WORK_START: layout::U32Field<204> = layout::U32Field::new(28);
const COMP_WORK_START_DENOMINATOR: layout::U32Field<204> = layout::U32Field::new(32);
const COMP_WORK_END: layout::U32Field<204> = layout::U32Field::new(36);
const COMP_WORK_END_DENOMINATOR: layout::U32Field<204> = layout::U32Field::new(40);
const COMP_DURATION: layout::U32Field<204> = layout::U32Field::new(44);
const COMP_DURATION_DENOMINATOR: layout::U32Field<204> = layout::U32Field::new(48);
const COMP_BACKGROUND_RED: usize = 52;
const COMP_BACKGROUND_GREEN: usize = 53;
const COMP_BACKGROUND_BLUE: usize = 54;
const COMP_WIDTH: layout::U16Field<204> = layout::U16Field::new(140);
const COMP_HEIGHT: layout::U16Field<204> = layout::U16Field::new(142);
const COMP_PIXEL_ASPECT: layout::U32Field<204> = layout::U32Field::new(144);
const COMP_PIXEL_ASPECT_DENOMINATOR: layout::U32Field<204> = layout::U32Field::new(148);
const COMP_FRAME_RATE_WHOLE: layout::U16Field<204> = layout::U16Field::new(156);
const COMP_FRAME_RATE_FRACTION: layout::U16Field<204> = layout::U16Field::new(158);
const COMP_DISPLAY_START: layout::I32Field<204> = layout::I32Field::new(164);
const COMP_DISPLAY_START_DENOMINATOR: layout::U32Field<204> = layout::U32Field::new(168);
const COMP_SHUTTER_ANGLE: layout::U16Field<204> = layout::U16Field::new(174);
const COMP_SHUTTER_PHASE: layout::I32Field<204> = layout::I32Field::new(180);
const COMP_BLUR_ADAPTIVE_LIMIT: layout::I32Field<204> = layout::I32Field::new(196);
const COMP_BLUR_SAMPLES: layout::I32Field<204> = layout::I32Field::new(200);

impl CompositionRecord {
    /// Constructs an AE26 24-fps square-pixel composition with the default
    /// work area and display start. Other nonzero slots are versioned defaults
    /// observed in independently authored empty compositions; their semantics
    /// are not yet established and opening in AE is not proven by this codec.
    pub(crate) fn empty_ae26(
        width: u16,
        height: u16,
        duration: Duration24,
    ) -> Result<Self, RecordError> {
        let mut image = layout::RecordImage::zeroed();
        COMP_RESOLUTION_X.set(&mut image, 1);
        COMP_RESOLUTION_Y.set(&mut image, 1);
        // These versioned defaults have no public field interpretation yet.
        let data = image.bytes_mut();
        put_u32(data, 4, 1024);
        put_u32(data, 8, 24_576);
        for offset in [16, 24] {
            put_u32(data, offset, 600);
        }
        // AE26 UI/render defaults, not specimen identity or save-time metadata.
        put_u32(data, 172, 180);
        put_u32(data, 176, 360);
        put_u32(data, 184, 360);
        put_u32(data, 196, 128);
        put_u32(data, 200, 16);
        COMP_WORK_START_DENOMINATOR.set(&mut image, 600);
        COMP_WORK_END.set(&mut image, u32::MAX);
        COMP_WORK_END_DENOMINATOR.set(&mut image, 600);
        COMP_DURATION.set(&mut image, duration.ticks());
        COMP_DURATION_DENOMINATOR.set(&mut image, 24_576);
        COMP_PIXEL_ASPECT.set(&mut image, 1);
        COMP_PIXEL_ASPECT_DENOMINATOR.set(&mut image, 1);
        COMP_FRAME_RATE_WHOLE.set(&mut image, 24);
        COMP_DISPLAY_START_DENOMINATOR.set(&mut image, 1);
        let mut record = Self(image);
        record.set_dimensions(width, height)?;
        Ok(record)
    }

    /// Decodes exactly one composition record.
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        Ok(Self(layout::RecordImage::decode(bytes, "cdta")?))
    }

    /// Composition preview color stored as 8-bit RGB, returned with synthesized opaque alpha.
    /// This setting is not an authored background layer or evidence of native alpha coverage.
    pub fn background_color(&self) -> [f64; 4] {
        let bytes = self.0.bytes();
        [
            f64::from(bytes[COMP_BACKGROUND_RED]) / 255.0,
            f64::from(bytes[COMP_BACKGROUND_GREEN]) / 255.0,
            f64::from(bytes[COMP_BACKGROUND_BLUE]) / 255.0,
            1.0,
        ]
    }

    /// Composition canvas dimensions in pixels.
    pub fn dimensions(&self) -> (u16, u16) {
        (COMP_WIDTH.get(&self.0), COMP_HEIGHT.get(&self.0))
    }

    /// Sets composition canvas dimensions without overwriting other fields.
    pub fn set_dimensions(&mut self, width: u16, height: u16) -> Result<(), RecordError> {
        if width == 0 || height == 0 {
            return Err(RecordError::Invalid("zero composition dimension"));
        }
        COMP_WIDTH.set(&mut self.0, width);
        COMP_HEIGHT.set(&mut self.0, height);
        Ok(())
    }

    /// Set the nominal 16.16 FPS and the corresponding native frame interval.
    pub(crate) fn set_frame_rate(&mut self, rate: crate::timing::FrameRate) {
        let (whole, fraction) = rate.parts();
        COMP_FRAME_RATE_WHOLE.set(&mut self.0, whole);
        COMP_FRAME_RATE_FRACTION.set(&mut self.0, fraction);
        let (numerator, denominator) = rate.frame_interval();
        put_u32(self.0.bytes_mut(), 4, numerator);
        put_u32(self.0.bytes_mut(), 8, denominator);
    }

    /// Nominal frames per second, with a 16-bit fractional part.
    pub fn frame_rate(&self) -> f64 {
        f64::from(COMP_FRAME_RATE_WHOLE.get(&self.0))
            + f64::from(COMP_FRAME_RATE_FRACTION.get(&self.0)) / 65_536.0
    }

    /// Pixel-aspect ratio represented as numerator and denominator.
    pub fn pixel_aspect_fraction(&self) -> (u32, u32) {
        (
            COMP_PIXEL_ASPECT.get(&self.0),
            COMP_PIXEL_ASPECT_DENOMINATOR.get(&self.0),
        )
    }

    /// Display start time represented as signed numerator and denominator.
    pub fn display_start_fraction(&self) -> (i32, u32) {
        (
            COMP_DISPLAY_START.get(&self.0),
            COMP_DISPLAY_START_DENOMINATOR.get(&self.0),
        )
    }

    /// The work-area start time and end marker, used to detect unsupported trims.
    pub fn work_area_bounds(&self) -> ((u32, u32), (u32, u32)) {
        (
            (
                COMP_WORK_START.get(&self.0),
                COMP_WORK_START_DENOMINATOR.get(&self.0),
            ),
            (
                COMP_WORK_END.get(&self.0),
                COMP_WORK_END_DENOMINATOR.get(&self.0),
            ),
        )
    }

    /// The two composition flags bytes (motion blur, frame blending, etc.).
    pub fn flags(&self) -> [u8; 2] {
        [self.0.bytes()[138], self.0.bytes()[139]]
    }

    /// Enables or disables the composition frame-blending master while
    /// preserving motion blur and every unrelated record flag.
    pub(crate) fn set_frame_blending(&mut self, enabled: bool) {
        let flags = &mut self.0.bytes_mut()[139];
        if enabled {
            *flags |= 16;
        } else {
            *flags &= !16;
        }
    }

    /// Motion-blur exposure angle in degrees, before destination validation.
    pub fn shutter_angle(&self) -> u16 {
        COMP_SHUTTER_ANGLE.get(&self.0)
    }

    /// Signed motion-blur exposure offset in degrees.
    pub fn shutter_phase(&self) -> i32 {
        COMP_SHUTTER_PHASE.get(&self.0)
    }

    /// Authored minimum samples and adaptive limit; malformed negatives remain visible.
    pub fn motion_blur_samples(&self) -> (i32, i32) {
        (
            COMP_BLUR_SAMPLES.get(&self.0),
            COMP_BLUR_ADAPTIVE_LIMIT.get(&self.0),
        )
    }

    /// Sets the AE26 composition-level motion-blur fields without changing
    /// unrelated flags or unknown record bytes.
    pub(crate) fn set_motion_blur(
        &mut self,
        enabled: bool,
        shutter_angle: u16,
        shutter_phase: i32,
        samples_per_frame: i32,
        adaptive_sample_limit: i32,
    ) -> Result<(), RecordError> {
        if shutter_angle > 720 {
            return Err(RecordError::Invalid(
                "motion-blur shutter angle exceeds 720",
            ));
        }
        if !(-360..=360).contains(&shutter_phase) {
            return Err(RecordError::Invalid(
                "motion-blur shutter phase is outside -360..=360",
            ));
        }
        if !(2..=64).contains(&samples_per_frame) {
            return Err(RecordError::Invalid(
                "motion-blur samples per frame is outside 2..=64",
            ));
        }
        if !(16..=256).contains(&adaptive_sample_limit) {
            return Err(RecordError::Invalid(
                "motion-blur adaptive sample limit is outside 16..=256",
            ));
        }

        let flags = &mut self.0.bytes_mut()[139];
        if enabled {
            *flags |= 8;
        } else {
            *flags &= !8;
        }
        COMP_SHUTTER_ANGLE.set(&mut self.0, shutter_angle);
        COMP_SHUTTER_PHASE.set(&mut self.0, shutter_phase);
        COMP_BLUR_SAMPLES.set(&mut self.0, samples_per_frame);
        COMP_BLUR_ADAPTIVE_LIMIT.set(&mut self.0, adaptive_sample_limit);
        Ok(())
    }

    /// Resolution-divisor pair; both must be 1 for a full-resolution composition.
    pub fn resolution_divisors(&self) -> (u16, u16) {
        (
            COMP_RESOLUTION_X.get(&self.0),
            COMP_RESOLUTION_Y.get(&self.0),
        )
    }

    /// Duration as an exact fraction of seconds: numerator and denominator.
    pub fn duration_fraction(&self) -> Result<(u32, u32), RecordError> {
        let denominator = COMP_DURATION_DENOMINATOR.get(&self.0);
        if denominator == 0 {
            return Err(RecordError::Invalid("zero composition duration divisor"));
        }
        Ok((COMP_DURATION.get(&self.0), denominator))
    }

    /// Sets the duration fraction (seconds) without modifying unknown fields.
    pub fn set_duration_fraction(
        &mut self,
        numerator: u32,
        denominator: u32,
    ) -> Result<(), RecordError> {
        if denominator == 0 || numerator == 0 {
            return Err(RecordError::Invalid("invalid composition duration"));
        }
        COMP_DURATION.set(&mut self.0, numerator);
        COMP_DURATION_DENOMINATOR.set(&mut self.0, denominator);
        Ok(())
    }

    /// Encodes this record, retaining unknown fields.
    pub fn encode(&self) -> [u8; 204] {
        self.0.encode()
    }
}

#[cfg(test)]
mod tests {
    use super::{CompositionRecord, HeadRecord, ItemRecord};
    use crate::{aep::Project, rifx::Chunk, timing::Duration24};

    fn descendants(chunks: &[Chunk], kind: [u8; 4]) -> Vec<&[u8]> {
        let mut result = Vec::new();
        for chunk in chunks {
            if chunk.id() == kind
                && let Some(bytes) = chunk.data_payload()
            {
                result.push(bytes);
            }
            if let Some(children) = chunk.children() {
                result.extend(descendants(children, kind));
            }
        }
        result
    }

    #[test]
    fn reads_real_composition_fields_and_writes_without_losing_other_bytes() {
        // Known independent fixture: one 100x100, 24fps, 1s composition, ID 1.
        let source = include_bytes!("../tests/fixtures/compositions.aep");
        let project = Project::parse(source).unwrap();
        let records = descendants(&project.chunks, *b"cdta");
        assert_eq!(records.len(), 1);
        let mut comp = CompositionRecord::decode(records[0]).unwrap();
        assert_eq!(comp.dimensions(), (100, 100));
        assert_eq!(comp.frame_rate(), 24.0);
        assert_eq!(comp.duration_fraction().unwrap(), (24_576, 24_576));
        assert_eq!(comp.background_color(), [0.2, 0.4, 0.6, 1.0]);
        assert_eq!(comp.encode().as_slice(), records[0]);
        let before = comp.encode();
        comp.set_dimensions(1920, 1080).unwrap();
        comp.set_duration_fraction(8, 1).unwrap();
        assert_eq!(comp.dimensions(), (1920, 1080));
        assert_eq!(comp.duration_fraction().unwrap(), (8, 1));
        let after = comp.encode();
        for index in 0..after.len() {
            if !matches!(index, 44..=51 | 140..=143) {
                assert_eq!(after[index], before[index], "unknown byte {index} changed");
            }
        }
        let item_records = descendants(&project.chunks, *b"idta");
        let original = item_records
            .into_iter()
            .find(|record| ItemRecord::decode(record).unwrap().item_type() == 4)
            .unwrap();
        let composition = ItemRecord::decode(original).unwrap();
        assert_eq!(composition.id(), 1);
        assert_eq!(composition.encode().as_slice(), original);
    }

    #[test]
    fn constructors_encode_distinct_valid_fields_without_specimen_identity() {
        let head = HeadRecord::empty_ae26(13);
        assert_eq!(
            HeadRecord::decode(&head.encode()).unwrap().format_version(),
            97
        );
        assert_eq!(head.next_item_id(), 13);
        let producer = head.producer_version_word();
        assert_eq!(((producer >> 26) & 31) * 8 + ((producer >> 19) & 7), 26);
        assert_eq!((producer >> 15) & 15, 5);
        assert_eq!(producer & 255, 89);
        assert_eq!(&head.encode()[18..20], &1_u16.to_be_bytes());
        let item = ItemRecord::empty_ae26_composition(1).unwrap();
        assert_eq!((item.item_type(), item.id()), (4, 1));
        assert_eq!(ItemRecord::decode(&item.encode()).unwrap(), item);
        assert_eq!(&item.encode()[80..84], &[0; 4]);
        let comp =
            CompositionRecord::empty_ae26(640, 360, Duration24::from_frames(72).unwrap()).unwrap();
        assert_eq!(comp.dimensions(), (640, 360));
        assert_eq!(comp.duration_fraction().unwrap(), (73_728, 24_576));
        assert_eq!(comp.frame_rate(), 24.0);
        assert_eq!(comp.pixel_aspect_fraction(), (1, 1));
        assert_eq!(comp.background_color(), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(comp.work_area_bounds(), ((0, 600), (u32::MAX, 600)));
        assert_eq!(CompositionRecord::decode(&comp.encode()).unwrap(), comp);
        assert!(
            CompositionRecord::empty_ae26(0, 360, Duration24::from_frames(1).unwrap()).is_err()
        );
        assert!(
            CompositionRecord::empty_ae26(640, 0, Duration24::from_frames(1).unwrap()).is_err()
        );
        assert!(ItemRecord::empty_ae26_composition(0).is_err());
    }

    #[test]
    fn rejects_truncated_records_and_zero_divisor() {
        assert!(ItemRecord::decode(&[0; 83]).is_err());
        assert!(CompositionRecord::decode(&[0; 203]).is_err());
        assert!(
            CompositionRecord::decode(&[0; 204])
                .unwrap()
                .duration_fraction()
                .is_err()
        );
    }

    #[test]
    fn decodes_signed_display_start_without_normalizing_raw_bytes() {
        let mut bytes = [0xa5; 204];
        bytes[164..168].copy_from_slice(&(-1024_i32).to_be_bytes());
        bytes[168..172].copy_from_slice(&24_576_u32.to_be_bytes());
        let record = CompositionRecord::decode(&bytes).unwrap();
        assert_eq!(record.display_start_fraction(), (-1024, 24_576));
        assert_eq!(record.encode(), bytes);
    }

    #[test]
    fn frame_blending_master_setter_preserves_other_composition_flags() {
        let mut bytes = [0xa5; 204];
        bytes[139] = 0b1010_1001;
        let mut record = CompositionRecord::decode(&bytes).unwrap();

        record.set_frame_blending(true);
        assert_eq!(record.flags()[1], 0b1011_1001);
        record.set_frame_blending(false);
        assert_eq!(record.flags()[1], 0b1010_1001);
        assert_eq!(record.encode(), bytes);
    }

    #[test]
    fn motion_blur_setter_matches_reader_offsets_and_preserves_other_bytes() {
        let mut bytes = [0xa5; 204];
        bytes[139] = 0b1010_0001;
        let mut record = CompositionRecord::decode(&bytes).unwrap();
        record.set_motion_blur(true, 271, -137, 23, 191).unwrap();

        assert_eq!(record.flags()[1], 0b1010_1001);
        assert_eq!(record.shutter_angle(), 271);
        assert_eq!(record.shutter_phase(), -137);
        assert_eq!(record.motion_blur_samples(), (23, 191));
        let encoded = record.encode();
        for index in 0..encoded.len() {
            if !matches!(index, 139 | 174..=175 | 180..=183 | 196..=203) {
                assert_eq!(
                    encoded[index], bytes[index],
                    "unrelated byte {index} changed"
                );
            }
        }

        record.set_motion_blur(false, 0, -360, 2, 16).unwrap();
        assert_eq!(record.flags()[1], 0b1010_0001);
        let before_rejection = record.encode();
        assert!(record.set_motion_blur(true, 721, 0, 16, 128).is_err());
        assert!(record.set_motion_blur(true, 180, -361, 16, 128).is_err());
        assert!(record.set_motion_blur(true, 180, 0, 1, 128).is_err());
        assert!(record.set_motion_blur(true, 180, 0, 16, 257).is_err());
        assert_eq!(record.encode(), before_rejection);
    }
}
