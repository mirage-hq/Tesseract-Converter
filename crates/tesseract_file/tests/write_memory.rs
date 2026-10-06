//! A separate test binary keeps allocator observations isolated from other tests.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use tesseract_file::TesseractFileBuilder;

const LARGE_BUFFER: usize = 4 * 1024 * 1024;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

fn allocated(size: usize) {
    if size >= LARGE_BUFFER {
        let live = LIVE.fetch_add(1, Ordering::SeqCst) + 1;
        PEAK.fetch_max(live, Ordering::SeqCst);
    }
}

fn freed(size: usize) {
    if size >= LARGE_BUFFER {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

// SAFETY: Every request is forwarded unchanged to System. Tracking uses only
// atomics, never allocates, and records only successful allocations/reallocations.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        freed(layout.size());
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, size) };
        if !replacement.is_null() {
            freed(layout.size());
            allocated(size);
        }
        replacement
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn write_releases_serialized_project_before_reopening() {
    // Whitespace makes the wire buffer large without introducing a second large
    // parsed field. The archive must preserve it byte-for-byte, not normalize it.
    let json = br#"{"$schema":"https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json","formatVersion":1,"dimensions":{"width":1080,"height":1920},"duration":6,"composition":{"id":"memory-test","name":"Test","layers":[]}}"#;
    let mut bytes = json.to_vec();
    bytes.resize(LARGE_BUFFER + 137, b' ');
    let builder = TesseractFileBuilder::from_project_json(&bytes).unwrap();
    let byte_length = bytes.len();
    drop(bytes);
    let directory = tempfile::tempdir().unwrap();
    assert_eq!(LIVE.load(Ordering::SeqCst), 1);
    PEAK.store(1, Ordering::SeqCst);

    let file = builder
        .write(directory.path().join("project.tsrct"))
        .unwrap();
    let peak = PEAK.load(Ordering::SeqCst);
    assert_eq!(file.project_json_bytes().len(), byte_length);
    assert_eq!(&file.project_json_bytes()[..json.len()], json);
    assert!(file.project_json_bytes()[json.len()..]
        .iter()
        .all(|byte| *byte == b' '));
    assert_eq!(
        peak, 1,
        "serialized and reopened project buffers overlapped"
    );
    drop(file);
    assert_eq!(LIVE.load(Ordering::SeqCst), 0);
}
