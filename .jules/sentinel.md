## 2025-09-07 - Image Dimension Bounds & Arithmetic Overflow Hardening
**Vulnerability:** Unbounded input image dimensions and unchecked multiplications when allocating upscaled pixel buffers (`scale_image`) and calculating vector loop bounds (`max_edges`).
**Learning:** `debug_assert_eq!` in `ArgbImage::new` stripped invariant checks in release builds, allowing potential out-of-bounds indexing or memory allocation crashes if oversized or malformed inputs bypassed early checks.
**Prevention:** Always validate input raster dimensions (`0 < dim <= 16384`) early during image ingestion, enforce release-mode dimension invariants with `assert_eq!`, and use `checked_mul` / `saturating_mul` for buffer allocations.
