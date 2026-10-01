# Synthetic source-dimension validation fixture

`source_rect_64x36.mp4` is a locally generated, silent black H.264 video:
64×36 pixels, 30 frames at 30fps, one second. It contains no third-party footage,
fonts, audio or Adobe project content. The project MIT license applies to this
authored fixture; this is not an independent Adobe rendering oracle.

SHA-256: `65705184750ea2747f14e1ad2946bc47ef832e88f6abd1f544cf2b6aae0f2ed2`.

Generation command (FFmpeg with libx264; encoded bytes may vary by version):

```sh
ffmpeg -hide_banner -loglevel error \
  -f lavfi -i color=c=black:s=64x36:r=30:d=1 -an \
  -c:v libx264 -pix_fmt yuv420p -threads 1 -map_metadata -1 \
  -fflags +bitexact -flags:v +bitexact -movflags +faststart \
  -n source_rect_64x36.mp4
```

`source_rect_cannot_disagree_with_packaged_video_frame` packages these real
encoded bytes against a declared 1920×1080 source rectangle and requires both
Check and Write to reject the dimension mismatch without publishing output.
It does not depend on picture content, long-GOP seeking or Adobe fidelity.

This replaces the converter's copy of the enclosing repository's `scrub_long_gop_30fps.mp4`, whose
provenance did not establish redistribution permission. The original codec
fixture outside the public conversion workspace is unchanged.
