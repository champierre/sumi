use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use sumi_core::{ConvertOptions, GrayModel, Mode, Report, SumiError};

mod json_report;

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

    /// How RGB colors are mapped to gray (colorimetric: sRGB luminance, close to Ghostscript)
    #[arg(long, value_enum, default_value_t = GrayModelArg::Luma)]
    gray_model: GrayModelArg,

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

    /// Print a machine-readable report to standard error instead of messages
    #[arg(long, value_enum, value_name = "FORMAT")]
    report: Option<ReportFormat>,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum GrayModelArg {
    Luma,
    Colorimetric,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ReportFormat {
    Json,
}

impl Args {
    fn json_report(&self) -> bool {
        self.report == Some(ReportFormat::Json)
    }

    fn report_settings(&self) -> json_report::Settings<'static> {
        json_report::Settings {
            mode: match self.mode {
                ModeArg::Grayscale => "grayscale",
                ModeArg::Monochrome => "monochrome",
            },
            gray_model: match self.gray_model {
                GrayModelArg::Luma => "luma",
                GrayModelArg::Colorimetric => "colorimetric",
            },
        }
    }
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
    /// Stable identifier of the failure, reported by `--report json`.
    kind: &'static str,
    message: String,
    details: Vec<String>,
    /// The conversion result, when the failure happened after converting. Boxed to keep
    /// `Result<Report, Failure>` small.
    report: Option<Box<Report>>,
}

impl Failure {
    fn new(code: u8, kind: &'static str, message: impl Into<String>) -> Self {
        Failure {
            code,
            kind,
            message: message.into(),
            details: Vec::new(),
            report: None,
        }
    }

    fn with_report(mut self, report: &Report) -> Self {
        self.report = Some(Box::new(report.clone()));
        self
    }
}

impl From<SumiError> for Failure {
    fn from(err: SumiError) -> Self {
        let (code, kind) = match &err {
            SumiError::InvalidOptions(_) => (2, "invalid_arguments"),
            SumiError::InvalidPdf(_) => (3, "invalid_pdf"),
            SumiError::EncryptedPdf => (4, "encrypted_pdf"),
            SumiError::Unsupported(_) => (4, "unsupported"),
            SumiError::Io(_) => (5, "io"),
            SumiError::LimitExceeded(_) => (1, "limit_exceeded"),
            _ => (1, "internal"),
        };
        let mut failure = Failure::new(code, kind, err.to_string());
        if let SumiError::Unsupported(parts) = err {
            failure.details = parts;
        }
        failure
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    if let Some(seconds) = args.timeout {
        let message = format!("timed out after {seconds} seconds");
        // Render the message now, since the thread cannot borrow `args`.
        let output = if args.json_report() {
            let error = json_report::ErrorInfo {
                exit_code: 6,
                kind: "timeout",
                message: &message,
                details: &[],
            };
            json_report::failure(&args.report_settings(), None, &error)
        } else {
            format!("sumi: {message}")
        };
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(seconds));
            eprintln!("{output}");
            std::process::exit(6);
        });
    }
    match run(&args) {
        Ok(report) => {
            if args.json_report() {
                eprintln!("{}", json_report::success(&args.report_settings(), &report));
            }
            ExitCode::SUCCESS
        }
        Err(failure) => {
            if args.json_report() {
                let error = json_report::ErrorInfo {
                    exit_code: failure.code,
                    kind: failure.kind,
                    message: &failure.message,
                    details: &failure.details,
                };
                eprintln!(
                    "{}",
                    json_report::failure(
                        &args.report_settings(),
                        failure.report.as_deref(),
                        &error
                    )
                );
            } else {
                eprintln!("sumi: {}", failure.message);
            }
            ExitCode::from(failure.code)
        }
    }
}

fn is_stdio(path: &Path) -> bool {
    path.as_os_str() == "-"
}

fn run(args: &Args) -> Result<Report, Failure> {
    if !is_stdio(&args.output) {
        if args.output.exists() && !args.overwrite {
            return Err(Failure::new(
                2,
                "invalid_arguments",
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
            return Err(Failure::new(
                2,
                "invalid_arguments",
                "input and output must be different files",
            ));
        }
    }

    let data = if is_stdio(&args.input) {
        let mut data = Vec::new();
        std::io::stdin()
            .read_to_end(&mut data)
            .map_err(|e| Failure::new(5, "io", format!("cannot read standard input: {e}")))?;
        data
    } else {
        std::fs::read(&args.input).map_err(|e| {
            Failure::new(
                5,
                "io",
                format!("cannot read {}: {e}", args.input.display()),
            )
        })?
    };

    let mut options = ConvertOptions::default();
    options.mode = match args.mode {
        ModeArg::Grayscale => Mode::Grayscale,
        ModeArg::Monochrome => Mode::Monochrome,
    };
    options.gray_model = match args.gray_model {
        GrayModelArg::Luma => GrayModel::Luma,
        GrayModelArg::Colorimetric => GrayModel::Colorimetric,
    };
    options.threshold = args.threshold;
    options.dither = args.dither;
    options.strict = args.strict;

    let converted = sumi_core::convert_bytes(&data, &options)?;
    let report = &converted.report;
    // With --report json, standard error carries only the JSON report, which already includes
    // the warnings and the statistics.
    if !args.json_report() {
        print_messages(report, args.verbose, options.mode);
    }

    if is_stdio(&args.output) {
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(&converted.pdf)
            .and_then(|()| stdout.flush())
            .map_err(|e| {
                Failure::new(5, "io", format!("cannot write standard output: {e}"))
                    .with_report(report)
            })?;
    } else {
        write_atomically(&args.output, &converted.pdf).map_err(|e| {
            Failure::new(
                5,
                "io",
                format!("cannot write {}: {e}", args.output.display()),
            )
            .with_report(report)
        })?;
    }
    Ok(converted.report)
}

fn print_messages(report: &Report, verbose: bool, mode: Mode) {
    for warning in &report.warnings {
        let count = if warning.count > 1 {
            format!(" ({} times)", warning.count)
        } else {
            String::new()
        };
        eprintln!("sumi: warning: {}{count}", warning.message);
    }
    if verbose {
        eprintln!(
            "sumi: {} pages, {} content streams, {} color operators, {} images, {} shadings converted to {}",
            report.pages,
            report.content_streams,
            report.color_operators,
            report.images,
            report.shadings,
            mode
        );
    }
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
