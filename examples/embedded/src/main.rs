//! A bare-metal star tracker image: catalog and pair index in read-only
//! memory, one entry point per frame.
//!
//! It exists to prove that the flight path builds without an operating
//! system and to measure what it occupies. It is linked without a board
//! support crate, so it has no start-up code and no memory map; a real
//! application calls `star_tracker_step` from its own main loop.
//!
//! `cargo build --release` in this directory, then
//! `node size.mjs target/thumbv7em-none-eabihf/release/asterope-embedded`.

#![no_std]
#![no_main]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

use asterope_core::camera::Camera;
use asterope_core::flight::StarTracker;
use asterope_core::math::DEG;

static CATALOG: &[u8] = include_bytes!("../../../data/flight-14deg.astc");
static INDEX: &[u8] = include_bytes!("../../../data/flight-14deg.asti");

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 1024;
const FOV_DEG: f64 = 14.0;
const HEAP_BYTES: usize = 2 << 20;

/// A frame arena: memory is handed out in order and never returned one
/// block at a time. Everything allocated while a frame is processed is
/// released together when the frame is done, which is sound here because
/// the tracker keeps nothing on the heap between frames except the catalog
/// it decoded at start-up. An RTOS heap or a crate such as `embedded-alloc`
/// works as well and needs less memory (see the budget table in
/// `docs/integration.md`).
struct Heap {
    memory: UnsafeCell<[u8; HEAP_BYTES]>,
    used: AtomicUsize,
    /// Level after start-up, to which each frame returns.
    floor: AtomicUsize,
}

// SAFETY: the example runs on one core without interrupts touching the heap.
unsafe impl Sync for Heap {}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let start = self
            .used
            .load(Ordering::Relaxed)
            .next_multiple_of(layout.align());
        let end = start.saturating_add(layout.size());
        if end > HEAP_BYTES {
            return core::ptr::null_mut();
        }
        self.used.store(end, Ordering::Relaxed);
        // SAFETY: `start..end` lies inside the heap array and is handed out once.
        unsafe { self.memory.get().cast::<u8>().add(start) }
    }

    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}

impl Heap {
    fn keep_everything_so_far(&self) {
        self.floor
            .store(self.used.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    fn release_frame(&self) {
        self.used
            .store(self.floor.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

#[global_allocator]
static HEAP: Heap = Heap {
    memory: UnsafeCell::new([0; HEAP_BYTES]),
    used: AtomicUsize::new(0),
    floor: AtomicUsize::new(0),
};

struct Slot(UnsafeCell<Option<StarTracker<'static>>>);

// SAFETY: as for `Heap`, a single thread of execution uses the tracker.
unsafe impl Sync for Slot {}

static TRACKER: Slot = Slot(UnsafeCell::new(None));

/// Solves the frame taken `dt` seconds after the previous one and writes the
/// attitude quaternion (ICRS to sensor, scalar last) to `q`.
///
/// Returns 1 with a solution, 0 without one, and -1 when the tracker could
/// not be set up or the frame has the wrong size.
///
/// # Safety
///
/// `pixels` must point to `count` readable values and `q` to four writable
/// ones; calls must not overlap.
#[no_mangle]
pub unsafe extern "C" fn star_tracker_step(
    pixels: *const u16,
    count: usize,
    dt: f64,
    q: *mut f64,
) -> i32 {
    // SAFETY: the caller guarantees exclusive, single-threaded use.
    let slot = unsafe { &mut *TRACKER.0.get() };
    if slot.is_none() {
        *slot = Camera::from_fov(WIDTH, HEIGHT, FOV_DEG * DEG)
            .and_then(|camera| StarTracker::from_bytes(camera, CATALOG, INDEX))
            .ok();
        HEAP.keep_everything_so_far();
    }
    let Some(tracker) = slot.as_mut() else {
        return -1;
    };
    // SAFETY: the caller guarantees `count` readable pixels.
    let frame = unsafe { core::slice::from_raw_parts(pixels, count) };
    let attitude = tracker.update(frame, dt).map(|fix| fix.attitude());
    HEAP.release_frame();
    match attitude {
        Ok(Some(attitude)) => {
            // SAFETY: the caller guarantees four writable values.
            unsafe {
                q.cast::<[f64; 4]>()
                    .write([attitude.x, attitude.y, attitude.z, attitude.w])
            };
            1
        }
        Ok(None) => 0,
        Err(_) => -1,
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}
