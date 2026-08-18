//! CLI integration tests: exercise the compiled `brztracer` binary end to end.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use image::ImageEncoder;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_brztracer"))
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

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("brztracer_cli_{name}"));
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
fn missing_input_file_fails_cleanly() {
    let out = run(&[
        "-i",
        "/nonexistent/brztracer_missing.png",
        "-o",
        "/tmp/brztracer_should_not_exist.svg",
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
    assert!(text.contains("2x|3x|4x|5x|6x"));

    let version = run(&["--version"]);
    assert!(version.status.success());
    let text = String::from_utf8_lossy(&version.stdout);
    assert!(text.contains("brztracer"));
}
