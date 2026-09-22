//! The library volume's directory contract, shared by web and the indexer so
//! they cannot disagree about where things live.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn incoming(&self) -> PathBuf {
        self.root.join("incoming")
    }
    pub fn quarantine(&self) -> PathBuf {
        self.root.join("quarantine")
    }
    pub fn library(&self) -> PathBuf {
        self.root.join("library")
    }
    pub fn derivatives(&self) -> PathBuf {
        self.root.join("derivatives")
    }
    pub fn exports(&self) -> PathBuf {
        self.root.join("exports")
    }

    /// `derivatives/<h0h1>/<h2h3>/<hash>-<variant>.<ext>`, sharded two levels to
    /// keep directory sizes sane on NFS. A curated rotation is part of the name
    /// (`<hash>-<variant>-r90.<ext>`) so the URL for a given (photo, rotation)
    /// always names the same bytes and can stay immutable. `None` unless `hash`
    /// is a lowercase hex BLAKE3 digest, so a caller-supplied string can never
    /// escape the tree, or unless `rotation` is 0, 90, 180 or 270.
    pub fn derivative_path(
        &self,
        hash: &str,
        variant: &str,
        rotation: i32,
        ext: &str,
    ) -> Option<PathBuf> {
        if !matches!(rotation, 0 | 90 | 180 | 270) {
            return None;
        }
        let suffix = if rotation == 0 {
            String::new()
        } else {
            format!("-r{rotation}")
        };
        Some(
            self.derivative_dir(hash)?
                .join(format!("{hash}-{variant}{suffix}.{ext}")),
        )
    }

    pub fn derivative_dir(&self, hash: &str) -> Option<PathBuf> {
        if !is_hash(hash) {
            return None;
        }
        Some(self.derivatives().join(&hash[0..2]).join(&hash[2..4]))
    }

    /// Resolve a `photos.rel_path` under `library/`, refusing anything that could
    /// leave it.
    pub fn library_path(&self, rel_path: &str) -> Option<PathBuf> {
        let p = Path::new(rel_path);
        let safe = p
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
        (safe && !rel_path.is_empty()).then(|| self.library().join(p))
    }
}

/// A BLAKE3 digest, hex-encoded.
pub fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivative_paths_are_sharded_and_safe() {
        let l = Layout::new("/v");
        let h = "ab".repeat(32);
        assert_eq!(
            l.derivative_path(&h, "thumb", 0, "jpg").unwrap(),
            PathBuf::from(format!("/v/derivatives/ab/ab/{h}-thumb.jpg"))
        );
        assert_eq!(
            l.derivative_path(&h, "thumb", 90, "jpg").unwrap(),
            PathBuf::from(format!("/v/derivatives/ab/ab/{h}-thumb-r90.jpg"))
        );
        assert!(l.derivative_path(&h, "thumb", 45, "jpg").is_none());
        assert!(
            l.derivative_path("../../etc/passwd", "thumb", 0, "jpg")
                .is_none()
        );
        assert!(
            l.derivative_path(&"AB".repeat(32), "thumb", 0, "jpg")
                .is_none()
        );
    }

    #[test]
    fn library_paths_cannot_escape() {
        let l = Layout::new("/v");
        assert!(l.library_path("2024/05/a.jpg").is_some());
        assert!(l.library_path("../x").is_none());
        assert!(l.library_path("/etc/passwd").is_none());
        assert!(l.library_path("").is_none());
    }
}
