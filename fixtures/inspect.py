#!/usr/bin/env python3
"""
inspect.py -- report which color-related PDF features a fixture PDF contains.

Usage:
    python3 inspect.py <file.pdf> [<file2.pdf> ...]

For each PDF, this script:
  - Reads the raw bytes and finds the PDF header version (e.g. "1.7").
  - Detects a classic "xref" table and/or a cross-reference *stream*
    (/Type /XRef), and object streams (/ObjStm).
  - Scans every `stream ... endstream` block, tries to Flate-decompress it
    with zlib (skipping anything that fails -- e.g. it's already
    image-encoded, or isn't actually Flate), and searches the decompressed
    (or, for uncompressed streams, the raw) bytes for content-stream color
    operators: rg/RG, k/K, cs/CS, sc/scn, sh, BI (inline image).
  - Scans the whole raw file (not just decompressed streams, since these
    dictionary keys usually live in plain, uncompressed object dictionaries
    or cross-reference-adjacent structures) for the dictionary-level
    features: /ICCBased, /Indexed, /Separation, /DeviceN, /Pattern,
    /Shading, /Subtype /Image (+ which of DeviceRGB/DeviceCMYK/DeviceGray
    it uses), /Subtype /Form, /DCTDecode, /SMask, /Group (transparency),
    /Annots.

Whitespace in dictionary keys/values is normalized before matching, so
`/Subtype/Image`, `/Subtype  /Image`, and `/Subtype\n/Image` all match.

Only the Python standard library is used.
"""

from __future__ import annotations

# NOTE: this file is deliberately named inspect.py (per the fixtures spec),
# which shadows the stdlib `inspect` module if this script's directory ends
# up first on sys.path. That in turn breaks `dataclasses` (it imports
# stdlib `inspect` internally) and can break other stdlib modules loaded
# later. To stay safe and stdlib-only, this script avoids `dataclasses`
# entirely and uses a plain class instead.

import re
import sys
import zlib


def normalize_ws(data: bytes) -> bytes:
    """Collapse all whitespace runs to a single space, for tolerant regex matching."""
    return re.sub(rb"[\x00\x09\x0a\x0c\x0d\x20]+", b" ", data)


class Features:
    def __init__(self) -> None:
        self.pdf_version: str | None = None
        self.has_classic_xref: bool = False
        self.has_xref_stream: bool = False
        self.has_objstm: bool = False

        self.op_rg_RG: bool = False       # rg / RG (DeviceRGB fill/stroke)
        self.op_k_K: bool = False         # k / K (DeviceCMYK fill/stroke)
        self.op_cs_CS: bool = False       # cs / CS (color space selection)
        self.op_sc_scn: bool = False      # sc / scn (set color w/ current color space)
        self.op_sh: bool = False          # sh (shading paint)
        self.op_BI: bool = False          # BI (inline image)

        self.icc_based: bool = False
        self.indexed: bool = False
        self.separation: bool = False
        self.devicen: bool = False
        self.pattern: bool = False
        self.shading_dict: bool = False
        self.image_devicergb: bool = False
        self.image_devicecmyk: bool = False
        self.image_devicegray: bool = False
        self.subtype_form: bool = False
        self.dct_decode: bool = False
        self.smask: bool = False
        self.group_transparency: bool = False
        self.annots: bool = False

        self.notes: list[str] = []


STREAM_RE = re.compile(rb"stream\r?\n(.*?)endstream", re.DOTALL)

# Content-stream operators. Matched with word-ish boundaries so we don't
# match inside other tokens/names.
OP_PATTERNS = {
    "op_rg_RG": re.compile(rb"(?<![A-Za-z0-9/])(rg|RG)(?![A-Za-z0-9])"),
    "op_k_K": re.compile(rb"(?<![A-Za-z0-9/])(k|K)(?![A-Za-z0-9])"),
    "op_cs_CS": re.compile(rb"(?<![A-Za-z0-9/])(cs|CS)(?![A-Za-z0-9])"),
    "op_sc_scn": re.compile(rb"(?<![A-Za-z0-9/])(sc|SC|scn|SCN)(?![A-Za-z0-9])"),
    "op_sh": re.compile(rb"(?<![A-Za-z0-9/])sh(?![A-Za-z0-9])"),
    "op_BI": re.compile(rb"(?<![A-Za-z0-9/])BI(?![A-Za-z0-9])"),
}

# Dictionary-level features, tolerant of whitespace variations (after
# normalize_ws collapses all whitespace runs to a single space).
DICT_PATTERNS = {
    "icc_based": re.compile(rb"/ICCBased\b"),
    "indexed": re.compile(rb"/Indexed\b"),
    "separation": re.compile(rb"/Separation\b"),
    "devicen": re.compile(rb"/DeviceN\b"),
    "pattern": re.compile(rb"/Pattern\b"),
    "shading_dict": re.compile(rb"/Shading\b"),
    "subtype_form": re.compile(rb"/Subtype */Form\b"),
    "dct_decode": re.compile(rb"/DCTDecode\b"),
    "smask": re.compile(rb"/SMask\b"),
    "group_transparency": re.compile(rb"/Group\b"),
    "annots": re.compile(rb"/Annots\b"),
    "has_objstm": re.compile(rb"/Type */ObjStm\b"),
    "has_xref_stream": re.compile(rb"/Type */XRef\b"),
}

IMAGE_SUBTYPE_RE = re.compile(rb"/Subtype */Image\b")
VERSION_RE = re.compile(rb"%PDF-(\d\.\d)")
CLASSIC_XREF_RE = re.compile(rb"(?:^|[\r\n])xref\s*[\r\n]")


def find_image_colorspace_near(data: bytes, match_start: int, window: int = 400) -> set[str]:
    """Look in a window around a `/Subtype /Image` hit for its /ColorSpace."""
    lo = max(0, match_start - window)
    hi = min(len(data), match_start + window)
    chunk = data[lo:hi]
    found = set()
    if re.search(rb"/ColorSpace */DeviceRGB\b", chunk) or re.search(rb"/DeviceRGB\b", chunk):
        found.add("DeviceRGB")
    if re.search(rb"/ColorSpace */DeviceCMYK\b", chunk) or re.search(rb"/DeviceCMYK\b", chunk):
        found.add("DeviceCMYK")
    if re.search(rb"/ColorSpace */DeviceGray\b", chunk) or re.search(rb"/DeviceGray\b", chunk):
        found.add("DeviceGray")
    return found


def inspect_pdf(path: str) -> Features:
    with open(path, "rb") as f:
        raw = f.read()

    feats = Features()

    m = VERSION_RE.search(raw)
    if m:
        feats.pdf_version = m.group(1).decode("ascii")

    if CLASSIC_XREF_RE.search(raw):
        feats.has_classic_xref = True

    has_image = False

    def scan_buffer(norm: bytes) -> None:
        """Scan one normalized buffer for both dict-level and image features.

        Called once on the raw (compressed) file, and once per decompressed
        stream -- object streams (/ObjStm) bundle whole object dictionaries
        (e.g. a /Separation color space dict) inside a Flate-compressed
        stream, so dictionary-level features can be invisible to a
        raw-bytes-only scan on files that use them. Scanning decompressed
        stream bodies too catches those.
        """
        nonlocal has_image
        for attr, pat in DICT_PATTERNS.items():
            if not getattr(feats, attr) and pat.search(norm):
                setattr(feats, attr, True)
        for m in IMAGE_SUBTYPE_RE.finditer(norm):
            has_image = True
            cs = find_image_colorspace_near(norm, m.start())
            if "DeviceRGB" in cs:
                feats.image_devicergb = True
            if "DeviceCMYK" in cs:
                feats.image_devicecmyk = True
            if "DeviceGray" in cs:
                feats.image_devicegray = True

    scan_buffer(normalize_ws(raw))

    # Walk every stream ... endstream block. Try to Flate-decompress it; if
    # that succeeds, scan *only* the decompressed bytes (scanning the still
    # -compressed bytes too would risk spurious operator-token matches in
    # essentially-random compressed binary noise). If decompression fails
    # (stream isn't Flate -- already JPEG-encoded, raw binary font data,
    # etc.), fall back to scanning the raw bytes as-is, e.g. for
    # uncompressed content streams.
    for sm in STREAM_RE.finditer(raw):
        blob = sm.group(1)
        # Peek at the object dictionary immediately preceding this stream
        # (i.e. "<< ... >> stream ... endstream") to tell content streams
        # apart from binary payload streams (embedded font programs, raster
        # image samples). Single-letter operators like k/K or cs/CS are
        # short enough to turn up by pure chance in decompressed binary
        # data (e.g. TrueType glyf/hmtx tables), so we only run the
        # content-operator scan on streams that don't look like one of
        # those binary payload types.
        preceding = normalize_ws(raw[max(0, sm.start() - 600):sm.start()])
        is_binary_payload = bool(
            re.search(rb"/FontFile\d?\b", preceding)
            or re.search(rb"/Subtype */Image\b", preceding)
        )

        try:
            candidate = zlib.decompress(blob)
        except Exception:
            candidate = blob
        cnorm = normalize_ws(candidate)

        # Belt-and-suspenders: embedded font *programs* (FontFile2/3 binary
        # sfnt data) are often referenced from a FontDescriptor object that
        # is *not* the stream's own immediately-preceding dictionary (the
        # stream's own dict is typically just << /Length .. /Length1 ..
        # /Filter /FlateDecode >>), so the lookback above can miss them.
        # As a second safety net, treat any stream whose decompressed body
        # is mostly non-printable bytes as binary payload too -- real PDF
        # content-stream operators only ever appear in printable-ASCII
        # text, so a low printable-byte ratio is a reliable signal that
        # any single-letter "operator" match found in it is coincidental.
        if not is_binary_payload and cnorm:
            printable = sum(1 for b in cnorm if 0x20 <= b <= 0x7E or b in (9, 10, 13))
            if printable / len(cnorm) < 0.85:
                is_binary_payload = True

        if not is_binary_payload:
            for attr, pat in OP_PATTERNS.items():
                if not getattr(feats, attr) and pat.search(cnorm):
                    setattr(feats, attr, True)

        # Object streams (/ObjStm) hold whole object dictionaries (e.g. a
        # /Separation color space dict, or an image's /Subtype /Image
        # dict) inside a Flate-compressed stream -- rescan every stream
        # body for dictionary-level features too, not just operators.
        # (Dictionary-key patterns are long/specific enough that scanning
        # binary payload streams for them too is not a meaningful risk.)
        scan_buffer(cnorm)

    if has_image and not (feats.image_devicergb or feats.image_devicecmyk or feats.image_devicegray):
        feats.notes.append("image present but colorspace not detected nearby (may be /Indexed or ICC-based)")

    return feats


def format_report(path: str, feats: Features) -> str:
    lines = [f"=== {path} ==="]
    lines.append(f"PDF version: {feats.pdf_version or 'unknown'}")
    lines.append(f"classic xref table: {feats.has_classic_xref}")
    lines.append(f"xref stream (/Type /XRef): {feats.has_xref_stream}")
    lines.append(f"object streams (/ObjStm): {feats.has_objstm}")
    lines.append("")
    lines.append("content operators:")
    lines.append(f"  rg/RG (DeviceRGB fill/stroke):     {feats.op_rg_RG}")
    lines.append(f"  k/K   (DeviceCMYK fill/stroke):    {feats.op_k_K}")
    lines.append(f"  cs/CS (color space select):        {feats.op_cs_CS}")
    lines.append(f"  sc/scn (set color):                {feats.op_sc_scn}")
    lines.append(f"  sh (shading paint):                {feats.op_sh}")
    lines.append(f"  BI (inline image):                 {feats.op_BI}")
    lines.append("")
    lines.append("dictionary features:")
    lines.append(f"  /ICCBased:                         {feats.icc_based}")
    lines.append(f"  /Indexed:                          {feats.indexed}")
    lines.append(f"  /Separation:                       {feats.separation}")
    lines.append(f"  /DeviceN:                          {feats.devicen}")
    lines.append(f"  /Pattern:                          {feats.pattern}")
    lines.append(f"  /Shading:                          {feats.shading_dict}")
    lines.append(f"  /Subtype /Image (DeviceRGB):       {feats.image_devicergb}")
    lines.append(f"  /Subtype /Image (DeviceCMYK):      {feats.image_devicecmyk}")
    lines.append(f"  /Subtype /Image (DeviceGray):      {feats.image_devicegray}")
    lines.append(f"  /Subtype /Form:                    {feats.subtype_form}")
    lines.append(f"  /DCTDecode (JPEG):                 {feats.dct_decode}")
    lines.append(f"  /SMask:                            {feats.smask}")
    lines.append(f"  /Group (transparency):             {feats.group_transparency}")
    lines.append(f"  /Annots:                           {feats.annots}")
    if feats.notes:
        lines.append("")
        lines.append("notes:")
        for n in feats.notes:
            lines.append(f"  - {n}")
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(f"usage: {argv[0]} <file.pdf> [<file2.pdf> ...]", file=sys.stderr)
        return 1
    for path in argv[1:]:
        feats = inspect_pdf(path)
        print(format_report(path, feats))
        print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
