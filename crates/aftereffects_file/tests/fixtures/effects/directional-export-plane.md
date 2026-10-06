# W07 forward Directional Blur source-plane control fixture

Public minimal `directional-export-plane.fx.json` is explicit editable FX, not an
import of hidden/original native data. Its opaque red 12×12 Rect selects the
existing native Solid writer via its editable-solid description. 180×160 square
pixels, one second, static ordinary2D, sole enabled Directional Blur, no masks,
matte, styles or parent. FX screen Direction90/Length40 with Scale200/Rotation90
must write native source Direction0/Length20. The test edits FX Scale300,
Rotation−45, Direction15 and Length90, freshly writing Direction60/Length30.

Independent managed native author/readback `directional-export-plane.jsx` ran
on AE26.5x89, request `tsrct-aep49-w07-forward-directional-readback-v1`,
job `0274f1c6551b47fa9c56fa5559f345df`, with verified cleanup/fresh READY.
It opened untouched fresh RED/base/edited export inputs, read one native Solid
and one editable enabled `ADBE Motion Blur`, then independently authored the
base and edited controls. Native readback matched the independent controls,
Scale/Rotation/Anchor/Position exactly. RED still read Direction90/Length40.
No native render was performed in this new probe.

Pinned independently authored `directional-export-plane.aep` SHA256:
`3d84390aa913d785afb397fe66645a6a690d13b89b5ccbd1cd70544f1703bf85`.
Composition1 `W07 independent export base`: native Direction0/Length20,
Scale[200,200,100], Rotation90. Composition16 `W07 independent export edit`:
Direction60/Length30, Scale[300,300,100], Rotation−45. Both30fps/1second,
180×160, one red12×12Solid, Anchor[6,6,0], Position[90,80,0]. These two targets
are consumed by the one base/edit forward-export case, not generic parse cases.
Authoring JSX is reproducible only through the managed native API; it uses private
input/output paths and does not render, transport or control another session.

Executable CPU assertions:
- `export_document::tests::directional_plane::fresh_directional_solid_export_inverts_static_screen_plane_and_fx_edits`
- `directional_solid_export_declines_nonuniform_reflected_disabled_and_vector_profiles`
- `directional_solid_export_declines_animated_owner_transform`

CPU own-reader assertions are supplementary; independent native inspection was
separately executed. Import product behavior is unchanged; no new import/render
fidelity claim. Native 30fps MP4, long-term Asset/fresh download, FX/native RGB and
alpha/kernel comparisons, animated/mixed/parented stages, and original49/P037
proof remain missing/unmeasured. This is draft partial forward-export progress,
not a completed bidirectional/fidelity certificate.
