//! Byte-preserving codecs for real timeline-layer `ldta` records.
//!
//! The offsets were cross-checked against the MIT-licensed `py-aep` project at
//! commit `e12a451c35bacd3f34a080090265f9370e66162b`. Only fields established by
//! that implementation are named here; every other byte remains untouched.

use std::str::Utf8Error;

use super::RecordError;

const LEGACY_LEN: usize = 160;
const MODERN_LEN: usize = 164;

/// One exact signed native rational used by fresh layer-record fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeRational {
    pub(crate) numerator: i32,
    pub(crate) denominator: u32,
}

/// Source-clock fields applied atomically to one fresh layer record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FreshLayerClockFields {
    pub(crate) stretch: NativeRational,
    pub(crate) start_time: NativeRational,
    pub(crate) in_point: NativeRational,
    pub(crate) out_point: NativeRational,
}

/// Boolean switches encoded in the four known layer-envelope flag bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerFlags {
    /// Higher-quality sampling is selected.
    pub sampling_quality: bool,
    /// The layer contributes to the environment map.
    pub environment_layer: bool,
    /// Per-character geometry faces the active camera.
    pub characters_toward_camera: bool,
    /// Per-character 3D is enabled.
    pub three_d_per_char: bool,
    /// Pixel-motion rather than frame-mix blending is selected.
    pub frame_blending_mode: bool,
    /// The layer is a guide layer.
    pub guide_layer: bool,
    /// A source-backed layer has an explicitly overridden name.
    pub name_set: bool,
    /// The AV layer is a null object.
    pub null_layer: bool,
    /// A camera or light is oriented toward its point of interest.
    pub point_of_interest_auto_orient: bool,
    /// A 3D layer is oriented toward the camera or point of interest.
    pub camera_or_point_of_interest_auto_orient: bool,
    /// Layer markers are locked.
    pub markers_locked: bool,
    /// The layer is soloed.
    pub solo: bool,
    /// The layer's 3D switch is enabled.
    pub three_d_layer: bool,
    /// The layer is an adjustment layer.
    pub adjustment_layer: bool,
    /// The layer is automatically oriented along its path.
    pub auto_orient_along_path: bool,
    /// Transformations are collapsed or continuously rasterized.
    pub collapse_transformation: bool,
    /// The layer is shy.
    pub shy: bool,
    /// The layer is locked.
    pub locked: bool,
    /// Frame blending is enabled.
    pub frame_blending: bool,
    /// Motion blur is enabled.
    pub motion_blur: bool,
    /// Effects are active.
    pub effects_active: bool,
    /// Audio is enabled.
    pub audio_enabled: bool,
    /// Video rendering is enabled.
    pub enabled: bool,
    /// The transfer mode preserves underlying transparency.
    pub preserve_transparency: bool,
    /// Dissolve uses the animated dancing-dissolve pattern.
    pub dancing_dissolve: bool,
}

/// A legacy 160-byte or modern 164-byte real timeline-layer descriptor.
///
/// Encoding returns the original bytes exactly. Getters do not normalize raw
/// fields, and unknown enum values remain available as their wire integers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerRecord {
    bytes: Vec<u8>,
}

impl LayerRecord {
    /// Construct a fresh, full-span AE26 2D AV layer for the bounded native writer.
    ///
    /// Reserved bytes are canonical zeroes, not copied from an imported layer.
    /// The flag/default layout is pinned by the native `transform_unseparated`
    /// fixture and py-aep e12a451's `LdtaChunk`; Adobe acceptance is separate.
    pub(crate) fn solid_ae26(
        id: u32,
        source_id: u32,
        duration: crate::timing::Duration24,
    ) -> Result<Self, RecordError> {
        if id == 0 || source_id == 0 || id == source_id {
            return Err(RecordError::Invalid("invalid solid layer/source identity"));
        }
        let mut bytes = vec![0; MODERN_LEN];
        bytes[0..4].copy_from_slice(&id.to_be_bytes());
        bytes[4..6].copy_from_slice(&2_u16.to_be_bytes()); // Best quality.
        bytes[8..12].copy_from_slice(&1_i32.to_be_bytes()); // 100% stretch.
        for offset in [16, 24, 32] {
            bytes[offset..offset + 4].copy_from_slice(&24_576_u32.to_be_bytes());
        }
        bytes[28..32].copy_from_slice(&duration.signed_ticks().to_be_bytes());
        bytes[37] = 1; // Explicit source-backed layer name, stored in Utf8.
        bytes[39] = 7; // Visual/audio/effects enabled, all other switches off.
        bytes[40..44].copy_from_slice(&source_id.to_be_bytes());
        bytes[59] = 1; // Observed initialized AV-layer default.
        bytes[61] = 1; // Default label, independent of layer/source identity.
        bytes[99] = 2; // Native Normal blend ordinal.
        bytes[108..112].copy_from_slice(&1_u32.to_be_bytes());
        Self::decode(&bytes)
    }

    /// Constructs a fresh source-backed AE26 Null AV layer.
    ///
    /// Native Nulls use an ordinary Solid footage item and AV envelope; the
    /// decoded Null flag is the only layer-record distinction. The source
    /// item is authored separately by the writer.
    pub(crate) fn null_ae26(
        id: u32,
        source_id: u32,
        duration: crate::timing::Duration24,
    ) -> Result<Self, RecordError> {
        let mut bytes = Self::solid_ae26(id, source_id, duration)?.encode();
        set_bit(&mut bytes[38], 7, true);
        Self::decode(&bytes)
    }

    /// Fresh source-less AE26 Shape Layer envelope (native type 4).
    /// Defaults are shared with the full-span visual Solid, but no footage
    /// source ID or Solid source item is encoded.
    pub(crate) fn shape_ae26(
        id: u32,
        duration: crate::timing::Duration24,
    ) -> Result<Self, RecordError> {
        let mut bytes = Self::solid_ae26(
            id,
            id.checked_add(1)
                .ok_or(RecordError::Invalid("shape layer ID overflow"))?,
            duration,
        )?
        .encode();
        bytes[39] = 135; // Pinned source-less shape: enabled visual/effects switches.
        bytes[40..44].fill(0); // Shape geometry is on the layer, not a footage source.
        bytes[61] = 8; // Native shape label.
        bytes[131] = 4; // Shape Layer.
        Self::decode(&bytes)
    }

    /// Fresh source-less two-node camera. The camera-specific defaults are
    /// pinned by `layers/type.aep`; no imported record bytes are retained.
    pub(crate) fn camera_ae26(
        id: u32,
        duration: crate::timing::Duration24,
    ) -> Result<Self, RecordError> {
        let mut bytes = Self::shape_ae26(id, duration)?.encode();
        bytes[4..6].fill(0); // Cameras have no raster quality setting.
        bytes[38] = 0x44; // 3D and point-of-interest auto orientation.
        bytes[39] = 1; // Enabled, without visual/audio/effect switches.
        bytes[59] = 0;
        bytes[61] = 4;
        bytes[99] = 0;
        bytes[131] = 2;
        bytes[151] = 1;
        // Canonical camera record scale: native 36 mm expressed in points.
        bytes[152..160].copy_from_slice(&(36.0_f64 / 25.4 * 72.0).to_be_bytes());
        Self::decode(&bytes)
    }

    /// Bound one fresh full-span layer to a local clock with an explicit
    /// composition start and a source-local visible duration (24,576 ticks/s).
    pub(crate) fn with_active_range(
        self,
        start_ticks: i32,
        length_ticks: i32,
    ) -> Result<Self, RecordError> {
        if start_ticks < 0 || length_ticks <= 0 {
            return Err(RecordError::Invalid("invalid native active range"));
        }
        let mut bytes = self.encode();
        bytes[12..16].copy_from_slice(&start_ticks.to_be_bytes());
        bytes[20..24].fill(0);
        bytes[28..32].copy_from_slice(&length_ticks.to_be_bytes());
        Self::decode(&bytes)
    }

    /// Atomically applies one checked source clock while preserving every
    /// unrelated fresh-record field.
    pub(crate) fn with_source_clock(
        self,
        clock: FreshLayerClockFields,
    ) -> Result<Self, RecordError> {
        let rationals = [
            clock.stretch,
            clock.start_time,
            clock.in_point,
            clock.out_point,
        ];
        if rationals.iter().any(|value| value.denominator == 0)
            || clock.stretch.numerator <= 0
            || clock.in_point.numerator < 0
            || clock.out_point.numerator < 0
        {
            return Err(RecordError::Invalid("invalid native source clock"));
        }
        let in_cross = i64::from(clock.in_point.numerator) * i64::from(clock.out_point.denominator);
        let out_cross =
            i64::from(clock.out_point.numerator) * i64::from(clock.in_point.denominator);
        if out_cross <= in_cross {
            return Err(RecordError::Invalid(
                "native source clock has a nonpositive source span",
            ));
        }

        let mut bytes = self.encode();
        for (numerator_offset, denominator_offset, value) in [
            (8, 108, clock.stretch),
            (12, 16, clock.start_time),
            (20, 24, clock.in_point),
            (28, 32, clock.out_point),
        ] {
            bytes[numerator_offset..numerator_offset + 4]
                .copy_from_slice(&value.numerator.to_be_bytes());
            bytes[denominator_offset..denominator_offset + 4]
                .copy_from_slice(&value.denominator.to_be_bytes());
        }
        Self::decode(&bytes)
    }

    /// Enables or disables established layer-level frame blending while
    /// preserving every unrelated fresh-record flag.
    pub(crate) fn with_frame_blending(
        self,
        enabled: bool,
        pixel_motion: bool,
    ) -> Result<Self, RecordError> {
        if pixel_motion && !enabled {
            return Err(RecordError::Invalid(
                "pixel-motion blending requires frame blending",
            ));
        }
        let mut bytes = self.encode();
        set_bit(&mut bytes[39], 4, enabled);
        set_bit(&mut bytes[37], 2, pixel_motion);
        Self::decode(&bytes)
    }

    /// Enables or disables the native audio channel switch while preserving
    /// every unrelated layer flag.
    pub(crate) fn with_audio_enabled(self, enabled: bool) -> Result<Self, RecordError> {
        let mut bytes = self.encode();
        set_bit(&mut bytes[39], 1, enabled);
        Self::decode(&bytes)
    }

    /// Enables or disables the established AE layer 3D switch while preserving
    /// every unrelated fresh-record flag.
    pub(crate) fn with_three_d_layer(self, enabled: bool) -> Result<Self, RecordError> {
        let mut bytes = self.encode();
        set_bit(&mut bytes[38], 2, enabled);
        Self::decode(&bytes)
    }

    /// Sets the independently observed AE precomposition collapse switch.
    pub(crate) fn with_collapse_transformations(self, enabled: bool) -> Result<Self, RecordError> {
        let mut bytes = self.encode();
        set_bit(&mut bytes[39], 7, enabled);
        Self::decode(&bytes)
    }

    /// Sets the native adjustment switch without changing other AV flags.
    pub(crate) fn with_adjustment_layer(self, enabled: bool) -> Result<Self, RecordError> {
        let mut bytes = self.encode();
        set_bit(&mut bytes[38], 1, enabled);
        Self::decode(&bytes)
    }

    /// Applies fields whose offsets and values are established by pinned native
    /// layer records. Unknown flags remain at the fresh constructor defaults.
    pub(crate) fn with_export_options(
        self,
        enabled: bool,
        motion_blur: bool,
        blend_mode: u8,
        parent_id: u32,
        matte_id: u32,
        matte_type: u8,
    ) -> Result<Self, RecordError> {
        const BLEND_MODES: [u8; 31] = [
            0, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 23, 24, 25, 26, 27, 28, 29, 30, 31,
            32, 33, 34, 35, 36, 37, 38,
        ];
        // Legacy records have no explicit matte-ID field. Do not silently
        // upgrade an unknown native layout or index beyond its preserved bytes.
        if self.bytes.len() != MODERN_LEN {
            return Err(RecordError::Invalid(
                "export options require the modern layer record layout",
            ));
        }
        if !BLEND_MODES.contains(&blend_mode) {
            return Err(RecordError::Invalid("unsupported native layer blend mode"));
        }
        if matte_type > 4 || (matte_type == 0) != (matte_id == 0) {
            return Err(RecordError::Invalid("invalid native track matte reference"));
        }
        if parent_id == self.id() || matte_id == self.id() {
            return Err(RecordError::Invalid("self-referencing native layer"));
        }
        let mut bytes = self.encode();
        set_bit(&mut bytes[39], 0, enabled);
        set_bit(&mut bytes[39], 3, motion_blur);
        bytes[99] = blend_mode;
        bytes[107] = matte_type;
        bytes[132..136].copy_from_slice(&parent_id.to_be_bytes());
        bytes[160..164].copy_from_slice(&matte_id.to_be_bytes());
        Self::decode(&bytes)
    }

    /// Decodes one known `ldta` layout without interpreting reserved bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        if !matches!(bytes.len(), LEGACY_LEN | MODERN_LEN) {
            return Err(RecordError::Invalid(
                "ldta length must be 160 (legacy) or 164 (modern) bytes",
            ));
        }
        Ok(Self {
            bytes: bytes.to_vec(),
        })
    }

    /// Encodes the descriptor without changing unknown bytes.
    pub fn encode(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    /// Returns the complete encoded descriptor, including unknown bytes.
    pub fn raw_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Project-wide layer identity.
    pub fn id(&self) -> u32 {
        self.u32_at(0)
    }

    /// Source project-item identity, or zero for a sourceless layer.
    pub fn source_id(&self) -> u32 {
        self.u32_at(40)
    }

    /// Transform-parent layer identity, or zero when unparented.
    pub fn parent_id(&self) -> u32 {
        self.u32_at(132)
    }

    /// Nonzero track-matte layer identity from the modern record extension.
    pub fn matte_layer_id(&self) -> Option<u32> {
        self.matte_layer_id_raw().filter(|&id| id != 0)
    }

    /// Raw modern matte field, distinguishing legacy absence from modern zero.
    pub fn matte_layer_id_raw(&self) -> Option<u32> {
        (self.bytes.len() == MODERN_LEN).then(|| self.u32_at(160))
    }

    /// Raw layer kind (0 AV, 1 light, 2 camera, 3 text, 4 shape, 5 model, 7 mesh).
    pub fn layer_type(&self) -> u8 {
        self.bytes[131]
    }

    /// Every currently known layer switch.
    pub fn flags(&self) -> LayerFlags {
        let flags_0 = self.bytes[37];
        let flags_1 = self.bytes[38];
        let flags_2 = self.bytes[39];
        let transfer = self.bytes[103];
        LayerFlags {
            sampling_quality: bit(flags_0, 6),
            environment_layer: bit(flags_0, 5),
            characters_toward_camera: bit(flags_0, 4),
            three_d_per_char: bit(flags_0, 3),
            frame_blending_mode: bit(flags_0, 2),
            guide_layer: bit(flags_0, 1),
            name_set: bit(flags_0, 0),
            null_layer: bit(flags_1, 7),
            point_of_interest_auto_orient: bit(flags_1, 6),
            camera_or_point_of_interest_auto_orient: bit(flags_1, 5),
            markers_locked: bit(flags_1, 4),
            solo: bit(flags_1, 3),
            three_d_layer: bit(flags_1, 2),
            adjustment_layer: bit(flags_1, 1),
            auto_orient_along_path: bit(flags_1, 0),
            collapse_transformation: bit(flags_2, 7),
            shy: bit(flags_2, 6),
            locked: bit(flags_2, 5),
            frame_blending: bit(flags_2, 4),
            motion_blur: bit(flags_2, 3),
            effects_active: bit(flags_2, 2),
            audio_enabled: bit(flags_2, 1),
            enabled: bit(flags_2, 0),
            preserve_transparency: bit(transfer, 0),
            dancing_dissolve: bit(transfer, 1),
        }
    }

    /// Start time in composition seconds, or `None` for a zero divisor.
    pub fn start_time(&self) -> Option<f64> {
        fraction(self.start_time_fraction())
    }

    /// Raw layer-relative in point, or `None` for a zero divisor.
    ///
    /// This is not composition time. Composition time is
    /// `start_time + in_point * stretch_ratio`.
    pub fn in_point(&self) -> Option<f64> {
        fraction(self.in_point_fraction())
    }

    /// Raw layer-relative out point, or `None` for a zero divisor.
    ///
    /// This is not composition time. Composition time is
    /// `start_time + out_point * stretch_ratio`.
    pub fn out_point(&self) -> Option<f64> {
        fraction(self.out_point_fraction())
    }

    /// Signed time-stretch ratio, where `1.0` means no stretch.
    pub fn stretch(&self) -> Option<f64> {
        fraction(self.stretch_fraction())
    }

    /// Exact signed start-time numerator and unsigned denominator.
    pub fn start_time_fraction(&self) -> (i32, u32) {
        (self.i32_at(12), self.u32_at(16))
    }

    /// Exact signed, layer-relative in-point numerator and denominator.
    pub fn in_point_fraction(&self) -> (i32, u32) {
        (self.i32_at(20), self.u32_at(24))
    }

    /// Exact signed, layer-relative out-point numerator and denominator.
    pub fn out_point_fraction(&self) -> (i32, u32) {
        (self.i32_at(28), self.u32_at(32))
    }

    /// Exact signed stretch-ratio numerator and unsigned denominator.
    pub fn stretch_fraction(&self) -> (i32, u32) {
        (self.i32_at(8), self.u32_at(108))
    }

    /// Raw quality enum.
    pub fn quality(&self) -> u16 {
        self.u16_at(4)
    }

    /// Embedded fixed-width layer name, stopping at the first NUL byte.
    pub fn embedded_name(&self) -> Result<&str, Utf8Error> {
        let bytes = &self.bytes[64..96];
        let end = bytes
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len());
        std::str::from_utf8(&bytes[..end])
    }

    /// Raw blend-mode enum.
    pub fn blend_mode(&self) -> u8 {
        self.bytes[99]
    }

    /// Raw track-matte-type enum.
    pub fn track_matte_type(&self) -> u8 {
        self.bytes[107]
    }

    /// Layer label color index.
    pub fn label(&self) -> u8 {
        self.bytes[61]
    }

    /// Derived auto-orient mode: 0 none, 1 path, 2 camera/POI, 3 characters.
    pub fn auto_orient(&self) -> u8 {
        let flags = self.flags();
        if flags.auto_orient_along_path {
            1
        } else if (flags.camera_or_point_of_interest_auto_orient
            || flags.point_of_interest_auto_orient)
            && flags.three_d_layer
        {
            2
        } else if flags.characters_toward_camera && flags.three_d_per_char {
            3
        } else {
            0
        }
    }

    /// Derived frame-blending mode: 0 disabled, 1 frame mix, 2 pixel motion.
    pub fn frame_blending_type(&self) -> u8 {
        let flags = self.flags();
        if !flags.frame_blending {
            0
        } else if flags.frame_blending_mode {
            2
        } else {
            1
        }
    }

    /// Raw light subtype or parametric-mesh subtype.
    pub fn light_and_mesh_type(&self) -> u8 {
        self.bytes[139]
    }

    /// Unidentified AE 26-era boolean-like byte retained at offset 119.
    pub fn unknown_ae26_flag(&self) -> u8 {
        self.bytes[119]
    }

    /// Unidentified AE 26-era floating-point field retained at offsets 120..128.
    pub fn unknown_ae26_float(&self) -> f64 {
        f64::from_bits(self.u64_at(120))
    }

    fn u16_at(&self, offset: usize) -> u16 {
        u16::from_be_bytes([self.bytes[offset], self.bytes[offset + 1]])
    }

    fn u32_at(&self, offset: usize) -> u32 {
        u32::from_be_bytes(
            self.bytes[offset..offset + size_of::<u32>()]
                .try_into()
                .expect("ldta field offset is bounded by both known layouts"),
        )
    }

    fn i32_at(&self, offset: usize) -> i32 {
        i32::from_be_bytes(
            self.bytes[offset..offset + size_of::<i32>()]
                .try_into()
                .expect("ldta field offset is bounded by both known layouts"),
        )
    }

    fn u64_at(&self, offset: usize) -> u64 {
        u64::from_be_bytes(
            self.bytes[offset..offset + size_of::<u64>()]
                .try_into()
                .expect("ldta field offset is bounded by both known layouts"),
        )
    }
}

fn bit(value: u8, index: u32) -> bool {
    value & (1 << index) != 0
}

fn set_bit(value: &mut u8, index: u32, enabled: bool) {
    if enabled {
        *value |= 1 << index;
    } else {
        *value &= !(1 << index);
    }
}

fn fraction((numerator, denominator): (i32, u32)) -> Option<f64> {
    (denominator != 0).then(|| f64::from(numerator) / f64::from(denominator))
}

#[cfg(test)]
mod tests {
    use super::{FreshLayerClockFields, LEGACY_LEN, LayerRecord, NativeRational};

    fn put_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    #[test]
    fn fresh_null_layer_is_a_source_backed_av_envelope() {
        let duration = crate::timing::Duration24::from_frames(24).unwrap();
        let record = LayerRecord::null_ae26(3, 2, duration).unwrap();

        assert_eq!(record.id(), 3);
        assert_eq!(record.source_id(), 2);
        assert_eq!(record.layer_type(), 0);
        assert!(record.flags().null_layer);
        assert!(record.flags().enabled);
    }

    #[test]
    fn fresh_source_clock_updates_only_the_four_rationals() {
        let duration = crate::timing::Duration24::from_frames(96).unwrap();
        let base = LayerRecord::solid_ae26(13, 14, duration).unwrap();
        let clock = FreshLayerClockFields {
            stretch: NativeRational {
                numerator: 3,
                denominator: 2,
            },
            start_time: NativeRational {
                numerator: -1,
                denominator: 2,
            },
            in_point: NativeRational {
                numerator: 1,
                denominator: 4,
            },
            out_point: NativeRational {
                numerator: 7,
                denominator: 4,
            },
        };
        let updated = base.clone().with_source_clock(clock).unwrap();

        assert_eq!(updated.stretch_fraction(), (3, 2));
        assert_eq!(updated.start_time_fraction(), (-1, 2));
        assert_eq!(updated.in_point_fraction(), (1, 4));
        assert_eq!(updated.out_point_fraction(), (7, 4));
        let before = base.encode();
        let after = updated.encode();
        for index in 0..after.len() {
            if !matches!(index, 8..=35 | 108..=111) {
                assert_eq!(
                    after[index], before[index],
                    "unrelated byte {index} changed"
                );
            }
        }
    }

    #[test]
    fn fresh_frame_blending_preserves_unrelated_switches() {
        let duration = crate::timing::Duration24::from_frames(24).unwrap();
        let base = LayerRecord::solid_ae26(13, 14, duration).unwrap();
        let mixed = base.clone().with_frame_blending(true, false).unwrap();
        assert_eq!(mixed.frame_blending_type(), 1);
        let motion = mixed.with_frame_blending(true, true).unwrap();
        assert_eq!(motion.frame_blending_type(), 2);
        let disabled = motion.with_frame_blending(false, false).unwrap();
        assert_eq!(disabled.frame_blending_type(), 0);
        assert!(base.with_frame_blending(false, true).is_err());
    }

    #[test]
    fn fresh_adjustment_switch_preserves_every_other_byte() {
        let duration = crate::timing::Duration24::from_frames(24).unwrap();
        let base = LayerRecord::solid_ae26(13, 14, duration)
            .unwrap()
            .with_three_d_layer(true)
            .unwrap();
        let enabled = base.clone().with_adjustment_layer(true).unwrap();
        assert!(enabled.flags().adjustment_layer);
        assert!(enabled.flags().three_d_layer);
        let mut expected = base.encode();
        expected[38] |= 1 << 1;
        assert_eq!(enabled.encode(), expected);
        assert_eq!(
            enabled.with_adjustment_layer(false).unwrap().encode(),
            base.encode()
        );
    }

    #[test]
    fn fresh_three_d_switch_changes_only_the_established_flag() {
        let duration = crate::timing::Duration24::from_frames(24).unwrap();
        let base = LayerRecord::solid_ae26(13, 14, duration).unwrap();
        let enabled = base.clone().with_three_d_layer(true).unwrap();

        assert!(enabled.flags().three_d_layer);
        let mut expected = base.encode();
        expected[38] |= 1 << 2;
        assert_eq!(enabled.encode(), expected);
        assert!(
            !enabled
                .with_three_d_layer(false)
                .unwrap()
                .flags()
                .three_d_layer
        );
    }

    #[test]
    fn fresh_collapse_switch_preserves_every_other_byte() {
        let duration = crate::timing::Duration24::from_frames(48).unwrap();
        let base = LayerRecord::solid_ae26(13, 14, duration).unwrap();
        let collapsed = base.clone().with_collapse_transformations(true).unwrap();
        let mut expected = base.encode();
        expected[39] |= 1 << 7;
        assert_eq!(collapsed.encode(), expected);
        assert_eq!(
            collapsed
                .with_collapse_transformations(false)
                .unwrap()
                .encode(),
            base.encode()
        );
    }

    #[test]
    fn fresh_shape_layer_has_no_footage_reference() {
        let record =
            LayerRecord::shape_ae26(13, crate::timing::Duration24::from_frames(24).unwrap())
                .unwrap();
        assert_eq!(record.id(), 13);
        assert_eq!(record.source_id(), 0);
        assert_eq!(record.layer_type(), 4);
        assert_eq!(record.raw_bytes()[39], 135);
        assert_eq!(record.raw_bytes()[61], 8);
    }

    #[test]
    fn audio_switch_preserves_all_other_descriptor_bytes() {
        for length in [super::LEGACY_LEN, super::MODERN_LEN] {
            let bytes: Vec<u8> = (0..length).map(|value| value as u8).collect();
            for enabled in [false, true] {
                let record = LayerRecord::decode(&bytes)
                    .unwrap()
                    .with_audio_enabled(enabled)
                    .unwrap();
                let mut expected = bytes.clone();
                if enabled {
                    expected[39] |= 2;
                } else {
                    expected[39] &= !2;
                }
                assert_eq!(record.encode(), expected);
                assert_eq!(record.flags().audio_enabled, enabled);
            }
        }
    }

    #[test]
    fn decodes_known_fields_and_roundtrips_unknown_bytes() {
        let mut bytes: Vec<u8> = (0..164).map(|value| value as u8).collect();
        put_u32(&mut bytes, 0, 41);
        bytes[4..6].copy_from_slice(&2_u16.to_be_bytes());
        put_i32(&mut bytes, 8, -3);
        put_i32(&mut bytes, 12, -24);
        put_u32(&mut bytes, 16, 24);
        put_i32(&mut bytes, 20, 12);
        put_u32(&mut bytes, 24, 24);
        put_i32(&mut bytes, 28, 72);
        put_u32(&mut bytes, 32, 24);
        bytes[37] = 0b0111_1111;
        bytes[38] = 0b1111_1111;
        bytes[39] = 0b1111_1111;
        put_u32(&mut bytes, 40, 77);
        bytes[61] = 9;
        bytes[64..96].fill(0);
        bytes[64..69].copy_from_slice(b"Layer");
        bytes[99] = 5;
        bytes[103] = 3;
        bytes[107] = 2;
        put_u32(&mut bytes, 108, 2);
        bytes[119] = 1;
        bytes[120..128].copy_from_slice(&1.25_f64.to_bits().to_be_bytes());
        bytes[131] = 3;
        put_u32(&mut bytes, 132, 11);
        bytes[139] = 4;
        put_u32(&mut bytes, 160, 12);

        let record = LayerRecord::decode(&bytes).unwrap();
        assert_eq!(record.encode(), bytes);
        assert_eq!(
            (record.id(), record.source_id(), record.parent_id()),
            (41, 77, 11)
        );
        assert_eq!(record.matte_layer_id(), Some(12));
        assert_eq!(record.layer_type(), 3);
        assert_eq!(record.quality(), 2);
        assert_eq!(record.embedded_name().unwrap(), "Layer");
        assert_eq!(
            (
                record.blend_mode(),
                record.track_matte_type(),
                record.label()
            ),
            (5, 2, 9)
        );
        assert_eq!(record.start_time(), Some(-1.0));
        assert_eq!(record.in_point(), Some(0.5));
        assert_eq!(record.out_point(), Some(3.0));
        assert_eq!(record.stretch(), Some(-1.5));
        assert_eq!(record.auto_orient(), 1);
        assert_eq!(record.frame_blending_type(), 2);
        assert_eq!(record.light_and_mesh_type(), 4);
        assert_eq!(record.unknown_ae26_flag(), 1);
        assert_eq!(record.unknown_ae26_float(), 1.25);
        let flags = record.flags();
        assert!(flags.enabled && flags.audio_enabled && flags.effects_active);
        assert!(flags.motion_blur && flags.adjustment_layer && flags.guide_layer);
        assert!(flags.preserve_transparency && flags.dancing_dissolve);
    }

    #[test]
    fn accepts_legacy_layout_and_reports_invalid_rationals() {
        let mut bytes = vec![0; 160];
        put_u32(&mut bytes, 0, 1);
        assert!(
            LayerRecord::decode(&bytes)
                .unwrap()
                .matte_layer_id_raw()
                .is_none()
        );
        let record = LayerRecord::decode(&bytes).unwrap();
        assert_eq!(record.start_time(), None);
        assert_eq!(record.in_point(), None);
        assert_eq!(record.out_point(), None);
        assert_eq!(record.stretch(), None);
    }

    #[test]
    fn review_audit_legacy_export_options_return_an_error_without_panicking() {
        let mut bytes = vec![0; LEGACY_LEN];
        put_u32(&mut bytes, 0, 1);
        let record = LayerRecord::decode(&bytes).unwrap();
        assert!(record.with_export_options(true, false, 2, 0, 0, 0).is_err());
    }

    #[test]
    fn rejects_every_unknown_record_length() {
        for length in [0, 159, 161, 163, 165] {
            assert!(LayerRecord::decode(&vec![0; length]).is_err());
        }
    }
}
