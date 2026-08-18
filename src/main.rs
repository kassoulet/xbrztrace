//! BRZtracer binary entry point: parse the CLI, run the pipeline, report
//! verbose metrics when requested.

use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;

use brztracer::cli::Cli;
use brztracer::image_loader;
use brztracer::svg_exporter;
use brztracer::vectorizer;
use brztracer::xbrz_engine::{self, ScalerConfig};

struct Timing {
    load_ms: f64,
    scale_ms: f64,
    vectorize_ms: f64,
    export_ms: f64,
    write_ms: f64,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("brztracer: error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // 1. Ingest
    let t0 = Instant::now();
    let image = image_loader::load(&cli.input)?;
    let t_load = t0.elapsed();

    // 2. xBRZ upscale
    let t1 = Instant::now();
    let scaled = xbrz_engine::scale_image(&image, cli.scale.factor(), &ScalerConfig::default());
    let t_scale = t1.elapsed();

    // 3. Vector trace
    let t2 = Instant::now();
    let regions = vectorizer::vectorize(&scaled, cli.merge_colors);
    let t_vectorize = t2.elapsed();

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
            scale_ms: t_scale.as_secs_f64() * 1000.0,
            vectorize_ms: t_vectorize.as_secs_f64() * 1000.0,
            export_ms: t_export.as_secs_f64() * 1000.0,
            write_ms: t_write.as_secs_f64() * 1000.0,
        };
        print_stats(&cli, &image, &scaled, &regions, &svg, &timing);
    }

    Ok(())
}

fn print_stats(
    cli: &Cli,
    image: &brztracer::xbrz_engine::ArgbImage,
    scaled: &brztracer::xbrz_engine::ArgbImage,
    regions: &[vectorizer::Region],
    svg: &str,
    t: &Timing,
) {
    let path_count: usize = regions.iter().map(|r| r.loops.len()).sum();
    let bytes = svg.len();
    let total_ms = t.load_ms + t.scale_ms + t.vectorize_ms + t.export_ms + t.write_ms;
    eprintln!("input:      {}x{} px", image.width, image.height);
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
    eprintln!(
        "timing:     load {:.2} ms | xbrz {:.2} ms | vectorize {:.2} ms | export {:.2} ms | write {:.2} ms | total {:.2} ms",
        t.load_ms, t.scale_ms, t.vectorize_ms, t.export_ms, t.write_ms, total_ms
    );
    eprintln!(
        "svg:        {:.1} KB ({} bytes)",
        bytes as f64 / 1024.0,
        bytes
    );
}
