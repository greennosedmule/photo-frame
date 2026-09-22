#![no_main]
// The full decode-orient-resize-encode path on arbitrary bytes. Tight limits
// keep each run fast and prove the limit checks hold before allocation.
use imagepipe::{Limits, derive, inspect};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = Limits { max_pixels: 1_000_000, max_bytes: 3_000_000 };
    if let Ok(info) = inspect(data, &limits) {
        // Cover every rotation, chosen from the input so the corpus explores them.
        let rotation = i32::from(data.first().copied().unwrap_or(0) % 4) * 90;
        let _ = derive(data, &info, &limits, rotation);
    }
});
