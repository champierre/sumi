//! Color space resolution (ISO 32000-1, 8.6).

use std::rc::Rc;

use lopdf::{Dictionary, Document, Object};

use crate::color::{cmyk_to_gray, lab_to_gray, rgb_to_gray};
use crate::error::Problem;
use crate::function::Function;
use crate::objects::{as_dict, decode_stream, deref, get, get_int, name_str};

#[derive(Debug, Clone)]
pub(crate) enum ColorSpace {
    Gray,
    Rgb,
    Cmyk,
    Lab,
    Indexed {
        base: Rc<ColorSpace>,
        hival: usize,
        lookup: Vec<u8>,
    },
    Separation {
        tint: Tint,
    },
    DeviceN {
        n: usize,
        tint: Tint,
    },
    /// `base` is set for uncolored tiling patterns.
    Pattern {
        base: Option<Rc<ColorSpace>>,
    },
    /// A color space that cannot be converted; the reason is reported as a warning.
    Unsupported(String),
}

#[derive(Debug, Clone)]
pub(crate) enum Tint {
    /// `/None` colorant: never marks the page.
    NoInk,
    /// `/All` colorant: registration color on every separation.
    All,
    Transform {
        alternate: Rc<ColorSpace>,
        function: Function,
    },
}

impl ColorSpace {
    pub(crate) fn components(&self) -> usize {
        match self {
            ColorSpace::Gray | ColorSpace::Indexed { .. } | ColorSpace::Separation { .. } => 1,
            ColorSpace::Rgb | ColorSpace::Lab => 3,
            ColorSpace::Cmyk => 4,
            ColorSpace::DeviceN { n, .. } => *n,
            ColorSpace::Pattern { base } => base.as_ref().map_or(0, |b| b.components()),
            ColorSpace::Unsupported(_) => 0,
        }
    }

    pub(crate) fn is_gray(&self) -> bool {
        matches!(self, ColorSpace::Gray)
    }

    /// Whether colors in this space can be mapped to gray.
    pub(crate) fn is_convertible(&self) -> bool {
        match self {
            ColorSpace::Separation { tint } | ColorSpace::DeviceN { tint, .. } => {
                !matches!(tint, Tint::NoInk)
            }
            ColorSpace::Pattern { .. } | ColorSpace::Unsupported(_) => false,
            _ => true,
        }
    }

    /// The color selected by `cs`/`CS` (ISO 32000-1, table 74).
    pub(crate) fn initial_color(&self) -> Vec<f64> {
        match self {
            ColorSpace::Cmyk => vec![0.0, 0.0, 0.0, 1.0],
            ColorSpace::Separation { .. } => vec![1.0],
            ColorSpace::DeviceN { n, .. } => vec![1.0; *n],
            other => vec![0.0; other.components()],
        }
    }

    /// Default image `/Decode` array for `bits` bits per component.
    pub(crate) fn default_decode(&self, bits: u32) -> Vec<[f64; 2]> {
        match self {
            ColorSpace::Indexed { .. } => vec![[0.0, ((1u64 << bits) - 1) as f64]],
            ColorSpace::Lab => vec![[0.0, 100.0], [-100.0, 100.0], [-100.0, 100.0]],
            other => vec![[0.0, 1.0]; other.components()],
        }
    }

    /// Maps color components to a gray level in `0.0..=1.0`.
    pub(crate) fn to_gray(&self, comps: &[f64]) -> Option<f64> {
        let c = |i: usize| comps.get(i).copied().unwrap_or(0.0);
        match self {
            ColorSpace::Gray => Some(crate::color::clamp01(c(0))),
            ColorSpace::Rgb => Some(rgb_to_gray(c(0), c(1), c(2))),
            ColorSpace::Cmyk => Some(cmyk_to_gray(c(0), c(1), c(2), c(3))),
            ColorSpace::Lab => Some(lab_to_gray(c(0))),
            ColorSpace::Indexed {
                base,
                hival,
                lookup,
            } => {
                let index = (c(0).round().max(0.0) as usize).min(*hival);
                base.to_gray(&self_lookup(base, lookup, index))
            }
            ColorSpace::Separation { tint } | ColorSpace::DeviceN { tint, .. } => match tint {
                Tint::NoInk => None,
                Tint::All => {
                    let ink = comps.iter().copied().fold(0.0, f64::max);
                    Some(crate::color::clamp01(1.0 - ink))
                }
                Tint::Transform {
                    alternate,
                    function,
                } => alternate.to_gray(&function.eval(comps)),
            },
            ColorSpace::Pattern { .. } | ColorSpace::Unsupported(_) => None,
        }
    }

    /// The gray level of every palette entry of an indexed color space.
    pub(crate) fn palette_grays(&self) -> Option<Vec<f64>> {
        let ColorSpace::Indexed {
            base,
            hival,
            lookup,
        } = self
        else {
            return None;
        };
        (0..=*hival)
            .map(|i| base.to_gray(&self_lookup(base, lookup, i)))
            .collect()
    }

    /// Resolves a color space operand or dictionary value.
    ///
    /// Names that are not color space families are looked up in the `/ColorSpace` resources.
    pub(crate) fn resolve(
        doc: &Document,
        obj: &Object,
        resources: Option<&Dictionary>,
        limit: usize,
    ) -> ColorSpace {
        match Self::resolve_depth(doc, obj, resources, limit, 0) {
            Ok(cs) => cs,
            Err(Problem::Unsupported(msg) | Problem::Invalid(msg) | Problem::Limit(msg)) => {
                ColorSpace::Unsupported(msg)
            }
        }
    }

    fn resolve_depth(
        doc: &Document,
        obj: &Object,
        resources: Option<&Dictionary>,
        limit: usize,
        depth: usize,
    ) -> Result<ColorSpace, Problem> {
        if depth > 8 {
            return Err(Problem::invalid("color space nested too deeply"));
        }
        let obj = deref(doc, obj);
        let (family, array) = match obj {
            Object::Name(name) => (name.as_slice(), &[][..]),
            Object::Array(items) => match items.first().map(|o| deref(doc, o)) {
                Some(Object::Name(name)) => (name.as_slice(), &items[1..]),
                _ => return Err(Problem::invalid("malformed color space array")),
            },
            _ => return Err(Problem::invalid("malformed color space")),
        };
        let param = |i: usize| array.get(i).map(|o| deref(doc, o));

        Ok(match family {
            b"DeviceGray" | b"G" | b"CalGray" => ColorSpace::Gray,
            b"DeviceRGB" | b"RGB" | b"CalRGB" => ColorSpace::Rgb,
            b"DeviceCMYK" | b"CMYK" | b"CalCMYK" => ColorSpace::Cmyk,
            b"Lab" => ColorSpace::Lab,
            b"ICCBased" => {
                let dict = param(0)
                    .and_then(|o| as_dict(doc, o))
                    .ok_or_else(|| Problem::invalid("ICCBased color space without a stream"))?;
                match get_int(doc, dict, b"N") {
                    Some(1) => ColorSpace::Gray,
                    Some(3) => ColorSpace::Rgb,
                    Some(4) => ColorSpace::Cmyk,
                    _ => match get(doc, dict, b"Alternate") {
                        Some(alt) => Self::resolve_depth(doc, alt, None, limit, depth + 1)?,
                        None => return Err(Problem::invalid("ICCBased color space without /N")),
                    },
                }
            }
            b"Indexed" | b"I" => {
                let base = param(0)
                    .ok_or_else(|| Problem::invalid("Indexed color space without a base"))?;
                let base = Self::resolve_depth(doc, base, resources, limit, depth + 1)?;
                if !base.is_convertible() || matches!(base, ColorSpace::Indexed { .. }) {
                    return Err(Problem::unsupported(format!(
                        "Indexed color space with a {} base",
                        base.kind()
                    )));
                }
                let hival = param(1)
                    .and_then(crate::objects::number)
                    .ok_or_else(|| Problem::invalid("Indexed color space without hival"))?
                    .clamp(0.0, 255.0) as usize;
                let mut lookup = match param(2) {
                    Some(Object::String(bytes, _)) => bytes.clone(),
                    Some(Object::Stream(stream)) => decode_stream(doc, stream, limit)?,
                    _ => {
                        return Err(Problem::invalid(
                            "Indexed color space without a lookup table",
                        ));
                    }
                };
                lookup.resize((hival + 1) * base.components(), 0);
                ColorSpace::Indexed {
                    base: Rc::new(base),
                    hival,
                    lookup,
                }
            }
            b"Separation" => {
                let tint = match param(0) {
                    Some(Object::Name(n)) if n == b"None" => Tint::NoInk,
                    Some(Object::Name(n)) if n == b"All" => Tint::All,
                    _ => Self::tint_transform(doc, param(1), param(2), limit, depth)?,
                };
                ColorSpace::Separation { tint }
            }
            b"DeviceN" => {
                let names = match param(0) {
                    Some(Object::Array(names)) if !names.is_empty() && names.len() <= 32 => names,
                    _ => {
                        return Err(Problem::invalid(
                            "DeviceN color space without colorant names",
                        ));
                    }
                };
                let no_ink = names
                    .iter()
                    .all(|n| matches!(deref(doc, n), Object::Name(n) if n == b"None"));
                let tint = if no_ink {
                    Tint::NoInk
                } else {
                    Self::tint_transform(doc, param(1), param(2), limit, depth)?
                };
                ColorSpace::DeviceN {
                    n: names.len(),
                    tint,
                }
            }
            b"Pattern" => {
                let base = match param(0) {
                    Some(base) => Some(Rc::new(Self::resolve_depth(
                        doc,
                        base,
                        resources,
                        limit,
                        depth + 1,
                    )?)),
                    None => None,
                };
                ColorSpace::Pattern { base }
            }
            name if array.is_empty() => {
                let value = resources
                    .and_then(|r| crate::objects::get_dict(doc, r, b"ColorSpace"))
                    .and_then(|d| d.get(name).ok())
                    .ok_or_else(|| {
                        Problem::invalid(format!(
                            "color space /{} not found in resources",
                            name_str(name)
                        ))
                    })?;
                // Resource entries may not refer to other resource names.
                Self::resolve_depth(doc, value, None, limit, depth + 1)?
            }
            other => {
                return Err(Problem::unsupported(format!(
                    "{} color space",
                    name_str(other)
                )));
            }
        })
    }

    fn tint_transform(
        doc: &Document,
        alternate: Option<&Object>,
        function: Option<&Object>,
        limit: usize,
        depth: usize,
    ) -> Result<Tint, Problem> {
        let alternate = alternate
            .ok_or_else(|| Problem::invalid("special color space without an alternate space"))?;
        let alternate = Self::resolve_depth(doc, alternate, None, limit, depth + 1)?;
        if !alternate.is_convertible() || matches!(alternate, ColorSpace::Indexed { .. }) {
            return Err(Problem::unsupported(format!(
                "alternate color space {}",
                alternate.kind()
            )));
        }
        let function = function
            .ok_or_else(|| Problem::invalid("special color space without a tint transform"))?;
        let function = Function::parse(doc, function, limit)?;
        Ok(Tint::Transform {
            alternate: Rc::new(alternate),
            function,
        })
    }

    pub(crate) fn kind(&self) -> String {
        match self {
            ColorSpace::Gray => "DeviceGray".into(),
            ColorSpace::Rgb => "DeviceRGB".into(),
            ColorSpace::Cmyk => "DeviceCMYK".into(),
            ColorSpace::Lab => "Lab".into(),
            ColorSpace::Indexed { .. } => "Indexed".into(),
            ColorSpace::Separation { .. } => "Separation".into(),
            ColorSpace::DeviceN { .. } => "DeviceN".into(),
            ColorSpace::Pattern { .. } => "Pattern".into(),
            ColorSpace::Unsupported(reason) => reason.clone(),
        }
    }
}

fn self_lookup(base: &ColorSpace, lookup: &[u8], index: usize) -> Vec<f64> {
    let n = base.components();
    let decode = base.default_decode(8);
    (0..n)
        .map(|j| {
            let byte = lookup.get(index * n + j).copied().unwrap_or(0) as f64;
            let [lo, hi] = decode.get(j).copied().unwrap_or([0.0, 1.0]);
            lo + byte * (hi - lo) / 255.0
        })
        .collect()
}
