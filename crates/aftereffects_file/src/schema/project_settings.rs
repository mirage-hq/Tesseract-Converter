//! AE26 project settings have compact (`nhed`) and expanded (`nnhd`) mirrors.
//! Their different offsets encode the same time-display and color defaults.

use crate::rifx::RifxError;

/// Preserving codec for one compact AE26 project-settings record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactProjectSettings([u8; 32]);

/// Preserving codec for one expanded AE26 project-settings record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpandedProjectSettings([u8; 40]);

fn default_prefix(bytes: &mut [u8]) {
    bytes[..4].copy_from_slice(&2048_u32.to_be_bytes()); // Observed versioned prefix.
    bytes[4..8].copy_from_slice(&5_u32.to_be_bytes());
    bytes[9] = 1; // Source timecode display starts at source timecode.
    bytes[10] = 1; // Reserved AE26 default.
    // Timecode display, feet/frames off; bit-depth code 0 means 8 bpc.
}

impl CompactProjectSettings {
    pub(crate) fn ae26_default() -> Self {
        let mut bytes = [0; 32];
        default_prefix(&mut bytes);
        bytes[12] = 30; // Default timecode base, independent of composition fps.
        bytes[13] = 16; // AE26 reserved default.
        bytes[14] = 2; // Frame-count display convention.
        bytes[16] = 1; // Transparency grid in thumbnails.
        Self(bytes)
    }

    /// Decodes the exact compact record, retaining unrecognized bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, RifxError> {
        Ok(Self(
            bytes
                .try_into()
                .map_err(|_| RifxError::Invalid("invalid nhed length"))?,
        ))
    }

    /// Writes the compact record, including its reserved fields.
    pub fn encode(&self) -> [u8; 32] {
        self.0
    }
}

impl ExpandedProjectSettings {
    pub(crate) fn ae26_default() -> Self {
        let mut bytes = [0; 40];
        default_prefix(&mut bytes);
        bytes[14..16].copy_from_slice(&30_u16.to_be_bytes());
        bytes[16..20].copy_from_slice(&16_u32.to_be_bytes());
        bytes[20] = 2;
        bytes[25] = 1;
        Self(bytes)
    }

    /// Decodes the exact expanded record, retaining unrecognized bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, RifxError> {
        Ok(Self(
            bytes
                .try_into()
                .map_err(|_| RifxError::Invalid("invalid nnhd length"))?,
        ))
    }

    /// Writes the expanded record, including its reserved fields.
    pub fn encode(&self) -> [u8; 40] {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{CompactProjectSettings, ExpandedProjectSettings};

    #[test]
    fn project_settings_mirrors_agree() {
        let a = CompactProjectSettings::ae26_default().encode();
        let b = ExpandedProjectSettings::ae26_default().encode();
        assert_eq!(&a[..12], &b[..12]);
        assert_eq!(u16::from(a[12]), u16::from_be_bytes([b[14], b[15]]));
        assert_eq!((a[14], a[15], a[16]), (b[20], b[24], b[25]));
        assert_eq!(CompactProjectSettings::decode(&a).unwrap().encode(), a);
        assert_eq!(ExpandedProjectSettings::decode(&b).unwrap().encode(), b);
        assert!(CompactProjectSettings::decode(&a[..31]).is_err());
        assert!(ExpandedProjectSettings::decode(&b[..39]).is_err());
    }
}
