# Sumi test fixtures

Test fixture PDFs for exercising Sumi's color-to-grayscale conversion across a
range of real-world PDF producers (a browser, CoreGraphics/Quartz,
Ghostscript, and LibreOffice), each hitting different color constructs
(DeviceRGB/DeviceCMYK operators, ICCBased/Separation/Pattern/Shading color
spaces, JPEG and raw raster images, transparency groups, object streams and
xref streams, etc).

## Layout

- `src/` — generator sources: HTML, a Swift/CoreGraphics script, a
  PostScript file, a flat-ODF (`.fodt`) document, and the raster images
  referenced by the HTML invoice.
- `generate.sh` — regenerates every fixture PDF below from `src/`. Idempotent;
  safe to re-run.
- `inspect.py` — stdlib-only script that reports which color-related PDF
  features a given PDF contains (content operators, color-space dictionary
  keys, image types, xref/object-stream structure, etc).
- `src/make_ycck_pdf.py` — wraps a JPEG in a minimal one-page PDF; used to
  build `ycck_jpeg.pdf`.
- `src/ycck_adobe.jpg` — a YCCK (Adobe APP14 transform 2) JPEG, extracted from
  [CLIA - Brochure 8 - Proficiency Testing DOs and DON'Ts](https://commons.wikimedia.org/wiki/File:CLIA_-_Brochure_8_-_Proficiency_Testing_DOs_and_DON%27Ts.pdf)
  (a work of the US federal government, public domain). Adobe stores YCCK with
  CMY inverted, which is what makes this image useful: read as plain CMYK it is
  almost black (gray 0.3), and only inverting all four components gives the real
  picture (gray 140.3). A YCCK file written by ImageMagick carries no such
  inversion, so it cannot stand in for this one.
- The fixture PDFs themselves (this directory).

## Regenerating

```sh
./generate.sh
```

This calls, in order:

```sh
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless=new --disable-gpu --no-pdf-header-footer \
  --virtual-time-budget=10000 \
  --print-to-pdf=fixtures/chrome_invoice.pdf \
  file://fixtures/src/invoice.html

swift fixtures/src/quartz_graphics.swift fixtures/quartz_graphics.pdf

gs -sDEVICE=pdfwrite -dCompatibilityLevel=1.7 \
   -dWriteObjStms=true -dWriteXRefStm=true -dNOPAUSE -dBATCH -dQUIET \
   -sOutputFile=fixtures/gs_objstm_cmyk.pdf fixtures/src/gs_objstm_cmyk.ps

/Applications/LibreOffice.app/Contents/MacOS/soffice --headless --norestore \
  --convert-to pdf --outdir <tmp> fixtures/src/lo_table.fodt
# -> moved to fixtures/libreoffice_table.pdf
```

Inspect any fixture with:

```sh
python3 inspect.py fixtures/chrome_invoice.pdf
```

## Fixtures

| File | Generator | Pages | Size | Features detected by `inspect.py` |
|---|---|---|---|---|
| `chrome_invoice.pdf` | Headless Chrome print-to-pdf of `src/invoice.html` (2-page Japanese invoice: blue header bar, red total text, gray/blue table rows, one yellow-text row, CSS `linear-gradient` banner, inline SVG bar chart, a transparent-background PNG seal, and a JPEG gradient logo) | 2 | 329 KB | PDF 1.4, classic `xref`; ops `rg/RG`, `cs/CS`, `sc/scn` (no `k/K`, no `sh`); `/ICCBased`, `/Pattern`, `/Shading`, `/Subtype /Image` (DeviceRGB **and** DeviceGray), `/Subtype /Form`, `/DCTDecode`, `/SMask`, `/Group` (transparency) |
| `quartz_graphics.pdf` | Swift + CoreGraphics/CoreText (`CGContext(consumer:mediaBox:)`): RGB & CMYK fills/strokes, red CoreText title, axial + radial `CGGradient`s, a 50%-alpha transparency layer, a small RGB bitmap `draw(image:in:)` | 2 | 62 KB | PDF 1.3 (header) / poppler reports 1.4 effective; classic `xref`; ops `cs/CS`, `sc/scn`, `sh` (**no** `rg/RG`, **no** `k/K` — see notes); `/ICCBased`, `/Shading`, `/Subtype /Form`, `/Group` (transparency) |
| `gs_objstm_cmyk.pdf` | Ghostscript `pdfwrite` from `src/gs_objstm_cmyk.ps` (`-dCompatibilityLevel=1.7 -dWriteObjStms=true -dWriteXRefStm=true`): `setcmykcolor`/`setrgbcolor`, a `/Separation` spot color, an axial `shfill`, a small RGB image via `colorimage` | 2 | 4.9 KB | PDF 1.7, **xref stream** (`/Type /XRef`), **object streams** (`/ObjStm`); ops `rg/RG`, `k/K`, `cs/CS`, `sc/scn` (no `sh` — see notes); `/Separation`, `/Pattern`, `/Shading`, `/Subtype /Image` (DeviceRGB) |
| `libreoffice_table.pdf` | `soffice --headless --convert-to pdf` from `src/lo_table.fodt` (flat ODF text: colored heading text, a table with a blue header row, light-blue alternating rows, a yellow "warning" row, and a solid-color table-cell "shape") | 1 | 26 KB | PDF 1.6, classic `xref`; ops `rg/RG` only; `/Group` (transparency) |

All four are comfortably under the 500 KB target.

## Notes / surprises

- **Quartz never emits the shorthand `rg`/`RG`/`k`/`K` operators**, even for
  plain RGB or CMYK fills. Every fill/stroke in `quartz_graphics.pdf` goes
  through an explicit color-space resource + `sc`/`SC` (e.g. `/Cs1 cs 0.85
  0.15 0.15 sc`), and those color spaces are `/ICCBased` (generic RGB /
  generic CMYK ICC profiles), never bare `/DeviceRGB` or `/DeviceCMYK` names.
  This is exactly the kind of "everything is ICCBased + cs/scn" case Sumi
  needs to handle correctly.
- **Ghostscript rewrote the clipped `shfill` as a shading *pattern* fill**,
  not a direct `sh` call. Because the PostScript clips to a rectangle before
  calling `shfill`, `pdfwrite` emitted a `/Pattern` color space wrapping the
  `/ShadingType 2` dictionary, painted via `cs`/`scn` + `re f`, rather than a
  bare `sh` operator. `/Shading` and `/Pattern` dictionary keys are present;
  the `sh` operator is not.
- **The `/Separation` spot-color dictionary is only visible inside the
  Ghostscript output's object stream.** `gs_objstm_cmyk.pdf` bundles most
  small objects (including the `/Separation` color-space array) into a
  compressed `/ObjStm`; `inspect.py` has to Flate-decompress every stream
  and rescan the decompressed bytes for dictionary keys as well as content
  operators, or it would silently miss color spaces that only exist "inside"
  compressed object streams.
- **`inspect.py` deliberately avoids scanning binary payload streams
  (embedded font programs, raster image samples) for single-letter content
  operators.** Byte sequences equal to a lone `k`, `K`, or the pair `cs`
  can and do turn up by pure chance inside decompressed TrueType `glyf`/
  `hmtx` tables and ICC profile data; without guarding against that, every
  fixture with an embedded font (i.e. all of them) produced spurious
  `k/K` or `cs/CS` hits. The guard: skip operator-scanning for streams
  whose preceding object dictionary mentions `/FontFile*` or `/Subtype
  /Image`, and as a second safety net, skip any stream whose decompressed
  body is less than 85% printable ASCII.
- `inspect.py` is deliberately named `inspect.py`. Careful: running it as
  `python3 inspect.py` puts its own directory first on `sys.path`, which
  shadows the stdlib `inspect` module — and `dataclasses` imports stdlib
  `inspect` internally, so an earlier version of this script that used
  `@dataclass` crashed on import with a circular-import `ImportError`. The
  script avoids `dataclasses` (and any other stdlib module that pulls in
  `inspect`) for that reason.
- LibreOffice's HTML-import filter (`writer_web_pdf_Export`) did not honor
  the table/cell CSS backgrounds well when the source was a plain HTML file
  (colors were dropped, an extra blank page appeared, columns wrapped one
  digit per line). Switching the LibreOffice source to a hand-written flat
  ODF text document (`src/lo_table.fodt`) and using the native
  `writer_pdf_Export` filter fixed all of that — the table renders exactly
  as styled. A `<draw:frame>`/`<draw:rect>` drawing shape written by hand
  also silently failed to render in LibreOffice's flat-ODF import (no error,
  just absent), so the "colored shape" in that fixture is a single-cell
  table with a solid background instead, which reliably renders as a filled
  rectangle in the page's content stream.
- None of the four fixtures happen to trigger `BI` (inline image), `/Indexed`,
  `/DeviceN`, or `/Annots` — those detectors exist in `inspect.py` for future
  fixtures that need them, but weren't required by any of the four generators
  here.
