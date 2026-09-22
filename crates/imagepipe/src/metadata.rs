//! EXIF extraction with `kamadak-exif`. Only what the index needs: orientation
//! and the capture date. GPS and everything else is ignored here and never
//! copied into derivatives.

use std::io::Cursor;

use exif::{In, Reader, Tag, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Exif {
    /// EXIF orientation 1..=8, absent when the file has none.
    pub orientation: Option<u8>,
    /// `DateTimeOriginal` as `YYYY-MM-DDTHH:MM:SSZ`. EXIF carries no zone, so the
    /// wall-clock time is kept as written and labelled UTC.
    pub taken_at: Option<TakenAt>,
}

/// A fixed-size RFC 3339 string, kept `Copy` so [`Exif`] stays trivially cheap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakenAt([u8; 20]);

impl TakenAt {
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

/// Parse EXIF out of a container (JPEG, PNG, WebP, TIFF).
pub fn from_container(bytes: &[u8]) -> Exif {
    match Reader::new().read_from_container(&mut Cursor::new(bytes)) {
        Ok(e) => extract(&e),
        Err(_) => Exif::default(),
    }
}

/// Parse a bare TIFF block, as found in a HEIC `Exif` item.
pub fn from_tiff(tiff: &[u8]) -> Exif {
    match Reader::new().read_raw(tiff.to_vec()) {
        Ok(e) => extract(&e),
        Err(_) => Exif::default(),
    }
}

fn extract(e: &exif::Exif) -> Exif {
    let orientation = e
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .and_then(|v| u8::try_from(v).ok())
        .filter(|v| (1..=8).contains(v));
    let taken_at = e
        .get_field(Tag::DateTimeOriginal, In::PRIMARY)
        .or_else(|| e.get_field(Tag::DateTime, In::PRIMARY))
        .and_then(|f| match &f.value {
            Value::Ascii(v) => v.first().and_then(|s| parse_exif_date(s)),
            _ => None,
        });
    Exif {
        orientation,
        taken_at,
    }
}

/// `2024:05:01 10:00:00` to `2024-05-01T10:00:00Z`, validating every field.
fn parse_exif_date(raw: &[u8]) -> Option<TakenAt> {
    let s = std::str::from_utf8(raw).ok()?.trim_end_matches('\0').trim();
    let b = s.as_bytes();
    if b.len() != 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<u32>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if b[4] != b':' || b[7] != b':' || b[10] != b' ' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    // Cameras write all-zero dates when the clock was never set.
    if y == 0 || !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60
    {
        return None;
    }
    let out = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}Z");
    let mut buf = [0u8; 20];
    buf.copy_from_slice(out.as_bytes());
    Some(TakenAt(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exif_dates() {
        let t = parse_exif_date(b"2024:05:01 10:00:00\0").unwrap();
        assert_eq!(t.as_str(), "2024-05-01T10:00:00Z");
    }

    #[test]
    fn rejects_unset_clock_and_junk() {
        assert!(parse_exif_date(b"0000:00:00 00:00:00").is_none());
        assert!(parse_exif_date(b"2024-05-01 10:00:00").is_none());
        assert!(parse_exif_date(b"yesterday").is_none());
        assert!(parse_exif_date(b"2024:13:01 10:00:00").is_none());
    }

    #[test]
    fn no_exif_is_default() {
        assert_eq!(from_container(b"not an image"), Exif::default());
    }
}
