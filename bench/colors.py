#!/usr/bin/env python3
"""Compare the gray that sumi and Ghostscript give representative colors.

Writes a PDF with one filled square per color, converts it with sumi (luma and colorimetric)
and Ghostscript, renders every PDF with poppler (pdftoppm) and reads the gray in the middle
of each square. The values go to bench/colors.json and a side by side picture to
docs/images/colors.svg (Japanese labels) and docs/images/colors-en.svg (English labels).
Standard library only.

Usage: colors.py [--sumi PATH] [--gs PATH]
"""
import argparse
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
RESULTS = HERE / "colors.json"
IMAGES = ROOT / "docs" / "images"

# (Japanese name, English name, RGB in 0-1)
COLORS = [
    ("赤", "Red", (1, 0, 0)),
    ("橙", "Orange", (1, 0.5, 0)),
    ("黄", "Yellow", (1, 1, 0)),
    ("緑", "Green", (0, 1, 0)),
    ("シアン", "Cyan", (0, 1, 1)),
    ("青", "Blue", (0, 0, 1)),
    ("紫", "Purple", (0.5, 0, 0.5)),
    ("マゼンタ", "Magenta", (1, 0, 1)),
    ("茶", "Brown", (0.6, 0.3, 0.1)),
    ("灰", "Gray", (0.5, 0.5, 0.5)),
]

PATCH = 72  # points, so one patch is 72 pixels at 72 dpi


def make_pdf(path):
    ops = []
    for index, (_, _, (r, g, b)) in enumerate(COLORS):
        ops.append(f"{r:g} {g:g} {b:g} rg {index * PATCH} 0 {PATCH} {PATCH} re f")
    content = "\n".join(ops).encode()
    width = PATCH * len(COLORS)
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {PATCH}] /Contents 4 0 R >>".encode(),
        b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream",
    ]
    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    for offset in offsets:
        out += b"%010d 00000 n \n" % offset
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref)
    path.write_bytes(bytes(out))


def sample(pdf, tmp):
    """Gray (0-255) in the middle of every patch, read from a 72 dpi PGM rendering."""
    prefix = tmp / pdf.stem
    subprocess.run(["pdftoppm", "-gray", "-r", "72", "-singlefile", str(pdf), str(prefix)],
                   check=True)
    data = (tmp / f"{pdf.stem}.pgm").read_bytes()
    # P5 header: magic, width, height, maxval, each followed by whitespace
    fields, pos = [], 0
    while len(fields) < 4:
        while data[pos:pos + 1].isspace():
            pos += 1
        start = pos
        while not data[pos:pos + 1].isspace():
            pos += 1
        fields.append(data[start:pos])
    pos += 1
    width = int(fields[1])
    pixels = data[pos:]
    row = PATCH // 2
    return [pixels[row * width + index * PATCH + PATCH // 2] for index in range(len(COLORS))]


def svg(results, lang):
    rows = [
        ("元の色" if lang == "ja" else "Original", None),
        ("sumi（luma、既定）" if lang == "ja" else "sumi (luma, default)", "sumi_luma"),
        ("sumi（colorimetric）" if lang == "ja" else "sumi (colorimetric)", "sumi_colorimetric"),
        (f"Ghostscript {results['ghostscript_version']}", "ghostscript"),
    ]
    label_w, cell, gap, head = 200, 76, 4, 28
    width = label_w + len(COLORS) * (cell + gap) + 8
    height = head + len(rows) * (cell + gap)
    font = "font-family=\"-apple-system, 'Hiragino Sans', 'Noto Sans CJK JP', sans-serif\""
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}" {font} font-size="14">',
        f'<rect width="{width}" height="{height}" fill="#fff"/>',
    ]
    for index, color in enumerate(results["colors"]):
        x = label_w + index * (cell + gap) + cell / 2
        name = color["name"] if lang == "ja" else color["name_en"]
        parts.append(f'<text x="{x}" y="18" text-anchor="middle" fill="#333">{name}</text>')
    for row_index, (label, key) in enumerate(rows):
        y = head + row_index * (cell + gap)
        parts.append(f'<text x="12" y="{y + cell / 2 + 5}" fill="#333">{label}</text>')
        for index, color in enumerate(results["colors"]):
            x = label_w + index * (cell + gap)
            if key is None:
                r, g, b = (round(c * 255) for c in color["rgb"])
                parts.append(f'<rect x="{x}" y="{y}" width="{cell}" height="{cell}" '
                             f'fill="rgb({r},{g},{b})" stroke="#ccc"/>')
                continue
            gray = color[key]
            parts.append(f'<rect x="{x}" y="{y}" width="{cell}" height="{cell}" '
                         f'fill="rgb({gray},{gray},{gray})" stroke="#ccc"/>')
            ink = "#000" if gray >= 128 else "#fff"
            parts.append(f'<text x="{x + cell / 2}" y="{y + cell / 2 + 5}" text-anchor="middle" '
                         f'fill="{ink}">{gray / 255:.2f}</text>')
    parts.append("</svg>")
    return "\n".join(parts) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--sumi", default=str(ROOT / "target" / "release" / "sumi"))
    parser.add_argument("--gs", default="gs")
    args = parser.parse_args()
    for tool in (args.sumi, args.gs, "pdftoppm"):
        if not shutil.which(tool):
            sys.exit(f"{tool} not found")

    with tempfile.TemporaryDirectory() as name:
        tmp = pathlib.Path(name)
        source = tmp / "colors.pdf"
        make_pdf(source)
        outputs = {
            "sumi_luma": [args.sumi, str(source), "-o", str(tmp / "luma.pdf")],
            "sumi_colorimetric": [args.sumi, str(source), "-o", str(tmp / "colorimetric.pdf"),
                                  "--gray-model", "colorimetric"],
            "ghostscript": [args.gs, "-q", "-dNOPAUSE", "-dBATCH", "-dSAFER",
                            "-sDEVICE=pdfwrite", "-sColorConversionStrategy=Gray",
                            "-dProcessColorModel=/DeviceGray", "-o", str(tmp / "gs.pdf"),
                            str(source)],
        }
        files = {"sumi_luma": "luma.pdf", "sumi_colorimetric": "colorimetric.pdf",
                 "ghostscript": "gs.pdf"}
        values = {}
        for key, command in outputs.items():
            subprocess.run(command, check=True)
            values[key] = sample(tmp / files[key], tmp)

    gs_version = subprocess.run([args.gs, "--version"], capture_output=True, text=True,
                                check=True).stdout.strip()
    sumi_version = subprocess.run([args.sumi, "--version"], capture_output=True, text=True,
                                  check=True).stdout.split()[-1]
    results = {
        "sumi_version": sumi_version,
        "ghostscript_version": gs_version,
        "colors": [
            {"name": name, "name_en": name_en, "rgb": list(rgb),
             **{key: values[key][index] for key in values}}
            for index, (name, name_en, rgb) in enumerate(COLORS)
        ],
    }
    RESULTS.write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n")
    (IMAGES / "colors.svg").write_text(svg(results, "ja"))
    (IMAGES / "colors-en.svg").write_text(svg(results, "en"))

    print(f"{'':10} {'luma':>6} {'colorim':>8} {'gs':>6}")
    for color in results["colors"]:
        print(f"{color['name_en']:10} {color['sumi_luma'] / 255:6.3f} "
              f"{color['sumi_colorimetric'] / 255:8.3f} {color['ghostscript'] / 255:6.3f}")


if __name__ == "__main__":
    main()
