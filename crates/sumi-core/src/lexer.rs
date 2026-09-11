//! Content stream tokenizer that keeps byte offsets, so the rewriter can replace individual
//! operators and copy everything else verbatim.

use lopdf::{Dictionary, Object, StringFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Number,
    Name,
    String,
    ArrayOpen,
    ArrayClose,
    DictOpen,
    DictClose,
    /// Operators, `true`/`false`/`null` and stray bytes.
    Keyword,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
}

pub(crate) fn is_whitespace(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

pub(crate) fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

pub(crate) fn is_regular(b: u8) -> bool {
    !is_whitespace(b) && !is_delimiter(b)
}

pub(crate) struct Lexer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Lexer { data, pos: 0 }
    }

    pub(crate) fn data(&self) -> &'a [u8] {
        self.data
    }

    pub(crate) fn text(&self, token: Token) -> &'a [u8] {
        &self.data[token.start..token.end]
    }

    fn skip_whitespace_and_comments(&mut self) {
        while let Some(&b) = self.data.get(self.pos) {
            if is_whitespace(b) {
                self.pos += 1;
            } else if b == b'%' {
                while self.pos < self.data.len() && !matches!(self.data[self.pos], b'\r' | b'\n') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    pub(crate) fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace_and_comments();
        let data = self.data;
        let start = self.pos;
        let b = *data.get(start)?;
        let (kind, end) = match b {
            b'(' => {
                let mut depth = 0usize;
                let mut i = start;
                loop {
                    match data.get(i) {
                        None => break (Kind::String, data.len()),
                        Some(b'\\') => i += 2,
                        Some(b'(') => {
                            depth += 1;
                            i += 1;
                        }
                        Some(b')') => {
                            depth -= 1;
                            i += 1;
                            if depth == 0 {
                                break (Kind::String, i);
                            }
                        }
                        Some(_) => i += 1,
                    }
                }
            }
            b'<' if data.get(start + 1) == Some(&b'<') => (Kind::DictOpen, start + 2),
            b'<' => {
                let end = data[start..]
                    .iter()
                    .position(|&c| c == b'>')
                    .map_or(data.len(), |p| start + p + 1);
                (Kind::String, end)
            }
            b'>' if data.get(start + 1) == Some(&b'>') => (Kind::DictClose, start + 2),
            b'[' => (Kind::ArrayOpen, start + 1),
            b']' => (Kind::ArrayClose, start + 1),
            b'/' => {
                let mut i = start + 1;
                while i < data.len() && is_regular(data[i]) {
                    i += 1;
                }
                (Kind::Name, i)
            }
            _ if is_delimiter(b) => (Kind::Keyword, start + 1),
            _ => {
                let mut i = start;
                while i < data.len() && is_regular(data[i]) {
                    i += 1;
                }
                let kind = if parse_number(&data[start..i]).is_some() {
                    Kind::Number
                } else {
                    Kind::Keyword
                };
                (kind, i)
            }
        };
        self.pos = end.min(data.len());
        Some(Token {
            kind,
            start,
            end: self.pos,
        })
    }

    /// After the `ID` operator, finds the inline image data and the end of the `EI` operator.
    ///
    /// Returns `(data_start, data_end, ei_end)`. When the data length is known (unfiltered
    /// images) it is trusted if an `EI` follows; otherwise the data is scanned for an `EI` that is
    /// followed by something that looks like content stream text.
    pub(crate) fn inline_image_data(
        &mut self,
        expected_len: Option<usize>,
    ) -> (usize, usize, usize) {
        let data = self.data;
        let data_start = (self.pos + 1).min(data.len());

        if let Some(len) = expected_len {
            let data_end = data_start.saturating_add(len);
            if data_end <= data.len() {
                let mut i = data_end;
                while i < data.len() && is_whitespace(data[i]) {
                    i += 1;
                }
                if data[i..].starts_with(b"EI") && data.get(i + 2).is_none_or(|&c| !is_regular(c)) {
                    self.pos = i + 2;
                    return (data_start, data_end, i + 2);
                }
            }
        }

        let mut i = data_start;
        while i + 1 < data.len() {
            if data[i] == b'E'
                && data[i + 1] == b'I'
                && (i == data_start || is_whitespace(data[i - 1]))
                && data.get(i + 2).is_none_or(|&c| !is_regular(c))
                && looks_like_content(&data[i + 2..])
            {
                let data_end = if i > data_start { i - 1 } else { i };
                self.pos = i + 2;
                return (data_start, data_end, i + 2);
            }
            i += 1;
        }
        self.pos = data.len();
        (data_start, data.len(), data.len())
    }
}

fn looks_like_content(rest: &[u8]) -> bool {
    rest.iter()
        .take(64)
        .all(|&c| c.is_ascii_graphic() || is_whitespace(c))
}

pub(crate) fn parse_number(text: &[u8]) -> Option<f64> {
    let mut digits = 0;
    let mut dots = 0;
    for (i, &c) in text.iter().enumerate() {
        match c {
            b'0'..=b'9' => digits += 1,
            b'.' => dots += 1,
            b'+' | b'-' if i == 0 => {}
            _ => return None,
        }
    }
    if digits == 0 || dots > 1 {
        return None;
    }
    let s = std::str::from_utf8(text).ok()?;
    let s = s.strip_prefix('+').unwrap_or(s);
    let s = s.strip_suffix('.').unwrap_or(s);
    let (sign, body) = match s.strip_prefix('-') {
        Some(body) => (-1.0, body),
        None => (1.0, s),
    };
    let body = if body.starts_with('.') {
        format!("0{body}")
    } else {
        body.to_string()
    };
    body.parse::<f64>().ok().map(|v| sign * v)
}

/// Parses tokens into a lopdf object (used for inline image dictionaries).
pub(crate) fn parse_object(
    lexer: &Lexer,
    tokens: &[Token],
    index: &mut usize,
    depth: usize,
) -> Option<Object> {
    let token = *tokens.get(*index)?;
    *index += 1;
    let text = lexer.text(token);
    Some(match token.kind {
        Kind::Number => {
            let v = parse_number(text)?;
            if text.contains(&b'.') {
                Object::Real(v as f32)
            } else {
                Object::Integer(v as i64)
            }
        }
        Kind::Name => Object::Name(decode_name(&text[1..])),
        Kind::String => Object::String(decode_string(text), StringFormat::Literal),
        Kind::Keyword => match text {
            b"true" => Object::Boolean(true),
            b"false" => Object::Boolean(false),
            b"null" => Object::Null,
            _ => return None,
        },
        Kind::ArrayOpen if depth < 16 => {
            let mut items = Vec::new();
            loop {
                match tokens.get(*index) {
                    None => return None,
                    Some(t) if t.kind == Kind::ArrayClose => {
                        *index += 1;
                        break;
                    }
                    Some(_) => items.push(parse_object(lexer, tokens, index, depth + 1)?),
                }
            }
            Object::Array(items)
        }
        Kind::DictOpen if depth < 16 => {
            let mut dict = Dictionary::new();
            loop {
                match tokens.get(*index) {
                    None => return None,
                    Some(t) if t.kind == Kind::DictClose => {
                        *index += 1;
                        break;
                    }
                    Some(t) if t.kind == Kind::Name => {
                        let key = decode_name(&lexer.text(*t)[1..]);
                        *index += 1;
                        let value = parse_object(lexer, tokens, index, depth + 1)?;
                        dict.set(key, value);
                    }
                    Some(_) => return None,
                }
            }
            Object::Dictionary(dict)
        }
        _ => return None,
    })
}

/// Serializes an object in content stream syntax (used to re-emit inline image dictionaries).
pub(crate) fn write_object(out: &mut Vec<u8>, obj: &Object) {
    match obj {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Boolean(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Object::Integer(i) => out.extend_from_slice(i.to_string().as_bytes()),
        Object::Real(r) => out.extend_from_slice(crate::color::format_number(*r as f64).as_bytes()),
        Object::Name(name) => {
            out.push(b'/');
            for &c in name {
                if is_regular(c) && c != b'#' && c.is_ascii_graphic() {
                    out.push(c);
                } else {
                    out.extend_from_slice(format!("#{c:02X}").as_bytes());
                }
            }
        }
        Object::String(bytes, _) => {
            out.push(b'<');
            for c in bytes {
                out.extend_from_slice(format!("{c:02X}").as_bytes());
            }
            out.push(b'>');
        }
        Object::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_object(out, item);
            }
            out.push(b']');
        }
        Object::Dictionary(dict) => {
            out.extend_from_slice(b"<<");
            for (key, value) in dict.iter() {
                write_object(out, &Object::Name(key.clone()));
                out.push(b' ');
                write_object(out, value);
            }
            out.extend_from_slice(b">>");
        }
        // Not valid inside content streams.
        Object::Stream(_) | Object::Reference(_) => out.extend_from_slice(b"null"),
    }
}

pub(crate) fn decode_name(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'#'
            && i + 2 < raw.len()
            && let Some(v) = std::str::from_utf8(&raw[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    out
}

/// Decodes a literal `( … )` or hex `< … >` string token.
pub(crate) fn decode_string(text: &[u8]) -> Vec<u8> {
    if text.first() == Some(&b'<') {
        let hex: Vec<u8> = text[1..]
            .iter()
            .copied()
            .take_while(|&c| c != b'>')
            .filter(u8::is_ascii_hexdigit)
            .collect();
        return hex
            .chunks(2)
            .map(|pair| {
                let hi = (pair[0] as char).to_digit(16).unwrap_or(0) as u8;
                let lo = pair
                    .get(1)
                    .map_or(0, |&c| (c as char).to_digit(16).unwrap_or(0) as u8);
                hi << 4 | lo
            })
            .collect();
    }

    let inner = text.strip_prefix(b"(").unwrap_or(text);
    let inner = inner.strip_suffix(b")").unwrap_or(inner);
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        let c = inner[i];
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&e) = inner.get(i) else { break };
        match e {
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'0'..=b'7' => {
                let mut v = 0u32;
                let mut n = 0;
                while n < 3 && i < inner.len() && (b'0'..=b'7').contains(&inner[i]) {
                    v = v * 8 + (inner[i] - b'0') as u32;
                    i += 1;
                    n += 1;
                }
                out.push(v as u8);
                continue;
            }
            b'\r' => {
                if inner.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
            }
            b'\n' => {}
            other => out.push(other),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(data: &[u8]) -> Vec<(Kind, String)> {
        let mut lexer = Lexer::new(data);
        std::iter::from_fn(|| lexer.next_token())
            .map(|t| {
                (
                    t.kind,
                    String::from_utf8_lossy(&data[t.start..t.end]).into_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn tokenizes_operators_strings_and_names() {
        let tokens =
            kinds(b"1 0 0 rg (a(b)c\\)) Tj/F1 12 Tf <48656c> % comment\n[.5 -3.]TJ<</MCID 0>>BDC");
        let expected = [
            (Kind::Number, "1"),
            (Kind::Number, "0"),
            (Kind::Number, "0"),
            (Kind::Keyword, "rg"),
            (Kind::String, "(a(b)c\\))"),
            (Kind::Keyword, "Tj"),
            (Kind::Name, "/F1"),
            (Kind::Number, "12"),
            (Kind::Keyword, "Tf"),
            (Kind::String, "<48656c>"),
            (Kind::ArrayOpen, "["),
            (Kind::Number, ".5"),
            (Kind::Number, "-3."),
            (Kind::ArrayClose, "]"),
            (Kind::Keyword, "TJ"),
            (Kind::DictOpen, "<<"),
            (Kind::Name, "/MCID"),
            (Kind::Number, "0"),
            (Kind::DictClose, ">>"),
            (Kind::Keyword, "BDC"),
        ];
        let expected: Vec<(Kind, String)> =
            expected.iter().map(|(k, s)| (*k, s.to_string())).collect();
        assert_eq!(tokens, expected);
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_number(b".5"), Some(0.5));
        assert_eq!(parse_number(b"-.25"), Some(-0.25));
        assert_eq!(parse_number(b"4."), Some(4.0));
        assert_eq!(parse_number(b"+7"), Some(7.0));
        assert_eq!(parse_number(b"1.2.3"), None);
        assert_eq!(parse_number(b"-"), None);
    }

    #[test]
    fn strings_and_names_decode() {
        assert_eq!(decode_string(b"(a\\(b\\)\\101\\n)"), b"a(b)A\n");
        assert_eq!(decode_string(b"<48 65 6c6>"), b"Hel`");
        assert_eq!(decode_name(b"A#20B"), b"A B");
    }

    #[test]
    fn inline_image_with_embedded_ei() {
        let data = b"BI /W 2 /H 1 /BPC 8 /CS /G /F /Fl ID \x01EI\xff\x00 EI Q";
        let mut lexer = Lexer::new(data);
        while let Some(t) = lexer.next_token() {
            if lexer.text(t) == b"ID" {
                break;
            }
        }
        let (start, end, ei_end) = lexer.inline_image_data(None);
        assert_eq!(&data[start..end], b"\x01EI\xff\x00");
        assert_eq!(&data[ei_end..], b" Q");
    }
}
