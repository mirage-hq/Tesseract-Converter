# Public discriminating Hold endpoint render revision

This new independently Adobe-authored control does not replace or prune the
original `hold_endpoint_flags.aep` oracle. Its disabled driver cannot mask the
visible controls. Native source SHA-256:
`9ce527d842991598197745862785fb84805e1c2458d3e2b44d88c20caac7f972`.
Composition1 `hold-render-public`:128x64, square pixels,24fps,2seconds.

- Left: red48x8 Solid at[32,32], rotation expression
  `thisComp.layer("driver").transform.rotation`; disabled native driver's signed
  -.5/.5/1.5s keys have0/90/180degrees, outgoingHold/Hold/Linear and incomingLinear.
  This exercises the newly admitted endpoint profile through a saved native alias.
- Right: red64x64 Solid at[96,32], direct opacity0/100/40percent at the same keys.
  No opaque layer overlaps this region, so rendered opacity is discriminating.
- Managed AE26.5x89 author/save/reopen/readback job
  `2a9b49b00a0249da99a316a07bec8dd4`; separate native30fps render job
  `9849807f2b794a129458e49410713a71`. Both completed cleanup/freshREADY. Exactly
  two managed operations; source24fps, full duration/canvas preserved.

Native MP4:60decoded frames,2seconds,128x64,30fps,13868bytes; SHA-256
`b67cf49013a6651ef9d3fa301a999290f8d6b1052cf87aec2a97fd9a494df770`.
Public immutable `long_term` Asset:`ROaNxCVfwxIfADbwmQvA_vid`, published with
repository `upload_asset`, freshly resolved/downloaded with identical bytes/hash.
No private/original49 material was used or published. No video is committed.

`native_hold_endpoint_flags_public_render_alias_lowers_saved_native_expression`
freshly reads this source and asserts lowered rotation keys, exact signed clocks,
values and endpoint flags, with expression execution removed from editable FX.
Rust queue2183: all six `native_hold_endpoint_flags_` tests and support-ledger
check PASS. Fresh CLI import preserves editable Rotation/Opacity key graphs and
hidden driver; converter build2170, Tesseract immutable build2185/export2204.

## Comparison is measured, NOT a visual pass

Full-resolution canonical RGB24 `validation_cli video`, rgb-hybrid,.25s,.99:
9samples including endpoint2s, mean0.974632540563, minimum0.965132273268 at.5s
(tied through1.25s), maximum0.988406516612. Both videos128x64/30fps/60frames/2s.
The unchanged strict gate FAILS. No resampling, time shifting, source pruning,
threshold relaxation or oracle replacement was used.

Critical native/Tesseract decoded frames0/14/15/44/45/59 expose the same timing:
left horizontal→vertical atframe15→horizontal atframe45; right center red channel
native0→252→101 versus Tesseract0→253→101. This is supplemental behavioral
inspection, not a replacement for the failed full-frame RGB gate. Source-native
reference vs fresh-import Tesseract was compared; generated-AEP native rendering
was not performed. Existing generated-output14-sample acceptance/readback remains
separate. Alpha/audio and original49 improvement are unproved. Both actual render
review threads remain open pending a passing required proof or explicit reviewer
acceptance of this precisely bounded evidence. Machine evidence and Asset receipt:
`hold_endpoint_render.json`.
