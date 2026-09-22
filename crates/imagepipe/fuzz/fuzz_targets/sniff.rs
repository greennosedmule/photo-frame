#![no_main]
// Magic-byte sniffing. Decoder-level targets are `inspect` and `derive`.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = imagepipe::sniff(data);
});
