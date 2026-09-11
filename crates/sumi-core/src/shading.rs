//! Converts shadings (gradients).
//!
//! The original color function is sampled and replaced by a gray sampled function, so axial,
//! radial and function-based shadings keep their geometry.

use lopdf::{Dictionary, Document, Object, Stream};

use crate::colorspace::ColorSpace;
use crate::error::Problem;
use crate::function::Function;
use crate::objects::{get, get_int, get_numbers, pairs};
use crate::options::Settings;

pub(crate) struct Plan {
    /// A sampled function replacing `/Function`.
    pub function: Stream,
    pub background: Option<Object>,
}

const SAMPLES_1D: usize = 256;
const SAMPLES_2D: usize = 33;

pub(crate) fn plan(
    doc: &Document,
    dict: &Dictionary,
    settings: &Settings,
) -> Result<Option<Plan>, Problem> {
    let tone = settings.tone;
    let limit = settings.limits.max_stream_bytes;
    let shading_type = get_int(doc, dict, b"ShadingType")
        .ok_or_else(|| Problem::invalid("shading without /ShadingType"))?;
    let space = match dict.get(b"ColorSpace") {
        Ok(cs) => ColorSpace::resolve(doc, cs, None, limit),
        Err(_) => return Err(Problem::invalid("shading without /ColorSpace")),
    };
    match &space {
        ColorSpace::Unsupported(reason) => {
            return Err(Problem::Unsupported(format!(
                "shading color space {reason}"
            )));
        }
        s if !s.is_convertible() => return Ok(None),
        s if s.is_gray() && !tone.is_monochrome() => return Ok(None),
        _ => {}
    }
    let Some(function) = get(doc, dict, b"Function") else {
        return Err(Problem::unsupported(format!(
            "type {shading_type} mesh shading without a function"
        )));
    };
    let function = Function::parse(doc, function, limit)?;
    let gray = |input: &[f64]| {
        (tone.apply(space.to_gray(&function.eval(input)).unwrap_or(0.0)) * 255.0).round() as u8
    };

    let mut fn_dict = Dictionary::new();
    fn_dict.set("FunctionType", 0);
    fn_dict.set("BitsPerSample", 8);
    fn_dict.set("Range", vec![Object::Integer(0), Object::Integer(1)]);
    let reals =
        |values: &[f64]| Object::Array(values.iter().map(|&v| Object::Real(v as f32)).collect());

    let samples =
        match shading_type {
            1 => {
                let domain = get_numbers(doc, dict, b"Domain")
                    .filter(|d| d.len() >= 4)
                    .map(|d| [[d[0], d[1]], [d[2], d[3]]])
                    .or_else(|| function.domain2())
                    .unwrap_or([[0.0, 1.0], [0.0, 1.0]]);
                fn_dict.set(
                    "Domain",
                    reals(&[domain[0][0], domain[0][1], domain[1][0], domain[1][1]]),
                );
                fn_dict.set(
                    "Size",
                    vec![
                        Object::Integer(SAMPLES_2D as i64),
                        Object::Integer(SAMPLES_2D as i64),
                    ],
                );
                let step = |[lo, hi]: [f64; 2], i: usize| {
                    lo + (hi - lo) * i as f64 / (SAMPLES_2D - 1) as f64
                };
                let mut samples = Vec::with_capacity(SAMPLES_2D * SAMPLES_2D);
                for j in 0..SAMPLES_2D {
                    for i in 0..SAMPLES_2D {
                        samples.push(gray(&[step(domain[0], i), step(domain[1], j)]));
                    }
                }
                samples
            }
            2..=7 => {
                let domain = match shading_type {
                    2 | 3 => get_numbers(doc, dict, b"Domain")
                        .map(|d| pairs(&d))
                        .and_then(|d| d.first().copied()),
                    _ => None,
                }
                .unwrap_or_else(|| function.domain());
                fn_dict.set("Domain", reals(&domain));
                fn_dict.set("Size", vec![Object::Integer(SAMPLES_1D as i64)]);
                (0..SAMPLES_1D)
                    .map(|i| {
                        gray(&[domain[0]
                            + (domain[1] - domain[0]) * i as f64 / (SAMPLES_1D - 1) as f64])
                    })
                    .collect()
            }
            other => return Err(Problem::invalid(format!("unknown shading type {other}"))),
        };

    let background = get_numbers(doc, dict, b"Background")
        .and_then(|b| space.to_gray(&b))
        .map(|g| Object::Array(vec![Object::Real(tone.apply(g) as f32)]));

    Ok(Some(Plan {
        function: crate::objects::flate_stream(fn_dict, &samples),
        background,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mode;
    use crate::color::Tone;

    #[test]
    fn axial_rgb_shading_becomes_gray_ramp() {
        let doc = Document::with_version("1.7");
        let mut function = Dictionary::new();
        function.set("FunctionType", 2);
        function.set("Domain", vec![0.into(), 1.into()]);
        function.set("C0", vec![1.into(), 0.into(), 0.into()]);
        function.set("C1", vec![1.into(), 1.into(), 1.into()]);
        function.set("N", 1);
        let mut shading = Dictionary::new();
        shading.set("ShadingType", 2);
        shading.set("ColorSpace", "DeviceRGB");
        shading.set("Coords", vec![0.into(), 0.into(), 100.into(), 0.into()]);
        shading.set("Function", function);

        let settings = Settings {
            tone: Tone {
                mode: Mode::Grayscale,
                threshold: 0.5,
            },
            dither: false,
            limits: Default::default(),
        };
        let plan = plan(&doc, &shading, &settings).unwrap().unwrap();
        let samples = plan.function.decompressed_content().unwrap();
        assert_eq!(samples.len(), SAMPLES_1D);
        assert_eq!(samples[0], 77); // pure red
        assert_eq!(samples[SAMPLES_1D - 1], 255);
    }
}
