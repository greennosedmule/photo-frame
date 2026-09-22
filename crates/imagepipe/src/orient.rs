//! Apply EXIF orientation to a decoded RGB8 buffer so every derivative is upright.

use crate::Decoded;

/// Orientation 1..=8 per the EXIF spec. Anything else is treated as 1.
pub fn apply(img: Decoded, orientation: u8) -> Decoded {
    if !(2..=8).contains(&orientation) {
        return img;
    }
    let (w, h) = (img.width as usize, img.height as usize);
    // 5..=8 involve a 90 degree turn, which swaps the axes.
    let (nw, nh) = if orientation >= 5 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; img.rgb.len()];
    for y in 0..h {
        for x in 0..w {
            // Destination coordinates for source pixel (x, y).
            let (dx, dy) = match orientation {
                2 => (w - 1 - x, y),         // mirror horizontal
                3 => (w - 1 - x, h - 1 - y), // rotate 180
                4 => (x, h - 1 - y),         // mirror vertical
                5 => (y, x),                 // transpose
                6 => (h - 1 - y, x),         // rotate 90 cw
                7 => (h - 1 - y, w - 1 - x), // transverse
                _ => (y, w - 1 - x),         // 8: rotate 90 ccw
            };
            let s = (y * w + x) * 3;
            let d = (dy * nw + dx) * 3;
            out[d..d + 3].copy_from_slice(&img.rgb[s..s + 3]);
        }
    }
    Decoded {
        width: nw as u32,
        height: nh as u32,
        rgb: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3x2 image with distinct pixels: values 1..=6 in the red channel.
    fn img() -> Decoded {
        let mut rgb = Vec::new();
        for v in 1..=6u8 {
            rgb.extend_from_slice(&[v, 0, 0]);
        }
        Decoded {
            width: 3,
            height: 2,
            rgb,
        }
    }

    fn reds(d: &Decoded) -> Vec<u8> {
        d.rgb.chunks(3).map(|p| p[0]).collect()
    }

    #[test]
    fn identity_and_unknown() {
        assert_eq!(reds(&apply(img(), 1)), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(reds(&apply(img(), 9)), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn flips_and_180() {
        assert_eq!(reds(&apply(img(), 2)), vec![3, 2, 1, 6, 5, 4]);
        assert_eq!(reds(&apply(img(), 4)), vec![4, 5, 6, 1, 2, 3]);
        assert_eq!(reds(&apply(img(), 3)), vec![6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn quarter_turns_swap_axes() {
        let cw = apply(img(), 6);
        assert_eq!((cw.width, cw.height), (2, 3));
        assert_eq!(reds(&cw), vec![4, 1, 5, 2, 6, 3]);
        let ccw = apply(img(), 8);
        assert_eq!(reds(&ccw), vec![3, 6, 2, 5, 1, 4]);
        assert_eq!(reds(&apply(img(), 5)), vec![1, 4, 2, 5, 3, 6]);
        assert_eq!(reds(&apply(img(), 7)), vec![6, 3, 5, 2, 4, 1]);
    }
}
