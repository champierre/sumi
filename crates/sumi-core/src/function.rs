//! PDF functions (ISO 32000-1, 7.10): needed to evaluate Separation/DeviceN tint transforms and
//! shading colors.

use lopdf::{Document, Object};

use crate::error::Problem;
use crate::objects::{as_dict, deref, get, get_int, get_number, get_numbers, pairs};

const MAX_SAMPLED_INPUTS: usize = 8;
const MAX_SAMPLES: usize = 16 * 1024 * 1024;
const MAX_PS_STEPS: usize = 100_000;
const MAX_PS_STACK: usize = 1_000;

#[derive(Debug, Clone)]
pub(crate) enum Function {
    Sampled(Sampled),
    Exponential {
        domain: [f64; 2],
        c0: Vec<f64>,
        c1: Vec<f64>,
        n: f64,
        range: Vec<[f64; 2]>,
    },
    Stitching {
        domain: [f64; 2],
        functions: Vec<Function>,
        bounds: Vec<f64>,
        encode: Vec<[f64; 2]>,
        range: Vec<[f64; 2]>,
    },
    PostScript {
        domain: Vec<[f64; 2]>,
        range: Vec<[f64; 2]>,
        program: Vec<PsOp>,
    },
    /// An array of single-output functions, as allowed for shadings.
    Array(Vec<Function>),
}

#[derive(Debug, Clone)]
pub(crate) struct Sampled {
    domain: Vec<[f64; 2]>,
    range: Vec<[f64; 2]>,
    size: Vec<usize>,
    bits: u32,
    encode: Vec<[f64; 2]>,
    decode: Vec<[f64; 2]>,
    samples: Vec<u8>,
}

fn interpolate(x: f64, x0: f64, x1: f64, y0: f64, y1: f64) -> f64 {
    if x1 == x0 {
        y0
    } else {
        y0 + (x - x0) * (y1 - y0) / (x1 - x0)
    }
}

fn clip(x: f64, [lo, hi]: [f64; 2]) -> f64 {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    if x.is_nan() { lo } else { x.clamp(lo, hi) }
}

fn clip_outputs(mut out: Vec<f64>, range: &[[f64; 2]]) -> Vec<f64> {
    for (v, r) in out.iter_mut().zip(range) {
        *v = clip(*v, *r);
    }
    out
}

impl Function {
    pub(crate) fn parse(
        doc: &Document,
        obj: &Object,
        max_stream_bytes: usize,
    ) -> Result<Function, Problem> {
        Self::parse_depth(doc, obj, max_stream_bytes, 0)
    }

    fn parse_depth(
        doc: &Document,
        obj: &Object,
        limit: usize,
        depth: usize,
    ) -> Result<Function, Problem> {
        if depth > 16 {
            return Err(Problem::invalid("functions nested too deeply"));
        }
        let obj = deref(doc, obj);
        if let Object::Array(items) = obj {
            let functions = items
                .iter()
                .map(|f| Self::parse_depth(doc, f, limit, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Function::Array(functions));
        }
        let dict =
            as_dict(doc, obj).ok_or_else(|| Problem::invalid("function is not a dictionary"))?;
        let domain = pairs(&get_numbers(doc, dict, b"Domain").unwrap_or_else(|| vec![0.0, 1.0]));
        let range = pairs(&get_numbers(doc, dict, b"Range").unwrap_or_default());
        if domain.is_empty() {
            return Err(Problem::invalid("function without /Domain"));
        }

        match get_int(doc, dict, b"FunctionType") {
            Some(0) => {
                let Object::Stream(stream) = obj else {
                    return Err(Problem::invalid("sampled function is not a stream"));
                };
                let size: Vec<usize> = get_numbers(doc, dict, b"Size")
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| s.max(1.0) as usize)
                    .collect();
                let bits = get_int(doc, dict, b"BitsPerSample").unwrap_or(8) as u32;
                if size.len() != domain.len() || size.len() > MAX_SAMPLED_INPUTS || range.is_empty()
                {
                    return Err(Problem::invalid("malformed sampled function"));
                }
                if ![1, 2, 4, 8, 12, 16, 24, 32].contains(&bits) {
                    return Err(Problem::invalid(
                        "unsupported BitsPerSample in sampled function",
                    ));
                }
                let count = size
                    .iter()
                    .try_fold(range.len(), |acc, &s| acc.checked_mul(s))
                    .filter(|&c| c <= MAX_SAMPLES)
                    .ok_or_else(|| Problem::invalid("sampled function is too large"))?;
                let encode = get_numbers(doc, dict, b"Encode")
                    .map(|e| pairs(&e))
                    .unwrap_or_else(|| size.iter().map(|&s| [0.0, (s - 1) as f64]).collect());
                let decode = get_numbers(doc, dict, b"Decode")
                    .map(|d| pairs(&d))
                    .unwrap_or_else(|| range.clone());
                if encode.len() < size.len() || decode.len() < range.len() {
                    return Err(Problem::invalid("malformed sampled function"));
                }
                let mut samples = crate::objects::decode_stream(doc, stream, limit)?;
                let needed = (count * bits as usize).div_ceil(8);
                if samples.len() < needed {
                    samples.resize(needed, 0);
                }
                Ok(Function::Sampled(Sampled {
                    domain,
                    range,
                    size,
                    bits,
                    encode,
                    decode,
                    samples,
                }))
            }
            Some(2) => {
                let c0 = get_numbers(doc, dict, b"C0").unwrap_or_else(|| vec![0.0]);
                let c1 = get_numbers(doc, dict, b"C1").unwrap_or_else(|| vec![1.0]);
                let n = get_number(doc, dict, b"N")
                    .ok_or_else(|| Problem::invalid("exponential function without /N"))?;
                Ok(Function::Exponential {
                    domain: domain[0],
                    c0,
                    c1,
                    n,
                    range,
                })
            }
            Some(3) => {
                let functions = match get(doc, dict, b"Functions") {
                    Some(Object::Array(items)) => items
                        .iter()
                        .map(|f| Self::parse_depth(doc, f, limit, depth + 1))
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err(Problem::invalid("stitching function without /Functions")),
                };
                let bounds = get_numbers(doc, dict, b"Bounds").unwrap_or_default();
                let encode = pairs(&get_numbers(doc, dict, b"Encode").unwrap_or_default());
                if functions.is_empty()
                    || bounds.len() + 1 != functions.len()
                    || encode.len() < functions.len()
                {
                    return Err(Problem::invalid("malformed stitching function"));
                }
                Ok(Function::Stitching {
                    domain: domain[0],
                    functions,
                    bounds,
                    encode,
                    range,
                })
            }
            Some(4) => {
                let Object::Stream(stream) = obj else {
                    return Err(Problem::invalid("PostScript function is not a stream"));
                };
                let code = crate::objects::decode_stream(doc, stream, limit)?;
                let program = parse_postscript(&code)?;
                Ok(Function::PostScript {
                    domain,
                    range,
                    program,
                })
            }
            _ => Err(Problem::unsupported("unknown function type")),
        }
    }

    /// The input domain of the first input.
    pub(crate) fn domain(&self) -> [f64; 2] {
        match self {
            Function::Sampled(s) => s.domain[0],
            Function::Exponential { domain, .. } | Function::Stitching { domain, .. } => *domain,
            Function::PostScript { domain, .. } => domain[0],
            Function::Array(fs) => fs.first().map_or([0.0, 1.0], Function::domain),
        }
    }

    /// The 2-D input domain of a function with two inputs.
    pub(crate) fn domain2(&self) -> Option<[[f64; 2]; 2]> {
        match self {
            Function::Sampled(s) if s.domain.len() >= 2 => Some([s.domain[0], s.domain[1]]),
            Function::PostScript { domain, .. } if domain.len() >= 2 => {
                Some([domain[0], domain[1]])
            }
            Function::Array(fs) => fs.first().and_then(Function::domain2),
            _ => None,
        }
    }

    pub(crate) fn eval(&self, input: &[f64]) -> Vec<f64> {
        match self {
            Function::Sampled(s) => s.eval(input),
            Function::Exponential {
                domain,
                c0,
                c1,
                n,
                range,
            } => {
                let x = clip(input.first().copied().unwrap_or(0.0), *domain);
                let xn = x.powf(*n);
                let out = c0.iter().zip(c1).map(|(a, b)| a + xn * (b - a)).collect();
                clip_outputs(out, range)
            }
            Function::Stitching {
                domain,
                functions,
                bounds,
                encode,
                range,
            } => {
                let x = clip(input.first().copied().unwrap_or(0.0), *domain);
                let k = bounds.iter().position(|&b| x < b).unwrap_or(bounds.len());
                let lo = if k == 0 { domain[0] } else { bounds[k - 1] };
                let hi = if k == bounds.len() {
                    domain[1]
                } else {
                    bounds[k]
                };
                let t = interpolate(x, lo, hi, encode[k][0], encode[k][1]);
                clip_outputs(functions[k].eval(&[t]), range)
            }
            Function::PostScript {
                domain,
                range,
                program,
            } => {
                let mut stack: Vec<PsValue> = domain
                    .iter()
                    .enumerate()
                    .map(|(i, d)| PsValue::Num(clip(input.get(i).copied().unwrap_or(0.0), *d)))
                    .collect();
                let mut steps = 0;
                // A failing program yields whatever is on the stack, like most viewers.
                let _ = run_postscript(program, &mut stack, &mut steps);
                let n = range.len();
                let mut out: Vec<f64> =
                    stack.iter().rev().take(n).rev().map(PsValue::num).collect();
                while out.len() < n {
                    out.insert(0, 0.0);
                }
                clip_outputs(out, range)
            }
            Function::Array(functions) => functions.iter().flat_map(|f| f.eval(input)).collect(),
        }
    }
}

impl Sampled {
    fn sample(&self, index: usize) -> f64 {
        let bits = self.bits as usize;
        let start = index * bits;
        let mut value: u64 = 0;
        for i in 0..bits {
            let bit = start + i;
            let byte = self.samples.get(bit / 8).copied().unwrap_or(0);
            value = (value << 1) | ((byte >> (7 - bit % 8)) & 1) as u64;
        }
        value as f64
    }

    fn eval(&self, input: &[f64]) -> Vec<f64> {
        let m = self.size.len();
        let n = self.range.len();
        let mut base = vec![0usize; m];
        let mut frac = vec![0f64; m];
        for i in 0..m {
            let x = clip(input.get(i).copied().unwrap_or(0.0), self.domain[i]);
            let e = interpolate(
                x,
                self.domain[i][0],
                self.domain[i][1],
                self.encode[i][0],
                self.encode[i][1],
            );
            let e = clip(e, [0.0, (self.size[i] - 1) as f64]);
            let floor = (e.floor() as usize).min(self.size[i].saturating_sub(2));
            base[i] = floor;
            frac[i] = if self.size[i] == 1 {
                0.0
            } else {
                e - floor as f64
            };
        }

        let max_sample = ((1u64 << self.bits) - 1) as f64;
        let mut out = vec![0f64; n];
        for corner in 0..(1usize << m) {
            let mut weight = 1.0;
            let mut index = 0;
            let mut stride = 1;
            for i in 0..m {
                let upper = corner >> i & 1 == 1;
                let coord = if upper && self.size[i] > 1 {
                    base[i] + 1
                } else {
                    base[i]
                };
                weight *= if upper { frac[i] } else { 1.0 - frac[i] };
                index += coord * stride;
                stride *= self.size[i];
            }
            if weight == 0.0 {
                continue;
            }
            for (j, o) in out.iter_mut().enumerate() {
                *o += weight * self.sample(index * n + j);
            }
        }
        let out = out
            .iter()
            .enumerate()
            .map(|(j, &s)| interpolate(s, 0.0, max_sample, self.decode[j][0], self.decode[j][1]))
            .collect();
        clip_outputs(out, &self.range)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum PsOp {
    Num(f64),
    Bool(bool),
    Op(Vec<u8>),
    If(Vec<PsOp>),
    IfElse(Vec<PsOp>, Vec<PsOp>),
}

#[derive(Debug, Clone, Copy)]
enum PsValue {
    Num(f64),
    Bool(bool),
}

impl PsValue {
    fn num(&self) -> f64 {
        match *self {
            PsValue::Num(n) => n,
            PsValue::Bool(b) => b as u8 as f64,
        }
    }
}

fn parse_postscript(code: &[u8]) -> Result<Vec<PsOp>, Problem> {
    enum Item {
        Op(PsOp),
        Proc(Vec<PsOp>),
    }

    let mut pos = 0;
    let mut stack: Vec<Vec<Item>> = Vec::new();
    let mut result = None;
    while pos < code.len() {
        let c = code[pos];
        if c.is_ascii_whitespace() || c == 0 {
            pos += 1;
        } else if c == b'%' {
            while pos < code.len() && code[pos] != b'\n' && code[pos] != b'\r' {
                pos += 1;
            }
        } else if c == b'{' {
            if stack.len() > 64 {
                return Err(Problem::invalid("PostScript function nested too deeply"));
            }
            stack.push(Vec::new());
            pos += 1;
        } else if c == b'}' {
            pos += 1;
            let items = stack
                .pop()
                .ok_or_else(|| Problem::invalid("unbalanced PostScript function"))?;
            let mut ops = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Item::Op(op) => ops.push(op),
                    Item::Proc(_) => return Err(Problem::invalid("procedure without if/ifelse")),
                }
            }
            match stack.last_mut() {
                Some(parent) => parent.push(Item::Proc(ops)),
                None => {
                    result = Some(ops);
                    break;
                }
            }
        } else {
            let start = pos;
            while pos < code.len()
                && !code[pos].is_ascii_whitespace()
                && !b"{}%".contains(&code[pos])
            {
                pos += 1;
            }
            let word = &code[start..pos];
            let current = stack
                .last_mut()
                .ok_or_else(|| Problem::invalid("PostScript function without braces"))?;
            let op = match word {
                b"true" => PsOp::Bool(true),
                b"false" => PsOp::Bool(false),
                b"if" => match current.pop() {
                    Some(Item::Proc(p)) => PsOp::If(p),
                    _ => return Err(Problem::invalid("if without procedure")),
                },
                b"ifelse" => match (current.pop(), current.pop()) {
                    (Some(Item::Proc(else_)), Some(Item::Proc(then))) => PsOp::IfElse(then, else_),
                    _ => return Err(Problem::invalid("ifelse without procedures")),
                },
                _ => match std::str::from_utf8(word)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok())
                {
                    Some(n) => PsOp::Num(n),
                    None => PsOp::Op(word.to_vec()),
                },
            };
            current.push(Item::Op(op));
        }
    }
    result.ok_or_else(|| Problem::invalid("unterminated PostScript function"))
}

fn run_postscript(program: &[PsOp], stack: &mut Vec<PsValue>, steps: &mut usize) -> Option<()> {
    use PsValue::{Bool, Num};

    for op in program {
        *steps += 1;
        if *steps > MAX_PS_STEPS || stack.len() > MAX_PS_STACK {
            return None;
        }
        match op {
            PsOp::Num(n) => stack.push(Num(*n)),
            PsOp::Bool(b) => stack.push(Bool(*b)),
            PsOp::If(proc_) => {
                if let Bool(true) = stack.pop()? {
                    run_postscript(proc_, stack, steps)?;
                }
            }
            PsOp::IfElse(then, else_) => match stack.pop()? {
                Bool(true) => run_postscript(then, stack, steps)?,
                _ => run_postscript(else_, stack, steps)?,
            },
            PsOp::Op(name) => {
                let pop = |s: &mut Vec<PsValue>| s.pop().map(|v| v.num());
                match name.as_slice() {
                    b"abs" => {
                        let a = pop(stack)?;
                        stack.push(Num(a.abs()))
                    }
                    b"neg" => {
                        let a = pop(stack)?;
                        stack.push(Num(-a))
                    }
                    b"ceiling" => {
                        let a = pop(stack)?;
                        stack.push(Num(a.ceil()))
                    }
                    b"floor" => {
                        let a = pop(stack)?;
                        stack.push(Num(a.floor()))
                    }
                    b"round" => {
                        let a = pop(stack)?;
                        stack.push(Num((a + 0.5).floor()))
                    }
                    b"truncate" | b"cvi" => {
                        let a = pop(stack)?;
                        stack.push(Num(a.trunc()))
                    }
                    b"cvr" => {
                        let a = pop(stack)?;
                        stack.push(Num(a))
                    }
                    b"sqrt" => {
                        let a = pop(stack)?;
                        stack.push(Num(a.max(0.0).sqrt()))
                    }
                    b"sin" => {
                        let a = pop(stack)?;
                        stack.push(Num(a.to_radians().sin()))
                    }
                    b"cos" => {
                        let a = pop(stack)?;
                        stack.push(Num(a.to_radians().cos()))
                    }
                    b"ln" => {
                        let a = pop(stack)?;
                        stack.push(Num(a.ln()))
                    }
                    b"log" => {
                        let a = pop(stack)?;
                        stack.push(Num(a.log10()))
                    }
                    b"add" | b"sub" | b"mul" | b"div" | b"idiv" | b"mod" | b"exp" | b"atan" => {
                        let b = pop(stack)?;
                        let a = pop(stack)?;
                        let v = match name.as_slice() {
                            b"add" => a + b,
                            b"sub" => a - b,
                            b"mul" => a * b,
                            b"div" => {
                                if b == 0.0 {
                                    0.0
                                } else {
                                    a / b
                                }
                            }
                            b"idiv" => {
                                if b as i64 == 0 {
                                    0.0
                                } else {
                                    (a as i64 / b as i64) as f64
                                }
                            }
                            b"mod" => {
                                if b as i64 == 0 {
                                    0.0
                                } else {
                                    (a as i64 % b as i64) as f64
                                }
                            }
                            b"exp" => a.powf(b),
                            _ => {
                                let deg = a.atan2(b).to_degrees();
                                if deg < 0.0 { deg + 360.0 } else { deg }
                            }
                        };
                        stack.push(Num(v));
                    }
                    b"eq" | b"ne" | b"gt" | b"ge" | b"lt" | b"le" => {
                        let b = pop(stack)?;
                        let a = pop(stack)?;
                        stack.push(Bool(match name.as_slice() {
                            b"eq" => a == b,
                            b"ne" => a != b,
                            b"gt" => a > b,
                            b"ge" => a >= b,
                            b"lt" => a < b,
                            _ => a <= b,
                        }));
                    }
                    b"and" | b"or" | b"xor" => {
                        let b = stack.pop()?;
                        let a = stack.pop()?;
                        stack.push(match (a, b) {
                            (Bool(a), Bool(b)) => Bool(match name.as_slice() {
                                b"and" => a & b,
                                b"or" => a | b,
                                _ => a ^ b,
                            }),
                            (a, b) => {
                                let (a, b) = (a.num() as i64, b.num() as i64);
                                Num(match name.as_slice() {
                                    b"and" => a & b,
                                    b"or" => a | b,
                                    _ => a ^ b,
                                } as f64)
                            }
                        });
                    }
                    b"not" => match stack.pop()? {
                        Bool(b) => stack.push(Bool(!b)),
                        Num(n) => stack.push(Num(!(n as i64) as f64)),
                    },
                    b"bitshift" => {
                        let shift = pop(stack)? as i64;
                        let a = pop(stack)? as i64;
                        let v = if shift >= 0 {
                            a.checked_shl(shift as u32).unwrap_or(0)
                        } else {
                            a >> (-shift).min(63)
                        };
                        stack.push(Num(v as f64));
                    }
                    b"pop" => {
                        stack.pop()?;
                    }
                    b"dup" => {
                        let top = *stack.last()?;
                        stack.push(top);
                    }
                    b"exch" => {
                        let len = stack.len();
                        if len < 2 {
                            return None;
                        }
                        stack.swap(len - 1, len - 2);
                    }
                    b"copy" => {
                        let n = pop(stack)?.max(0.0) as usize;
                        if n > stack.len() || stack.len() + n > MAX_PS_STACK {
                            return None;
                        }
                        let start = stack.len() - n;
                        stack.extend_from_within(start..);
                    }
                    b"index" => {
                        let n = pop(stack)?.max(0.0) as usize;
                        let v = *stack.get(stack.len().checked_sub(n + 1)?)?;
                        stack.push(v);
                    }
                    b"roll" => {
                        let j = pop(stack)? as i64;
                        let n = pop(stack)?.max(0.0) as usize;
                        if n > stack.len() {
                            return None;
                        }
                        if n > 0 {
                            let start = stack.len() - n;
                            let shift = j.rem_euclid(n as i64) as usize;
                            stack[start..].rotate_right(shift);
                        }
                    }
                    _ => return None,
                }
            }
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ps(code: &str, domain: usize, range: usize, input: &[f64]) -> Vec<f64> {
        let f = Function::PostScript {
            domain: vec![[0.0, 1.0]; domain],
            range: vec![[0.0, 1.0]; range],
            program: parse_postscript(code.as_bytes()).unwrap(),
        };
        f.eval(input)
    }

    #[test]
    fn postscript_tint_transform() {
        // Typical spot color: tint -> CMYK.
        let out = ps(
            "{ dup 0.2 mul exch dup 0.9 mul exch 0.8 mul 0 }",
            1,
            4,
            &[0.5],
        );
        let expected = [0.1, 0.45, 0.4, 0.0];
        for (a, b) in out.iter().zip(expected) {
            assert!((a - b).abs() < 1e-9, "{out:?}");
        }
    }

    #[test]
    fn postscript_conditionals_and_stack_ops() {
        assert_eq!(ps("{ 0.5 gt { 1 } { 0 } ifelse }", 1, 1, &[0.7]), vec![1.0]);
        assert_eq!(ps("{ 0.5 gt { 1 } { 0 } ifelse }", 1, 1, &[0.2]), vec![0.0]);
        assert_eq!(ps("{ 1 2 3 3 1 roll pop pop }", 1, 1, &[0.0]), vec![1.0]);
        assert_eq!(ps("{ pop 0.25 dup add }", 1, 1, &[0.0]), vec![0.5]);
    }

    #[test]
    fn exponential_and_stitching() {
        let red = Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![1.0, 0.0, 0.0],
            c1: vec![0.0, 0.0, 1.0],
            n: 1.0,
            range: vec![],
        };
        assert_eq!(red.eval(&[0.5]), vec![0.5, 0.0, 0.5]);
        let stitched = Function::Stitching {
            domain: [0.0, 1.0],
            functions: vec![red.clone(), red],
            bounds: vec![0.5],
            encode: vec![[0.0, 1.0], [1.0, 0.0]],
            range: vec![],
        };
        assert_eq!(stitched.eval(&[0.25]), vec![0.5, 0.0, 0.5]);
        assert_eq!(stitched.eval(&[1.0]), vec![1.0, 0.0, 0.0]);
    }

    #[test]
    fn sampled_linear_interpolation() {
        let f = Function::Sampled(Sampled {
            domain: vec![[0.0, 1.0]],
            range: vec![[0.0, 1.0]],
            size: vec![2],
            bits: 8,
            encode: vec![[0.0, 1.0]],
            decode: vec![[0.0, 1.0]],
            samples: vec![0, 255],
        });
        assert!((f.eval(&[0.5])[0] - 0.5).abs() < 1e-9);
        assert_eq!(f.eval(&[1.0]), vec![1.0]);
    }
}
