# Independent native hard Shadow source-plane control

- Native source: `shadow-static-source-plane.aep`, SHA-256
  `1c6c8df52829b289a2c8e8320619f4226e2b5b889c9e1dc93fe7cc898fcf6473`.
- Independently authored with the managed headless-adobe `Client.run_jsx`
  (request `tsrct-aep49-w07-shadow-plane-author-v1`), not the converter writer.
  The adjacent JSX is the pinned native authoring function body; execute only
  through that managed API, never as an independent transport.
- Composition ID1, `W07-shadow-plane`, 180x160 square pixels, duration1s, 30fps.
- A red32x24 Solid at[90,80], Scale[200,200], Rotation90. Sole native Effect
  Drop Shadow: black, Opacity255, Direction90, Distance20, Softness0, ShadowOnly0.
  Native getter readback confirmed Direction90/Distance20/Rotation90 and
  Scale[200,200,100]. Source controls and effect remain editable.
- Managed `render_aep` request `tsrct-aep49-w07-shadow-plane-rgba-v1`,
  format `rgba_mov`, output30fps, audiooff. Native MOV SHA-256
  `0a42ae866b6930a11007ae19924fd95db2a571647d5f90c4b82d27a5d0329038`.
  Native media stays in ignored/local managed storage, not Git.
- First decoded straight RGBA frame: red (R>200,A>200) bounds
  (67,49)-(112,110), black (R<10,A>200) bounds (67,113)-(112,150).
  Alpha>200 bounds (67,49)-(112,150). The hard shadow moves downward40px,
  not rightward20px: native layer Scale/Rotation act on its source-plane offset.
  Antialiased edge pixels are not treated as exact rectangle equality.
- Managed author/render cleanup and fresh READY succeeded. This is native
  numerical/readback/render evidence, not human UI inspection.
- `structure_document::shadow_plane::tests::native_static_hard_shadow_uses_the_transformed_source_plane`
  failed before repair (rust queue1315, commit794be2471): editable FX offset
  [20,-1.2246467991473533e-15], expected[0,40]. It checks a fresh native-source
  import, not a converter-generated roundtrip.

## Independent reviewer control/readback follow-up

- Managed `Client.run_jsx` request
  `r03-4829-c0d2205a5518-controls-v1`, native job
  `453e7d13d4ef4f1face504061d4568cb`, AE26.5x89, completed cleanup/READY.
  One native call, zero models; not an additional render.
- The adjacent `shadow-static-source-plane-readback.jsx` is the executed trusted
  function body (SHA-256
  `db36b4f34298e9cc80bc9b6b6026ce26c45cf8625af775af307469c014f14f5e`).
  It accepts pinned `source`, `fresh`, and `edited` private AEP assets through
  managed `context.assets`; do not run it with another transport.
- Native getters independently confirmed that this ordinary AV Solid's Layer
  Transform inventory has **no `ADBE Skew` or `ADBE Skew Axis`**. Those controls
  belong to other native profiles, such as Shape-group transforms, not this
  eligible Solid owner. Auto Orient was None. Actual input edits to Scale300%
  and Rotation0 were read back, with the independent source closed unsaved.
- Immutable converter1472, commit `c0d2205a55185db115728de95586d4d29a814d28`,
  freshly imported the independent source and exported at explicit30fps.
  Native getters accepted the fresh output and an edited-FX offset `[0,60]`:
  one enabled Drop Shadow Layer Style, Distance40/60 respectively, Angle90,
  Global Angle off, Size0. Companion owner Scale[200,200,100]/Rotation90 and
  composition durations/FPS were retained. The source canvas remained180x160.
  Fresh AEP SHA-256
  `322e119f7b574173527b1f945fe51af633ce59c7267d027d7e00504232d2f92e`;
  edited AEP SHA-256
  `180bb804dbc19d29833b2b5de0d5c1b6a2267e8b214fa1ee5aae72a6cab3d228`.
- This proves editable exported controls and one deliberate edit independently
  of the converter reader. It does not prove Layer Style pixel equivalence to
  the original native Effect, clipping, RGB/alpha, or long-term Asset delivery.
- Compensation now also excludes native auto-orient and checks the authored
  raw Transform for animation/expressions before aliases can be lowered.
  The native-source auto-orient flag test is supplemental CPU guard evidence,
  not independent native path-orientation/render proof.

## Limitations

This control proves the isolated static hard-shadow offset axis only. It does
not establish soft-kernel, arbitrary affine, nested/animated/parented transforms,
3D, source clocks, motion blur or general RGB/alpha equality. The helper's
static input-edit/guard tests are supplementary CPU evidence, not independent
native input-edit/export proof. Fresh FX-render comparison, exported native render/RGB/alpha, 30fps MP4
long-term Asset publication and original P037 remain unrun. Exported-native
control/readback is now bounded as above; no feature-wide fidelity or complete
bidirectional delivery is claimed.
