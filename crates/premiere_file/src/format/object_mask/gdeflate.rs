//! Checked CPU decoding of the DirectStorage GDeflate tile stream.
//! TileStream.h defines a zero lastTileSize as one full 64 KiB tile.
//! Use the sys API: gdeflate 0.4.1's safe wrapper ignores decoder errors.

use crate::error::{ensure, unsupported, Result};
use gdeflate_sys as sys;
use std::{ptr::NonNull, sync::Once};

const TILE_BYTES: usize = 65_536;
const HEADER_BYTES: usize = 8;
const CODEC_ID: u8 = 4;
const TILE_SIZE_INDEX: u32 = 1;
const LAST_TILE_MASK: u32 = (1 << 18) - 1;
const RESERVED_BITS: u32 = 0xfff0_0000;

// Source-pinned read-ahead proof for gdeflate_decompress_template.h (0.4.1).
// ENSURE_BITS (60–67) refills 32-bit words without consulting in_end. Once
// refills reach an all-zero tail, each of 32 streams retains at most 63 real
// bits. Nonzero block headers consume one of stream 0's remaining real bits;
// zero headers enter stored blocks, whose original-in_end check bounds their
// progress. Output-producing operations consume at most 20 bits/output byte.
// Thus D <= 20*O + 32*29 + 64*4569 + 111*19 bits and refills <= 63+ceil(D/32).
// This bound is valid ONLY with the original nbytes, never the padded length.
/// Match length >=3 and <=60 consumed bits (186–201, 505–550).
const BITS_PER_OUTPUT_BYTE: usize = 20;
/// At most one pending offset per stream at the first zero-tail read (186–201).
const PENDING_COPY_BITS: usize = 32 * 29;
/// Up to 63 nonzero headers plus the block in progress (294–428).
const HUFFMAN_BLOCK_BITS: usize = 64 * 4569;
/// Zero stored headers are bounded by the original-in_end check (439–448).
const STORED_BLOCK_BITS: usize = 111 * 19;
/// Aggregate retained bits/top-ups over 32 streams (60–67, 284–290).
const EXTRA_REFILLS: usize = 63;

fn read_slack(output_len: usize) -> Option<usize> {
    let bits = output_len
        .checked_mul(BITS_PER_OUTPUT_BYTE)?
        .checked_add(PENDING_COPY_BITS)?
        .checked_add(HUFFMAN_BLOCK_BITS)?
        .checked_add(STORED_BLOCK_BITS)?;
    bits.div_ceil(32).checked_add(EXTRA_REFILLS)?.checked_mul(4)
}

/// The entire backing allocation is readable and its tail is zero. Construction
/// is the only way to establish this invariant; no mutable buffer escapes it.
/// Private concrete uses are Vec<u8> and the test Guarded buffer only: their
/// AsRef/AsMut views must cover the same stable allocation. This is not a public
/// abstraction over arbitrary caller-provided buffer implementations.
struct PaddedTile<B> {
    bytes: B,
    original_len: usize,
    output_len: usize,
}

impl<B: AsRef<[u8]> + AsMut<[u8]>> PaddedTile<B> {
    fn new(input: &[u8], mut bytes: B, output_len: usize) -> Self {
        let required = input
            .len()
            .checked_add(read_slack(output_len).expect("bounded output"))
            .expect("bounded tile allocation");
        assert!(bytes.as_ref().len() >= required);
        bytes.as_mut()[..input.len()].copy_from_slice(input);
        bytes.as_mut()[input.len()..].fill(0);
        Self {
            bytes,
            original_len: input.len(),
            output_len,
        }
    }
}

// C's x86 dispatch pointer and CPU-feature cache use volatile, NOT atomics.
// The first actual call (nonnull page, count=1) runs dispatch even on bad data:
// setup_cpu_features completes before dispatch replaces decompress_impl. Once
// publishes both writes before any later caller enters C. Later calls only read
// the selected pointer; allocation/free do not initialize either global. On
// builds without DISPATCH, these decode paths access neither mutable global.
// All native GDeflate calls in this module must pass through this gate.
// premiere_file is currently the sole gdeflate-sys user. Any future process-wide
// caller must share this gate, not introduce an independent Once or bypass it.
static INITIALIZE: Once = Once::new();

fn initialized_call(
    once: &Once,
    mut call: impl FnMut() -> sys::libdeflate_result,
) -> sys::libdeflate_result {
    let mut first_result = None;
    once.call_once(|| first_result = Some(call()));
    // Use the first call's result/output, without replaying or caching its input.
    first_result.unwrap_or_else(call)
}

/// Exclusive decoder; calls accept only zero-tailed tiles with ORIGINAL nbytes.
/// Version changes require re-auditing the read-slack proof and guarded/ASan tests.
struct Decompressor(NonNull<sys::libdeflate_gdeflate_decompressor>);

impl Decompressor {
    fn new() -> Result<Self> {
        // SAFETY: allocation takes no inputs; null is handled before use.
        NonNull::new(unsafe { sys::libdeflate_alloc_gdeflate_decompressor() })
            .map(Self)
            .ok_or_else(|| unsupported("Object Mask GDeflate decoder allocation failed"))
    }

    fn tile(&mut self, input: &PaddedTile<impl AsRef<[u8]>>, output: &mut [u8]) -> Result<()> {
        assert_eq!(input.output_len, output.len());
        assert!(
            input.bytes.as_ref().len() - input.original_len
                >= read_slack(output.len()).expect("bounded output")
        );
        let mut page = sys::libdeflate_gdeflate_in_page {
            data: input.bytes.as_ref().as_ptr().cast(),
            nbytes: input.original_len,
        };
        let mut actual = 0;
        // SAFETY: the decoder is exclusive and live. Input and writable output
        // are separate allocations (C restrict); the pointer covers the WHOLE
        // padded allocation. C does NOT bound reads by nbytes: the zero tail,
        // ORIGINAL length and output-derived read_slack bound them as proved
        // above. Writes are bounded by out_end (template 205–256, 443–448,
        // 518–550). This audit applies only to gdeflate-sys =0.4.1, Cargo.lock
        // checksum 57d037ff28c21adce29618dbe9271e8b5a7ed121efe8482b43182db9649797d0;
        // no other call pattern or version is permitted.
        // The Once gate also publishes C dispatch/CPU-feature initialization;
        // volatile alone would leave concurrent first calls unsound.
        let status = initialized_call(&INITIALIZE, || unsafe {
            sys::libdeflate_gdeflate_decompress(
                self.0.as_ptr(),
                &mut page,
                1,
                output.as_mut_ptr().cast(),
                output.len(),
                &mut actual,
            )
        });
        ensure!(
            status == sys::libdeflate_result_LIBDEFLATE_SUCCESS && actual == output.len(),
            "Object Mask GDeflate tile failed: status {status}, decoded {actual}, expected {}",
            output.len()
        );
        Ok(())
    }
}

impl Drop for Decompressor {
    fn drop(&mut self) {
        // SAFETY: this pointer was allocated by the matching allocator and is
        // freed exactly once, after every borrow used to decode has ended.
        unsafe { sys::libdeflate_free_gdeflate_decompressor(self.0.as_ptr()) }
    }
}

/// Decode into a caller-bounded crop; never allocate from stream declarations.
pub(super) fn decode(payload: &[u8], output: &mut [u8]) -> Result<()> {
    let header = payload
        .get(..HEADER_BYTES)
        .ok_or_else(|| unsupported("truncated Object Mask GDeflate header"))?;
    ensure!(
        header[0] == CODEC_ID && header[1] == !CODEC_ID,
        "invalid Object Mask GDeflate magic"
    );
    let tiles = usize::from(u16::from_le_bytes([header[2], header[3]]));
    let flags = u32::from_le_bytes(header[4..8].try_into().expect("four flag bytes"));
    ensure!(
        tiles > 0 && flags & 3 == TILE_SIZE_INDEX && flags & RESERVED_BITS == 0,
        "invalid Object Mask GDeflate tile flags/count"
    );
    let last = usize::try_from((flags >> 2) & LAST_TILE_MASK)
        .map_err(|_| unsupported("Object Mask last tile size overflows"))?;
    ensure!(last < TILE_BYTES, "invalid Object Mask last tile size");
    let last = if last == 0 { TILE_BYTES } else { last };
    ensure!(
        (tiles - 1)
            .checked_mul(TILE_BYTES)
            .and_then(|size| size.checked_add(last))
            == Some(output.len()),
        "Object Mask GDeflate output size differs from crop"
    );
    // tiles is a u16, so the offset table cannot overflow usize on supported targets.
    let data_start = HEADER_BYTES + tiles * 4;
    let offsets = payload
        .get(HEADER_BYTES..data_start)
        .ok_or_else(|| unsupported("truncated Object Mask GDeflate offsets"))?;
    let data = &payload[data_start..];
    let offset = |index: usize| -> usize {
        u32::from_le_bytes(
            offsets[index * 4..index * 4 + 4]
                .try_into()
                .expect("bounded tile offset"),
        ) as usize
    };
    // Offset zero holds the final tile's compressed length, not its start.
    let final_start = if tiles == 1 { 0 } else { offset(tiles - 1) };
    ensure!(
        final_start.checked_add(offset(0)) == Some(data.len()) && offset(0) > 0,
        "Object Mask GDeflate final tile does not end at payload boundary"
    );
    let mut decoder = Decompressor::new()?;
    for (index, destination) in output.chunks_mut(TILE_BYTES).enumerate() {
        let start = if index == 0 { 0 } else { offset(index) };
        let end = if index + 1 == tiles {
            data.len()
        } else {
            offset(index + 1)
        };
        ensure!(
            start < end,
            "Object Mask GDeflate tile offsets are not increasing"
        );
        let input = data
            .get(start..end)
            .ok_or_else(|| unsupported("Object Mask GDeflate tile is outside payload"))?;
        let length = input
            .len()
            .checked_add(read_slack(destination.len()).expect("tile output bound"))
            .ok_or_else(|| unsupported("Object Mask padded tile length overflows"))?;
        let padded = PaddedTile::new(input, super::zeroed(length)?, destination.len());
        decoder.tile(&padded, destination)?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod guarded_tests {
    use super::*;

    /// Test-only allocation ending at a PROT_NONE page, using the host page size.
    struct Guarded {
        mapping: *mut libc::c_void,
        mapped_len: usize,
        start: *mut u8,
        len: usize,
    }
    impl Guarded {
        fn new(len: usize) -> Self {
            // SAFETY: sysconf has no pointer arguments.
            let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).unwrap();
            let mapped_len = (len.div_ceil(page) + 1) * page;
            // SAFETY: anonymous private mapping, no file or pre-existing address.
            let mapping = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    mapped_len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANON,
                    -1,
                    0,
                )
            };
            assert_ne!(mapping, libc::MAP_FAILED);
            // SAFETY: guard is page-aligned and entirely within this mapping.
            let guard = unsafe { mapping.cast::<u8>().add(mapped_len - page) };
            assert_eq!(
                unsafe { libc::mprotect(guard.cast(), page, libc::PROT_NONE) },
                0
            );
            // SAFETY: len bytes preceding the guard belong to the readable mapping.
            let start = unsafe { guard.sub(len) };
            Self {
                mapping,
                mapped_len,
                start,
                len,
            }
        }
    }
    impl AsRef<[u8]> for Guarded {
        fn as_ref(&self) -> &[u8] {
            // SAFETY: readable, initialized anonymous memory; bounded before guard.
            unsafe { std::slice::from_raw_parts(self.start, self.len) }
        }
    }
    impl AsMut<[u8]> for Guarded {
        fn as_mut(&mut self) -> &mut [u8] {
            // SAFETY: exclusive borrow of owned writable bytes before the guard.
            unsafe { std::slice::from_raw_parts_mut(self.start, self.len) }
        }
    }
    impl Drop for Guarded {
        fn drop(&mut self) {
            // SAFETY: this object owns the exact mapping and releases it once.
            assert_eq!(unsafe { libc::munmap(self.mapping, self.mapped_len) }, 0);
        }
    }

    #[test]
    fn object_mask_zero_tail_bounds_actual_c_reads_and_writes() {
        let initial = include_bytes!("../../../tests/fixtures/object_mask/initial.prmf");
        let raster =
            super::super::Raster::decode_index(initial, [1280, 720], 8_511_237_907, 1).unwrap();
        let payload = raster.frames[0].payload;
        let tiles = usize::from(u16::from_le_bytes(payload[2..4].try_into().unwrap()));
        let offset = |i: usize| {
            u32::from_le_bytes(payload[8 + i * 4..12 + i * 4].try_into().unwrap()) as usize
        };
        let data = &payload[8 + tiles * 4..];
        let tile0 = &data[..offset(1)];
        let tile1 = &data[offset(1)..offset(2)];
        let mut cases = vec![
            vec![0; 4],
            vec![0; 1],
            tile1[..129].to_vec(),
            tile0[..1].to_vec(),
            tile0[..16].to_vec(),
            tile0[..160].to_vec(),
        ];
        // Fixed invalid-Huffman/random bytes exercise table construction, not
        // only the short startup read. Results may be BAD_DATA or short output.
        let mut seed = 0x93a4_717b_u32;
        for length in (1..=16).map(|n| n * 19) {
            cases.push(
                (0..length)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        seed as u8
                    })
                    .collect(),
            );
        }
        for size in [65_536, 8_362] {
            for input in &cases {
                let padded = PaddedTile::new(
                    input,
                    Guarded::new(input.len() + read_slack(size).unwrap()),
                    size,
                );
                let mut output = Guarded::new(size);
                assert!(Decompressor::new()
                    .unwrap()
                    .tile(&padded, output.as_mut())
                    .is_err());
            }
        }
    }
}

#[cfg(test)]
mod initialization_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Barrier,
    };

    #[test]
    fn object_mask_gdeflate_first_call_publishes_once_without_replay() {
        // A fresh gate on every run: independent of test order or platform C
        // dispatch selection. Relaxed state is published by Once's happens-before.
        let once = Once::new();
        let barrier = Barrier::new(16);
        let calls = AtomicUsize::new(0);
        let initialized = AtomicBool::new(false);
        std::thread::scope(|scope| {
            for id in 0..16 {
                let (once, barrier, calls, initialized) = (&once, &barrier, &calls, &initialized);
                scope.spawn(move || {
                    barrier.wait();
                    let result = initialized_call(once, || {
                        if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                            initialized.store(true, Ordering::Relaxed);
                        } else {
                            assert!(initialized.load(Ordering::Relaxed));
                        }
                        id
                    });
                    assert_eq!(result, id);
                });
            }
        });
        assert_eq!(calls.load(Ordering::Relaxed), 16);
    }
}
