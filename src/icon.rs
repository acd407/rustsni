//! SNI icon pixmap handling.
//!
//! Icons are transferred as `a(iiay)` — an array of `(width, height, ARGB32_be)`.

/// A decoded icon pixmap in native-endian ARGB32 (Cairo format on LE: `[B,G,R,A]`).
#[derive(Debug, Clone, PartialEq)]
pub struct IconPixmap {
    pub width: u32,
    pub height: u32,
    /// Pixel data in native-endian ARGB32 (ready for Cairo ImageSurface).
    pub data: Vec<u8>,
}

/// Convert simple `(width, height, &[u8])` tuples into `IconPixmap`s.
///
/// This is the public API for callers who already have raw ARGB32 big-endian
/// pixel data (e.g. from a decoded image file) and don't want to build
/// `rustbus::params::Param` trees manually.
pub fn from_tuples(tuples: &[(i32, i32, &[u8])]) -> Vec<IconPixmap> {
    let mut result = Vec::new();
    for &(w, h, raw) in tuples {
        if w <= 0 || h <= 0 {
            continue;
        }
        let expected = (w as usize) * (h as usize) * 4;
        if raw.len() < expected {
            continue;
        }
        let mut data = raw[..expected].to_vec();
        // big-endian [A,R,G,B] → native LE Cairo [B,G,R,A]
        for pixel in data.chunks_exact_mut(4) {
            let a = pixel[0];
            let r = pixel[1];
            let g = pixel[2];
            let b = pixel[3];
            pixel[0] = b;
            pixel[1] = g;
            pixel[2] = r;
            pixel[3] = a;
        }
        result.push(IconPixmap {
            width: w as u32,
            height: h as u32,
            data,
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_pixel_conversion() {
        // big-endian ARGB: A=0xFF, R=0x11, G=0x22, B=0x33
        let raw: &[&[u8]] = &[&[0xFF, 0x11, 0x22, 0x33]];
        let tuples: Vec<(i32, i32, &[u8])> = vec![(1, 1, raw[0])];
        let pixmaps = from_tuples(&tuples);
        assert_eq!(pixmaps.len(), 1);
        assert_eq!(pixmaps[0].width, 1);
        assert_eq!(pixmaps[0].height, 1);
        // LE Cairo [B,G,R,A] = [0x33, 0x22, 0x11, 0xFF]
        assert_eq!(&pixmaps[0].data, &[0x33, 0x22, 0x11, 0xFF]);
    }

    #[test]
    fn two_by_two_icon() {
        // 2x2 = 4 pixels, each 4 bytes
        let raw: Vec<u8> = vec![
            0xFF, 0xAA, 0xBB, 0xCC, // pixel (0,0)
            0x80, 0x11, 0x22, 0x33, // pixel (1,0)
            0x00, 0x44, 0x55, 0x66, // pixel (0,1)
            0xFF, 0x77, 0x88, 0x99, // pixel (1,1)
        ];
        let tuples: Vec<(i32, i32, &[u8])> = vec![(2, 2, &raw)];
        let pixmaps = from_tuples(&tuples);
        assert_eq!(pixmaps.len(), 1);
        assert_eq!(pixmaps[0].width, 2);
        assert_eq!(pixmaps[0].height, 2);
        assert_eq!(pixmaps[0].data.len(), 16);
        // First pixel: [A,R,G,B] → [B,G,R,A]
        assert_eq!(&pixmaps[0].data[0..4], &[0xCC, 0xBB, 0xAA, 0xFF]);
    }

    #[test]
    fn skip_zero_dimensions() {
        let raw: &[u8] = &[0; 4];
        let tuples: Vec<(i32, i32, &[u8])> = vec![(0, 1, raw), (1, 0, raw), (-1, 1, raw)];
        assert!(from_tuples(&tuples).is_empty());
    }

    #[test]
    fn skip_insufficient_data() {
        // Need 4 bytes for 1x1, give only 2
        let raw: &[u8] = &[0xFF, 0x00];
        let tuples: Vec<(i32, i32, &[u8])> = vec![(1, 1, raw)];
        assert!(from_tuples(&tuples).is_empty());
    }

    #[test]
    fn extra_data_is_ignored() {
        // 1x1 needs 4 bytes, give 8 — should only use first 4
        let raw: &[u8] = &[0xFF, 0x11, 0x22, 0x33, 0x00, 0x00, 0x00, 0x00];
        let tuples: Vec<(i32, i32, &[u8])> = vec![(1, 1, raw)];
        let pixmaps = from_tuples(&tuples);
        assert_eq!(pixmaps.len(), 1);
        assert_eq!(pixmaps[0].data.len(), 4);
    }

    #[test]
    fn empty_input() {
        let tuples: Vec<(i32, i32, &[u8])> = vec![];
        assert!(from_tuples(&tuples).is_empty());
    }
}
