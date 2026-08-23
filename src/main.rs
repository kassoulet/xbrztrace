//! xBRZtrace binary entry point: parse the CLI, run the pipeline, report
//! verbose metrics when requested.

use std::collections::HashSet;
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;

use xbrztrace::cli::Cli;
use xbrztrace::image_loader::{self, detect_integer_zoom, dezoom};
use xbrztrace::optimizer;
use xbrztrace::quantize;
use xbrztrace::svg_exporter;
use xbrztrace::vectorizer;
use xbrztrace::xbrz_engine::{self, ArgbImage, ScalerConfig};

struct Timing {
    load_ms: f64,
    quantize_ms: Option<f64>,
    scale_ms: f64,
    vectorize_ms: f64,
    optimize_ms: f64,
    export_ms: f64,
    write_ms: f64,
}

/// Number of distinct non-transparent colors in an image.
fn distinct_colors(img: &ArgbImage) -> usize {
    img.pixels
        .iter()
        .filter(|p| p.a() != 0)
        .collect::<HashSet<_>>()
        .len()
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xbrztrace: error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // 1. Ingest
    let t0 = Instant::now();
    let mut image = image_loader::load(&cli.input)?;
    let t_load = t0.elapsed();

    // 1b. Detect and handle integer-zoomed input
    let zoom_info = detect_integer_zoom(&image);
    if zoom_info.is_zoomed {
        eprintln!(
            "warning: input appears to be {}x integer-zoomed; dezooming before processing",
            zoom_info.factor
        );
        image = dezoom(&image, zoom_info.factor);
    }

    // 1b. Optional color quantization (cleans lossy-input noise before xBRZ)
    let mut t_quantize = None;
    let mut quantize_stats = None;
    if let Some(tolerance) = cli.quantize {
        let t = Instant::now();
        let before = distinct_colors(&image);
        image = quantize::quantize(&image, tolerance);
        quantize_stats = Some((tolerance, before, distinct_colors(&image)));
        t_quantize = Some(t.elapsed());
    }

    // 2. xBRZ upscale
    let t1 = Instant::now();
    let scaled = xbrz_engine::scale_image(&image, cli.scale.factor(), &ScalerConfig::default());
    let t_scale = t1.elapsed();

    // 3. Vector trace
    let t2 = Instant::now();
    let mut regions = vectorizer::vectorize(&scaled, cli.merge_colors);
    let t_vectorize = t2.elapsed();

    // 3b. SVGO-style post-processing: strip redundant control points and
    // flatten every loop into a minimal polyline.
    let t_opt = Instant::now();
    let opt_stats = optimizer::optimize(&mut regions);
    let t_optimize = t_opt.elapsed();

    // 4. Serialize
    let t3 = Instant::now();
    let svg = svg_exporter::export(&regions, scaled.width, scaled.height);
    let t_export = t3.elapsed();

    // 5. Write
    let t4 = Instant::now();
    std::fs::write(&cli.output, &svg)
        .with_context(|| format!("cannot write output file `{}`", cli.output.display()))?;
    let t_write = t4.elapsed();

    if cli.verbose {
        let timing = Timing {
            load_ms: t_load.as_secs_f64() * 1000.0,
            quantize_ms: t_quantize.map(|d| d.as_secs_f64() * 1000.0),
            scale_ms: t_scale.as_secs_f64() * 1000.0,
            vectorize_ms: t_vectorize.as_secs_f64() * 1000.0,
            optimize_ms: t_optimize.as_secs_f64() * 1000.0,
            export_ms: t_export.as_secs_f64() * 1000.0,
            write_ms: t_write.as_secs_f64() * 1000.0,
        };
        print_stats(
            &cli,
            &image,
            quantize_stats,
            &scaled,
            &regions,
            &opt_stats,
            &svg,
            &timing,
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)] // one argument per reported metric group
fn print_stats(
    cli: &Cli,
    image: &ArgbImage,
    quantize_stats: Option<(f64, usize, usize)>,
    scaled: &ArgbImage,
    regions: &[vectorizer::Region],
    opt: &optimizer::OptimizeStats,
    svg: &str,
    t: &Timing,
) {
    let path_count: usize = regions.iter().map(|r| r.loops.len()).sum();
    let bytes = svg.len();
    eprintln!("input:      {}x{} px", image.width, image.height);
    if let Some((tolerance, before, after)) = quantize_stats {
        eprintln!("quantize:   {before} -> {after} colors (tolerance {tolerance})");
    }
    eprintln!(
        "output:     {}x{} px ({}x)",
        scaled.width,
        scaled.height,
        cli.scale.factor()
    );
    eprintln!(
        "colors:     {} ({} <path> elements, {} loops)",
        regions.len(),
        regions.len(),
        path_count
    );
    let pct = if opt.points_before > 0 {
        (1.0 - opt.points_after as f64 / opt.points_before as f64) * 100.0
    } else {
        0.0
    };
    eprintln!(
        "optimize:   {} loops, {} -> {} control points ({pct:.1}% fewer)",
        opt.loops, opt.points_before, opt.points_after
    );
    let mut segments = vec![format!("load {:.2} ms", t.load_ms)];
    if let Some(q) = t.quantize_ms {
        segments.push(format!("quantize {q:.2} ms"));
    }
    segments.push(format!("xbrz {:.2} ms", t.scale_ms));
    segments.push(format!("vectorize {:.2} ms", t.vectorize_ms));
    segments.push(format!("optimize {:.2} ms", t.optimize_ms));
    segments.push(format!("export {:.2} ms", t.export_ms));
    segments.push(format!("write {:.2} ms", t.write_ms));
    let total_ms = t.load_ms
        + t.quantize_ms.unwrap_or(0.0)
        + t.scale_ms
        + t.vectorize_ms
        + t.optimize_ms
        + t.export_ms
        + t.write_ms;
    eprintln!(
        "timing:     {} | total {:.2} ms",
        segments.join(" | "),
        total_ms
    );
    eprintln!(
        "svg:        {:.1} KB ({} bytes)",
        bytes as f64 / 1024.0,
        bytes
    );
}
