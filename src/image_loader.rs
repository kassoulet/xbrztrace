//! Image ingestion: decode PNG/JPEG (or any format the `image` crate
//! supports) into a normalized RGBA grid.
//!
//! Also provides integer-zoom detection and dezoom (downscale) for
//! pre-scaled pixel art inputs.

use std::path::Path;

use anyhow::{Context, Result};

use crate::xbrz_engine::{Argb, ArgbImage};

/// Decode the image at `path` into an RGBA grid. The format is detected from
/// the file content (not the extension), so mislabeled files still load as
/// long as the bytes are a recognizable image format.
pub fn load(path: &Path) -> Result<ArgbImage> {
    let reader = image::ImageReader::open(path)
        .with_context(|| format!("cannot open input image `{}`", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("cannot detect the format of `{}`", path.display()))?;

    let img = reader
        .decode()
        .with_context(|| format!("failed to decode image `{}`", path.display()))?
        .to_rgba8();

    let (width, height) = img.dimensions();
    let width = width as usize;
    let height = height as usize;
    let pixels = img
        .pixels()
        .map(|p| Argb::from_rgba(p[0], p[1], p[2], p[3]))
        .collect::<Vec<_>>();

    Ok(ArgbImage::new(width, height, pixels))
}

/// Result of integer-zoom detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomInfo {
    /// The detected integer zoom factor (2, 3, 4, etc.), or 1 if not zoomed.
    pub factor: u8,
    /// Whether the image appears to be integer-zoomed (factor > 1).
    pub is_zoomed: bool,
}

/// Detect if an image appears to be integer-zoomed (scaled up by an integer
/// factor using nearest-neighbor).
///
/// Checks factors 2 through 6. Returns the largest factor that produces a
/// perfect match when the downscaled image is upscaled back to the original
/// dimensions using nearest-neighbor.
pub fn detect_integer_zoom(img: &ArgbImage) -> ZoomInfo {
    const MAX_FACTOR: u8 = 6;

    for factor in (2..=MAX_FACTOR).rev() {
        let factor = factor as usize;
        if !img.width.is_multiple_of(factor) || !img.height.is_multiple_of(factor) {
            continue;
        }

        let small_w = img.width / factor;
        let small_h = img.height / factor;

        // Downscale by taking the top-left pixel of each factor x factor block
        let mut small_pixels = Vec::with_capacity(small_w * small_h);
        for y in 0..small_h {
            for x in 0..small_w {
                small_pixels.push(img.pixels[(y * factor) * img.width + x * factor]);
            }
        }
        let small_img = ArgbImage::new(small_w, small_h, small_pixels);

        // Upscale back using nearest-neighbor and compare
        let mut matches = true;
        for y in 0..img.height {
            for x in 0..img.width {
                let src_px = small_img.pixels[(y / factor) * small_w + (x / factor)];
                if src_px != img.pixels[y * img.width + x] {
                    matches = false;
                    break;
                }
            }
            if !matches {
                break;
            }
        }

        if matches {
            return ZoomInfo {
                factor: factor as u8,
                is_zoomed: true,
            };
        }
    }

    ZoomInfo {
        factor: 1,
        is_zoomed: false,
    }
}

/// Downscale an image by an integer factor using nearest-neighbor (top-left
/// pixel of each block).
pub fn dezoom(img: &ArgbImage, factor: u8) -> ArgbImage {
    let factor = factor as usize;
    assert!(factor > 1, "dezoom factor must be > 1");
    assert_eq!(img.width % factor, 0, "width must be divisible by factor");
    assert_eq!(img.height % factor, 0, "height must be divisible by factor");

    let small_w = img.width / factor;
    let small_h = img.height / factor;

    let mut small_pixels = Vec::with_capacity(small_w * small_h);
    for y in 0..small_h {
        for x in 0..small_w {
            small_pixels.push(img.pixels[(y * factor) * img.width + x * factor]);
        }
    }
    ArgbImage::new(small_w, small_h, small_pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;
    use std::io::Write;

    /// Write a PNG to `path` from an RGBA pixel buffer.
    fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
        let mut file = std::fs::File::create(path).unwrap();
        let encoder = image::codecs::png::PngEncoder::new(&mut file);
        encoder
            .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
            .unwrap();
    }

    #[test]
    fn loads_png_with_alpha() {
        let dir = std::env::temp_dir().join("xbrztrace_loader_test_rgba");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rgba.png");

        // 2x2: red opaque, green opaque, blue half-transparent, transparent.
        let rgba: Vec<u8> = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 128, 0, 0, 0, 0];
        write_png(&path, 2, 2, &rgba);

        let img = load(&path).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.pixels[0], Argb::from_rgba(255, 0, 0, 255));
        assert_eq!(img.pixels[1], Argb::from_rgba(0, 255, 0, 255));
        assert_eq!(img.pixels[2], Argb::from_rgba(0, 0, 255, 128));
        assert_eq!(img.pixels[3], Argb::from_rgba(0, 0, 0, 0));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_is_an_error() {
        let err = load(Path::new("/nonexistent/xbrztrace_missing.png")).unwrap_err();
        assert!(err.to_string().contains("cannot open input image"));
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let dir = std::env::temp_dir().join("xbrztrace_loader_test_corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("corrupt.png");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(b"this is definitely not an image")
            .unwrap();

        let err = load(&path).unwrap_err();
        assert!(err.to_string().contains("failed to decode image"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detects_2x_zoom() {
        // Create a 2x2 base image
        let base = vec![
            Argb::from_rgba(255, 0, 0, 255),
            Argb::from_rgba(0, 255, 0, 255),
            Argb::from_rgba(0, 0, 255, 255),
            Argb::from_rgba(255, 255, 0, 255),
        ];
        let base_img = ArgbImage::new(2, 2, base);

        // Create 4x4 zoomed version (2x)
        let mut zoomed_pixels = vec![Argb(0); 16];
        for y in 0..4 {
            for x in 0..4 {
                zoomed_pixels[y * 4 + x] = base_img.pixels[(y / 2) * 2 + (x / 2)];
            }
        }
        let zoomed = ArgbImage::new(4, 4, zoomed_pixels);

        let info = detect_integer_zoom(&zoomed);
        assert_eq!(info.factor, 2);
        assert!(info.is_zoomed);

        let dezoomed = dezoom(&zoomed, 2);
        assert_eq!(dezoomed.width, 2);
        assert_eq!(dezoomed.height, 2);
        assert_eq!(dezoomed.pixels, base_img.pixels);
    }

    #[test]
    fn detects_3x_zoom() {
        let base = vec![Argb::from_rgba(10, 20, 30, 255); 9];
        let base_img = ArgbImage::new(3, 3, base);

        let mut zoomed_pixels = vec![Argb(0); 81];
        for y in 0..9 {
            for x in 0..9 {
                zoomed_pixels[y * 9 + x] = base_img.pixels[(y / 3) * 3 + (x / 3)];
            }
        }
        let zoomed = ArgbImage::new(9, 9, zoomed_pixels);

        let info = detect_integer_zoom(&zoomed);
        assert_eq!(info.factor, 3);
        assert!(info.is_zoomed);
    }

    #[test]
    fn no_false_positive_on_non_zoomed() {
        // A natural-looking 4x4 image that isn't a perfect integer zoom
        let pixels: Vec<Argb> = (0..16)
            .map(|i| Argb::from_rgba((i * 17) as u8, (i * 31) as u8, (i * 11) as u8, 255))
            .collect();
        let img = ArgbImage::new(4, 4, pixels);

        let info = detect_integer_zoom(&img);
        assert_eq!(info.factor, 1);
        assert!(!info.is_zoomed);
    }

    #[test]
    fn non_divisible_dimensions_not_zoomed() {
        let pixels = vec![Argb::from_rgba(255, 0, 0, 255); 15];
        let img = ArgbImage::new(5, 3, pixels); // 5x3 not divisible by 2,3,4,5,6

        let info = detect_integer_zoom(&img);
        assert_eq!(info.factor, 1);
        assert!(!info.is_zoomed);
    }
}
