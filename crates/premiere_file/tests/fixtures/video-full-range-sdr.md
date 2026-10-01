# Full-range SDR admission fixture

`video-full-range-sdr.mp4` contains six generated red 64x64 frames at 30 fps,
H264 8-bit 4:2:0 with coherent explicit full-range BT.709 bitstream/container flags.
It contains no proprietary project/media and no audio. CPU tests inspect the
codec, dimensions and range declarations; this fixture is not native pixel proof.

Generated offline using FFmpeg 9.0.1/libx264:

```sh
ffmpeg -hide_banner -loglevel error -n \
  -f lavfi -i 'color=c=red:s=64x64:r=30,format=rgb24,scale=out_color_matrix=bt709:out_range=pc,format=yuv420p' \
  -frames:v 6 -an -c:v libx264 -preset veryfast -crf 28 \
  -pix_fmt yuv420p -g 30 -bf 0 -threads 1 \
  -color_range pc -colorspace bt709 -color_primaries bt709 -color_trc bt709 \
  -video_track_timescale 30 -movie_timescale 30 -map_metadata -1 \
  crates/premiere_file/tests/fixtures/video-full-range-sdr.mp4
```

SHA256: `747e0bd8a7383bee641cdc1d5fb03299395269471c637471088c3cba10eacfd0`.
