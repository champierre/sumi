//! Color math shared by content streams, images and shadings.
//!
//! The RGB and CMYK formulas are the ones the PDF specification gives for converting device
//! colors to DeviceGray (ISO 32000-1, 10.3), which is also what Acrobat and Ghostscript use.
//! [`GrayModel::Colorimetric`] を選んだときの RGB だけは、値を sRGB として輝度を求める。

use std::sync::OnceLock;

use crate::{GrayModel, Mode};

pub(crate) fn clamp01(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

pub(crate) fn rgb_to_gray(r: f64, g: f64, b: f64, model: GrayModel) -> f64 {
    match model {
        GrayModel::Luma => clamp01(0.30 * r + 0.59 * g + 0.11 * b),
        GrayModel::Colorimetric => {
            let y = 0.2126 * srgb_to_linear(r)
                + 0.7152 * srgb_to_linear(g)
                + 0.0722 * srgb_to_linear(b);
            linear_to_srgb(y)
        }
    }
}

/// sRGB の値をリニアな値に戻す（IEC 61966-2-1）。
fn srgb_to_linear(v: f64) -> f64 {
    let v = clamp01(v);
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// リニアな値を sRGB の曲線で戻す。[`srgb_to_linear`] の逆関数。
fn linear_to_srgb(y: f64) -> f64 {
    let y = clamp01(y);
    if y <= 0.0031308 {
        12.92 * y
    } else {
        clamp01(1.055 * y.powf(1.0 / 2.4) - 0.055)
    }
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

/// 8bit の画素を [`GrayModel::Colorimetric`] でグレーにするための変換表。
pub(crate) struct Srgb8 {
    /// 8bit の値 → リニアな値（0〜65535）。
    linear: [u32; 256],
    /// 輝度（0〜65535）→ 8bit のグレー。
    encode: Box<[u8]>,
}

impl Srgb8 {
    /// 輝度の係数。合計が 65536 になるように丸めてある。
    const WEIGHTS: [u32; 3] = [13933, 46871, 4732];

    pub(crate) fn get() -> &'static Srgb8 {
        static TABLES: OnceLock<Srgb8> = OnceLock::new();
        TABLES.get_or_init(|| Srgb8 {
            linear: std::array::from_fn(|v| {
                (srgb_to_linear(v as f64 / 255.0) * 65535.0).round() as u32
            }),
            encode: (0..=65535u32)
                .map(|y| (linear_to_srgb(y as f64 / 65535.0) * 255.0).round() as u8)
                .collect(),
        })
    }

    /// [`rgb_to_gray`] の [`GrayModel::Colorimetric`] を 8bit の画素で計算する。
    pub(crate) fn gray(&self, r: u8, g: u8, b: u8) -> u8 {
        let [wr, wg, wb] = Self::WEIGHTS;
        let y = wr * self.linear[r as usize]
            + wg * self.linear[g as usize]
            + wb * self.linear[b as usize];
        // 最大でも 65536 × 65535 + 32768 なので u32 に収まる。
        self.encode[((y + 32768) >> 16) as usize]
    }
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
        assert!((rgb_to_gray(1.0, 0.0, 0.0, GrayModel::Luma) - 0.30).abs() < 1e-9);
        assert!((rgb_to_gray(1.0, 1.0, 1.0, GrayModel::Luma) - 1.0).abs() < 1e-9);
        assert_eq!(cmyk_to_gray(0.0, 0.0, 0.0, 1.0), 0.0);
        assert_eq!(cmyk_to_gray(0.0, 0.0, 0.0, 0.0), 1.0);
        assert!((cmyk_to_gray(1.0, 0.0, 0.0, 0.0) - 0.70).abs() < 1e-9);
    }

    #[test]
    fn colorimetric_follows_srgb_luminance() {
        let gray = |r, g, b| rgb_to_gray(r, g, b, GrayModel::Colorimetric);
        // Ghostscript の既定（ICC）の出力は赤 0.506、緑 0.863、青 0.271、(0.2,0.4,0.8) 0.408。
        assert!((gray(1.0, 0.0, 0.0) - 0.498).abs() < 1e-3);
        assert!((gray(0.0, 1.0, 0.0) - 0.862).abs() < 1e-3);
        assert!((gray(0.0, 0.0, 1.0) - 0.298).abs() < 1e-3);
        assert!((gray(0.2, 0.4, 0.8) - 0.418).abs() < 1e-3);
        assert_eq!(gray(0.0, 0.0, 0.0), 0.0);
        assert!((gray(1.0, 1.0, 1.0) - 1.0).abs() < 1e-9);
        // 無彩色の値は変わらない。
        for v in [0.01, 0.2, 0.5, 0.8] {
            assert!((gray(v, v, v) - v).abs() < 1e-9, "{v}");
        }
        // 範囲外の値は丸める。
        assert_eq!(gray(-1.0, f64::NAN, -0.5), 0.0);
        assert!((gray(2.0, 2.0, 2.0) - 1.0).abs() < 1e-9);
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
            let float = |model| {
                rgb_to_gray(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, model) * 255.0
            };
            assert!(
                (rgb8_to_gray(r, g, b) as f64 - float(GrayModel::Luma)).abs() <= 1.0,
                "{r} {g} {b}"
            );
            assert!(
                (Srgb8::get().gray(r, g, b) as f64 - float(GrayModel::Colorimetric)).abs() <= 0.5,
                "{r} {g} {b}"
            );
        }
        for v in 0..=255u8 {
            assert_eq!(Srgb8::get().gray(v, v, v), v);
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
        assert_eq!(tone.apply(rgb_to_gray(0.5, 0.5, 0.5, GrayModel::Luma)), 1.0);
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
