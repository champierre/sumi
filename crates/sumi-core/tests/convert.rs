//! Conversion tests on small PDFs built in memory.

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat, dictionary};
use sumi_core::{ConvertOptions, Mode, SumiError, convert_bytes};

/// Builds a one-page PDF. `setup` can add objects and page resources.
fn pdf(content: &[u8], setup: impl FnOnce(&mut Document, &mut Dictionary)) -> Vec<u8> {
    pdf_with_page(content, |doc, resources, _| setup(doc, resources))
}

fn pdf_with_page(
    content: &[u8],
    setup: impl FnOnce(&mut Document, &mut Dictionary, &mut Dictionary),
) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(
        dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"},
    );
    let mut resources = dictionary! {"Font" => dictionary! {"F1" => font_id}};
    let mut page = dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
    };
    setup(&mut doc, &mut resources, &mut page);
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content.to_vec()));
    page.set("Contents", content_id);
    page.set("Resources", resources);
    let page_id = doc.add_object(page);
    doc.objects.insert(
        pages_id,
        Object::Dictionary(
            dictionary! {"Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1},
        ),
    );
    let catalog_id = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
    doc.trailer.set("Root", catalog_id);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

fn flate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn gray() -> ConvertOptions {
    ConvertOptions::grayscale()
}

fn page_id(doc: &Document) -> ObjectId {
    doc.get_pages()[&1]
}

fn page_content(pdf: &[u8]) -> String {
    let doc = Document::load_mem(pdf).unwrap();
    // lopdf appends a newline after every content stream.
    String::from_utf8_lossy(&doc.get_page_content(page_id(&doc)))
        .trim_end()
        .to_string()
}

fn stream_text(doc: &Document, id: ObjectId) -> String {
    let stream = doc.get_object(id).unwrap().as_stream().unwrap();
    String::from_utf8_lossy(
        &stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone()),
    )
    .into_owned()
}

fn resources(doc: &Document) -> &Dictionary {
    let page = doc.get_dictionary(page_id(doc)).unwrap();
    page.get(b"Resources").unwrap().as_dict().unwrap()
}

fn resource_id(doc: &Document, category: &[u8], name: &[u8]) -> ObjectId {
    resources(doc)
        .get(category)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(name)
        .unwrap()
        .as_reference()
        .unwrap()
}

fn tokens(content: &str) -> Vec<&str> {
    content.split_whitespace().collect()
}

#[test]
fn device_colors_become_gray_and_text_is_kept() {
    let input = pdf(b"1 0 0 rg 0 0 1 RG 0 0 0 1 k 0.2 g BT /F1 12 Tf 10 10 Td (Hello \\(world\\)) Tj ET 10 10 50 50 re B", |_, _| {});
    let converted = convert_bytes(&input, &gray()).unwrap();
    let content = page_content(&converted.pdf);
    assert_eq!(
        content,
        "0.3 g 0.11 G 0 g 0.2 g BT /F1 12 Tf 10 10 Td (Hello \\(world\\)) Tj ET 10 10 50 50 re B"
    );
    assert_eq!(converted.report.color_operators, 3);
    assert!(converted.report.is_complete());
    for op in ["rg", "RG", "k", "K"] {
        assert!(!tokens(&content).contains(&op));
    }
}

#[test]
fn unchanged_content_is_not_rewritten() {
    let input = pdf(b"0.5 g 0 0 10 10 re f", |_, _| {});
    let converted = convert_bytes(&input, &gray()).unwrap();
    assert_eq!(converted.report.content_streams, 0);
    assert_eq!(page_content(&converted.pdf), "0.5 g 0 0 10 10 re f");
}

#[test]
fn icc_based_color_spaces_are_rewritten() {
    let input = pdf(
        b"/CS0 cs 0 0 1 sc 0 0 100 100 re f /CS0 CS 1 1 0 SCN S",
        |doc, resources| {
            let icc = doc.add_object(Stream::new(dictionary! {"N" => 3}, vec![0; 16]));
            resources.set(
                "ColorSpace",
                dictionary! {"CS0" => vec![Object::Name(b"ICCBased".to_vec()), icc.into()]},
            );
        },
    );
    let content = page_content(&convert_bytes(&input, &gray()).unwrap().pdf);
    assert_eq!(
        content,
        "/DeviceGray cs 0.11 sc 0 0 100 100 re f /DeviceGray CS 0.89 SCN S"
    );
}

#[test]
fn graphics_state_stack_tracks_color_spaces() {
    let input = pdf(
        b"/CS0 cs q 1 0 0 sc Q 0 1 0 sc q 0.5 g Q 1 1 1 sc",
        |doc, resources| {
            let icc = doc.add_object(Stream::new(dictionary! {"N" => 3}, vec![0; 16]));
            resources.set(
                "ColorSpace",
                dictionary! {"CS0" => vec![Object::Name(b"ICCBased".to_vec()), icc.into()]},
            );
        },
    );
    let content = page_content(&convert_bytes(&input, &gray()).unwrap().pdf);
    assert_eq!(content, "/DeviceGray cs q 0.3 sc Q 0.59 sc q 0.5 g Q 1 sc");
}

#[test]
fn separation_uses_tint_transform_and_initial_color() {
    let input = pdf(b"/Spot cs 0 0 10 10 re f 0.5 sc", |_, resources| {
        let tint = dictionary! {
            "FunctionType" => 2,
            "Domain" => vec![0.into(), 1.into()],
            "C0" => vec![0.into(), 0.into(), 0.into(), 0.into()],
            "C1" => vec![0.into(), 1.into(), 1.into(), 0.into()],
            "N" => 1,
        };
        let spot = vec![
            Object::Name(b"Separation".to_vec()),
            Object::Name(b"Red".to_vec()),
            Object::Name(b"DeviceCMYK".to_vec()),
            tint.into(),
        ];
        resources.set("ColorSpace", dictionary! {"Spot" => spot});
    });
    let content = page_content(&convert_bytes(&input, &gray()).unwrap().pdf);
    // Full tint: 1 - (0.59 + 0.11) = 0.3; half tint: 1 - 0.35 = 0.65.
    assert_eq!(content, "/DeviceGray cs 0.3 sc 0 0 10 10 re f 0.65 sc");
}

#[test]
fn form_xobjects_are_converted_recursively() {
    let input = pdf(b"/Fm1 Do", |doc, resources| {
        let inner = doc.add_object(Stream::new(
            dictionary! {"Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()]},
            b"0 0 1 RG 0 0 m 10 10 l S".to_vec(),
        ));
        let outer = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                "Group" => dictionary! {"S" => "Transparency", "CS" => "DeviceRGB"},
                "Resources" => dictionary! {"XObject" => dictionary! {"Fm2" => inner}},
            },
            b"0 1 0 rg /Fm2 Do".to_vec(),
        ));
        resources.set("XObject", dictionary! {"Fm1" => outer});
    });
    let converted = convert_bytes(&input, &gray()).unwrap();
    let doc = Document::load_mem(&converted.pdf).unwrap();
    let outer = resource_id(&doc, b"XObject", b"Fm1");
    assert_eq!(stream_text(&doc, outer), "0.59 g /Fm2 Do");
    let outer_dict = &doc.get_object(outer).unwrap().as_stream().unwrap().dict;
    let group = outer_dict.get(b"Group").unwrap().as_dict().unwrap();
    assert_eq!(group.get(b"CS").unwrap().as_name().unwrap(), b"DeviceGray");
    let inner = outer_dict
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"XObject")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Fm2")
        .unwrap()
        .as_reference()
        .unwrap();
    assert_eq!(stream_text(&doc, inner), "0.11 G 0 0 m 10 10 l S");
    assert_eq!(converted.report.content_streams, 2);
}

#[test]
fn uncolored_patterns_get_a_gray_base_color_space() {
    let input = pdf(b"/P0 cs 1 0 0 /Pat scn 0 0 10 10 re f", |doc, resources| {
        let pattern = doc.add_object(Stream::new(
            dictionary! {"PatternType" => 1, "PaintType" => 2, "TilingType" => 1, "BBox" => vec![0.into(), 0.into(), 4.into(), 4.into()], "XStep" => 4, "YStep" => 4},
            b"0 0 2 2 re f".to_vec(),
        ));
        resources.set("ColorSpace", dictionary! {"P0" => vec![Object::Name(b"Pattern".to_vec()), Object::Name(b"DeviceRGB".to_vec())]});
        resources.set("Pattern", dictionary! {"Pat" => pattern});
    });
    let converted = convert_bytes(&input, &gray()).unwrap();
    assert_eq!(
        page_content(&converted.pdf),
        "/SumiPatternGray cs 0.3 /Pat scn 0 0 10 10 re f"
    );
    let doc = Document::load_mem(&converted.pdf).unwrap();
    let spaces = resources(&doc)
        .get(b"ColorSpace")
        .unwrap()
        .as_dict()
        .unwrap();
    let added = spaces.get(b"SumiPatternGray").unwrap().as_array().unwrap();
    assert_eq!(added[1].as_name().unwrap(), b"DeviceGray");
}

#[test]
fn rgb_image_with_png_predictor_becomes_gray() {
    let input = pdf(b"q 2 0 0 1 0 0 cm /Im0 Do Q", |doc, resources| {
        // One PNG-filtered row (filter type 0): red, blue.
        let data = flate(&[0, 255, 0, 0, 0, 0, 255]);
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1,
                "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "FlateDecode",
                "DecodeParms" => dictionary! {"Predictor" => 15, "Colors" => 3, "Columns" => 2},
            },
            data,
        ));
        resources.set("XObject", dictionary! {"Im0" => image});
    });
    let converted = convert_bytes(&input, &gray()).unwrap();
    let doc = Document::load_mem(&converted.pdf).unwrap();
    let image = doc
        .get_object(resource_id(&doc, b"XObject", b"Im0"))
        .unwrap()
        .as_stream()
        .unwrap();
    assert_eq!(
        image.dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
        b"DeviceGray"
    );
    assert!(image.dict.get(b"DecodeParms").is_err());
    assert_eq!(image.decompressed_content().unwrap(), vec![77, 28]);
    assert_eq!(converted.report.images, 1);
}

#[test]
fn indexed_image_keeps_samples_and_gets_gray_palette() {
    let input = pdf(b"/Im0 Do", |doc, resources| {
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1, "BitsPerComponent" => 8,
                "ColorSpace" => vec![Object::Name(b"Indexed".to_vec()), Object::Name(b"DeviceRGB".to_vec()), 1.into(), Object::String(vec![255, 0, 0, 255, 255, 255], StringFormat::Hexadecimal)],
            },
            vec![0, 1],
        ));
        resources.set("XObject", dictionary! {"Im0" => image});
    });
    let converted = convert_bytes(&input, &gray()).unwrap();
    let doc = Document::load_mem(&converted.pdf).unwrap();
    let image = doc
        .get_object(resource_id(&doc, b"XObject", b"Im0"))
        .unwrap()
        .as_stream()
        .unwrap();
    let space = image.dict.get(b"ColorSpace").unwrap().as_array().unwrap();
    assert_eq!(space[1].as_name().unwrap(), b"DeviceGray");
    assert_eq!(space[3].as_str().unwrap(), &[77, 255]);
    assert_eq!(image.content, vec![0, 1]);
}

#[test]
fn color_key_mask_becomes_soft_mask() {
    let input = pdf(b"/Im0 Do", |doc, resources| {
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1, "BitsPerComponent" => 8,
                "ColorSpace" => "DeviceRGB",
                "Mask" => vec![250.into(), 255.into(), 250.into(), 255.into(), 250.into(), 255.into()],
            },
            vec![255, 255, 255, 255, 0, 0],
        ));
        resources.set("XObject", dictionary! {"Im0" => image});
    });
    let converted = convert_bytes(&input, &gray()).unwrap();
    let doc = Document::load_mem(&converted.pdf).unwrap();
    let image = doc
        .get_object(resource_id(&doc, b"XObject", b"Im0"))
        .unwrap()
        .as_stream()
        .unwrap();
    assert!(image.dict.get(b"Mask").is_err());
    let mask_id = image.dict.get(b"SMask").unwrap().as_reference().unwrap();
    let mask = doc.get_object(mask_id).unwrap().as_stream().unwrap();
    assert_eq!(mask.decompressed_content().unwrap(), vec![0, 255]);
}

#[test]
fn inline_images_are_converted() {
    let mut content = b"q 2 0 0 1 0 0 cm BI /W 2 /H 1 /BPC 8 /CS /RGB ID ".to_vec();
    content.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
    content.extend_from_slice(b" EI Q 1 0 0 rg");
    let input = pdf(&content, |_, _| {});
    let converted = convert_bytes(&input, &gray()).unwrap();
    let out = page_content(&converted.pdf);
    assert!(
        out.starts_with("q 2 0 0 1 0 0 cm BI /W 2 /H 1 /CS /G /BPC 8 /F [/AHx /Fl] ID "),
        "{out}"
    );
    assert!(out.ends_with(">\nEI Q 0.3 g"), "{out}");
    assert_eq!(converted.report.images, 1);
}

#[test]
fn inline_indexed_image_gets_gray_palette() {
    let input = pdf(
        b"BI /W 1 /H 1 /BPC 8 /CS [/I /RGB 0 <FF0000>] ID \x00 EI",
        |_, _| {},
    );
    let out = page_content(&convert_bytes(&input, &gray()).unwrap().pdf);
    assert_eq!(out, "BI /W 1 /H 1 /BPC 8 /CS [/I /G 0 <4D>] ID \x00\nEI");
}

#[test]
fn shadings_are_resampled_in_gray() {
    let input = pdf(b"/Sh0 sh", |_, resources| {
        let function = dictionary! {
            "FunctionType" => 2, "Domain" => vec![0.into(), 1.into()],
            "C0" => vec![1.into(), 0.into(), 0.into()], "C1" => vec![0.into(), 0.into(), 1.into()], "N" => 1,
        };
        let shading = dictionary! {
            "ShadingType" => 2, "ColorSpace" => "DeviceRGB",
            "Coords" => vec![0.into(), 0.into(), 200.into(), 0.into()], "Function" => function,
            "Background" => vec![0.into(), 1.into(), 0.into()],
        };
        resources.set("Shading", dictionary! {"Sh0" => shading});
    });
    let converted = convert_bytes(&input, &gray()).unwrap();
    let doc = Document::load_mem(&converted.pdf).unwrap();
    let shading = resources(&doc)
        .get(b"Shading")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Sh0")
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(
        shading.get(b"ColorSpace").unwrap().as_name().unwrap(),
        b"DeviceGray"
    );
    let background = shading.get(b"Background").unwrap().as_array().unwrap();
    assert!((background[0].as_float().unwrap() - 0.59).abs() < 1e-6);
    let function = doc
        .get_object(shading.get(b"Function").unwrap().as_reference().unwrap())
        .unwrap()
        .as_stream()
        .unwrap();
    let samples = function.decompressed_content().unwrap();
    assert_eq!((samples[0], samples[255]), (77, 28));
    assert_eq!(converted.report.shadings, 1);
}

#[test]
fn annotations_and_appearance_strings_are_converted() {
    let input = pdf_with_page(b"", |doc, _, page| {
        let appearance = doc.add_object(Stream::new(
            dictionary! {"Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()]},
            b"1 0 0 RG 0 0 10 10 re S".to_vec(),
        ));
        let annot = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Square",
            "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "C" => vec![1.into(), 0.into(), 0.into()],
            "DA" => Object::string_literal("/Helv 0 Tf 0 0 1 rg"),
            "AP" => dictionary! {"N" => appearance},
        });
        page.set("Annots", vec![annot.into()]);
    });
    let converted = convert_bytes(&input, &gray()).unwrap();
    let doc = Document::load_mem(&converted.pdf).unwrap();
    let page = doc.get_dictionary(page_id(&doc)).unwrap();
    let annot_id = page.get(b"Annots").unwrap().as_array().unwrap()[0]
        .as_reference()
        .unwrap();
    let annot = doc.get_dictionary(annot_id).unwrap();
    let c = annot.get(b"C").unwrap().as_array().unwrap();
    assert_eq!(c.len(), 1);
    assert!((c[0].as_float().unwrap() - 0.3).abs() < 1e-6);
    assert_eq!(
        annot.get(b"DA").unwrap().as_str().unwrap(),
        b"/Helv 0 Tf 0.11 g"
    );
    let ap = annot
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_reference()
        .unwrap();
    assert_eq!(stream_text(&doc, ap), "0.3 G 0 0 10 10 re S");
}

#[test]
fn monochrome_thresholds_colors() {
    let input = pdf(b"1 1 0 rg 0 0 1 RG 0.5 g 0.4 G", |_, _| {});
    let content = page_content(
        &convert_bytes(&input, &ConvertOptions::monochrome(0.5))
            .unwrap()
            .pdf,
    );
    assert_eq!(content, "1 g 0 G 1 g 0 G");
    let content = page_content(
        &convert_bytes(&input, &ConvertOptions::monochrome(0.95))
            .unwrap()
            .pdf,
    );
    assert_eq!(content, "0 g 0 G 0 g 0 G");
}

#[test]
fn monochrome_images_are_bilevel() {
    let input = pdf(b"/Im0 Do", |doc, resources| {
        let image = doc.add_object(Stream::new(
            dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 3, "Height" => 1, "BitsPerComponent" => 8, "ColorSpace" => "DeviceGray"},
            vec![0, 200, 90],
        ));
        resources.set("XObject", dictionary! {"Im0" => image});
    });
    let mut options = ConvertOptions::default();
    options.mode = Mode::Monochrome;
    let converted = convert_bytes(&input, &options).unwrap();
    let doc = Document::load_mem(&converted.pdf).unwrap();
    let image = doc
        .get_object(resource_id(&doc, b"XObject", b"Im0"))
        .unwrap()
        .as_stream()
        .unwrap();
    assert_eq!(
        image
            .dict
            .get(b"BitsPerComponent")
            .unwrap()
            .as_i64()
            .unwrap(),
        1
    );
    assert_eq!(image.decompressed_content().unwrap(), vec![0b0100_0000]);
}

#[test]
fn strict_mode_rejects_unconvertible_parts() {
    let input = pdf(b"/Im0 Do", |doc, resources| {
        let image = doc.add_object(Stream::new(
            dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB", "Filter" => "JPXDecode"},
            vec![0; 8],
        ));
        resources.set("XObject", dictionary! {"Im0" => image});
    });
    let lenient = convert_bytes(&input, &gray()).unwrap();
    assert_eq!(lenient.report.warnings.len(), 1);
    assert!(
        lenient.report.warnings[0].message.contains("JPXDecode"),
        "{:?}",
        lenient.report.warnings
    );

    let mut strict = gray();
    strict.strict = true;
    assert!(matches!(
        convert_bytes(&input, &strict),
        Err(SumiError::Unsupported(_))
    ));
}

#[test]
fn shared_content_streams_are_converted_once() {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let content = doc.add_object(Stream::new(
        Dictionary::new(),
        b"1 0 0 rg 0 0 10 10 re f".to_vec(),
    ));
    let kids: Vec<Object> = (0..3)
        .map(|_| {
            doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), 10.into(), 10.into()], "Contents" => content})
                .into()
        })
        .collect();
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => 3, "Resources" => Dictionary::new()}));
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
    doc.trailer.set("Root", catalog);
    let mut input = Vec::new();
    doc.save_to(&mut input).unwrap();

    let converted = convert_bytes(&input, &gray()).unwrap();
    assert_eq!(converted.report.pages, 3);
    assert_eq!(converted.report.content_streams, 1);
    let out = Document::load_mem(&converted.pdf).unwrap();
    let contents: Vec<Vec<ObjectId>> = out
        .get_pages()
        .values()
        .map(|&p| out.get_page_contents(p))
        .collect();
    assert!(contents.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn invalid_input_and_options_are_rejected() {
    assert!(matches!(
        convert_bytes(b"hello", &gray()),
        Err(SumiError::InvalidPdf(_))
    ));
    assert!(matches!(
        convert_bytes(b"%PDF-1.7\ngarbage", &gray()),
        Err(SumiError::InvalidPdf(_))
    ));
    let input = pdf(b"", |_, _| {});
    let mut options = gray();
    options.threshold = 1.5;
    assert!(matches!(
        convert_bytes(&input, &options),
        Err(SumiError::InvalidOptions(_))
    ));
}

#[test]
fn encrypted_documents_are_rejected() {
    let input = pdf_with_page(b"0 g", |doc, _, _| {
        let encrypt = doc.add_object(dictionary! {
            "Filter" => "Standard", "V" => 1, "R" => 2, "P" => -4,
            "O" => Object::String(vec![0; 32], StringFormat::Hexadecimal),
            "U" => Object::String(vec![0; 32], StringFormat::Hexadecimal),
        });
        doc.trailer.set("Encrypt", encrypt);
        doc.trailer.set(
            "ID",
            vec![
                Object::String(vec![0], StringFormat::Hexadecimal),
                Object::String(vec![0], StringFormat::Hexadecimal),
            ],
        );
    });
    let result = convert_bytes(&input, &gray());
    assert!(matches!(result, Err(SumiError::EncryptedPdf)), "{result:?}");
}

/// Assembles a PDF from object bodies (object numbers start at 1) with a correct xref table.
fn raw_pdf(objects: &[&str]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[test]
fn unreadable_page_content_is_an_error() {
    let input = raw_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>",
        // An object number that does not fit in 32 bits makes the object unparseable.
        "<< /Length 8 /Broken 18446744073386459286 0 R >>\nstream\n1 0 0 rg\nendstream",
    ]);
    let result = convert_bytes(&input, &gray());
    assert!(
        matches!(&result, Err(SumiError::InvalidPdf(msg)) if msg.contains("object 4")),
        "{result:?}"
    );
}

#[test]
fn unreadable_non_visual_object_is_a_warning() {
    let input = raw_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>",
        "<< /Length 8 >>\nstream\n1 0 0 rg\nendstream",
        "<< /Type /StructTreeRoot /K 6 0 R >>",
        "<< /Type /StructElem /S /Document /Pg 18446744073386459286 0 R >>",
    ]);
    let converted = convert_bytes(&input, &gray()).unwrap();
    assert_eq!(page_content(&converted.pdf), "0.3 g");
    assert!(
        converted
            .report
            .warnings
            .iter()
            .any(|w| w.message.contains("object 6")),
        "{:?}",
        converted.report.warnings
    );
}
