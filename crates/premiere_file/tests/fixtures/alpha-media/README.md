# Minimal alpha movie sources

These redistributable sources are generated media, not licensed project content
or Adobe render proof. The tests derive a one-second 16x16 native clip from the
existing minimal `one-clip.xml` fixture and use normal public import/publication.

Each source contains 30 progressive frames at 30 fps. In row-major order the
ARGB source pixel is `[y*16+x, 80+x*8, 40+y*8, 160]`, so every 8-bit alpha code
occurs in each frame. The ProRes fixture explicitly stores 16-bit coded alpha;
its original packets must be retained, not routed through 8-bit preparation.

Reproduction with FFmpeg 9.0.1:

```python
from pathlib import Path
Path('source.argb').write_bytes(bytes(
    v for y in range(16) for x in range(16)
    for v in [y*16+x, 80+x*8, 40+y*8, 160]
) * 30)
```

```sh
ffmpeg -f rawvideo -pixel_format argb -video_size 16x16 -framerate 30 \
  -i source.argb -frames:v 30 -an -map_metadata -1 \
  -video_track_timescale 30 -movie_timescale 30 -c:v qtrle animation.mov
ffmpeg -f rawvideo -pixel_format argb -video_size 16x16 -framerate 30 \
  -i source.argb -frames:v 30 -an -map_metadata -1 \
  -video_track_timescale 30 -movie_timescale 30 -c:v prores_ks \
  -profile:v 4 -alpha_bits 16 -pix_fmt yuva444p10le \
  -color_primaries bt709 -color_trc bt709 -colorspace bt709 prores4444.mov
```

The QTRLE test explicitly prepares a whole source using the existing library
backend, then binds the original and replacement hashes through a media map.
It checks alpha admission, dimensions, cadence/count, source identity and
packaged replacement bytes. It does not prove native RGB/alpha rendering or
WebCodecs support. No project baking or implicit preparation is exercised.
