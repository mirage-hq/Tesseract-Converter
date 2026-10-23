#!/usr/bin/env python3
"""Encode the in-house sRGB chart as BT.2020 PQ or HLG 16-bit RGB (SDR white = 203 cd/m2).

PQ: sRGB EOTF -> linear BT.709 -> BT.2020 primaries -> x203 cd/m2 -> ST 2084 inverse EOTF.
HLG: same linear BT.2020 light scaled by 0.26496 (BT.2408 reference white -> HLG signal 0.75) -> BT.2100 HLG OETF
(scene light; no inverse OOTF).
usage: color_hdr_clip_chart.py a1_bg_chart.png pq|hlg out.ppm  (16-bit PPM input for the
ffmpeg commands recorded in color_fixtures.provenance.json, which produce color_hdr_pq_clip.mp4 /
color_hdr_hlg_clip.mp4)
"""
import sys
import numpy as np
from PIL import Image

src, transfer, out = sys.argv[1:4]
rgb = np.asarray(Image.open(src).convert('RGB'), dtype=np.float64) / 255.0
lin = np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)
m = np.array([[0.6274040, 0.3292820, 0.0433136],
              [0.0690970, 0.9195400, 0.0113612],
              [0.0163916, 0.0880132, 0.8955950]])
lin2020 = np.clip(lin @ m.T, 0.0, None)
if transfer == 'pq':
    y = lin2020 * 203.0 / 10000.0
    m1, m2, c1, c2, c3 = 2610 / 16384, 2523 / 4096 * 128, 3424 / 4096, 2413 / 4096 * 32, 2392 / 4096 * 32
    yp = y ** m1
    sig = ((c1 + c2 * yp) / (1 + c3 * yp)) ** m2
elif transfer == 'hlg':
    e = lin2020 * 0.26496
    a, b, c = 0.17883277, 1 - 4 * 0.17883277, 0.5 - 0.17883277 * np.log(4 * 0.17883277)
    sig = np.where(e <= 1 / 12, np.sqrt(3 * np.maximum(e, 0)), a * np.log(np.maximum(12 * e - b, 1e-12)) + c)
else:
    raise SystemExit('transfer must be pq or hlg')
code = np.round(np.clip(sig, 0, 1) * 65535).astype(np.uint16)
# Pillow cannot write 16-bit RGB; write a 16-bit PPM (P6, maxval 65535, big-endian) for ffmpeg.
h, w, _ = code.shape
with open(out, 'wb') as f:
    f.write(b'P6\n%d %d\n65535\n' % (w, h))
    f.write(code.astype('>u2').tobytes())
print(transfer, 'signal min/max', float(sig.min()), float(sig.max()))
