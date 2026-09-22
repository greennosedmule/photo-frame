//! Decode, EXIF, resize, encode. Bytes in, bytes out: no I/O, no HTTP, no database.
//!
//! Everything here handles untrusted input, so it is safe Rust with no C
//! dependencies, and resource limits are checked before any allocation.

mod decode;
mod derive;
mod format;
mod limits;
pub mod metadata;
mod orient;

pub use decode::{HeicDecoder, ImageDecoder, JpegDecoder, decoder_for};
pub use derive::{Derivative, VARIANTS, Variant};
pub use format::{Format, sniff};
pub use limits::{LimitError, Limits};

/// Dimensions read from a file header without decoding pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported or unrecognised image format")]
    UnsupportedFormat,
    #[error(transparent)]
    Limit(#[from] LimitError),
    #[error("decode failed: {0}")]
    Decode(String),
}

/// A decoded, upright-agnostic RGB8 image.
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Swappable decoder boundary, so a `libheif`-backed implementation can be added
/// behind a Cargo feature later without touching callers.
pub trait Decoder: Send + Sync {
    fn formats(&self) -> &'static [Format];

    /// Read dimensions from the header only. Callers check them against
    /// [`Limits`] before calling [`Decoder::decode`].
    fn dimensions(&self, bytes: &[u8]) -> Result<Dimensions, Error>;

    fn decode(&self, bytes: &[u8], limits: &Limits) -> Result<Decoded, Error>;
}

/// Everything the index needs to know about a file, read without decoding pixels.
#[derive(Debug, Clone)]
pub struct Info {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    /// EXIF orientation, 1 when absent. For HEIC the decoder applies rotation
    /// itself, so `derive` ignores this for that format.
    pub orientation: u8,
    /// `DateTimeOriginal` as `YYYY-MM-DDTHH:MM:SSZ`.
    pub taken_at: Option<String>,
}

/// Sniff by magic bytes, read the header and EXIF, and enforce `limits` before
/// any pixel buffer exists. The caller has already bounded the input length.
pub fn inspect(bytes: &[u8], limits: &Limits) -> Result<Info, Error> {
    limits.check_input(bytes.len())?;
    let format = sniff(bytes).ok_or(Error::UnsupportedFormat)?;
    let decoder = decoder_for(format).ok_or(Error::UnsupportedFormat)?;
    let dims = decoder.dimensions(bytes)?;
    limits.check(dims.width, dims.height)?;
    let exif = match format {
        Format::Heic => HeicDecoder::exif(bytes).unwrap_or_default(),
        _ => metadata::from_container(bytes),
    };
    Ok(Info {
        format,
        width: dims.width,
        height: dims.height,
        orientation: exif.orientation.unwrap_or(1),
        taken_at: exif.taken_at.map(|t| t.as_str().to_string()),
    })
}

/// Decode, orient and generate every derivative variant. `rotation` is the
/// curated correction in degrees clockwise (0, 90, 180 or 270), applied after
/// the file's own orientation so it always means "turn what you see".
pub fn derive(
    bytes: &[u8],
    info: &Info,
    limits: &Limits,
    rotation: i32,
) -> Result<Vec<Derivative>, Error> {
    // EXIF orientation values that are a pure clockwise turn.
    let turn = match rotation {
        0 => 1,
        90 => 6,
        180 => 3,
        270 => 8,
        other => return Err(Error::Decode(format!("unsupported rotation {other}"))),
    };
    limits.check_input(bytes.len())?;
    let decoder = decoder_for(info.format).ok_or(Error::UnsupportedFormat)?;
    let decoded = decoder.decode(bytes, limits)?;
    let upright = match info.format {
        Format::Heic => decoded,
        _ => orient::apply(decoded, info.orientation),
    };
    derive::derive(orient::apply(upright, turn))
}
