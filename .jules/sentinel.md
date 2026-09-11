## 2025-09-07 - Image Dimension Bounds & Arithmetic Overflow Hardening
**Vulnerability:** Unbounded input image dimensions and unchecked multiplications when allocating upscaled pixel buffers (`scale_image`) and calculating vector loop bounds (`max_edges`).
**Learning:** `debug_assert_eq!` in `ArgbImage::new` stripped invariant checks in release builds, allowing potential out-of-bounds indexing or memory allocation crashes if oversized or malformed inputs bypassed early checks.
**Prevention:** Always validate input raster dimensions (`0 < dim <= 16384`) early during image ingestion, enforce release-mode dimension invariants with `assert_eq!`, and use `checked_mul` / `saturating_mul` for buffer allocations.

## 2025-09-08 - Pre-Decoding Image Limits for Decompression Bomb Prevention
**Vulnerability:** `image_loader::load` decoded the full raster image into memory before checking `width` and `height` against `MAX_DIMENSION`. Malicious image files specifying large dimensions in headers could cause huge memory allocations during `reader.decode()` leading to Denial of Service (DoS via OOM).
**Learning:** `image::ImageReader` requires explicit `limits` configuration (`max_image_width`, `max_image_height`, `max_alloc`) before calling `decode()` so that header dimensions are validated before memory is allocated.
**Prevention:** Always set explicit decoder limits on `ImageReader` prior to decoding untrusted image inputs.
