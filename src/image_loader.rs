//! Image ingestion: decode PNG/JPEG (or any format the `image` crate
//! supports) into a normalized RGBA grid.

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
        let dir = std::env::temp_dir().join("brztracer_loader_test_rgba");
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
        let err = load(Path::new("/nonexistent/brztracer_missing.png")).unwrap_err();
        assert!(err.to_string().contains("cannot open input image"));
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let dir = std::env::temp_dir().join("brztracer_loader_test_corrupt");
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
}
