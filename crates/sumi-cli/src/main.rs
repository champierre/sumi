use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use sumi_core::{ConvertOptions, Mode, SumiError};

/// Convert the colors of a PDF to grayscale or monochrome, keeping text and vector graphics.
#[derive(Debug, Parser)]
#[command(name = "sumi", version, after_help = EXIT_CODES)]
struct Args {
    /// Input PDF file, or `-` for standard input
    input: PathBuf,

    /// Output PDF file, or `-` for standard output
    #[arg(short, long)]
    output: PathBuf,

    /// Conversion mode
    #[arg(long, value_enum, default_value_t = ModeArg::Grayscale)]
    mode: ModeArg,

    /// Gray level (0.0-1.0) below which colors become black in monochrome mode
    #[arg(long, default_value_t = 0.5, value_parser = parse_threshold)]
    threshold: f32,

    /// Dither images instead of thresholding them in monochrome mode
    #[arg(long)]
    dither: bool,

    /// Fail instead of leaving unsupported parts of the document unchanged
    #[arg(long)]
    strict: bool,

    /// Replace the output file if it already exists
    #[arg(long)]
    overwrite: bool,

    /// Abort when the conversion takes longer than this many seconds
    #[arg(long, value_name = "SECONDS")]
    timeout: Option<u64>,

    /// Print conversion statistics
    #[arg(short, long)]
    verbose: bool,
}

const EXIT_CODES: &str = "Exit codes:
  0  success
  1  generic error
  2  invalid arguments
  3  invalid PDF
  4  unsupported PDF feature (encrypted PDF, or --strict found unconverted parts)
  5  I/O error
  6  timeout";

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ModeArg {
    Grayscale,
    Monochrome,
}

fn parse_threshold(s: &str) -> Result<f32, String> {
    let value: f32 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err("must be between 0.0 and 1.0".to_string())
    }
}

struct Failure {
    code: u8,
    message: String,
}

impl Failure {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Failure {
            code,
            message: message.into(),
        }
    }
}

impl From<SumiError> for Failure {
    fn from(err: SumiError) -> Self {
        let code = match &err {
            SumiError::InvalidOptions(_) => 2,
            SumiError::InvalidPdf(_) => 3,
            SumiError::EncryptedPdf | SumiError::Unsupported(_) => 4,
            SumiError::Io(_) => 5,
            _ => 1,
        };
        Failure::new(code, err.to_string())
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    if let Some(seconds) = args.timeout {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(seconds));
            eprintln!("sumi: timed out after {seconds} seconds");
            std::process::exit(6);
        });
    }
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("sumi: {}", failure.message);
            ExitCode::from(failure.code)
        }
    }
}

fn is_stdio(path: &Path) -> bool {
    path.as_os_str() == "-"
}

fn run(args: &Args) -> Result<(), Failure> {
    if !is_stdio(&args.output) {
        if args.output.exists() && !args.overwrite {
            return Err(Failure::new(
                2,
                format!(
                    "{} already exists (use --overwrite to replace it)",
                    args.output.display()
                ),
            ));
        }
        if !is_stdio(&args.input)
            && let (Ok(input), Ok(output)) = (args.input.canonicalize(), args.output.canonicalize())
            && input == output
        {
            return Err(Failure::new(2, "input and output must be different files"));
        }
    }

    let data = if is_stdio(&args.input) {
        let mut data = Vec::new();
        std::io::stdin()
            .read_to_end(&mut data)
            .map_err(|e| Failure::new(5, format!("cannot read standard input: {e}")))?;
        data
    } else {
        std::fs::read(&args.input)
            .map_err(|e| Failure::new(5, format!("cannot read {}: {e}", args.input.display())))?
    };

    let mut options = ConvertOptions::default();
    options.mode = match args.mode {
        ModeArg::Grayscale => Mode::Grayscale,
        ModeArg::Monochrome => Mode::Monochrome,
    };
    options.threshold = args.threshold;
    options.dither = args.dither;
    options.strict = args.strict;

    let converted = sumi_core::convert_bytes(&data, &options)?;
    let report = &converted.report;
    for warning in &report.warnings {
        let count = if warning.count > 1 {
            format!(" ({} times)", warning.count)
        } else {
            String::new()
        };
        eprintln!("sumi: warning: {}{count}", warning.message);
    }
    if args.verbose {
        eprintln!(
            "sumi: {} pages, {} content streams, {} color operators, {} images, {} shadings converted to {}",
            report.pages,
            report.content_streams,
            report.color_operators,
            report.images,
            report.shadings,
            options.mode
        );
    }

    if is_stdio(&args.output) {
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(&converted.pdf)
            .and_then(|()| stdout.flush())
            .map_err(|e| Failure::new(5, format!("cannot write standard output: {e}")))?;
    } else {
        write_atomically(&args.output, &converted.pdf)
            .map_err(|e| Failure::new(5, format!("cannot write {}: {e}", args.output.display())))?;
    }
    Ok(())
}

fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!(".{file_name}.sumi-{}.tmp", std::process::id()));
    let result = std::fs::write(&temp, data).and_then(|()| std::fs::rename(&temp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
