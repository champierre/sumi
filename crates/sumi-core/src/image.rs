//! Converts image XObjects and inline images.
//!
//! Indexed images keep their sample data and only get a gray palette. Other images are decoded,
//! converted to 8-bit gray (or 1-bit in monochrome mode) and re-encoded with Flate.

use std::borrow::Cow;
use std::io::Cursor;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};

use crate::color::{cmyk8_to_gray, rgb8_to_gray};
use crate::colorspace::ColorSpace;
use crate::content::Env;
use crate::error::Problem;
use crate::objects::{
    decode_stream, filters, flate, flate_stream, get, get_bool, get_int, get_numbers, name_str,
    pairs, set_flate_content,
};
use crate::options::Settings;
use crate::report::Report;

enum Converted {
    /// New `/ColorSpace` for an indexed image; the samples are unchanged.
    Palette { hival: usize, lookup: Vec<u8> },
    /// DeviceGray samples.
    Pixels {
        width: usize,
        height: usize,
        bits: u32,
        data: Vec<u8>,
        /// 8-bit soft mask replacing a color key `/Mask`.
        soft_mask: Option<Vec<u8>>,
    },
}

fn convert(
    doc: &Document,
    dict: &Dictionary,
    stream: &Stream,
    resources: Option<&Dictionary>,
    settings: &Settings,
) -> Result<Option<Converted>, Problem> {
    let tone = settings.tone;
    let limits = &settings.limits;
    if get_bool(doc, dict, b"ImageMask") == Some(true) {
        return Ok(None);
    }
    let filters = filters(doc, dict);
    let last = filters.last().map(Vec::as_slice);
    let space = match dict.get(b"ColorSpace") {
        Ok(cs) => ColorSpace::resolve(doc, cs, resources, limits.max_stream_bytes),
        Err(_) if last == Some(b"JPXDecode") => {
            return Err(Problem::unsupported("JPEG 2000 image"));
        }
        Err(_) => return Err(Problem::invalid("image without /ColorSpace")),
    };
    let width = get_int(doc, dict, b"Width")
        .filter(|&w| w > 0)
        .ok_or_else(|| Problem::invalid("image without /Width"))? as usize;
    let height = get_int(doc, dict, b"Height")
        .filter(|&h| h > 0)
        .ok_or_else(|| Problem::invalid("image without /Height"))? as usize;
    let bits = get_int(doc, dict, b"BitsPerComponent").unwrap_or(8) as u32;
    if ![1, 2, 4, 8, 16].contains(&bits) {
        return Err(Problem::invalid(format!(
            "image with {bits} bits per component"
        )));
    }
    if (width as u64).saturating_mul(height as u64) > limits.max_image_pixels {
        return Err(Problem::unsupported(format!(
            "image of {width}x{height} pixels exceeds the size limit"
        )));
    }
    let decode = get_numbers(doc, dict, b"Decode").map(|d| pairs(&d));

    match &space {
        ColorSpace::Unsupported(reason) => {
            return Err(Problem::Unsupported(format!("image color space {reason}")));
        }
        s if !s.is_convertible() => return Ok(None),
        ColorSpace::Gray => {
            let default_decode = decode
                .as_deref()
                .is_none_or(|d| d.first() == Some(&[0.0, 1.0]));
            if !tone.is_monochrome() || (bits == 1 && default_decode) {
                return Ok(None);
            }
        }
        ColorSpace::Indexed { .. } => {
            let grays = space
                .palette_grays()
                .ok_or_else(|| Problem::invalid("malformed palette"))?;
            let lookup = grays
                .iter()
                .map(|&g| (tone.apply(g) * 255.0).round() as u8)
                .collect::<Vec<_>>();
            return Ok(Some(Converted::Palette {
                hival: grays.len() - 1,
                lookup,
            }));
        }
        _ => {}
    }

    let (width, height, space, bits, samples, decode) = match last {
        Some(b"DCTDecode") => {
            if filters.len() != 1 {
                return Err(Problem::unsupported(
                    "DCTDecode combined with other filters",
                ));
            }
            let jpeg = decode_jpeg(&stream.content, settings)?;
            let space = if jpeg.components == space.components() && !jpeg.converted {
                space
            } else {
                match jpeg.components {
                    1 => ColorSpace::Gray,
                    3 => ColorSpace::Rgb,
                    _ => ColorSpace::Cmyk,
                }
            };
            let decode = if jpeg.converted { None } else { decode };
            (jpeg.width, jpeg.height, space, 8, jpeg.pixels, decode)
        }
        Some(f @ (b"JPXDecode" | b"JBIG2Decode" | b"CCITTFaxDecode")) => {
            return Err(Problem::Unsupported(format!(
                "{} image in {}",
                name_str(f),
                space.kind()
            )));
        }
        _ => (
            width,
            height,
            space,
            bits,
            decode_stream(doc, stream, limits.max_stream_bytes)?,
            decode,
        ),
    };

    let n = space.components();
    let decode = match decode {
        Some(d) if d.len() >= n => d,
        _ => space.default_decode(bits),
    };
    let row_bytes = (width * n * bits as usize).div_ceil(8);
    let needed = row_bytes * height;
    let samples = if samples.len() < needed {
        let mut padded = samples;
        padded.resize(needed, 0);
        Cow::Owned(padded)
    } else {
        Cow::Owned(samples)
    };

    let gray = to_gray8(&space, bits, &decode, width, height, &samples);
    let soft_mask = match get(doc, dict, b"Mask") {
        Some(Object::Array(_)) if dict.get(b"SMask").is_err() => get_numbers(doc, dict, b"Mask")
            .filter(|ranges| ranges.len() >= 2 * n)
            .map(|ranges| color_key_mask(&ranges, n, bits, width, height, &samples)),
        _ => None,
    };
    let (bits, data) = if tone.is_monochrome() {
        (1, to_bilevel(&gray, width, height, settings))
    } else {
        (8, gray)
    };
    Ok(Some(Converted::Pixels {
        width,
        height,
        bits,
        data,
        soft_mask,
    }))
}

fn sample(row: &[u8], index: usize, bits: u32) -> u32 {
    match bits {
        8 => row[index] as u32,
        16 => (row[2 * index] as u32) << 8 | row[2 * index + 1] as u32,
        _ => {
            let bit = index * bits as usize;
            let byte = row[bit / 8] as u32;
            (byte >> (8 - bit % 8 - bits as usize)) & ((1 << bits) - 1)
        }
    }
}

fn to_gray8(
    space: &ColorSpace,
    bits: u32,
    decode: &[[f64; 2]],
    width: usize,
    height: usize,
    samples: &[u8],
) -> Vec<u8> {
    let n = space.components();
    let row_bytes = (width * n * bits as usize).div_ceil(8);
    let mut out = vec![0u8; width * height];
    let rows = samples.chunks(row_bytes.max(1)).zip(out.chunks_mut(width));
    let default_decode = decode[..n] == space.default_decode(bits)[..];

    if bits == 8
        && default_decode
        && matches!(space, ColorSpace::Gray | ColorSpace::Rgb | ColorSpace::Cmyk)
    {
        for (src, dst) in rows {
            match space {
                ColorSpace::Gray => dst.copy_from_slice(&src[..width]),
                ColorSpace::Rgb => {
                    for (d, p) in dst.iter_mut().zip(src.as_chunks::<3>().0) {
                        *d = rgb8_to_gray(p[0], p[1], p[2]);
                    }
                }
                _ => {
                    for (d, p) in dst.iter_mut().zip(src.as_chunks::<4>().0) {
                        *d = cmyk8_to_gray(p[0], p[1], p[2], p[3]);
                    }
                }
            }
        }
        return out;
    }

    let max = ((1u64 << bits) - 1) as f64;
    let value = |s: u32, [lo, hi]: [f64; 2]| lo + s as f64 * (hi - lo) / max;
    let to_byte = |g: Option<f64>| (g.unwrap_or(1.0) * 255.0).round() as u8;

    if n == 1 && bits <= 8 {
        let lut: Vec<u8> = (0..=max as u32)
            .map(|s| to_byte(space.to_gray(&[value(s, decode[0])])))
            .collect();
        for (src, dst) in rows {
            for (x, d) in dst.iter_mut().enumerate() {
                *d = lut[sample(src, x, bits) as usize];
            }
        }
        return out;
    }

    let mut comps = vec![0f64; n];
    for (src, dst) in rows {
        for (x, d) in dst.iter_mut().enumerate() {
            for (c, comp) in comps.iter_mut().enumerate() {
                *comp = value(sample(src, x * n + c, bits), decode[c]);
            }
            *d = to_byte(space.to_gray(&comps));
        }
    }
    out
}

/// Packs gray pixels into 1-bit rows (1 = white), thresholding or dithering.
fn to_bilevel(gray: &[u8], width: usize, height: usize, settings: &Settings) -> Vec<u8> {
    let row_bytes = width.div_ceil(8);
    let mut out = vec![0u8; row_bytes * height];
    let tone = settings.tone;
    if !settings.dither {
        let white: Vec<bool> = (0..=255)
            .map(|g| tone.apply(g as f64 / 255.0) == 1.0)
            .collect();
        for (y, row) in gray.chunks(width).enumerate() {
            for (x, &g) in row.iter().enumerate() {
                if white[g as usize] {
                    out[y * row_bytes + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        return out;
    }

    let threshold = tone.threshold * 255.0;
    let mut current = vec![0f64; width + 2];
    let mut next = vec![0f64; width + 2];
    for (y, row) in gray.chunks(width).enumerate() {
        for (x, &g) in row.iter().enumerate() {
            let v = g as f64 + current[x + 1];
            let (bit, target) = if v < threshold {
                (false, 0.0)
            } else {
                (true, 255.0)
            };
            if bit {
                out[y * row_bytes + x / 8] |= 0x80 >> (x % 8);
            }
            let err = v - target;
            current[x + 2] += err * 7.0 / 16.0;
            next[x] += err * 3.0 / 16.0;
            next[x + 1] += err * 5.0 / 16.0;
            next[x + 2] += err / 16.0;
        }
        std::mem::swap(&mut current, &mut next);
        next.iter_mut().for_each(|e| *e = 0.0);
    }
    out
}

fn color_key_mask(
    ranges: &[f64],
    n: usize,
    bits: u32,
    width: usize,
    height: usize,
    samples: &[u8],
) -> Vec<u8> {
    let row_bytes = (width * n * bits as usize).div_ceil(8);
    let mut mask = vec![255u8; width * height];
    for (src, dst) in samples.chunks(row_bytes.max(1)).zip(mask.chunks_mut(width)) {
        for (x, d) in dst.iter_mut().enumerate() {
            let masked = (0..n).all(|c| {
                let s = sample(src, x * n + c, bits) as f64;
                ranges[2 * c] <= s && s <= ranges[2 * c + 1]
            });
            if masked {
                *d = 0;
            }
        }
    }
    mask
}

struct Jpeg {
    width: usize,
    height: usize,
    components: usize,
    /// The decoder converted the colors (YCCK to RGB), so `/Decode` no longer applies.
    converted: bool,
    pixels: Vec<u8>,
}

fn decode_jpeg(data: &[u8], settings: &Settings) -> Result<Jpeg, Problem> {
    use zune_jpeg::JpegDecoder;
    use zune_jpeg::zune_core::colorspace::ColorSpace as Z;
    use zune_jpeg::zune_core::options::DecoderOptions;

    let options = DecoderOptions::default()
        .set_max_width(1 << 16)
        .set_max_height(1 << 16);
    let error = |e: zune_jpeg::errors::DecodeErrors| {
        Problem::invalid(format!("JPEG decoding failed: {e:?}"))
    };

    let mut headers = JpegDecoder::new_with_options(Cursor::new(data), options);
    headers.decode_headers().map_err(error)?;
    let info = headers
        .info()
        .ok_or_else(|| Problem::invalid("JPEG without dimensions"))?;
    let (width, height) = (info.width as usize, info.height as usize);
    if (width as u64) * (height as u64) > settings.limits.max_image_pixels {
        return Err(Problem::unsupported(format!(
            "image of {width}x{height} pixels exceeds the size limit"
        )));
    }
    let input = headers
        .input_colorspace()
        .ok_or_else(|| Problem::invalid("JPEG without color space"))?;
    // YCCK (Adobe APP14 transform 2) is left unconverted on purpose.
    // zune-jpeg's YCCK to RGB path returns inverted colors (checked with 0.5.15 and
    // 0.5.16-rc2): a near-white poster decodes to a gray level of 28 instead of 169,
    // so the page comes out almost black. Decoding to CMYK is not implemented either
    // ("Unimplemented colorspace mapping from YCCK to CMYK"), and inverting the RGB
    // the decoder returns does not recover the right tone. Leaving the image in color
    // with a warning is better than silently turning the page black.
    if input == Z::YCCK {
        return Err(Problem::unsupported("YCCK (Adobe CMYK) JPEG image"));
    }
    let (output, components) = match input {
        Z::Luma => (Z::Luma, 1),
        Z::CMYK => (Z::CMYK, 4),
        _ => (Z::RGB, 3),
    };

    let mut decoder =
        JpegDecoder::new_with_options(Cursor::new(data), options.jpeg_set_out_colorspace(output));
    let pixels = decoder.decode().map_err(error)?;
    if pixels.len() < width * height * components {
        return Err(Problem::invalid("truncated JPEG"));
    }
    Ok(Jpeg {
        width,
        height,
        components,
        converted: input == Z::YCCK,
        pixels,
    })
}

fn gray_image_dict(width: usize, height: usize, bits: u32) -> Dictionary {
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Image".to_vec()));
    dict.set("Width", width as i64);
    dict.set("Height", height as i64);
    dict.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
    dict.set("BitsPerComponent", bits as i64);
    dict
}

/// Converts an image XObject in place.
pub(crate) fn convert_xobject(
    doc: &mut Document,
    id: ObjectId,
    settings: &Settings,
    report: &mut Report,
) -> Result<(), Problem> {
    let (converted, matte) = {
        let Ok(Object::Stream(stream)) = doc.get_object(id) else {
            return Ok(());
        };
        let Some(converted) = convert(doc, &stream.dict, stream, None, settings)? else {
            return Ok(());
        };
        // /Matte is expressed in the parent image's color space.
        let matte = match stream.dict.get(b"SMask") {
            Ok(Object::Reference(smask_id)) => doc
                .get_object(*smask_id)
                .ok()
                .and_then(|o| o.as_stream().ok())
                .and_then(|s| get_numbers(doc, &s.dict, b"Matte"))
                .and_then(|m| {
                    let space = ColorSpace::resolve(
                        doc,
                        stream.dict.get(b"ColorSpace").ok()?,
                        None,
                        settings.limits.max_stream_bytes,
                    );
                    space.to_gray(&m)
                })
                .map(|g| (*smask_id, settings.tone.apply(g))),
            _ => None,
        };
        (converted, matte)
    };

    match converted {
        Converted::Palette { hival, lookup } => {
            let space = Object::Array(vec![
                Object::Name(b"Indexed".to_vec()),
                Object::Name(b"DeviceGray".to_vec()),
                Object::Integer(hival as i64),
                Object::String(lookup, StringFormat::Hexadecimal),
            ]);
            doc.get_object_mut(id)?
                .as_stream_mut()?
                .dict
                .set("ColorSpace", space);
        }
        Converted::Pixels {
            width,
            height,
            bits,
            data,
            soft_mask,
        } => {
            let mask_id = soft_mask
                .map(|mask| doc.add_object(flate_stream(gray_image_dict(width, height, 8), &mask)));
            let stream = doc.get_object_mut(id)?.as_stream_mut()?;
            stream.dict.remove(b"Decode");
            // Color key ranges refer to the original components.
            if matches!(stream.dict.get(b"Mask"), Ok(Object::Array(_))) {
                stream.dict.remove(b"Mask");
            }
            for (key, value) in gray_image_dict(width, height, bits).iter() {
                stream.dict.set(key.clone(), value.clone());
            }
            if let Some(mask_id) = mask_id {
                stream.dict.set("SMask", Object::Reference(mask_id));
            }
            set_flate_content(stream, &data);
        }
    }
    if let Some((smask_id, gray)) = matte
        && let Ok(Object::Stream(smask)) = doc.get_object_mut(smask_id)
    {
        smask
            .dict
            .set("Matte", Object::Array(vec![Object::Real(gray as f32)]));
    }
    report.images += 1;
    Ok(())
}

/// An inline image (`BI … ID … EI`) found in a content stream.
pub(crate) struct InlineImage {
    /// The dictionary as written, with abbreviated keys.
    raw: Dictionary,
    /// The dictionary with full key and filter names.
    normalized: Dictionary,
    unfiltered_len: Option<usize>,
}

fn full_key(key: &[u8]) -> &[u8] {
    match key {
        b"BPC" => b"BitsPerComponent",
        b"CS" => b"ColorSpace",
        b"D" => b"Decode",
        b"DP" => b"DecodeParms",
        b"F" => b"Filter",
        b"H" => b"Height",
        b"IM" => b"ImageMask",
        b"I" => b"Interpolate",
        b"W" => b"Width",
        other => other,
    }
}

fn full_filter(name: &[u8]) -> Vec<u8> {
    match name {
        b"AHx" => b"ASCIIHexDecode".to_vec(),
        b"A85" => b"ASCII85Decode".to_vec(),
        b"LZW" => b"LZWDecode".to_vec(),
        b"Fl" => b"FlateDecode".to_vec(),
        b"RL" => b"RunLengthDecode".to_vec(),
        b"CCF" => b"CCITTFaxDecode".to_vec(),
        b"DCT" => b"DCTDecode".to_vec(),
        other => other.to_vec(),
    }
}

impl InlineImage {
    pub(crate) fn new(env: &Env, raw: Dictionary) -> Self {
        let mut normalized = Dictionary::new();
        for (key, value) in raw.iter() {
            let value = match (full_key(key), value) {
                (b"Filter", Object::Name(n)) => Object::Name(full_filter(n)),
                (b"Filter", Object::Array(items)) => Object::Array(
                    items
                        .iter()
                        .map(|o| match o {
                            Object::Name(n) => Object::Name(full_filter(n)),
                            other => other.clone(),
                        })
                        .collect(),
                ),
                (_, value) => value.clone(),
            };
            normalized.set(full_key(key), value);
        }

        let doc = env.doc;
        let unfiltered_len = (|| {
            if !filters(doc, &normalized).is_empty() {
                return None;
            }
            let width = get_int(doc, &normalized, b"Width")? as usize;
            let height = get_int(doc, &normalized, b"Height")? as usize;
            let (components, bits) = if get_bool(doc, &normalized, b"ImageMask") == Some(true) {
                (1, 1)
            } else {
                let space = ColorSpace::resolve(
                    doc,
                    normalized.get(b"ColorSpace").ok()?,
                    env.resources,
                    env.settings.limits.max_stream_bytes,
                );
                (
                    space.components(),
                    get_int(doc, &normalized, b"BitsPerComponent")? as usize,
                )
            };
            width
                .checked_mul(components)?
                .checked_mul(bits)?
                .div_ceil(8)
                .checked_mul(height)
        })();

        InlineImage {
            raw,
            normalized,
            unfiltered_len,
        }
    }

    pub(crate) fn unfiltered_len(&self) -> Option<usize> {
        self.unfiltered_len.filter(|&len| len > 0)
    }

    /// Returns the replacement `BI … EI` bytes, or `None` if the image needs no change.
    pub(crate) fn convert(&self, env: &Env, data: &[u8]) -> Result<Option<Vec<u8>>, Problem> {
        let stream = Stream::new(self.normalized.clone(), data.to_vec());
        let Some(converted) = convert(
            env.doc,
            &self.normalized,
            &stream,
            env.resources,
            env.settings,
        )?
        else {
            return Ok(None);
        };

        let mut out = b"BI".to_vec();
        let entry = |out: &mut Vec<u8>, key: &[u8], value: &Object| {
            out.push(b' ');
            crate::lexer::write_object(out, &Object::Name(key.to_vec()));
            out.push(b' ');
            crate::lexer::write_object(out, value);
        };
        match converted {
            Converted::Palette { hival, lookup } => {
                let space = Object::Array(vec![
                    Object::Name(b"I".to_vec()),
                    Object::Name(b"G".to_vec()),
                    Object::Integer(hival as i64),
                    Object::String(lookup, StringFormat::Hexadecimal),
                ]);
                for (key, value) in self.raw.iter() {
                    let value = if full_key(key) == b"ColorSpace" {
                        &space
                    } else {
                        value
                    };
                    entry(&mut out, key, value);
                }
                out.extend_from_slice(b" ID ");
                out.extend_from_slice(data);
                out.extend_from_slice(b"\nEI");
            }
            Converted::Pixels {
                width,
                height,
                bits,
                data,
                ..
            } => {
                entry(&mut out, b"W", &Object::Integer(width as i64));
                entry(&mut out, b"H", &Object::Integer(height as i64));
                entry(&mut out, b"CS", &Object::Name(b"G".to_vec()));
                entry(&mut out, b"BPC", &Object::Integer(bits as i64));
                entry(
                    &mut out,
                    b"F",
                    &Object::Array(vec![
                        Object::Name(b"AHx".to_vec()),
                        Object::Name(b"Fl".to_vec()),
                    ]),
                );
                if let Ok(interpolate) = self.normalized.get(b"Interpolate") {
                    entry(&mut out, b"I", interpolate);
                }
                out.extend_from_slice(b" ID ");
                // ASCIIHex keeps the data free of bytes that could be mistaken for `EI`.
                for (i, byte) in flate(&data).iter().enumerate() {
                    if i > 0 && i % 40 == 0 {
                        out.push(b'\n');
                    }
                    out.extend_from_slice(format!("{byte:02X}").as_bytes());
                }
                out.extend_from_slice(b">\nEI");
            }
        }
        Ok(Some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mode;
    use crate::color::Tone;

    fn settings(mode: Mode, dither: bool) -> Settings {
        Settings {
            tone: Tone {
                mode,
                threshold: 0.5,
            },
            dither,
            limits: Default::default(),
        }
    }

    #[test]
    fn rgb_and_cmyk_samples() {
        let rgb = to_gray8(
            &ColorSpace::Rgb,
            8,
            &[[0.0, 1.0]; 3],
            2,
            1,
            &[255, 0, 0, 255, 255, 255],
        );
        assert_eq!(rgb, vec![77, 255]);
        let cmyk = to_gray8(
            &ColorSpace::Cmyk,
            8,
            &[[0.0, 1.0]; 4],
            1,
            1,
            &[0, 0, 0, 255],
        );
        assert_eq!(cmyk, vec![0]);
        // Inverted CMYK as written by Adobe applications.
        let inverted = to_gray8(
            &ColorSpace::Cmyk,
            8,
            &[[1.0, 0.0]; 4],
            1,
            1,
            &[255, 255, 255, 255],
        );
        assert_eq!(inverted, vec![255]);
    }

    #[test]
    fn low_bit_depth_samples() {
        // 4-bit gray: 0xF0 -> 15, 0
        let gray = to_gray8(&ColorSpace::Gray, 4, &[[0.0, 1.0]], 2, 1, &[0xF0]);
        assert_eq!(gray, vec![255, 0]);
        // 1-bit RGB: 0b111_000_00 -> white, black
        let rgb = to_gray8(&ColorSpace::Rgb, 1, &[[0.0, 1.0]; 3], 2, 1, &[0b1110_0000]);
        assert_eq!(rgb, vec![255, 0]);
    }

    #[test]
    fn bilevel_packing() {
        let packed = to_bilevel(
            &[0, 255, 100, 200, 0, 0, 0, 0, 255],
            9,
            1,
            &settings(Mode::Monochrome, false),
        );
        assert_eq!(packed, vec![0b0101_0000, 0b1000_0000]);
        let dithered = to_bilevel(&[128; 64], 8, 8, &settings(Mode::Monochrome, true));
        let white: u32 = dithered.iter().map(|b| b.count_ones()).sum();
        assert!((28..=36).contains(&white), "{white}");
    }
}
