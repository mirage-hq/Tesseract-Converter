//! Stable identities shared by runtime evaluation and editable-key conversion.

use std::collections::BTreeSet;

/// Append one numeric component to the runtime's original FNV seed stream.
pub fn hash_random_seed_part(hash: &mut u64, value: u64) {
    *hash ^= value;
    *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
}

/// Fold a target prefix and its owner-local millisecond time into the JS seed.
pub fn random_seed(prefix: u64, time_ms: u64) -> u32 {
    let mut hash = prefix;
    hash_random_seed_part(&mut hash, time_ms);
    let folded = hash ^ (hash >> 32);
    // Masking makes the narrowing cast lossless.
    (folded & u64::from(u32::MAX)) as u32
}

/// Preserve the existing conversion identity namespace and byte ordering.
pub fn conversion_identity_seed(target_wire: &[u8], source_code: &[u8]) -> u64 {
    let mut hash = stable_hash_part(0xcbf2_9ce4_8422_2325, b"jerboa-keyframe-conversion-v1");
    hash = stable_hash_part(hash, target_wire);
    stable_hash_part(hash, source_code)
}

/// Allocate a deterministic key identity without colliding with authored keys.
pub fn converted_keyframe_id(
    identity_seed: u64,
    layer_time_ms: i64,
    used_ids: &mut BTreeSet<String>,
) -> String {
    let mut nonce = 0_u64;
    loop {
        let mut hash = stable_hash_part(identity_seed, &layer_time_ms.to_le_bytes());
        hash = stable_hash_part(hash, &nonce.to_le_bytes());
        let candidate = format!("converted-{identity_seed:016x}-{hash:016x}");
        if used_ids.insert(candidate.clone()) {
            return candidate;
        }
        nonce = nonce.wrapping_add(1);
    }
}

fn stable_hash_part(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converted_ids_are_deterministic_and_avoid_existing_ids() {
        let seed = conversion_identity_seed(b"target", b"source");
        let mut ids = BTreeSet::new();
        let first = converted_keyframe_id(seed, 12, &mut ids);
        assert_eq!(first, converted_keyframe_id(seed, 12, &mut BTreeSet::new()));
        assert_ne!(first, converted_keyframe_id(seed, 12, &mut ids));
        assert_eq!(ids.len(), 2);
    }
}
