//! Small helpers for reading lopdf objects leniently.

use std::io::Write;

use lopdf::{Dictionary, Document, Object, Stream};

use crate::error::Problem;

static NULL: Object = Object::Null;

/// Follows references; a dangling reference yields `Null`.
pub(crate) fn deref<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    match doc.dereference(obj) {
        Ok((_, o)) => o,
        Err(_) => &NULL,
    }
}

/// Dictionary lookup that follows references and treats `Null` as missing.
pub(crate) fn get<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    match dict.get(key).ok().map(|o| deref(doc, o)) {
        Some(Object::Null) | None => None,
        some => some,
    }
}

pub(crate) fn number(obj: &Object) -> Option<f64> {
    match *obj {
        Object::Integer(i) => Some(i as f64),
        Object::Real(r) => Some(r as f64),
        _ => None,
    }
}

pub(crate) fn get_number(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<f64> {
    get(doc, dict, key).and_then(number)
}

pub(crate) fn get_int(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<i64> {
    get(doc, dict, key).and_then(|o| match *o {
        Object::Integer(i) => Some(i),
        Object::Real(r) => Some(r as i64),
        _ => None,
    })
}

pub(crate) fn get_bool(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<bool> {
    match get(doc, dict, key) {
        Some(Object::Boolean(b)) => Some(*b),
        _ => None,
    }
}

pub(crate) fn numbers(doc: &Document, obj: &Object) -> Option<Vec<f64>> {
    match deref(doc, obj) {
        Object::Array(items) => items.iter().map(|o| number(deref(doc, o))).collect(),
        _ => None,
    }
}

pub(crate) fn get_numbers(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<Vec<f64>> {
    get(doc, dict, key).and_then(|o| numbers(doc, o))
}

pub(crate) fn get_name<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a [u8]> {
    match get(doc, dict, key) {
        Some(Object::Name(n)) => Some(n),
        _ => None,
    }
}

/// Returns the dictionary of a dictionary or stream value.
pub(crate) fn as_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match deref(doc, obj) {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

pub(crate) fn get_dict<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Dictionary> {
    dict.get(key).ok().and_then(|o| as_dict(doc, o))
}

pub(crate) fn pairs(nums: &[f64]) -> Vec<[f64; 2]> {
    nums.as_chunks::<2>().0.to_vec()
}

pub(crate) fn name_str(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

/// The stream's filter names in decoding order.
pub(crate) fn filters(doc: &Document, dict: &Dictionary) -> Vec<Vec<u8>> {
    match get(doc, dict, b"Filter") {
        Some(Object::Name(n)) => vec![n.clone()],
        Some(Object::Array(items)) => items
            .iter()
            .filter_map(|o| match deref(doc, o) {
                Object::Name(n) => Some(n.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Decodes a stream with a size limit.
///
/// lopdf only looks at direct `/Filter` and `/DecodeParms` dictionary values, so indirect values
/// and single-element `/DecodeParms` arrays are normalized first.
pub(crate) fn decode_stream(
    doc: &Document,
    stream: &Stream,
    limit: usize,
) -> Result<Vec<u8>, Problem> {
    let filters = filters(doc, &stream.dict);
    if filters.is_empty() {
        if stream.content.len() > limit {
            return Err(Problem::Limit(format!(
                "a stream is larger than {limit} bytes"
            )));
        }
        return Ok(stream.content.clone());
    }

    let params = match get(doc, &stream.dict, b"DecodeParms") {
        None => None,
        Some(Object::Dictionary(d)) => Some(d.clone()),
        Some(Object::Array(items)) => {
            // One entry per filter; lopdf applies a single dictionary, which is only correct when
            // at most one filter has parameters.
            let dicts: Vec<&Dictionary> = items.iter().filter_map(|o| as_dict(doc, o)).collect();
            match dicts.len() {
                0 => None,
                1 => Some(dicts[0].clone()),
                _ => {
                    return Err(Problem::unsupported(
                        "streams with several parameterized filters",
                    ));
                }
            }
        }
        Some(_) => None,
    };

    let direct_filter = match stream.dict.get(b"Filter") {
        Ok(Object::Name(_)) => true,
        Ok(Object::Array(items)) => items.iter().all(|o| matches!(o, Object::Name(_))),
        _ => false,
    };
    let direct_params = matches!(
        stream.dict.get(b"DecodeParms"),
        Err(_) | Ok(Object::Dictionary(_))
    );
    if direct_filter && direct_params {
        return Ok(stream.decompressed_content_with_limit(limit)?);
    }

    let mut dict = Dictionary::new();
    dict.set(
        "Filter",
        Object::Array(filters.into_iter().map(Object::Name).collect()),
    );
    if let Some(params) = params {
        dict.set("DecodeParms", params);
    }
    let normalized = Stream::new(dict, stream.content.clone());
    Ok(normalized.decompressed_content_with_limit(limit)?)
}

pub(crate) fn flate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    // Writing into a Vec cannot fail.
    encoder.write_all(data).expect("write to Vec");
    encoder.finish().expect("write to Vec")
}

/// Creates a Flate-compressed stream.
pub(crate) fn flate_stream(mut dict: Dictionary, data: &[u8]) -> Stream {
    dict.remove(b"DecodeParms");
    dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    Stream::new(dict, flate(data))
}

/// Replaces a stream's data with Flate-compressed `data`.
pub(crate) fn set_flate_content(stream: &mut Stream, data: &[u8]) {
    stream.dict.remove(b"DecodeParms");
    stream
        .dict
        .set("Filter", Object::Name(b"FlateDecode".to_vec()));
    stream.set_content(flate(data));
}
