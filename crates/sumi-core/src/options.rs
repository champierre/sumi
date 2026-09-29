use std::fmt;
use std::str::FromStr;

/// Conversion mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Convert every color to a shade of gray.
    #[default]
    Grayscale,
    /// Convert every color to pure black or pure white.
    Monochrome,
}

impl FromStr for Mode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "grayscale" | "gray" => Ok(Mode::Grayscale),
            "monochrome" | "mono" => Ok(Mode::Monochrome),
            other => Err(format!(
                "unknown mode `{other}` (expected grayscale or monochrome)"
            )),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Mode::Grayscale => "grayscale",
            Mode::Monochrome => "monochrome",
        })
    }
}

/// RGB の色をグレーに変換する方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GrayModel {
    /// ガンマ補正済みの値にそのまま `0.30 × R + 0.59 × G + 0.11 × B` を掛ける。PDF 仕様の式。
    #[default]
    Luma,
    /// 値を sRGB として読み、リニアに戻して Rec. 709 の係数で輝度を求め、sRGB の曲線で
    /// グレーの値に戻す。[`GrayModel::Luma`] より原色が明るくなり、Ghostscript が既定で行う
    /// ICC の変換に近い。
    Colorimetric,
}

impl FromStr for GrayModel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "luma" => Ok(GrayModel::Luma),
            "colorimetric" => Ok(GrayModel::Colorimetric),
            other => Err(format!(
                "unknown gray model `{other}` (expected luma or colorimetric)"
            )),
        }
    }
}

impl fmt::Display for GrayModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            GrayModel::Luma => "luma",
            GrayModel::Colorimetric => "colorimetric",
        })
    }
}

/// Options for [`convert`](crate::convert) and [`convert_bytes`](crate::convert_bytes).
///
/// Construct with [`ConvertOptions::default`], [`ConvertOptions::grayscale`] or
/// [`ConvertOptions::monochrome`] and adjust the public fields. New fields may be added in
/// minor releases.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ConvertOptions {
    pub mode: Mode,
    /// RGB の色をグレーに変換する方式。
    pub gray_model: GrayModel,
    /// Gray level in `0.0..=1.0` below which a color becomes black in monochrome mode.
    pub threshold: f32,
    /// Use Floyd–Steinberg dithering instead of a hard threshold for images in monochrome mode.
    pub dither: bool,
    /// Fail with [`SumiError::Unsupported`](crate::SumiError::Unsupported) instead of leaving
    /// unconvertible parts of the document unchanged.
    pub strict: bool,
    pub limits: Limits,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions {
            mode: Mode::Grayscale,
            gray_model: GrayModel::Luma,
            threshold: 0.5,
            dither: false,
            strict: false,
            limits: Limits::default(),
        }
    }
}

impl ConvertOptions {
    pub fn grayscale() -> Self {
        Self::default()
    }

    pub fn monochrome(threshold: f32) -> Self {
        ConvertOptions {
            mode: Mode::Monochrome,
            threshold,
            ..Self::default()
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.threshold) {
            return Err(format!(
                "threshold must be between 0.0 and 1.0, got {}",
                self.threshold
            ));
        }
        Ok(())
    }
}

/// Options in the form the converter uses internally.
#[derive(Debug, Clone)]
pub(crate) struct Settings {
    pub tone: crate::color::Tone,
    pub gray_model: GrayModel,
    pub dither: bool,
    pub limits: Limits,
}

impl From<&ConvertOptions> for Settings {
    fn from(options: &ConvertOptions) -> Self {
        Settings {
            tone: crate::color::Tone {
                mode: options.mode,
                threshold: options.threshold as f64,
            },
            gray_model: options.gray_model,
            dither: options.dither,
            limits: options.limits.clone(),
        }
    }
}

/// Limits that protect against malicious or broken input.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Limits {
    /// Maximum decompressed size of a single stream, in bytes.
    pub max_stream_bytes: usize,
    /// Maximum number of pixels in a single image.
    pub max_image_pixels: u64,
    /// Maximum nesting depth of form XObjects, patterns and similar structures.
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_stream_bytes: 256 * 1024 * 1024,
            max_image_pixels: 100_000_000,
            max_depth: 32,
        }
    }
}
