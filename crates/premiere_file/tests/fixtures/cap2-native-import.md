# Native static Crop and explicit FrameHold

Both XML fixtures derive from the same Adobe-native source, selected sequence
`95dcca2e-32e2-4916-9d29-1276c821c0db`. The source project SHA-256 is
`3f47fe84ac2c285da95da40aa53189400f963a5fd87e68be57e26ece47d93174`.
Its decoded XML SHA-256 is
`9219d0571a5b2f542ba73d93e5a37aa04b07589a94cff77f62cfe43a21c2830f`.
The full source, source media and independent reference video stay outside the
public converter repository; these fixtures contain only the necessary native
numeric fields and record identities. Premiere UI values were not inspected.

`cap2-native-static-crop.xml` contains the complete native
`VideoFilterComponent` records 403/405 and `VideoComponentParam` records
817–828, extracted without changing any fields. Both clips save Left
`StartKeyframe=0` and stale `CurrentValue=99`, Right
`StartKeyframe=49.107574462891`, zero Top/Bottom/Edge Feather and Zoom false.
The test attaches each extracted component to the minimal one-clip test graph
and existing H.264 media. It proves editable mask position/size, media identity
and clip/source timing; the original source's broader effects are outside it.

`cap2-native-frame-hold.xml` retains native `VideoClip:404` and `Clip`
versions, Source reference 111, In/Out `879350472000`/`1091242152000`,
`FrameHold=4` and `FrameHoldStart=879350472000`. Irrelevant label, marker and
clip-UUID metadata were removed. Native placement 224 is
`3602158560000..3814050240000` ticks on a 24000/1001 fps sequence.
The test uses those exact placement/source values, changes only the graph
record/source references and supplies the existing ten-second H.264 test media.
It proves two editable playback keys at 14181/15015 ms, both selecting source
3462 ms. It also proves contextual native-export rejection and rejection of
unknown/incomplete holds, out-of-bounds held times, combined clocks and holds
on unsupported master, graphic and adjustment hosts.

The independent Adobe review video associated with this selected sequence has
SHA-256 `a7889f068d1ac73b76967174fb70bda5b38186684f0b58e3b6e10f21e5f5846e`.
Diagnostic samples at frames 350/359 (zero based, time `frame * 1001 / 24000`)
cover two instants within the cropped hold while other visible content continues
moving. This is **structural-only
fixture proof**: no focused Adobe video for the staged H.264 derivatives, scored
visual gate, alpha gate or audio-fidelity claim is attached to these fixtures.

Regression symbols:

- `crop::native_static_crop_uses_authored_start_instead_of_stale_current_value`
- `crop::native_static_crop_still_rejects_invalid_authored_values_and_animation`
- `frame_hold::native_explicit_frame_hold_imports_as_editable_constant_playback`
- `frame_hold::native_frame_hold_rejects_unknown_incomplete_out_of_bounds_and_combined_forms`
- `frame_hold::native_frame_hold_is_rejected_on_master_sources_graphics_and_adjustments`

Before the fixes, Crop fails with opposing edges leaving no visible area and
FrameHold fails as an unknown native field. No ProRes admission or color-profile
allowlist change accompanies these import fixes.


Additional selected native import records:

- `cap2-native-retime.xml`: timing fields of `VideoClipTrackItem:255` and
  `VideoClip:648` (inner timeline `10594584000..360215856000` ticks; source
  0..69924254400 ticks; `PlaybackSpeed` 0.2). The regression binds those fields
  to the main opening nest's inner window frames 9..19 at 24000/1001 and checks
  retained media identity, active range, source range and editable playback.
- `cap2-native-geometry2.xml`: `VideoFilterComponent:407` and parameters 829..840,
  copied numerically from the same decoded native source, including the empty
  time-varying Rotation marker and the Scale Height 100→118 cubic ease. The
  regression stages them on the existing Adobe-native adjustment generator
  fixture with native 226's placement and 408's source In/Out, and checks the
  Corner Pin effect's eight coordinate key tracks. The staging sequence rate
  is changed to 24000/1001; unrelated placements need not survive that staging.

These retain selected native fields with irrelevant metadata and references
removed. They contain no customer media or reference video. The source hashes and
selected sequence identity and independent reference association above also apply
here. Their committed regressions are structural proof; independent candidate
frame comparison is tracked outside the public source tree.
