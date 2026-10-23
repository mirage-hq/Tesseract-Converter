#!/usr/bin/env python3
"""Encode the in-house sRGB chart as a Sony S-Log3 / S-Gamut3.Cine camera picture.

sRGB EOTF -> linear BT.709 scene light (chart white = 1.0, so 18% grey = 0.18) ->
S-Gamut3.Cine primaries (D65, matrix derived from the published chromaticities) ->
S-Log3 OETF (Sony technical summary: 10-bit code value CV, 18% grey = CV 420) ->
the camera's legal-range convention: R'G'B' 10-bit codes equal CV, so the 16-bit
RGB written here is (CV - 64) / 876 for FFmpeg's full-to-limited (out_range=tv)
conversion. Negative gamut excursions are clipped to scene 0 before the curve.
usage: color_aces_slog3_clip.py chart.png out.ppm  (16-bit PPM input for the ffmpeg
command recorded in color_fixtures.provenance.json, which produces
color_aces_slog3_clip.mp4)
"""
import sys
import numpy as np
from PIL import Image


def rgb_to_xyz(primaries, white):
    xyz = lambda x, y: np.array([x / y, 1.0, (1 - x - y) / y])
    p = np.stack([xyz(*c) for c in primaries], axis=1)
    return p * np.linalg.solve(p, xyz(*white))


D65 = (0.3127, 0.3290)
BT709 = [(0.640, 0.330), (0.300, 0.600), (0.150, 0.060)]
SGAMUT3_CINE = [(0.766, 0.275), (0.225, 0.800), (0.089, -0.087)]


def slog3(x):
    x = np.maximum(x, 0.0)
    log = (420.0 + np.log10((x + 0.01) / (0.18 + 0.01)) * 261.5)
    lin = x * (171.2102946929 - 95.0) / 0.01125000 + 95.0
    return np.where(x >= 0.01125000, log, lin)


src, out = sys.argv[1:3]
rgb = np.asarray(Image.open(src).convert('RGB'), dtype=np.float64) / 255.0
lin = np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)
m = np.linalg.solve(rgb_to_xyz(SGAMUT3_CINE, D65), rgb_to_xyz(BT709, D65))
cv = slog3(lin @ m.T)
code = np.round(np.clip((cv - 64.0) / 876.0, 0, 1) * 65535).astype(np.uint16)
h, w, _ = code.shape
with open(out, 'wb') as f:
    f.write(b'P6\n%d %d\n65535\n' % (w, h))
    f.write(code.astype('>u2').tobytes())
print('S-Log3 CV min/max', float(cv.min()), float(cv.max()))
