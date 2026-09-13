## 2025-09-07 - Image Dimension Bounds & Arithmetic Overflow Hardening
**Vulnerability:** Unbounded input image dimensions and unchecked multiplications when allocating upscaled pixel buffers (`scale_image`) and calculating vector loop bounds (`max_edges`).
**Learning:** `debug_assert_eq!` in `ArgbImage::new` stripped invariant checks in release builds, allowing potential out-of-bounds indexing or memory allocation crashes if oversized or malformed inputs bypassed early checks.
**Prevention:** Always validate input raster dimensions (`0 < dim <= 16384`) early during image ingestion, enforce release-mode dimension invariants with `assert_eq!`, and use `checked_mul` / `saturating_mul` for buffer allocations.

## 2025-09-08 - Pre-Decoding Image Limits for Decompression Bomb Prevention
**Vulnerability:** `image_loader::load` decoded the full raster image into memory before checking `width` and `height` against `MAX_DIMENSION`. Malicious image files specifying large dimensions in headers could cause huge memory allocations during `reader.decode()` leading to Denial of Service (DoS via OOM).
**Learning:** `image::ImageReader` requires explicit `limits` configuration (`max_image_width`, `max_image_height`, `max_alloc`) before calling `decode()` so that header dimensions are validated before memory is allocated.
**Prevention:** Always set explicit decoder limits on `ImageReader` prior to decoding untrusted image inputs.

## 2025-09-09 - 32-bit Signed Integer Overflow in Grid Index Calculations
**Vulnerability:** `vectorizer::vectorize` calculated pixel offsets in the visited bitmap using 32-bit signed integer arithmetic (`(y as i32) * (img.width as i32) + (x as i32)`). Large or upscaled images with `y * width >= 2,147,483,648` caused `i32` overflow, leading to panics in debug builds or out-of-bounds array access in release builds.
**Learning:** Downcasting loop counters (`usize`) to `i32` for 2D grid index calculations breaks when grid dimensions exceed ~46,340 pixels, which can easily happen after multi-X scaling (e.g., 6x scaling on valid input images).
**Prevention:** Always perform array and bitmap index calculations using `usize` arithmetic (`y * width + x`) without casting loop variables down to fixed-width signed integers (`i32`).
