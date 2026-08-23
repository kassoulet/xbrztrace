//! CLI integration tests: exercise the compiled `xbrztrace` binary end to end.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use image::ImageEncoder;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xbrztrace"))
}

/// Write a 16x16 RGBA sprite (the ghost) as a PNG.
fn write_ghost_png(path: &Path) {
    let rows = [
        "................",
        "..##########....",
        ".#RRRRRRRRR#....",
        ".#RRRRRRRRR#....",
        ".#RWWWWWWRR#....",
        ".#RWWWWWWRR#....",
        ".#RWWWWWWRR#....",
        ".#RRRRRRRRR#....",
        ".#RKKRKKRRR#....",
        ".#RKKRKKRRR#....",
        ".#RRRRRRRRR#....",
        ".#RRRRRRRRR#....",
        ".#RRRRRRRRR#....",
        "..####..####....",
        "................",
        "................",
    ];
    let palette: std::collections::HashMap<char, [u8; 4]> = [
        ('.', [0, 0, 0, 0]),
        ('#', [40, 40, 48, 255]),
        ('R', [220, 60, 60, 255]),
        ('W', [240, 240, 245, 255]),
        ('K', [20, 20, 25, 255]),
    ]
    .into_iter()
    .collect();
    let mut rgba = Vec::new();
    for row in rows {
        for ch in row.chars() {
            rgba.extend_from_slice(&palette[&ch]);
        }
    }
    let mut file = std::fs::File::create(path).unwrap();
    let encoder = image::codecs::png::PngEncoder::new(&mut file);
    encoder
        .write_image(&rgba, 16, 16, image::ExtendedColorType::Rgba8)
        .unwrap();
}

/// Write the 16x16 ghost with deterministic per-channel noise (±jitter on
/// every RGB channel), simulating the near-duplicate colors JPEG compression
/// produces around edges.
fn write_noisy_png(path: &Path, jitter: i16) {
    let rows = [
        "................",
        "..##########....",
        ".#RRRRRRRRR#....",
        ".#RRRRRRRRR#....",
        ".#RWWWWWWRR#....",
        ".#RWWWWWWRR#....",
        ".#RWWWWWWRR#....",
        ".#RRRRRRRRR#....",
        ".#RKKRKKRRR#....",
        ".#RKKRKKRRR#....",
        ".#RRRRRRRRR#....",
        ".#RRRRRRRRR#....",
        ".#RRRRRRRRR#....",
        "..####..####....",
        "................",
        "................",
    ];
    let palette: std::collections::HashMap<char, [u8; 4]> = [
        ('.', [0, 0, 0, 0]),
        ('#', [40, 40, 48, 255]),
        ('R', [220, 60, 60, 255]),
        ('W', [240, 240, 245, 255]),
        ('K', [20, 20, 25, 255]),
    ]
    .into_iter()
    .collect();

    let mut state: u64 = 0x1234_5678_9abc_def0;
    let mut jitter = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 33) as i16 % (2 * jitter + 1)) - jitter
    };

    let mut rgba = Vec::new();
    for row in rows {
        for ch in row.chars() {
            let base = palette[&ch];
            let mut px = [0u8; 4];
            for i in 0..3 {
                px[i] = (base[i] as i16 + jitter()).clamp(0, 255) as u8;
            }
            px[3] = base[3];
            rgba.extend_from_slice(&px);
        }
    }
    let mut file = std::fs::File::create(path).unwrap();
    let encoder = image::codecs::png::PngEncoder::new(&mut file);
    encoder
        .write_image(&rgba, 16, 16, image::ExtendedColorType::Rgba8)
        .unwrap();
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xbrztrace_cli_{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().expect("failed to spawn binary")
}

fn read_svg(path: &Path) -> String {
    std::fs::read_to_string(path).expect("output svg missing")
}

#[test]
fn converts_png_to_svg() {
    let dir = temp_dir("convert");
    let input = dir.join("ghost.png");
    let output = dir.join("ghost.svg");
    write_ghost_png(&input);

    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "-s",
        "4x",
    ]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let svg = read_svg(&output);
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("width=\"64\" height=\"64\" viewBox=\"0 0 64 64\""));
    assert!(svg.contains("<path "));
    assert!(svg.ends_with("</svg>\n"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn all_scale_factors_work() {
    let dir = temp_dir("scales");
    let input = dir.join("ghost.png");
    write_ghost_png(&input);
    for scale in ["2x", "3x", "4x", "5x", "6x"] {
        let output = dir.join(format!("ghost_{scale}.svg"));
        let out = run(&[
            "-i",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "-s",
            scale,
        ]);
        assert!(
            out.status.success(),
            "scale {scale} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(output.exists());
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn default_scale_is_4x() {
    let dir = temp_dir("default_scale");
    let input = dir.join("ghost.png");
    let output = dir.join("ghost.svg");
    write_ghost_png(&input);
    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let svg = read_svg(&output);
    assert!(svg.contains("width=\"64\" height=\"64\""));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_merge_colors_emits_more_paths() {
    let dir = temp_dir("nomerge");
    let input = dir.join("ghost.png");
    write_ghost_png(&input);

    let merged = dir.join("merged.svg");
    let out1 = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        merged.to_str().unwrap(),
    ]);
    assert!(out1.status.success());
    let merged_svg = read_svg(&merged);
    let merged_count = merged_svg.matches("<path ").count();

    let unmerged = dir.join("unmerged.svg");
    let out2 = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        unmerged.to_str().unwrap(),
        "--merge-colors=false",
    ]);
    assert!(out2.status.success());
    let unmerged_svg = read_svg(&unmerged);
    let unmerged_count = unmerged_svg.matches("<path ").count();

    assert!(unmerged_count > merged_count);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn verbose_reports_stats_on_stderr() {
    let dir = temp_dir("verbose");
    let input = dir.join("ghost.png");
    let output = dir.join("ghost.svg");
    write_ghost_png(&input);

    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "-v",
    ]);
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("input:"), "stderr: {stderr}");
    assert!(stderr.contains("16x16"));
    assert!(stderr.contains("64x64"));
    assert!(stderr.contains("svg:"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn verbose_reports_optimize_stats() {
    let dir = temp_dir("verbose_opt");
    let input = dir.join("ghost.png");
    let output = dir.join("ghost.svg");
    write_ghost_png(&input);

    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "-v",
    ]);
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("optimize:"), "stderr: {stderr}");
    assert!(stderr.contains("control points"), "stderr: {stderr}");
    std::fs::remove_dir_all(&dir).ok();
}

/// Write a 4x4 image with one 2x2 red blob on a transparent background:
/// a single color forming one connected region.
fn write_single_blob_png(path: &Path) {
    let mut rgba = vec![0u8; 4 * 4 * 4];
    for y in 1..3 {
        for x in 1..3 {
            let i = (y * 4 + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
    }
    let mut file = std::fs::File::create(path).unwrap();
    let encoder = image::codecs::png::PngEncoder::new(&mut file);
    encoder
        .write_image(&rgba, 4, 4, image::ExtendedColorType::Rgba8)
        .unwrap();
}

#[test]
fn optimized_output_omits_redundant_fill_rule() {
    let dir = temp_dir("opt_attrs");
    let input = dir.join("blob.png");
    let output = dir.join("blob.svg");
    write_single_blob_png(&input);

    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let svg = read_svg(&output);
    // The solid blob is a single connected loop, so its path (the opaque
    // #ff0000 fill) must drop the redundant evenodd attribute. xBRZ edge
    // blends produce separate translucent multi-loop paths that keep it.
    let solid = svg
        .lines()
        .find(|l| l.contains("fill=\"#ff0000\"") && !l.contains("fill-opacity"))
        .expect("solid blob path missing");
    assert!(
        !solid.contains("fill-rule"),
        "single-loop path must omit fill-rule: {solid}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn multi_loop_paths_keep_fill_rule() {
    let dir = temp_dir("opt_fillrule");
    let input = dir.join("ghost.png");
    let output = dir.join("ghost.svg");
    write_ghost_png(&input);

    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let svg = read_svg(&output);
    // The ghost's outline color forms three disconnected bars, so at least
    // one path must keep even-odd fill — but not every path (the red body
    // is a single loop and drops it).
    let paths = svg.matches("<path ").count();
    let fill_rules = svg.matches("fill-rule").count();
    assert!(paths > 0);
    assert!(fill_rules > 0, "multi-loop colors need evenodd: {svg}");
    assert!(
        fill_rules < paths,
        "single-loop paths should omit fill-rule: {svg}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn quantize_reduces_path_count_on_noisy_input() {
    let dir = temp_dir("quantize");
    let input = dir.join("noisy.png");
    write_noisy_png(&input, 8);

    let plain = dir.join("plain.svg");
    let out = run(&["-i", input.to_str().unwrap(), "-o", plain.to_str().unwrap()]);
    assert!(out.status.success());
    let plain_count = read_svg(&plain).matches("<path ").count();

    let quantized = dir.join("quantized.svg");
    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        quantized.to_str().unwrap(),
        "--quantize",
    ]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let quantized_count = read_svg(&quantized).matches("<path ").count();

    let quantized64 = dir.join("quantized64.svg");
    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        quantized64.to_str().unwrap(),
        "--quantize",
        "64",
    ]);
    assert!(out.status.success());
    let quantized64_count = read_svg(&quantized64).matches("<path ").count();

    // JPEG-style noise explodes the path count; quantization collapses it.
    assert!(
        quantized_count < plain_count,
        "quantize should reduce paths: plain {plain_count}, quantized {quantized_count}"
    );
    assert!(
        quantized64_count < plain_count,
        "quantize 64 should reduce paths: plain {plain_count}, quantized {quantized64_count}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn invalid_quantize_tolerance_fails_cleanly() {
    let dir = temp_dir("badquant");
    let input = dir.join("ghost.png");
    write_ghost_png(&input);
    for bad in ["0", "abc", "nan"] {
        let output = dir.join(format!("out_{bad}.svg"));
        let out = run(&[
            "-i",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--quantize",
            bad,
        ]);
        assert!(
            !out.status.success(),
            "tolerance {bad:?} should be rejected"
        );
        assert!(!output.exists());
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn verbose_with_quantize_reports_merge_stats() {
    let dir = temp_dir("quantize_verbose");
    let input = dir.join("noisy.png");
    write_noisy_png(&input, 6);
    let output = dir.join("out.svg");
    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--quantize",
        "-v",
    ]);
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("quantize:"), "stderr: {stderr}");
    assert!(stderr.contains("->"), "stderr: {stderr}");
    assert!(stderr.contains("quantize"), "stderr: {stderr}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_input_file_fails_cleanly() {
    let out = run(&[
        "-i",
        "/nonexistent/xbrztrace_missing.png",
        "-o",
        "/tmp/xbrztrace_should_not_exist.svg",
    ]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot open input image"),
        "stderr: {stderr}"
    );
}

#[test]
fn corrupt_input_fails_cleanly() {
    let dir = temp_dir("corrupt");
    let input = dir.join("corrupt.png");
    std::fs::File::create(&input)
        .unwrap()
        .write_all(b"definitely not an image")
        .unwrap();
    let output = dir.join("out.svg");
    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(!output.exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("failed to decode image"),
        "stderr: {stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn invalid_scale_fails_cleanly() {
    let dir = temp_dir("badscale");
    let input = dir.join("ghost.png");
    write_ghost_png(&input);
    let output = dir.join("out.svg");
    let out = run(&[
        "-i",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "-s",
        "9x",
    ]);
    assert!(!out.status.success());
    assert!(!output.exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("9x"), "stderr: {stderr}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn help_and_version_work() {
    let help = run(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("--input"));
    assert!(text.contains("--merge-colors"));
    assert!(text.contains("--quantize"));
    assert!(text.contains("2x|3x|4x|5x|6x"));

    let version = run(&["--version"]);
    assert!(version.status.success());
    let text = String::from_utf8_lossy(&version.stdout);
    assert!(text.contains("xbrztrace"));
}
