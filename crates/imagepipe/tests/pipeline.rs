use imagepipe::{Error, Format, LimitError, Limits, derive, inspect};

/// Gradient test image so resizing has something to chew on.
fn rgb(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            v.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 128]);
        }
    }
    v
}

fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 90)
        .encode(&rgb(w, h), w as u16, h as u16, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    out
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::write_buffer_with_format(
        &mut std::io::Cursor::new(&mut out),
        &rgb(w, h),
        w,
        h,
        image::ExtendedColorType::Rgb8,
        image::ImageFormat::Png,
    )
    .unwrap();
    out
}

/// Splice an APP1 EXIF segment (orientation + DateTimeOriginal + a GPS-ish
/// marker string) right after the SOI marker.
fn with_exif(jpeg: &[u8], orientation: u16, date: &str) -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II*\0");
    tiff.extend_from_slice(&8u32.to_le_bytes());
    // IFD0: two entries.
    tiff.extend_from_slice(&2u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&[orientation as u8, (orientation >> 8) as u8, 0, 0]);
    tiff.extend_from_slice(&0x8769u16.to_le_bytes()); // ExifIFD pointer
    tiff.extend_from_slice(&4u16.to_le_bytes()); // LONG
    tiff.extend_from_slice(&1u32.to_le_bytes());
    let exif_ifd = 8 + 2 + 2 * 12 + 4;
    tiff.extend_from_slice(&(exif_ifd as u32).to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes()); // next IFD
    // Exif IFD: one entry, data placed right after it.
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x9003u16.to_le_bytes()); // DateTimeOriginal
    tiff.extend_from_slice(&2u16.to_le_bytes()); // ASCII
    tiff.extend_from_slice(&20u32.to_le_bytes());
    tiff.extend_from_slice(&((exif_ifd + 2 + 12 + 4) as u32).to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(date.as_bytes());
    tiff.push(0);

    let mut seg = b"Exif\0\0".to_vec();
    seg.extend_from_slice(&tiff);
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&seg);
    out.extend_from_slice(&jpeg[2..]);
    out
}

fn dims(jpeg: &[u8]) -> (u32, u32) {
    let d = imagepipe::decoder_for(Format::Jpeg)
        .unwrap()
        .dimensions(jpeg)
        .unwrap();
    (d.width, d.height)
}

#[test]
fn jpeg_inspect_and_derive() {
    let bytes = jpeg(3000, 2000);
    let info = inspect(&bytes, &Limits::default()).unwrap();
    assert_eq!(
        (info.format, info.width, info.height),
        (Format::Jpeg, 3000, 2000)
    );
    assert_eq!((info.orientation, info.taken_at.as_deref()), (1, None));

    let ds = derive(&bytes, &info, &Limits::default(), 0).unwrap();
    let got: Vec<_> = ds.iter().map(|d| (d.variant, dims(&d.bytes))).collect();
    assert_eq!(
        got,
        vec![
            ("display", (2560, 1707)),
            ("preview", (1280, 854)),
            ("thumb", (400, 267)),
            ("blur", (64, 43))
        ]
    );
}

#[test]
fn small_images_are_not_upscaled() {
    let bytes = jpeg(300, 200);
    let info = inspect(&bytes, &Limits::default()).unwrap();
    let ds = derive(&bytes, &info, &Limits::default(), 0).unwrap();
    assert_eq!(dims(&ds[0].bytes), (300, 200));
    assert_eq!(dims(&ds[2].bytes), (300, 200)); // thumb: native, not 400 wide
    assert_eq!(dims(&ds[3].bytes), (64, 43));
}

#[test]
fn exif_orientation_and_date_are_read_and_applied() {
    let bytes = with_exif(&jpeg(600, 400), 6, "2024:05:01 10:20:30");
    let info = inspect(&bytes, &Limits::default()).unwrap();
    assert_eq!(info.orientation, 6);
    assert_eq!(info.taken_at.as_deref(), Some("2024-05-01T10:20:30Z"));
    // Header dimensions are the stored ones; derivatives come out upright.
    assert_eq!((info.width, info.height), (600, 400));
    let ds = derive(&bytes, &info, &Limits::default(), 0).unwrap();
    assert_eq!(dims(&ds[0].bytes), (400, 600));
}

#[test]
fn derivatives_carry_no_metadata() {
    let bytes = with_exif(&jpeg(600, 400), 1, "2024:05:01 10:20:30");
    let info = inspect(&bytes, &Limits::default()).unwrap();
    for d in derive(&bytes, &info, &Limits::default(), 0).unwrap() {
        assert!(
            !d.bytes.windows(4).any(|w| w == b"Exif"),
            "{} kept EXIF",
            d.variant
        );
        assert_eq!(
            imagepipe::metadata::from_container(&d.bytes),
            Default::default()
        );
    }
}

#[test]
fn png_round_trip() {
    let bytes = png(500, 300);
    let info = inspect(&bytes, &Limits::default()).unwrap();
    assert_eq!(
        (info.format, info.width, info.height),
        (Format::Png, 500, 300)
    );
    let ds = derive(&bytes, &info, &Limits::default(), 0).unwrap();
    assert_eq!(ds.len(), 4);
    assert_eq!(dims(&ds[0].bytes), (500, 300));
}

#[test]
fn limits_reject_before_decoding() {
    let bytes = jpeg(1000, 1000);
    let tight = Limits {
        max_pixels: 500_000,
        ..Limits::default()
    };
    assert!(matches!(
        inspect(&bytes, &tight),
        Err(Error::Limit(LimitError::TooManyPixels { .. }))
    ));
    // And `derive` enforces it independently of `inspect`.
    let info = inspect(&bytes, &Limits::default()).unwrap();
    assert!(matches!(
        derive(&bytes, &info, &tight, 0),
        Err(Error::Limit(_))
    ));
}

#[test]
fn junk_and_unsupported_are_rejected() {
    let l = Limits::default();
    assert!(matches!(
        inspect(b"GIF89a....", &l),
        Err(Error::UnsupportedFormat)
    ));
    assert!(matches!(inspect(&[], &l), Err(Error::UnsupportedFormat)));
    // Right magic bytes, garbage body: a decode error, not a panic.
    let mut fake = vec![0xFF, 0xD8, 0xFF, 0xE0];
    fake.extend_from_slice(&[0u8; 64]);
    assert!(inspect(&fake, &l).is_err());
    // AVIF is sniffed but has no decoder yet.
    assert!(matches!(
        inspect(b"\0\0\0\x18ftypavif\0\0\0\0\0\0\0\0", &l),
        Err(Error::UnsupportedFormat)
    ));
}

#[test]
fn truncated_jpeg_does_not_panic() {
    let bytes = jpeg(400, 300);
    let l = Limits::default();
    for cut in [bytes.len() / 2, bytes.len() - 10, 30] {
        let _ = inspect(&bytes[..cut], &l).and_then(|i| derive(&bytes[..cut], &i, &l, 0));
    }
}

/// Seeded byte-level mutation of valid files. Not a substitute for `cargo fuzz`
/// (which runs weekly in CI), but it catches panics on every `cargo test`.
#[test]
fn mutated_inputs_never_panic() {
    let limits = Limits {
        max_pixels: 4_000_000,
        max_bytes: 16_000_000,
    };
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let sources = [
        with_exif(&jpeg(160, 120), 6, "2024:05:01 10:20:30"),
        png(120, 90),
    ];
    let mut decoded_ok = 0;
    for src in &sources {
        for _ in 0..150 {
            let mut b = src.clone();
            match rand() % 4 {
                0 => {
                    for _ in 0..(1 + rand() % 8) {
                        let i = (rand() as usize) % b.len();
                        b[i] ^= 1 << (rand() % 8);
                    }
                }
                1 => b.truncate((rand() as usize) % b.len()),
                2 => {
                    let i = (rand() as usize) % b.len();
                    let n = (rand() as usize) % 64;
                    b.splice(i..i, std::iter::repeat_n(0xFFu8, n));
                }
                _ => {
                    let (i, j) = ((rand() as usize) % b.len(), (rand() as usize) % b.len());
                    b.swap(i, j);
                }
            }
            if let Ok(info) = inspect(&b, &limits)
                && derive(&b, &info, &limits, 0).is_ok()
            {
                decoded_ok += 1;
            }
        }
    }
    // Some mutations are harmless; the point is that none of them panicked.
    assert!(decoded_ok > 0);
}

/// A header that lies about its size must be rejected by the limit check, not
/// by an allocation failure. (JPEG's frame header has no checksum, unlike PNG's
/// IHDR, so an edited size reaches the limit check.)
#[test]
fn oversized_declared_dimensions_are_rejected_up_front() {
    let mut j = jpeg(16, 16);
    let sof = j
        .windows(2)
        .position(|w| w == [0xFF, 0xC0])
        .expect("baseline SOF0 marker");
    // SOF0 layout: marker(2) length(2) precision(1) height(2) width(2).
    j[sof + 5..sof + 7].copy_from_slice(&16_000u16.to_be_bytes());
    j[sof + 7..sof + 9].copy_from_slice(&16_000u16.to_be_bytes());
    let l = Limits::default();
    assert!(matches!(
        inspect(&j, &l),
        Err(Error::Limit(LimitError::TooManyPixels { .. }))
    ));
    // The decode entry point enforces it independently.
    let info = imagepipe::Info {
        format: Format::Jpeg,
        width: 16,
        height: 16,
        orientation: 1,
        taken_at: None,
    };
    assert!(matches!(derive(&j, &info, &l, 0), Err(Error::Limit(_))));
}

/// Big originals are expected. A 48 MP header must pass `inspect` under the
/// default limits, while an input over `max_bytes` is refused before parsing.
#[test]
fn large_originals_are_accepted_and_oversized_input_is_not() {
    let mut j = jpeg(16, 16);
    let sof = j
        .windows(2)
        .position(|w| w == [0xFF, 0xC0])
        .expect("baseline SOF0 marker");
    j[sof + 5..sof + 7].copy_from_slice(&6048u16.to_be_bytes()); // height
    j[sof + 7..sof + 9].copy_from_slice(&8064u16.to_be_bytes()); // width
    let info = inspect(&j, &Limits::default()).expect("48 MP is within the defaults");
    assert_eq!((info.width, info.height), (8064, 6048));

    let small = Limits {
        max_bytes: 100,
        ..Limits::default()
    };
    assert!(matches!(
        inspect(&jpeg(400, 300), &small),
        Err(Error::Limit(LimitError::InputTooLarge { .. }))
    ));
}

/// Real 48 MP end-to-end run for timing and memory:
/// `cargo test --release -p imagepipe --test pipeline big_48mp -- --ignored --nocapture`
#[test]
#[ignore]
fn big_48mp_derives_within_limits() {
    let (w, h) = (8064u32, 6048u32);
    let bytes = jpeg(w, h);
    let t = std::time::Instant::now();
    let l = Limits::default();
    let info = inspect(&bytes, &l).unwrap();
    let ds = derive(&bytes, &info, &l, 0).unwrap();
    eprintln!(
        "input {} MB, derived {} variants in {:?}",
        bytes.len() / 1_000_000,
        ds.len(),
        t.elapsed()
    );
    assert_eq!(dims(&ds[0].bytes), (2560, 1920));
}

#[test]
fn curated_rotation_turns_the_upright_image() {
    let l = Limits::default();
    let bytes = jpeg(600, 400);
    let info = inspect(&bytes, &l).unwrap();
    let d = |rot| derive(&bytes, &info, &l, rot).unwrap();
    assert_eq!(dims(&d(0)[0].bytes), (600, 400));
    assert_eq!(dims(&d(90)[0].bytes), (400, 600));
    assert_eq!(dims(&d(180)[0].bytes), (600, 400));
    assert_eq!(dims(&d(270)[0].bytes), (400, 600));
    assert_eq!(dims(&d(90)[2].bytes), (267, 400)); // thumb follows the turned shape
    assert!(matches!(
        derive(&bytes, &info, &l, 45),
        Err(Error::Decode(_))
    ));
}

/// The correction applies on top of EXIF orientation: a file EXIF says to turn
/// 90 degrees, plus a curated 90, lands at 180 (landscape again).
#[test]
fn curated_rotation_composes_with_exif_orientation() {
    let l = Limits::default();
    let bytes = with_exif(&jpeg(600, 400), 6, "2024:05:01 10:20:30");
    let info = inspect(&bytes, &l).unwrap();
    assert_eq!(
        dims(&derive(&bytes, &info, &l, 0).unwrap()[0].bytes),
        (400, 600)
    );
    assert_eq!(
        dims(&derive(&bytes, &info, &l, 90).unwrap()[0].bytes),
        (600, 400)
    );
    assert_eq!(
        dims(&derive(&bytes, &info, &l, 270).unwrap()[0].bytes),
        (600, 400)
    );
}
