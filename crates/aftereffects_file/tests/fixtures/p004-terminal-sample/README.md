# Public terminal-zero AVC input

Self-generated `testsrc2` pixels, not private P004 footage or a reference render.

```sh
ffmpeg -f lavfi -i 'testsrc2=size=32x18:rate=120' -frames:v 45 -c:v libx264 -bf 0 -pix_fmt yuv420p -an public-unmodified.mp4
```

Preserve all 45 compressed samples and their zero-origin constant presentation grid. Change the version-zero `stts` from 45 samples × 128 media ticks to two runs: 44 × 128, then one × 0. Set `mdhd` duration to 5632 (timescale 15360); set the sole origin-zero unit-rate `elst`, `mvhd`, and `tkhd` duration to 367 movie ticks (timescale 1000). Rebuild containing atom lengths. No `ctts` is present. This reproduces the independently observed P004 container profile without its private pixels. Native Adobe independently imports the unchanged private source as 45 full frames, duration .375 seconds.
