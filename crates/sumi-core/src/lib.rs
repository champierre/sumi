//! Sumi converts the colors of a PDF to grayscale or monochrome without rasterizing it.
//!
//! Text, fonts and vector graphics are kept; only color operators, images, shadings and
//! annotation colors are rewritten.
//!
//! ```no_run
//! use sumi_core::{convert, ConvertOptions, Mode};
//!
//! fn main() -> Result<(), sumi_core::SumiError> {
//!     let mut options = ConvertOptions::default();
//!     options.mode = Mode::Grayscale;
//!     let report = convert("input.pdf", "output.pdf", &options)?;
//!     for warning in &report.warnings {
//!         eprintln!("not converted: {}", warning.message);
//!     }
//!     Ok(())
//! }
//! ```

mod color;
mod colorspace;
mod content;
mod convert;
mod error;
mod function;
mod image;
mod lexer;
mod objects;
mod options;
mod report;
mod shading;

use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use lopdf::{Document, LoadOptions};

pub use error::{Result, SumiError};
pub use options::{ConvertOptions, Limits, Mode};
pub use report::{Report, Warning};

use options::Settings;

/// The result of [`convert_bytes`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Converted {
    pub pdf: Vec<u8>,
    pub report: Report,
}

/// Converts a PDF file and writes the result to `output`.
///
/// The output is written to a temporary file next to `output` and renamed into place, so a
/// failed conversion never leaves a partial file behind.
pub fn convert(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    options: &ConvertOptions,
) -> Result<Report> {
    let data = std::fs::read(input)?;
    let converted = convert_bytes(&data, options)?;
    write_atomically(output.as_ref(), &converted.pdf)?;
    Ok(converted.report)
}

/// Converts a PDF file to grayscale with default options.
pub fn grayscale(input: impl AsRef<Path>, output: impl AsRef<Path>) -> Result<Report> {
    convert(input, output, &ConvertOptions::grayscale())
}

/// Converts a PDF file to black and white.
pub fn monochrome(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    threshold: f32,
) -> Result<Report> {
    convert(input, output, &ConvertOptions::monochrome(threshold))
}

/// Converts a PDF held in memory.
pub fn convert_bytes(input: &[u8], options: &ConvertOptions) -> Result<Converted> {
    options.validate().map_err(SumiError::InvalidOptions)?;
    let header_window = &input[..input.len().min(1024)];
    if !header_window.windows(5).any(|w| w == b"%PDF-") {
        return Err(SumiError::InvalidPdf("missing %PDF- header".to_string()));
    }

    let settings = Settings::from(options);
    let converted = match catch_unwind(AssertUnwindSafe(|| convert_document(input, &settings))) {
        Ok(result) => result?,
        Err(panic) => {
            let message = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic while converting".to_string());
            return Err(SumiError::Internal(message));
        }
    };

    if options.strict && !converted.report.warnings.is_empty() {
        let messages = converted
            .report
            .warnings
            .iter()
            .map(|w| w.message.clone())
            .collect();
        return Err(SumiError::Unsupported(messages));
    }
    Ok(converted)
}

fn convert_document(input: &[u8], settings: &Settings) -> Result<Converted> {
    let load_options = LoadOptions {
        max_decompressed_size: Some(settings.limits.max_stream_bytes),
        ..LoadOptions::default()
    };
    let doc = Document::load_mem_with_options(input, load_options).map_err(|err| match err {
        lopdf::Error::Decryption(_)
        | lopdf::Error::InvalidPassword
        | lopdf::Error::AlreadyEncrypted => SumiError::EncryptedPdf,
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { limit }) => {
            SumiError::LimitExceeded(format!("a stream decompresses to more than {limit} bytes"))
        }
        other => SumiError::InvalidPdf(other.to_string()),
    })?;
    if doc.trailer.has(b"Encrypt") || doc.encryption_state.is_some() {
        return Err(SumiError::EncryptedPdf);
    }
    if doc.get_pages().is_empty() {
        return Err(SumiError::InvalidPdf(
            "the document has no pages".to_string(),
        ));
    }
    let dropped = check_unreadable_objects(input, &doc)?;

    let mut converter = convert::Converter::new(doc, settings);
    converter.run()?;
    let mut report = converter.report;
    for warning in dropped {
        report.warn(warning);
    }
    let mut doc = converter.doc;

    doc.prune_objects();
    // The output uses a classic cross-reference table; drop keys that only apply to the
    // cross-reference streams and incremental updates of the input.
    for key in [
        &b"Prev"[..],
        b"XRefStm",
        b"Type",
        b"W",
        b"Index",
        b"Filter",
        b"DecodeParms",
        b"Length",
    ] {
        doc.trailer.remove(key);
    }
    let mut pdf = Vec::new();
    doc.save_to(&mut pdf)?;
    Ok(Converted { pdf, report })
}

/// Finds objects that are referenced and present in the file but could not be parsed.
///
/// The PDF parser skips objects it cannot read. If a page needs one of them (content streams,
/// resources, annotations) the output would silently lose content, so the input is refused.
/// Other unreadable objects (e.g. a broken structure tree element) are reported as warnings.
fn check_unreadable_objects(input: &[u8], doc: &Document) -> Result<Vec<String>> {
    fn collect(obj: &lopdf::Object, refs: &mut std::collections::BTreeSet<u32>) {
        match obj {
            lopdf::Object::Reference((id, _)) => {
                refs.insert(*id);
            }
            lopdf::Object::Array(items) => items.iter().for_each(|o| collect(o, refs)),
            lopdf::Object::Dictionary(dict) => dict.iter().for_each(|(_, o)| collect(o, refs)),
            lopdf::Object::Stream(stream) => stream.dict.iter().for_each(|(_, o)| collect(o, refs)),
            _ => {}
        }
    }

    let mut refs = std::collections::BTreeSet::new();
    doc.objects.values().for_each(|o| collect(o, &mut refs));
    doc.trailer.iter().for_each(|(_, o)| collect(o, &mut refs));
    let loaded: std::collections::BTreeSet<u32> = doc.objects.keys().map(|(id, _)| *id).collect();
    let missing: std::collections::BTreeSet<u32> = refs.difference(&loaded).copied().collect();
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let declared: std::collections::BTreeSet<u32> =
        declared_object_numbers(input).into_iter().collect();
    let unreadable: std::collections::BTreeSet<u32> =
        missing.intersection(&declared).copied().collect();
    if unreadable.is_empty() {
        return Ok(Vec::new());
    }

    // Everything a page draws is reachable from its /Contents, /Resources (possibly inherited)
    // and /Annots; /Parent and /P point back up the tree and are not followed.
    let mut stack: Vec<&lopdf::Object> = Vec::new();
    for page in doc.get_pages().into_values() {
        let mut node = Some(page);
        let mut depth = 0;
        while let Some(id) = node.take()
            && depth < 64
            && let Ok(dict) = doc.get_dictionary(id)
        {
            let keys: &[&[u8]] = if depth == 0 {
                &[b"Contents", b"Resources", b"Annots"]
            } else {
                &[b"Resources"]
            };
            stack.extend(keys.iter().filter_map(|k| dict.get(k).ok()));
            node = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
            depth += 1;
        }
    }
    let mut visited = std::collections::HashSet::new();
    while let Some(obj) = stack.pop() {
        match obj {
            lopdf::Object::Reference(id) => {
                if unreadable.contains(&id.0) {
                    return Err(SumiError::InvalidPdf(format!(
                        "object {} used by a page could not be read",
                        id.0
                    )));
                }
                if visited.insert(*id)
                    && let Ok(target) = doc.get_object(*id)
                {
                    stack.push(target);
                }
            }
            lopdf::Object::Array(items) => stack.extend(items),
            lopdf::Object::Dictionary(dict) => stack.extend(
                dict.iter()
                    .filter(|(k, _)| !matches!(k.as_slice(), b"Parent" | b"P"))
                    .map(|(_, v)| v),
            ),
            lopdf::Object::Stream(stream) => stack.extend(stream.dict.iter().map(|(_, v)| v)),
            _ => {}
        }
    }
    Ok(unreadable
        .iter()
        .map(|id| format!("object {id} could not be read and was dropped"))
        .collect())
}

/// Object numbers declared as `N G obj` in the raw file.
fn declared_object_numbers(input: &[u8]) -> Vec<u32> {
    let is_space = |b: u8| matches!(b, b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ');
    let mut numbers = Vec::new();
    let mut pos = 0;
    while let Some(offset) = input[pos..].windows(3).position(|w| w == b"obj") {
        let at = pos + offset;
        pos = at + 3;
        if input.get(at + 3).is_some_and(|b| b.is_ascii_alphanumeric()) {
            continue;
        }
        let mut i = at;
        let skip = |i: &mut usize, pred: &dyn Fn(u8) -> bool| {
            let start = *i;
            while *i > 0 && pred(input[*i - 1]) {
                *i -= 1;
            }
            *i < start
        };
        if !skip(&mut i, &is_space)
            || !skip(&mut i, &|b| b.is_ascii_digit())
            || !skip(&mut i, &is_space)
        {
            continue;
        }
        let end = i;
        if !skip(&mut i, &|b| b.is_ascii_digit()) || (i > 0 && input[i - 1].is_ascii_alphanumeric())
        {
            continue;
        }
        if let Some(n) = std::str::from_utf8(&input[i..end])
            .ok()
            .and_then(|s| s.parse().ok())
        {
            numbers.push(n);
        }
    }
    numbers
}

fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!(".{file_name}.sumi-{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
