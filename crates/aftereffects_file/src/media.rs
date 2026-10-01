//! Optional AE file-footage descriptor decoding.
//!
//! Media payload failures are item-local so one malformed/offline source does
//! not prevent unrelated project structure from loading.

mod descriptor;

pub use descriptor::{
    MediaDecodeError, MediaDescriptor, MediaDuration, MediaFrameRate, MediaKind, PhotoshopSource,
};

use crate::rifx::Chunk;

/// Decode one main `Pin ` file source without opening its authored path.
pub(super) fn decode(pin: &Chunk) -> Result<MediaDescriptor, MediaDecodeError> {
    descriptor::decode(pin)
}

pub(super) fn decode_native(pin: &Chunk) -> Result<MediaDescriptor, MediaDecodeError> {
    descriptor::decode_native(pin)
}
