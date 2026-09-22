#![no_main]
// Header parsing, EXIF and limit checks on arbitrary bytes. Must never panic
// and must reject oversized declarations before allocating.
use imagepipe::{Limits, inspect};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = inspect(data, &Limits::default());
});
