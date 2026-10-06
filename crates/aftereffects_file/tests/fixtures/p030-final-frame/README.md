# Public final-frame movie-clock rounding fixture

Self-generated FFmpeg testsrc2, no original P030 pixels or private media. 241 AVC frames at 24 fps, 32×18 square pixels. Generate using:

```sh
ffmpeg -f lavfi -i 'testsrc2=size=32x18:rate=24' -frames:v 241 -c:v libx264 -pix_fmt yuv420p -an public-241-frames.mp4
```

Set the single version-zero `elst` segment duration from 10042 to 10041 movie ticks (timescale 1000), leaving its unit rate, media origin, complete compressed samples, and ceil-rounded tkhd/mvhd duration unchanged. This reproduces the P030 clock/header profile without its private pixels. It is a regression input, not a reference render or published Asset fidelity certificate.
