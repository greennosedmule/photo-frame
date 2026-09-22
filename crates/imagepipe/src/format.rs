/// Supported input formats, identified by magic bytes only. Never trust an
/// extension or a declared MIME type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Jpeg,
    Png,
    WebP,
    Heic,
    Avif,
}

impl Format {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::WebP => "image/webp",
            Self::Heic => "image/heic",
            Self::Avif => "image/avif",
        }
    }
}

/// Identify a format from the first bytes of a file. Returns `None` for
/// anything unsupported.
pub fn sniff(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Format::Jpeg);
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(Format::Png);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(Format::WebP);
    }
    // ISO BMFF: bytes 4..8 are "ftyp", 8..12 the major brand.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return match &bytes[8..12] {
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => Some(Format::Heic),
            b"avif" | b"avis" => Some(Format::Avif),
            _ => None,
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_by_magic_bytes() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Some(Format::Jpeg));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(Format::WebP));
        assert_eq!(sniff(b"\0\0\0\x18ftypheic\0\0\0\0"), Some(Format::Heic));
        assert_eq!(sniff(b"\0\0\0\x18ftypavif\0\0\0\0"), Some(Format::Avif));
    }

    #[test]
    fn rejects_everything_else() {
        assert_eq!(sniff(b""), None);
        assert_eq!(sniff(b"GIF89a"), None);
        assert_eq!(sniff(b"<svg xmlns="), None);
        assert_eq!(sniff(b"\0\0\0\x18ftypmp42\0\0\0\0"), None);
    }
}
