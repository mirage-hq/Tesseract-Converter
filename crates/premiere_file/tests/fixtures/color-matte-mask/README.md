# Native-derived Color Matte opacity-mask host regression

`native-controls.xml` is the closure of native Opacity component1203 and its mask
from `../opacity-mask-records.xml` (SHA256
`bee79e788d43e78910b3dc24ee78da6bc77147024f06f40898471707788b9329`).
The existing mask source is derived from the pinned Adobe-native keyed-mask
fixture; its saved path payload/keys, inactive metadata and mask controls remain
unchanged. IDs advance by20000 and Mask Path source ticks advance by
914457600000000 to align the existing Color Matte generator clock. No path bytes,
values or interpolation change. Controls SHA256:
`e27b06644e1f0c9cfd847daf000cc007c1a653123eff110a8f3a8391247095ac`.

Tests attach this closure to the red matte of the pinned
`feature_color_matte_strict.prproj`, target
`c8acf9c1-34b2-4086-9f55-d528950a7059`. Numeric/Motion mutations are supplemental.
The blue matte and actual Video/asset bytes must survive; malformed required
coverage must omit the red host, never expose it unmasked.

This proves editable host mapping, not a newly independently Adobe-authored
masked Color Matte, native/UI opening, RGB/alpha or Feather fidelity. No new
native render was authorized. Reverse conversion is unchanged/out of scope.
