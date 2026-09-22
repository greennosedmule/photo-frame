//! Derivative generation: resize in linear light with Lanczos3, never upscale,
//! encode without metadata (so GPS and everything else is stripped).

use std::sync::OnceLock;

use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

use crate::{Decoded, Error};

#[derive(Debug, Clone, Copy)]
pub struct Variant {
    pub name: &'static str,
    pub long_edge: u32,
    pub jpeg_quality: u8,
}

/// Largest first: each smaller variant is resized from the previous one.
pub const VARIANTS: [Variant; 4] = [
    Variant {
        name: "display",
        long_edge: 2560,
        jpeg_quality: 82,
    },
    Variant {
        name: "preview",
        long_edge: 1280,
        jpeg_quality: 80,
    },
    Variant {
        name: "thumb",
        long_edge: 400,
        jpeg_quality: 78,
    },
    Variant {
        name: "blur",
        long_edge: 64,
        jpeg_quality: 60,
    },
];

#[derive(Debug, Clone)]
pub struct Derivative {
    pub variant: &'static str,
    pub ext: &'static str,
    pub bytes: Vec<u8>,
}

fn to_linear() -> &'static [u16; 256] {
    static T: OnceLock<[u16; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u16; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let c = i as f64 / 255.0;
            let l = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
            *v = (l * 65535.0).round() as u16;
        }
        t
    })
}

fn to_srgb() -> &'static [u8] {
    static T: OnceLock<Vec<u8>> = OnceLock::new();
    T.get_or_init(|| {
        (0..=65535u32)
            .map(|i| {
                let l = f64::from(i) / 65535.0;
                let c = if l <= 0.003_130_8 {
                    l * 12.92
                } else {
                    1.055 * l.powf(1.0 / 2.4) - 0.055
                };
                (c * 255.0).round().clamp(0.0, 255.0) as u8
            })
            .collect()
    })
}

fn encode_err(e: impl std::fmt::Display) -> Error {
    Error::Decode(format!("resize/encode: {e}"))
}

/// A linear-light RGB image, 16 bits per channel, as native-endian bytes.
struct Linear(Image<'static>);

impl Linear {
    fn from_srgb(d: &Decoded) -> Result<Self, Error> {
        let lut = to_linear();
        let mut img = Image::new(d.width, d.height, PixelType::U16x3);
        for (dst, src) in img
            .buffer_mut()
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&d.rgb)
        {
            *dst = lut[*src as usize].to_ne_bytes();
        }
        Ok(Self(img))
    }

    fn resized(&self, w: u32, h: u32) -> Result<Self, Error> {
        let mut dst = Image::new(w, h, PixelType::U16x3);
        Resizer::new()
            .resize(
                &self.0,
                &mut dst,
                &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)),
            )
            .map_err(encode_err)?;
        Ok(Self(dst))
    }

    fn to_srgb8(&self) -> Vec<u8> {
        let lut = to_srgb();
        self.0
            .buffer()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| lut[u16::from_ne_bytes(*b) as usize])
            .collect()
    }
}

/// Fit within `long_edge` on the long side, never upscaling.
fn target(w: u32, h: u32, long_edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= long_edge {
        return (w, h);
    }
    let scale = f64::from(long_edge) / f64::from(long);
    (
        ((f64::from(w) * scale).round() as u32).max(1),
        ((f64::from(h) * scale).round() as u32).max(1),
    )
}

fn jpeg(rgb: &[u8], w: u32, h: u32, quality: u8) -> Result<Vec<u8>, Error> {
    let too_big = |_| Error::Decode("image too large to encode".into());
    let (w16, h16) = (
        u16::try_from(w).map_err(too_big)?,
        u16::try_from(h).map_err(too_big)?,
    );
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, quality)
        .encode(rgb, w16, h16, jpeg_encoder::ColorType::Rgb)
        .map_err(encode_err)?;
    Ok(out)
}

/// Generate every variant from an upright RGB image.
///
/// Takes the image by value so the 8-bit buffer is freed as soon as the linear
/// copy exists: for a 48 MP source that is 144 MB not held alongside 288 MB.
pub fn derive(upright: Decoded) -> Result<Vec<Derivative>, Error> {
    let (mut cw, mut ch) = (upright.width, upright.height);
    let mut current = Linear::from_srgb(&upright)?;
    drop(upright);
    let mut out = Vec::with_capacity(VARIANTS.len());
    for v in VARIANTS {
        let (tw, th) = target(cw, ch, v.long_edge);
        if (tw, th) != (cw, ch) {
            current = current.resized(tw, th)?;
            (cw, ch) = (tw, th);
        }
        out.push(Derivative {
            variant: v.name,
            ext: "jpg",
            bytes: jpeg(&current.to_srgb8(), cw, ch, v.jpeg_quality)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_never_upscales() {
        assert_eq!(target(4000, 3000, 2560), (2560, 1920));
        assert_eq!(target(3000, 4000, 400), (300, 400));
        assert_eq!(target(200, 100, 2560), (200, 100));
        assert_eq!(target(10_000, 1, 64), (64, 1));
    }

    #[test]
    fn linear_round_trip_is_close() {
        let d = Decoded {
            width: 4,
            height: 1,
            rgb: vec![0, 0, 0, 10, 100, 200, 255, 255, 255, 128, 128, 128],
        };
        let back = Linear::from_srgb(&d).unwrap().to_srgb8();
        for (a, b) in d.rgb.iter().zip(&back) {
            assert!((i16::from(*a) - i16::from(*b)).abs() <= 1, "{a} vs {b}");
        }
    }
}
