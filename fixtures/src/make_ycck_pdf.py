#!/usr/bin/env python3
"""Wrap a JPEG in a minimal one-page PDF as a /DeviceCMYK DCTDecode image.

Used by generate.sh to build fixtures/ycck_jpeg.pdf, the regression fixture for
YCCK (Adobe APP14 transform 2) JPEGs. Standard library only.

  python3 make_ycck_pdf.py input.jpg output.pdf
"""

import pathlib
import sys


def make_pdf(jpeg: bytes, width: int, height: int) -> bytes:
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] "
        b"/Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
    ]
    content = b"q 120 0 0 80 0 0 cm /Im0 Do Q"
    objects.append(b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream")
    objects.append(
        b"<< /Type /XObject /Subtype /Image /Width %d /Height %d "
        b"/ColorSpace /DeviceCMYK /BitsPerComponent 8 /Filter /DCTDecode /Length %d >>\nstream\n"
        % (width, height, len(jpeg))
        + jpeg
        + b"\nendstream"
    )

    buf = bytearray(b"%PDF-1.4\n")
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(buf))
        buf += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    start = len(buf)
    buf += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    for offset in offsets:
        buf += b"%010d 00000 n \n" % offset
    buf += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objects) + 1,
        start,
    )
    return bytes(buf)


def jpeg_size(data: bytes) -> tuple[int, int]:
    """Reads the frame header to get the image size."""
    i = 2
    while i < len(data):
        if data[i] != 0xFF:
            break
        marker = data[i + 1]
        length = int.from_bytes(data[i + 2 : i + 4], "big")
        if marker in (0xC0, 0xC1, 0xC2):
            return (
                int.from_bytes(data[i + 7 : i + 9], "big"),
                int.from_bytes(data[i + 5 : i + 7], "big"),
            )
        if marker == 0xDA:
            break
        i += 2 + length
    raise SystemExit("could not read the JPEG size")


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    jpeg = pathlib.Path(sys.argv[1]).read_bytes()
    width, height = jpeg_size(jpeg)
    pathlib.Path(sys.argv[2]).write_bytes(make_pdf(jpeg, width, height))


if __name__ == "__main__":
    main()
