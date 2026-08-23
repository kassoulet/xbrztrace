//! xBRZtrace: convert pixel art images (PNG/JPEG) into scalable SVG vectors.
//!
//! Pipeline: [`image_loader`] decodes the input raster, [`xbrz_engine`]
//! upscales it with the xBRZ algorithm, [`vectorizer`] traces the pixel
//! boundaries into closed polygons, [`optimizer`] strips redundant control
//! points and flattens the loops, and [`svg_exporter`] serializes them.

pub mod cli;
pub mod image_loader;
pub mod optimizer;
pub mod quantize;
pub mod svg_exporter;
pub mod vectorizer;
pub mod xbrz_engine;
