//! Color math shared by content streams, images and shadings.
//!
//! The RGB and CMYK formulas are the ones the PDF specification gives for converting device
//! colors to DeviceGray (ISO 32000-1, 10.3), which is also what Acrobat and Ghostscript use.

use crate::Mode;

pub(crate) fn clamp01(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

pub(crate) fn rgb_to_gray(r: f64, g: f64, b: f64) -> f64 {
    clamp01(0.30 * r + 0.59 * g + 0.11 * b)
}

pub(crate) fn cmyk_to_gray(c: f64, m: f64, y: f64, k: f64) -> f64 {
    clamp01(1.0 - (0.30 * c + 0.59 * m + 0.11 * y + k).min(1.0))
}

/// Approximates the lightness of a CIE L*a*b* color.
pub(crate) fn lab_to_gray(l: f64) -> f64 {
    clamp01(l / 100.0)
}

/// Integer version of [`rgb_to_gray`] for 8-bit samples (weights 77/151/28 out of 256).
pub(crate) fn rgb8_to_gray(r: u8, g: u8, b: u8) -> u8 {
    ((77 * r as u32 + 151 * g as u32 + 28 * b as u32 + 128) >> 8) as u8
}

/// Integer version of [`cmyk_to_gray`] for 8-bit samples.
pub(crate) fn cmyk8_to_gray(c: u8, m: u8, y: u8, k: u8) -> u8 {
    let ink = ((77 * c as u32 + 151 * m as u32 + 28 * y as u32 + 128) >> 8) + k as u32;
    255 - ink.min(255) as u8
}

/// Maps a gray level to the output tone for the selected mode.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tone {
    pub mode: Mode,
    pub threshold: f64,
}

impl Tone {
    /// Tolerance so that e.g. `0.5 0.5 0.5 rg` is not pushed below a 0.5 threshold by rounding.
    const EPSILON: f64 = 1e-6;

    pub(crate) fn apply(self, gray: f64) -> f64 {
        let gray = clamp01(gray);
        match self.mode {
            Mode::Grayscale => gray,
            Mode::Monochrome => {
                if gray + Self::EPSILON < self.threshold {
                    0.0
                } else {
                    1.0
                }
            }
        }
    }

    pub(crate) fn is_monochrome(self) -> bool {
        self.mode == Mode::Monochrome
    }
}

/// Formats a number for a content stream: at most four decimals, no exponent, no trailing zeros.
pub(crate) fn format_number(v: f64) -> String {
    let rounded = (v * 10000.0).round() / 10000.0;
    if rounded == 0.0 || !rounded.is_finite() {
        return "0".to_string();
    }
    let s = format!("{rounded:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_conversions_follow_pdf_spec() {
        assert!((rgb_to_gray(1.0, 0.0, 0.0) - 0.30).abs() < 1e-9);
        assert!((rgb_to_gray(1.0, 1.0, 1.0) - 1.0).abs() < 1e-9);
        assert_eq!(cmyk_to_gray(0.0, 0.0, 0.0, 1.0), 0.0);
        assert_eq!(cmyk_to_gray(0.0, 0.0, 0.0, 0.0), 1.0);
        assert!((cmyk_to_gray(1.0, 0.0, 0.0, 0.0) - 0.70).abs() < 1e-9);
    }

    #[test]
    fn integer_conversions_match_float() {
        for &(r, g, b) in &[
            (255u8, 0u8, 0u8),
            (0, 255, 0),
            (0, 0, 255),
            (12, 200, 99),
            (255, 255, 255),
        ] {
            let expected =
                rgb_to_gray(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0) * 255.0;
            assert!(
                (rgb8_to_gray(r, g, b) as f64 - expected).abs() <= 1.0,
                "{r} {g} {b}"
            );
        }
        assert_eq!(cmyk8_to_gray(0, 0, 0, 255), 0);
        assert_eq!(cmyk8_to_gray(0, 0, 0, 0), 255);
    }

    #[test]
    fn monochrome_threshold() {
        let tone = Tone {
            mode: Mode::Monochrome,
            threshold: 0.5,
        };
        assert_eq!(tone.apply(0.3), 0.0);
        assert_eq!(tone.apply(0.7), 1.0);
        assert_eq!(tone.apply(rgb_to_gray(0.5, 0.5, 0.5)), 1.0);
    }

    #[test]
    fn number_formatting() {
        assert_eq!(format_number(0.3), "0.3");
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(-0.00001), "0");
        assert_eq!(format_number(0.123456), "0.1235");
    }
}
