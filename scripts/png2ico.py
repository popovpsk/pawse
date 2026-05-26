#!/usr/bin/env python3
"""Pack one or more PNG files into a Windows .ico (PNG-compressed entries).

Usage: png2ico.py <out.ico> <in1.png> [in2.png ...]

Uses only the standard library. Each PNG is stored verbatim as a Vista-style
PNG-compressed icon entry, which Windows Vista and later read natively. Sizes of
256 are encoded as 0 in the directory entry per the ICO spec.
"""
import struct
import sys


def png_size(data: bytes) -> int:
    # PNG signature (8 bytes) + IHDR length (4) + "IHDR" (4) then width (4), height (4).
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG file")
    width, height = struct.unpack(">II", data[16:24])
    if width != height:
        raise ValueError(f"icon must be square, got {width}x{height}")
    return width


def main() -> None:
    if len(sys.argv) < 3:
        sys.exit("usage: png2ico.py <out.ico> <in1.png> [in2.png ...]")
    out_path = sys.argv[1]
    pngs = []
    for path in sys.argv[2:]:
        with open(path, "rb") as f:
            data = f.read()
        pngs.append((png_size(data), data))
    pngs.sort(key=lambda p: p[0])

    count = len(pngs)
    header = struct.pack("<HHH", 0, 1, count)  # reserved, type=icon, count
    offset = 6 + 16 * count
    entries = bytearray()
    blobs = bytearray()
    for size, data in pngs:
        dim = 0 if size >= 256 else size  # 0 means 256 in the ICO directory
        entries += struct.pack(
            "<BBBBHHII",
            dim, dim, 0, 0,  # width, height, colors, reserved
            1, 32,           # planes, bits-per-pixel
            len(data), offset,
        )
        blobs += data
        offset += len(data)

    with open(out_path, "wb") as f:
        f.write(header)
        f.write(entries)
        f.write(blobs)


if __name__ == "__main__":
    main()
