//! The flight path on an emulated Cortex-M7.
//!
//! `examples/embedded` shows that the tracker links without an operating
//! system. This image goes one step further and runs: it starts on the bare
//! processor that QEMU models as the Arm MPS2 board (AN500), solves two
//! frames kept in memory, the first by a search and the second by
//! tracking, and reports through the debugger's semihosting
//! channel
//!
//! - whether each quaternion equals, bit for bit, the one the build machine
//!   computed for the same frame,
//! - how many instructions each frame took,
//! - how much heap and stack were in use at the most.
//!
//! `node run.mjs` builds it, runs it and prints the table. An emulator
//! counts instructions; it does not model caches, memory wait states or a
//! real clock, so none of this is a timing on hardware.

#![no_std]
#![no_main]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use core::cell::UnsafeCell;
use core::fmt::{self, Write};
use core::panic::PanicInfo;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use asterope_core::camera::Camera;
use asterope_core::flight::StarTracker;
use asterope_core::math::DEG;
use asterope_core::track::TrackEvent;

include!(concat!(env!("OUT_DIR"), "/expected.rs"));

static CATALOG: &[u8] = include_bytes!("../../../data/flight-14deg.astc");
static INDEX: &[u8] = include_bytes!("../../../data/flight-14deg.asti");

const WIDTH: usize = 1024;
const HEIGHT: usize = 1024;
const FRAME_BYTES: usize = WIDTH * HEIGHT * 2;

/// 16-bit pixels on a 4-byte boundary.
#[repr(C, align(4))]
struct Pixels([u8; FRAME_BYTES]);

/// One frame beside the code, one loaded into RAM, where a camera would
/// deliver it: together they fill what the board has.
#[link_section = ".frames"]
#[used]
static FIRST_FRAME: Pixels = Pixels(*include_bytes!(concat!(env!("OUT_DIR"), "/frame0.bin")));
#[link_section = ".ramframes"]
#[used]
static SECOND_FRAME: Pixels = Pixels(*include_bytes!(concat!(env!("OUT_DIR"), "/frame1.bin")));

/// The decoded catalog, 426 kB, and everything one frame allocates: the
/// arena gives nothing back before the frame is done, so it has to hold
/// the sum, about 1 MB.
const HEAP_BYTES: usize = 1792 << 10;

/// The frame arena of `examples/embedded`, with the highest level kept.
struct Heap {
    memory: UnsafeCell<[u8; HEAP_BYTES]>,
    used: AtomicUsize,
    floor: AtomicUsize,
    peak: AtomicUsize,
}

// SAFETY: one thread of execution; the only interrupt does not allocate.
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
        self.peak.fetch_max(end, Ordering::Relaxed);
        // SAFETY: `start..end` lies inside the heap array and is handed out once.
        unsafe { self.memory.get().cast::<u8>().add(start) }
    }

    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}

#[global_allocator]
static HEAP: Heap = Heap {
    memory: UnsafeCell::new([0; HEAP_BYTES]),
    used: AtomicUsize::new(0),
    floor: AtomicUsize::new(0),
    peak: AtomicUsize::new(0),
};

extern "C" {
    static __stack_top: u32;
    static __stack_limit: u32;
    static mut __data_start: u32;
    static mut __data_end: u32;
    static __data_load: u32;
    static mut __bss_start: u32;
    static mut __bss_end: u32;
}

#[repr(C)]
union Vector {
    handler: unsafe extern "C" fn(),
    address: *const u32,
    reserved: usize,
}

// SAFETY: the table is read by the processor only.
unsafe impl Sync for Vector {}

unsafe extern "C" fn fault() {
    semihost(0x04, c"fault\n".as_ptr() as usize);
    exit(1)
}

/// Initial stack pointer, reset and exception handlers, as the processor
/// expects them at address zero.
#[link_section = ".vectors"]
#[used]
static VECTORS: [Vector; 16] = [
    Vector {
        address: addr_of!(__stack_top),
    },
    Vector { handler: Reset },
    Vector { handler: fault },
    Vector { handler: fault },
    Vector { handler: fault },
    Vector { handler: fault },
    Vector { handler: fault },
    Vector { reserved: 0 },
    Vector { reserved: 0 },
    Vector { reserved: 0 },
    Vector { reserved: 0 },
    Vector { handler: fault },
    Vector { handler: fault },
    Vector { reserved: 0 },
    Vector { handler: fault },
    Vector { handler: SysTick },
];

const SYSTICK_CONTROL: *mut u32 = 0xE000_E010 as *mut u32;
const SYSTICK_RELOAD: *mut u32 = 0xE000_E014 as *mut u32;
const SYSTICK_VALUE: *mut u32 = 0xE000_E018 as *mut u32;
const COPROCESSOR_ACCESS: *mut u32 = 0xE000_ED88 as *mut u32;
const SYSTICK_SPAN: u32 = 0x00FF_FFFF;
const STACK_PAINT: u32 = 0xA57E_209E;

static WRAPS: AtomicU32 = AtomicU32::new(0);

#[no_mangle]
unsafe extern "C" fn SysTick() {
    WRAPS.fetch_add(1, Ordering::Relaxed);
}

/// Processor clock ticks since `restart_clock`.
fn ticks() -> u64 {
    loop {
        let before = WRAPS.load(Ordering::Relaxed);
        // SAFETY: a read of a memory-mapped timer register.
        let value = unsafe { read_volatile(SYSTICK_VALUE) };
        if WRAPS.load(Ordering::Relaxed) == before {
            return u64::from(before) * u64::from(SYSTICK_SPAN + 1)
                + u64::from(SYSTICK_SPAN - value);
        }
    }
}

fn restart_clock() {
    // SAFETY: writes to the timer's own registers.
    unsafe {
        write_volatile(SYSTICK_CONTROL, 0);
        write_volatile(SYSTICK_RELOAD, SYSTICK_SPAN);
        write_volatile(SYSTICK_VALUE, 0);
        WRAPS.store(0, Ordering::Relaxed);
        write_volatile(SYSTICK_CONTROL, 0b111);
    }
}

/// A semihosting request to the debugger (here, the emulator).
fn semihost(operation: usize, argument: usize) -> usize {
    let result: usize;
    // SAFETY: the breakpoint is taken by the emulator, which reads r0 and r1
    // and returns in r0.
    unsafe {
        asm!("bkpt #0xab", inout("r0") operation => result, in("r1") argument);
    }
    result
}

fn exit(code: usize) -> ! {
    let block = [0x20026usize, code];
    semihost(0x20, block.as_ptr() as usize);
    loop {}
}

struct Console;

impl Write for Console {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let mut buffer = [0u8; 96];
        for chunk in text.as_bytes().chunks(buffer.len() - 1) {
            buffer[..chunk.len()].copy_from_slice(chunk);
            buffer[chunk.len()] = 0;
            semihost(0x04, buffer.as_ptr() as usize);
        }
        Ok(())
    }
}

/// A loop of exactly three instructions a turn, to learn how many
/// instructions the emulator runs to a clock tick.
#[inline(never)]
fn calibration_loop(turns: u32) {
    // SAFETY: the loop only counts a register down.
    unsafe {
        asm!("2:", "subs {n}, {n}, #1", "nop", "bne 2b", n = inout(reg) turns => _);
    }
}

fn stack_in_use() -> usize {
    // SAFETY: the stack region is ours, and painted at reset.
    unsafe {
        let mut word = addr_of!(__stack_limit);
        let top = addr_of!(__stack_top);
        while word < top && read_volatile(word) == STACK_PAINT {
            word = word.add(1);
        }
        top as usize - word as usize
    }
}

fn frame(index: usize) -> &'static [u16] {
    let pixels = if index == 0 {
        &FIRST_FRAME
    } else {
        &SECOND_FRAME
    };
    // SAFETY: little-endian 16-bit pixels in a static aligned to four bytes.
    unsafe { core::slice::from_raw_parts(pixels.0.as_ptr().cast::<u16>(), WIDTH * HEIGHT) }
}

fn run() -> Result<bool, fmt::Error> {
    let mut out = Console;
    restart_clock();
    calibration_loop(3_000_000);
    let per_tick = 9_000_000.0 / ticks() as f64;
    writeln!(out, "instructions per clock tick: {per_tick:.3}")?;

    let Ok(mut tracker) = Camera::from_fov(WIDTH as u32, HEIGHT as u32, 14.0 * DEG)
        .and_then(|camera| StarTracker::from_bytes(camera, CATALOG, INDEX))
    else {
        writeln!(out, "the tracker could not be set up")?;
        return Ok(false);
    };
    HEAP.floor
        .store(HEAP.used.load(Ordering::Relaxed), Ordering::Relaxed);
    writeln!(
        out,
        "heap after start-up: {} bytes",
        HEAP.floor.load(Ordering::Relaxed)
    )?;

    let mut all_equal = true;
    for (index, expected) in EXPECTED.iter().enumerate() {
        HEAP.peak
            .store(HEAP.floor.load(Ordering::Relaxed), Ordering::Relaxed);
        restart_clock();
        let fix = tracker.update(frame(index), FRAME_PERIOD);
        let instructions = ticks() as f64 * per_tick;
        HEAP.used
            .store(HEAP.floor.load(Ordering::Relaxed), Ordering::Relaxed);
        let (event, bits) = match fix {
            Ok(fix) => (
                match fix.event {
                    TrackEvent::Acquired => "acquired",
                    TrackEvent::Tracked => "tracked",
                    TrackEvent::Coasting => "coasting",
                    TrackEvent::Lost => "lost",
                },
                fix.attitude()
                    .map(|q| [q.x.to_bits(), q.y.to_bits(), q.z.to_bits(), q.w.to_bits()]),
            ),
            Err(_) => ("error", None),
        };
        let equal = bits == Some(*expected);
        all_equal &= equal;
        writeln!(
            out,
            "frame {index}: {event}, bits equal: {equal}, instructions: {instructions:.0}, heap: {} bytes, stack: {} bytes",
            HEAP.peak.load(Ordering::Relaxed) - HEAP.floor.load(Ordering::Relaxed),
            stack_in_use()
        )?;
    }
    Ok(all_equal)
}

/// Where the processor starts: enable the floating-point unit, set up the
/// memory a Rust program expects, paint the stack, and go.
#[no_mangle]
unsafe extern "C" fn Reset() {
    // SAFETY: start-up code, the only thing running.
    unsafe {
        write_volatile(
            COPROCESSOR_ACCESS,
            read_volatile(COPROCESSOR_ACCESS) | (0xF << 20),
        );
        asm!("dsb", "isb");
        let mut source = addr_of!(__data_load);
        let mut target = addr_of_mut!(__data_start);
        while target < addr_of_mut!(__data_end) {
            write_volatile(target, read_volatile(source));
            target = target.add(1);
            source = source.add(1);
        }
        let mut word = addr_of_mut!(__bss_start);
        while word < addr_of_mut!(__bss_end) {
            write_volatile(word, 0);
            word = word.add(1);
        }
        let here: u32 = 0;
        let mut word = addr_of!(__stack_limit) as *mut u32;
        while (word as usize) + 256 < addr_of!(here) as usize {
            write_volatile(word, STACK_PAINT);
            word = word.add(1);
        }
    }
    main()
}

#[inline(never)]
fn main() -> ! {
    match run() {
        Ok(true) => exit(0),
        _ => exit(1),
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    semihost(0x04, c"panic\n".as_ptr() as usize);
    exit(1)
}
