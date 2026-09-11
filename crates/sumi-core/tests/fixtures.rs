//! Regression tests on real-world PDFs in `fixtures/` (see fixtures/README.md).
//!
//! When `pdftoppm` (poppler) is installed, the converted pages are also rendered and checked for
//! colored pixels. Poppler is only used as an external test tool.

use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::content::Content;
use lopdf::{Document, Object};
use sumi_core::{ConvertOptions, convert_bytes};

/// Fixtures that convert completely. `ycck_jpeg.pdf` is excluded on purpose: it is the
/// regression fixture for an image that is deliberately left unconverted, and it has its
/// own test below.
fn fixtures() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = all_fixtures()
        .into_iter()
        .filter(|p| p.file_name().is_none_or(|n| n != "ycck_jpeg.pdf"))
        .collect();
    assert!(!paths.is_empty());
    paths.sort();
    paths
}

fn all_fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "pdf"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty());
    paths
}

/// A YCCK (Adobe APP14 transform 2) JPEG is left in color with a warning.
///
/// zune-jpeg decodes YCCK to inverted RGB (checked with 0.5.15 and 0.5.16-rc2), which used
/// to turn a near-white poster almost black. Converting to CMYK is not implemented in the
/// decoder either, so the image is reported as unsupported instead of being converted wrong.
#[test]
fn ycck_jpeg_is_left_unconverted_with_a_warning() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ycck_jpeg.pdf");
    let input = std::fs::read(&path).unwrap();
    let converted = convert_bytes(&input, &ConvertOptions::grayscale()).unwrap();
    assert!(
        converted
            .report
            .warnings
            .iter()
            .any(|w| w.message.contains("YCCK")),
        "{:?}",
        converted.report.warnings
    );
    assert_eq!(
        converted.report.images, 0,
        "the image must stay unconverted"
    );
}

/// Text showing operations of every page, in order.
fn text_operations(doc: &Document) -> Vec<(String, Vec<Object>)> {
    doc.get_pages()
        .values()
        .flat_map(|&page| {
            let content = Content::decode(&doc.get_page_content(page)).unwrap();
            content
                .operations
                .into_iter()
                .filter(|op| {
                    matches!(
                        op.operator.as_str(),
                        "Tj" | "TJ" | "'" | "\"" | "Tf" | "Td" | "Tm"
                    )
                })
                .map(|op| (op.operator, op.operands))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn fixtures_keep_structure_and_text() {
    for path in fixtures() {
        let input = std::fs::read(&path).unwrap();
        let converted = convert_bytes(&input, &ConvertOptions::grayscale()).unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(
            converted.report.is_complete(),
            "{name}: {:?}",
            converted.report.warnings
        );

        let before = Document::load_mem(&input).unwrap();
        let after = Document::load_mem(&converted.pdf).unwrap();
        assert_eq!(
            before.get_pages().len(),
            after.get_pages().len(),
            "{name}: page count"
        );
        for (&a, &b) in before.get_pages().values().zip(after.get_pages().values()) {
            let media_box = |doc: &Document, page| {
                doc.get_dictionary(page)
                    .unwrap()
                    .get(b"MediaBox")
                    .map(|m| format!("{m:?}"))
                    .ok()
            };
            assert_eq!(
                media_box(&before, a),
                media_box(&after, b),
                "{name}: MediaBox"
            );
        }
        assert_eq!(
            text_operations(&before),
            text_operations(&after),
            "{name}: text operations"
        );

        for &page in after.get_pages().values() {
            let content = Content::decode(&after.get_page_content(page)).unwrap();
            let colored: Vec<_> = content
                .operations
                .iter()
                .filter(|op| matches!(op.operator.as_str(), "rg" | "RG" | "k" | "K"))
                .collect();
            assert!(
                colored.is_empty(),
                "{name}: color operators remain: {colored:?}"
            );
        }
    }
}

fn pdftoppm_available() -> bool {
    Command::new("pdftoppm").arg("-v").output().is_ok()
}

fn read_ppm(path: &Path) -> (usize, usize, Vec<u8>) {
    let data = std::fs::read(path).unwrap();
    let mut fields = Vec::new();
    let mut pos = 0;
    while fields.len() < 4 {
        while data[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let start = pos;
        while !data[pos].is_ascii_whitespace() {
            pos += 1;
        }
        fields.push(String::from_utf8_lossy(&data[start..pos]).into_owned());
    }
    let (width, height) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    (width, height, data[pos + 1..].to_vec())
}

#[test]
fn fixtures_render_without_color() {
    if !pdftoppm_available() {
        eprintln!("pdftoppm not found; skipping render test");
        return;
    }
    let out_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fixtures_render");
    std::fs::create_dir_all(&out_dir).unwrap();
    for path in fixtures() {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        for options in [ConvertOptions::grayscale(), ConvertOptions::monochrome(0.5)] {
            let converted = convert_bytes(&std::fs::read(&path).unwrap(), &options).unwrap();
            let pdf_path = out_dir.join(format!("{name}-{}.pdf", options.mode));
            std::fs::write(&pdf_path, &converted.pdf).unwrap();
            let prefix = out_dir.join(format!("{name}-{}", options.mode));
            let status = Command::new("pdftoppm")
                .args(["-r", "30"])
                .arg(&pdf_path)
                .arg(&prefix)
                .status()
                .unwrap();
            assert!(status.success(), "{name}: pdftoppm failed");

            let pages: Vec<PathBuf> = std::fs::read_dir(&out_dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| {
                    let file = p.file_name().unwrap().to_string_lossy().into_owned();
                    file.starts_with(&format!("{name}-{}-", options.mode)) && file.ends_with(".ppm")
                })
                .collect();
            assert!(!pages.is_empty(), "{name}: nothing rendered");
            for page in pages {
                let (width, height, pixels) = read_ppm(&page);
                let colored = pixels
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .filter(|p| p.iter().max().unwrap() - p.iter().min().unwrap() > 3)
                    .count();
                assert_eq!(
                    colored,
                    0,
                    "{}: {colored} of {} pixels are colored",
                    page.display(),
                    width * height
                );
                std::fs::remove_file(page).unwrap();
            }
        }
    }
}
