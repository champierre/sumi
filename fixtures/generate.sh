#!/usr/bin/env bash
# Regenerates every fixture PDF under fixtures/ from the sources in fixtures/src/.
# Idempotent: safe to run multiple times: each run simply overwrites the outputs.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$HERE/src"
OUT="$HERE"

CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
SOFFICE="/Applications/LibreOffice.app/Contents/MacOS/soffice"

echo "== fixtures/generate.sh =="
echo "src: $SRC"
echo "out: $OUT"

# ---------------------------------------------------------------------------
# 1. chrome_invoice.pdf -- headless Chrome print-to-pdf of a Japanese invoice
# ---------------------------------------------------------------------------
echo "[1/4] chrome_invoice.pdf (headless Chrome)"
if [ ! -x "$CHROME" ]; then
  echo "  !! Chrome not found at $CHROME -- skipping chrome_invoice.pdf"
else
  "$CHROME" \
    --headless=new \
    --disable-gpu \
    --no-pdf-header-footer \
    --virtual-time-budget=10000 \
    --print-to-pdf="$OUT/chrome_invoice.pdf" \
    "file://$SRC/invoice.html" >/dev/null 2>&1
  echo "  -> $OUT/chrome_invoice.pdf"
fi

# ---------------------------------------------------------------------------
# 2. quartz_graphics.pdf -- Swift + CoreGraphics/CoreText PDF
# ---------------------------------------------------------------------------
echo "[2/4] quartz_graphics.pdf (swift + CoreGraphics)"
if ! command -v swift >/dev/null 2>&1; then
  echo "  !! swift not found -- skipping quartz_graphics.pdf"
else
  swift "$SRC/quartz_graphics.swift" "$OUT/quartz_graphics.pdf"
  echo "  -> $OUT/quartz_graphics.pdf"
fi

# ---------------------------------------------------------------------------
# 3. gs_objstm_cmyk.pdf -- PostScript -> Ghostscript pdfwrite with object
#    streams and an xref stream (CompatibilityLevel 1.7)
# ---------------------------------------------------------------------------
echo "[3/4] gs_objstm_cmyk.pdf (ghostscript pdfwrite)"
if ! command -v gs >/dev/null 2>&1; then
  echo "  !! gs not found -- skipping gs_objstm_cmyk.pdf"
else
  gs -sDEVICE=pdfwrite \
     -dCompatibilityLevel=1.7 \
     -dWriteObjStms=true \
     -dWriteXRefStm=true \
     -dNOPAUSE -dBATCH -dQUIET \
     -sOutputFile="$OUT/gs_objstm_cmyk.pdf" \
     "$SRC/gs_objstm_cmyk.ps"
  echo "  -> $OUT/gs_objstm_cmyk.pdf"
fi

# ---------------------------------------------------------------------------
# 4. libreoffice_table.pdf -- LibreOffice headless conversion of a flat ODF
#    text document (colored table + colored shape)
# ---------------------------------------------------------------------------
echo "[4/4] libreoffice_table.pdf (soffice --headless --convert-to pdf)"
if [ ! -x "$SOFFICE" ]; then
  echo "  !! soffice not found at $SOFFICE -- skipping libreoffice_table.pdf"
else
  LO_TMP="$(mktemp -d)"
  # soffice names the output after the input basename; run with a background
  # timeout guard so a hung instance cannot block the whole regeneration.
  ( "$SOFFICE" --headless --norestore --convert-to pdf --outdir "$LO_TMP" "$SRC/lo_table.fodt" >/dev/null 2>&1 ) &
  SOFFICE_PID=$!
  WAITED=0
  while kill -0 "$SOFFICE_PID" 2>/dev/null; do
    sleep 2
    WAITED=$((WAITED + 2))
    if [ "$WAITED" -ge 90 ]; then
      echo "  !! soffice exceeded 90s -- killing and skipping libreoffice_table.pdf"
      kill "$SOFFICE_PID" 2>/dev/null || true
      break
    fi
  done
  wait "$SOFFICE_PID" 2>/dev/null || true
  if [ -f "$LO_TMP/lo_table.pdf" ]; then
    mv "$LO_TMP/lo_table.pdf" "$OUT/libreoffice_table.pdf"
    echo "  -> $OUT/libreoffice_table.pdf"
  else
    echo "  !! libreoffice_table.pdf was not produced"
  fi
  rm -rf "$LO_TMP"
fi

# ---------------------------------------------------------------------------
# 5. ycck_jpeg.pdf -- a YCCK (Adobe APP14 transform 2) CMYK JPEG in a minimal PDF.
#    The JPEG in src/ycck_adobe.jpg is a real Adobe-produced image, taken from a
#    public domain PDF (see README.md). An image written by ImageMagick will not do:
#    it stores YCCK without Adobe's inversion, so it cannot tell the two apart.
# ---------------------------------------------------------------------------
echo "[5/5] ycck_jpeg.pdf (python)"
python3 "$SRC/make_ycck_pdf.py" "$SRC/ycck_adobe.jpg" "$OUT/ycck_jpeg.pdf"
echo "  -> $OUT/ycck_jpeg.pdf"

echo "== done =="
ls -la "$OUT"/*.pdf 2>/dev/null || true
