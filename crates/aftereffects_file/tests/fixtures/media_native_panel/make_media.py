"""Author deterministic primary OpenEXR stills for independent AE media cases.

This writes real, uncompressed scanline images, not AEP/FX output. The movie is
an existing independently-authored source, copied byte-for-byte separately.
"""
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parent / "media"
WIDTH, HEIGHT = 64, 48


def attribute(name: str, kind: str, data: bytes) -> bytes:
    return name.encode() + b"\0" + kind.encode() + b"\0" + struct.pack("<I", len(data)) + data


def image(name: str, red: bool) -> None:
    channels = b"".join(
        channel.encode() + b"\0" + struct.pack("<iB3xi", 2, 0, 1) + struct.pack("<i", 1)
        for channel in ("B", "G", "R")
    ) + b"\0"
    box = struct.pack("<iiii", 0, 0, WIDTH - 1, HEIGHT - 1)
    header = b"".join((
        struct.pack("<II", 20000630, 2),
        attribute("channels", "chlist", channels),
        attribute("compression", "compression", b"\0"),
        attribute("dataWindow", "box2i", box),
        attribute("displayWindow", "box2i", box),
        attribute("lineOrder", "lineOrder", b"\0"),
        attribute("pixelAspectRatio", "float", struct.pack("<f", 1)),
        attribute("screenWindowCenter", "v2f", struct.pack("<ff", 0, 0)),
        attribute("screenWindowWidth", "float", struct.pack("<f", 1)),
        b"\0",
    ))
    rows = []
    for y in range(HEIGHT):
        # Colored checker pattern makes crop, fit and takeover visible; opposite
        # dominant hues prevent a replacement case from passing on identical media.
        pixels = [((x // 8 + y // 8) % 2) for x in range(WIDTH)]
        blue = [0.05] * WIDTH
        primary = [0.9 if pixel else 0.45 for pixel in pixels]
        secondary = [0.07 if pixel else 0.15 for pixel in pixels]
        r, g = (primary, secondary) if red else (secondary, primary)
        payload = b"".join(struct.pack("<" + "f" * WIDTH, *values) for values in (blue, g, r))
        rows.append(struct.pack("<iI", y, len(payload)) + payload)
    start = len(header) + HEIGHT * 8
    offsets = b"".join(struct.pack("<Q", start + y * len(rows[0])) for y in range(HEIGHT))
    (ROOT / name).write_bytes(header + offsets + b"".join(rows))


def png_chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def unsupported_png() -> None:
    pixels = b"".join(b"\0" + b"\xff\xc0\x20" * WIDTH for _ in range(HEIGHT))
    (ROOT / "unsupported.png").write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 2, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(pixels))
        + png_chunk(b"IEND", b"")
    )


if __name__ == "__main__":
    ROOT.mkdir(parents=True, exist_ok=True)
    image("red.exr", True)
    image("green.exr", False)
    unsupported_png()
