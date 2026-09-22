//! `Decoder` implementations. Header reads never allocate pixel buffers; callers
//! check [`Limits`] against the returned dimensions before calling `decode`.

use std::io::Cursor;

use crate::{Decoded, Decoder, Dimensions, Error, Format, Limits};

/// JPEG via `zune-jpeg`.
pub struct JpegDecoder;

impl Decoder for JpegDecoder {
    fn formats(&self) -> &'static [Format] {
        &[Format::Jpeg]
    }

    fn dimensions(&self, bytes: &[u8]) -> Result<Dimensions, Error> {
        let mut d = zune_jpeg::JpegDecoder::new(zune_core::bytestream::ZCursor::new(bytes));
        d.decode_headers()
            .map_err(|e| Error::Decode(e.to_string()))?;
        let info = d
            .info()
            .ok_or_else(|| Error::Decode("no JPEG header".into()))?;
        Ok(Dimensions {
            width: u32::from(info.width),
            height: u32::from(info.height),
        })
    }

    fn decode(&self, bytes: &[u8], limits: &Limits) -> Result<Decoded, Error> {
        use zune_core::colorspace::ColorSpace;
        use zune_core::options::DecoderOptions;

        let dims = self.dimensions(bytes)?;
        limits.check(dims.width, dims.height)?;
        let opts = DecoderOptions::default()
            .jpeg_set_out_colorspace(ColorSpace::RGB)
            .set_max_width(dims.width as usize)
            .set_max_height(dims.height as usize);
        let mut d = zune_jpeg::JpegDecoder::new_with_options(
            zune_core::bytestream::ZCursor::new(bytes),
            opts,
        );
        let rgb = d.decode().map_err(|e| Error::Decode(e.to_string()))?;
        if rgb.len() != dims.width as usize * dims.height as usize * 3 {
            return Err(Error::Decode("unexpected JPEG output size".into()));
        }
        Ok(Decoded {
            width: dims.width,
            height: dims.height,
            rgb,
        })
    }
}

/// PNG and WebP (decode only) via the `image` crate.
pub struct ImageDecoder;

fn image_format(bytes: &[u8]) -> Result<image::ImageFormat, Error> {
    match crate::sniff(bytes) {
        Some(Format::Png) => Ok(image::ImageFormat::Png),
        Some(Format::WebP) => Ok(image::ImageFormat::WebP),
        _ => Err(Error::UnsupportedFormat),
    }
}

impl Decoder for ImageDecoder {
    fn formats(&self) -> &'static [Format] {
        &[Format::Png, Format::WebP]
    }

    fn dimensions(&self, bytes: &[u8]) -> Result<Dimensions, Error> {
        let (width, height) =
            image::ImageReader::with_format(Cursor::new(bytes), image_format(bytes)?)
                .into_dimensions()
                .map_err(|e| Error::Decode(e.to_string()))?;
        Ok(Dimensions { width, height })
    }

    fn decode(&self, bytes: &[u8], limits: &Limits) -> Result<Decoded, Error> {
        let dims = self.dimensions(bytes)?;
        limits.check(dims.width, dims.height)?;
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image_format(bytes)?);
        let mut il = image::Limits::default();
        il.max_image_width = Some(dims.width);
        il.max_image_height = Some(dims.height);
        // RGBA intermediate buffers can be 4 bytes per pixel.
        il.max_alloc = Some(limits.max_bytes.saturating_mul(2).max(64 * 1024 * 1024));
        reader.limits(il);
        let img = reader.decode().map_err(|e| Error::Decode(e.to_string()))?;
        let rgb = img.into_rgb8();
        Ok(Decoded {
            width: rgb.width(),
            height: rgb.height(),
            rgb: rgb.into_raw(),
        })
    }
}

/// HEIC / HEIF via `heic-rs`. Rotation and mirroring are applied by the
/// decoder, so callers must not apply EXIF orientation again.
pub struct HeicDecoder;

impl HeicDecoder {
    /// EXIF from the primary item's `Exif` block, if any.
    pub fn exif(bytes: &[u8]) -> Option<crate::metadata::Exif> {
        let ctx = heic_rs::context::Context::open(bytes).ok()?;
        let tiff = ctx.exif(ctx.meta.primary).ok()??;
        Some(crate::metadata::from_tiff(tiff))
    }
}

impl Decoder for HeicDecoder {
    fn formats(&self) -> &'static [Format] {
        &[Format::Heic]
    }

    fn dimensions(&self, bytes: &[u8]) -> Result<Dimensions, Error> {
        let info = heic_rs::probe(bytes).map_err(|e| Error::Decode(e.to_string()))?;
        Ok(Dimensions {
            width: info.width,
            height: info.height,
        })
    }

    fn decode(&self, bytes: &[u8], limits: &Limits) -> Result<Decoded, Error> {
        let dims = self.dimensions(bytes)?;
        limits.check(dims.width, dims.height)?;
        let opts = heic_rs::DecodeOptions {
            layout: heic_rs::PixelLayout::Rgb8,
            max_pixels: Some(limits.max_pixels),
            ..heic_rs::DecodeOptions::default()
        };
        let img = heic_rs::decode(bytes, &opts).map_err(|e| Error::Decode(e.to_string()))?;
        Ok(Decoded {
            width: img.width,
            height: img.height,
            rgb: img.data,
        })
    }
}

/// The decoder for a sniffed format. AVIF has no pure-Rust decoder wired up yet,
/// so it is reported as unsupported and the file is quarantined.
pub fn decoder_for(format: Format) -> Option<&'static dyn Decoder> {
    match format {
        Format::Jpeg => Some(&JpegDecoder),
        Format::Png | Format::WebP => Some(&ImageDecoder),
        Format::Heic => Some(&HeicDecoder),
        Format::Avif => None,
    }
}
