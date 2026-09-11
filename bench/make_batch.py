#!/usr/bin/env python3
"""Build a 50-page invoice PDF by repeating fixtures/src/invoice.html and printing it with Chrome.

Usage: make_batch.py [--copies 25] [--chrome PATH]
Writes bench/inputs/invoice-50pages.pdf (each copy of the invoice has 2 pages).
"""
import argparse
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
SOURCE = HERE.parent / "fixtures" / "src" / "invoice.html"
DEFAULT_CHROME = (
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
    if sys.platform == "darwin"
    else shutil.which("google-chrome") or shutil.which("google-chrome-stable") or "google-chrome"
)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--copies", type=int, default=25)
    parser.add_argument("--chrome", default=DEFAULT_CHROME)
    args = parser.parse_args()

    html = SOURCE.read_text(encoding="utf-8")
    match = re.search(r"(<body[^>]*>)(.*)(</body>)", html, re.S)
    head, body, tail = html[: match.end(1)], match.group(2), html[match.start(3):]
    repeated = head + body * args.copies + tail
    # Resolve the images next to the original HTML.
    repeated = repeated.replace("<head>", f'<head><base href="{SOURCE.parent.as_uri()}/">', 1)

    output = HERE / "inputs" / f"invoice-{args.copies * 2}pages.pdf"
    output.parent.mkdir(exist_ok=True)
    with tempfile.NamedTemporaryFile("w", suffix=".html", encoding="utf-8", delete=False) as page:
        page.write(repeated)
    subprocess.run(
        [args.chrome, "--headless=new", "--disable-gpu", "--no-pdf-header-footer",
         f"--print-to-pdf={output}", pathlib.Path(page.name).as_uri()],
        check=True, capture_output=True,
    )
    pathlib.Path(page.name).unlink()
    print(output)


if __name__ == "__main__":
    main()
