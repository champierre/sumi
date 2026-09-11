//! Rewrites color operators in content streams.
//!
//! Only the bytes of color operators (and convertible inline images) are replaced; everything
//! else, including text, paths, strings and comments, is copied verbatim.

use std::collections::HashMap;
use std::rc::Rc;

use lopdf::{Dictionary, Document, Object};

use crate::color::format_number;
use crate::colorspace::ColorSpace;
use crate::error::Problem;
use crate::lexer::{Kind, Lexer, Token, decode_name, is_regular, parse_number, parse_object};
use crate::options::Settings;
use crate::report::Report;

pub(crate) struct Env<'a> {
    pub doc: &'a Document,
    pub resources: Option<&'a Dictionary>,
    pub settings: &'a Settings,
    /// Resource name to use for an added `[/Pattern /DeviceGray]` color space.
    pub pattern_gray_name: &'a [u8],
    /// Page content starts in DeviceGray; forms, patterns and glyphs inherit the caller's
    /// color space, which is unknown here.
    pub inherited_state: bool,
}

pub(crate) struct Output {
    pub data: Vec<u8>,
    pub changed: bool,
    pub uses_pattern_gray: bool,
}

#[derive(Clone)]
struct GState {
    /// `None` when inherited from an unknown caller.
    fill: Option<Rc<ColorSpace>>,
    stroke: Option<Rc<ColorSpace>>,
}

const MAX_STATE_DEPTH: usize = 1024;

struct Rewriter<'a, 'r> {
    env: &'a Env<'a>,
    report: &'r mut Report,
    input: &'a [u8],
    out: Vec<u8>,
    copied: usize,
    changed: bool,
    uses_pattern_gray: bool,
    spaces: HashMap<Vec<u8>, Rc<ColorSpace>>,
    state: GState,
    stack: Vec<GState>,
}

pub(crate) fn rewrite(input: &[u8], env: &Env, report: &mut Report) -> Result<Output, Problem> {
    let initial = if env.inherited_state {
        None
    } else {
        Some(Rc::new(ColorSpace::Gray))
    };
    let mut rw = Rewriter {
        env,
        report,
        input,
        out: Vec::new(),
        copied: 0,
        changed: false,
        uses_pattern_gray: false,
        spaces: HashMap::new(),
        state: GState {
            fill: initial.clone(),
            stroke: initial,
        },
        stack: Vec::new(),
    };

    let mut lexer = Lexer::new(input);
    let mut operands: Vec<Token> = Vec::new();
    while let Some(token) = lexer.next_token() {
        let text = lexer.text(token);
        if token.kind != Kind::Keyword || matches!(text, b"true" | b"false" | b"null") {
            operands.push(token);
            continue;
        }
        match text {
            b"q" => {
                if rw.stack.len() < MAX_STATE_DEPTH {
                    rw.stack.push(rw.state.clone());
                }
            }
            b"Q" => {
                if let Some(state) = rw.stack.pop() {
                    rw.state = state;
                }
            }
            b"g" => rw.device_color(&lexer, &operands, token, 1, false),
            b"G" => rw.device_color(&lexer, &operands, token, 1, true),
            b"rg" => rw.device_color(&lexer, &operands, token, 3, false),
            b"RG" => rw.device_color(&lexer, &operands, token, 3, true),
            b"k" => rw.device_color(&lexer, &operands, token, 4, false),
            b"K" => rw.device_color(&lexer, &operands, token, 4, true),
            b"cs" => rw.set_space(&lexer, &operands, token, false),
            b"CS" => rw.set_space(&lexer, &operands, token, true),
            b"sc" | b"scn" => rw.set_color(&lexer, &operands, token, false),
            b"SC" | b"SCN" => rw.set_color(&lexer, &operands, token, true),
            b"BI" => rw.inline_image(&mut lexer, token)?,
            _ => {}
        }
        operands.clear();
    }

    let changed = rw.changed;
    let mut out = rw.out;
    if changed {
        out.extend_from_slice(&input[rw.copied..]);
    }
    Ok(Output {
        data: out,
        changed,
        uses_pattern_gray: rw.uses_pattern_gray,
    })
}

impl Rewriter<'_, '_> {
    fn replace(&mut self, start: usize, end: usize, text: &[u8]) {
        if start < self.copied || end < start {
            return;
        }
        self.out.extend_from_slice(&self.input[self.copied..start]);
        if start > 0
            && is_regular(self.input[start - 1])
            && text.first().is_some_and(|&c| is_regular(c))
        {
            self.out.push(b' ');
        }
        self.out.extend_from_slice(text);
        if self.input.get(end).is_some_and(|&c| is_regular(c))
            && text.last().is_some_and(|&c| is_regular(c))
        {
            self.out.push(b' ');
        }
        self.copied = end;
        self.changed = true;
    }

    fn current(&mut self, stroke: bool) -> &mut Option<Rc<ColorSpace>> {
        if stroke {
            &mut self.state.stroke
        } else {
            &mut self.state.fill
        }
    }

    /// The trailing `n` operands if they are all numbers.
    fn numbers(lexer: &Lexer, operands: &[Token], n: usize) -> Option<(usize, Vec<f64>)> {
        if n == 0 || operands.len() < n {
            return None;
        }
        let tail = &operands[operands.len() - n..];
        let values = tail
            .iter()
            .map(|t| {
                if t.kind == Kind::Number {
                    parse_number(lexer.text(*t))
                } else {
                    None
                }
            })
            .collect::<Option<Vec<f64>>>()?;
        Some((tail[0].start, values))
    }

    fn device_color(
        &mut self,
        lexer: &Lexer,
        operands: &[Token],
        op: Token,
        n: usize,
        stroke: bool,
    ) {
        *self.current(stroke) = Some(Rc::new(ColorSpace::Gray));
        let Some((start, values)) = Self::numbers(lexer, operands, n) else {
            return;
        };
        let tone = self.env.settings.tone;
        if n == 1 && !tone.is_monochrome() {
            return;
        }
        let space = match n {
            1 => ColorSpace::Gray,
            3 => ColorSpace::Rgb,
            _ => ColorSpace::Cmyk,
        };
        let gray = tone.apply(space.to_gray(&values).unwrap_or(0.0));
        let text = format!("{} {}", format_number(gray), if stroke { "G" } else { "g" });
        self.replace(start, op.end, text.as_bytes());
        self.report.color_operators += 1;
    }

    fn resolve(&mut self, name: &[u8]) -> Rc<ColorSpace> {
        if let Some(space) = self.spaces.get(name) {
            return space.clone();
        }
        let env = self.env;
        let space = Rc::new(ColorSpace::resolve(
            env.doc,
            &Object::Name(name.to_vec()),
            env.resources,
            env.settings.limits.max_stream_bytes,
        ));
        self.spaces.insert(name.to_vec(), space.clone());
        space
    }

    fn set_space(&mut self, lexer: &Lexer, operands: &[Token], op: Token, stroke: bool) {
        let Some(&name_token) = operands.last().filter(|t| t.kind == Kind::Name) else {
            return;
        };
        let name = decode_name(&lexer.text(name_token)[1..]);
        let space = self.resolve(&name);
        *self.current(stroke) = Some(space.clone());
        let tone = self.env.settings.tone;
        let (cs_op, sc_op) = if stroke { ("CS", "SC") } else { ("cs", "sc") };

        match &*space {
            ColorSpace::Pattern { base: None } => {}
            ColorSpace::Pattern { base: Some(base) } => {
                if base.is_convertible() {
                    let mut text = b"/".to_vec();
                    text.extend_from_slice(self.env.pattern_gray_name);
                    text.extend_from_slice(format!(" {cs_op}").as_bytes());
                    self.replace(name_token.start, op.end, &text);
                    self.uses_pattern_gray = true;
                    self.report.color_operators += 1;
                } else if let ColorSpace::Unsupported(reason) = &**base {
                    self.report
                        .warn(format!("pattern color space not converted: {reason}"));
                }
            }
            ColorSpace::Unsupported(reason) => {
                self.report
                    .warn(format!("color space not converted: {reason}"));
            }
            s if !s.is_convertible() => {}
            s if s.is_gray() => {}
            s => {
                let gray = tone.apply(s.to_gray(&s.initial_color()).unwrap_or(0.0));
                let mut text = format!("/DeviceGray {cs_op}");
                if gray != 0.0 {
                    text.push_str(&format!(" {} {sc_op}", format_number(gray)));
                }
                self.replace(name_token.start, op.end, text.as_bytes());
                self.report.color_operators += 1;
            }
        }
    }

    fn set_color(&mut self, lexer: &Lexer, operands: &[Token], op: Token, stroke: bool) {
        let pattern = operands.last().filter(|t| t.kind == Kind::Name).copied();
        let numeric = &operands[..operands.len() - pattern.is_some() as usize];
        let count = numeric
            .iter()
            .rev()
            .take_while(|t| t.kind == Kind::Number)
            .count();
        let Some((start, values)) = Self::numbers(lexer, numeric, count) else {
            return;
        };
        let tone = self.env.settings.tone;
        let by_count = || match count {
            1 => Some(ColorSpace::Gray),
            3 => Some(ColorSpace::Rgb),
            4 => Some(ColorSpace::Cmyk),
            _ => None,
        };
        let space = self.current(stroke).clone();

        if let Some(pattern) = pattern {
            let Some(ColorSpace::Pattern { base: Some(base) }) = space.as_deref() else {
                return;
            };
            if count != base.components() || !base.is_convertible() {
                return;
            }
            let gray = tone.apply(base.to_gray(&values).unwrap_or(0.0));
            let mut text = format!("{} ", format_number(gray)).into_bytes();
            text.extend_from_slice(lexer.text(pattern));
            text.push(b' ');
            text.extend_from_slice(lexer.text(op));
            self.replace(start, op.end, &text);
            self.report.color_operators += 1;
            return;
        }

        let owned;
        let space: &ColorSpace = match space.as_deref() {
            Some(s) if s.is_convertible() && s.components() == count => s,
            // The tracked color space is unknown (inherited) or disagrees with the operands.
            Some(s) if !s.is_convertible() => return,
            _ => match by_count() {
                Some(s) => {
                    owned = s;
                    &owned
                }
                None => return,
            },
        };
        if space.is_gray() && !tone.is_monochrome() {
            return;
        }
        let Some(gray) = space.to_gray(&values) else {
            return;
        };
        let text = format!(
            "{} {}",
            format_number(tone.apply(gray)),
            String::from_utf8_lossy(lexer.text(op))
        );
        self.replace(start, op.end, text.as_bytes());
        self.report.color_operators += 1;
    }

    fn inline_image(&mut self, lexer: &mut Lexer, bi: Token) -> Result<(), Problem> {
        let mut tokens = Vec::new();
        loop {
            let Some(token) = lexer.next_token() else {
                return Ok(());
            };
            if token.kind == Kind::Keyword && lexer.text(token) == b"ID" {
                break;
            }
            tokens.push(token);
        }

        let mut dict = Dictionary::new();
        let mut index = 0;
        while index < tokens.len() {
            let key_token = tokens[index];
            index += 1;
            if key_token.kind != Kind::Name {
                continue;
            }
            let key = decode_name(&lexer.text(key_token)[1..]);
            match parse_object(lexer, &tokens, &mut index, 0) {
                Some(value) => dict.set(key, value),
                None => break,
            }
        }

        let image = crate::image::InlineImage::new(self.env, dict);
        let (data_start, data_end, ei_end) = lexer.inline_image_data(image.unfiltered_len());
        let data = &lexer.data()[data_start..data_end];
        match image.convert(self.env, data) {
            Ok(Some(bytes)) => {
                self.replace(bi.start, ei_end, &bytes);
                self.report.images += 1;
            }
            Ok(None) => {}
            Err(Problem::Limit(msg)) => return Err(Problem::Limit(msg)),
            Err(Problem::Unsupported(msg) | Problem::Invalid(msg)) => {
                self.report
                    .warn(format!("inline image not converted: {msg}"));
            }
        }
        Ok(())
    }
}
