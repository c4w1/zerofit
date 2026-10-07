//! Heap allocations per decode of each fixture, measured with a counting
//! global allocator. Prints a Markdown table.
//!
//! ```sh
//! cargo bench -p zerofit-bench --bench alloc_count
//! ```

#![allow(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};

use zerofit_bench as w;

/// Counts allocations and allocated bytes, delegating to the system
/// allocator.
struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: every method forwards its arguments unchanged to `System`, which
// upholds the `GlobalAlloc` contract; counting does not affect allocation.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: the caller's contract is forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract is forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        // SAFETY: the caller's contract is forwarded unchanged.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn measure(f: &dyn Fn() -> f64) -> (u64, u64) {
    let allocs = ALLOCS.load(Ordering::Relaxed);
    let bytes = BYTES.load(Ordering::Relaxed);
    black_box(f());
    (
        ALLOCS.load(Ordering::Relaxed) - allocs,
        BYTES.load(Ordering::Relaxed) - bytes,
    )
}

fn main() {
    println!("| Fixture | Messages | Workload | Allocations | Bytes allocated |");
    println!("|---|---:|---|---:|---:|");
    for f in w::fixtures() {
        let bytes = &f.bytes;
        let workloads: [(&str, &dyn Fn() -> f64); 5] = [
            ("zerofit_records", &|| w::records(bytes) as f64),
            ("zerofit_all_fields", &|| w::all_fields(bytes)),
            ("zerofit_profile_fields", &|| w::profile_fields(bytes)),
            ("zerofit_stream", &|| w::stream(bytes) as f64),
            ("fitparser", &|| w::fitparser(bytes) as f64),
        ];
        for (label, workload) in workloads {
            let (allocs, alloc_bytes) = measure(workload);
            println!(
                "| {} | {} | {label} | {allocs} | {alloc_bytes} |",
                f.name, f.messages
            );
        }
    }
}
