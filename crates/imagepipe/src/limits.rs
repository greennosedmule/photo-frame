/// Hard decode limits, enforced before allocation. A file over any limit is
/// quarantined by the caller, never decoded.
///
/// `max_pixels` bounds the decoded buffer (3 bytes per pixel, so 80 MP is about
/// 240 MB); `max_bytes` bounds the input file. Large originals are expected:
/// the point of the indexer is to derive manageable sizes from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_pixels: u64,
    pub max_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pixels: 80_000_000,
            max_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LimitError {
    #[error("image has {pixels} pixels, limit is {max}")]
    TooManyPixels { pixels: u64, max: u64 },
    #[error("input is {bytes} bytes, limit is {max}")]
    InputTooLarge { bytes: u64, max: u64 },
}

impl Limits {
    /// Check header-declared dimensions before any pixel buffer is allocated.
    pub fn check(&self, width: u32, height: u32) -> Result<(), LimitError> {
        let pixels = u64::from(width) * u64::from(height);
        if pixels > self.max_pixels {
            return Err(LimitError::TooManyPixels {
                pixels,
                max: self.max_pixels,
            });
        }
        Ok(())
    }

    /// Check the input length before reading or parsing it.
    pub fn check_input(&self, len: usize) -> Result<(), LimitError> {
        let bytes = len as u64;
        if bytes > self.max_bytes {
            return Err(LimitError::InputTooLarge {
                bytes,
                max: self.max_bytes,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_before_allocating() {
        let l = Limits::default();
        assert!(l.check(6000, 4000).is_ok()); // 24 MP
        assert!(l.check(8064, 6048).is_ok()); // 48 MP iPhone
        assert!(matches!(
            l.check(10_000, 10_000),
            Err(LimitError::TooManyPixels { .. })
        ));
        // u32::MAX squared must not overflow.
        assert!(l.check(u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn input_size_is_limited_separately_from_pixels() {
        let l = Limits {
            max_pixels: u64::MAX,
            max_bytes: 1000,
        };
        assert!(l.check(100_000, 100_000).is_ok());
        assert!(l.check_input(1000).is_ok());
        assert!(matches!(
            l.check_input(1001),
            Err(LimitError::InputTooLarge { .. })
        ));
    }
}
