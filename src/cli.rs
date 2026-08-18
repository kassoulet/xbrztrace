//! Command-line interface: argument parsing and validation.

use std::path::PathBuf;

use clap::Parser;

/// xBRZ scaling factor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    X2,
    X3,
    X4,
    X5,
    X6,
}

impl Scale {
    pub fn factor(self) -> u8 {
        match self {
            Scale::X2 => 2,
            Scale::X3 => 3,
            Scale::X4 => 4,
            Scale::X5 => 5,
            Scale::X6 => 6,
        }
    }
}

fn parse_scale(s: &str) -> Result<Scale, String> {
    match s {
        "2x" | "2" => Ok(Scale::X2),
        "3x" | "3" => Ok(Scale::X3),
        "4x" | "4" => Ok(Scale::X4),
        "5x" | "5" => Ok(Scale::X5),
        "6x" | "6" => Ok(Scale::X6),
        _ => Err(format!(
            "invalid scale `{s}`: expected one of 2x, 3x, 4x, 5x, 6x"
        )),
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "brztracer",
    version,
    about = "Convert pixel art (PNG/JPEG) into scalable SVG vectors using the xBRZ algorithm"
)]
pub struct Cli {
    /// Path to the input raster file (PNG or JPEG).
    #[arg(short, long, value_name = "PATH")]
    pub input: PathBuf,

    /// Output path for the generated .svg file.
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,

    /// xBRZ scaling factor (2x–6x).
    #[arg(
        short,
        long,
        value_name = "2x|3x|4x|5x|6x",
        value_parser = parse_scale,
        default_value = "4x"
    )]
    pub scale: Scale,

    /// Group identical RGBA fill colors into single compound <path> elements.
    /// Pass --merge-colors=false to emit one path per connected region.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "true",
        default_value_t = true
    )]
    pub merge_colors: bool,

    /// Print timing metrics, input/output dimensions, path counts and file
    /// size stats to stderr.
    #[arg(short, long)]
    pub verbose: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses_all_flags() {
        let cli = Cli::try_parse_from([
            "brztracer",
            "-i",
            "in.png",
            "-o",
            "out.svg",
            "-s",
            "3x",
            "--merge-colors=false",
            "-v",
        ])
        .unwrap();
        assert_eq!(cli.scale, Scale::X3);
        assert!(!cli.merge_colors);
        assert!(cli.verbose);
        assert_eq!(cli.input.to_str().unwrap(), "in.png");
    }

    #[test]
    fn cli_defaults() {
        let cli = Cli::try_parse_from(["brztracer", "-i", "in.png", "-o", "out.svg"]).unwrap();
        assert_eq!(cli.scale, Scale::X4);
        assert!(cli.merge_colors);
        assert!(!cli.verbose);
    }

    #[test]
    fn bare_merge_colors_flag_means_true() {
        let cli = Cli::try_parse_from([
            "brztracer",
            "-i",
            "in.png",
            "-o",
            "out.svg",
            "--merge-colors",
        ])
        .unwrap();
        assert!(cli.merge_colors);
    }

    #[test]
    fn invalid_scale_is_rejected() {
        let err = Cli::try_parse_from(["brztracer", "-i", "a.png", "-o", "b.svg", "-s", "9x"]);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("9x"), "expected error about 9x, got: {msg}");
    }

    #[test]
    fn missing_required_args_is_an_error() {
        assert!(Cli::try_parse_from(["brztracer"]).is_err());
    }

    #[test]
    fn help_mentions_scale_values() {
        let mut cmd = Cli::command();
        let help = cmd.render_long_help().to_string();
        assert!(help.contains("2x"));
        assert!(help.contains("6x"));
    }
}
