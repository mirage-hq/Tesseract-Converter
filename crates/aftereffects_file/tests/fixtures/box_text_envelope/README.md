# Independent Box automatic-leading source

`native_box_menlo18.aep` SHA256
`9cb3ea4f56f2db5b01f90a6175b1486e74e270ea8c9fb156cf2ce422eec56436`
was independently authored through managed AE JSX on AE26.5x89, job
`87ca4ad7ee29406583c9c24383c86035`, 2026-10-03.
Native composition1 (`Box Content Oracle`),320×180,square pixels,30fps,1second.
One native Box layer:306.5×46.8,Menlo-Regular18,right justification,
text `1080P / 30 FPS`,fill RGB[0.7059,0.8745,0.8235]. Native nonempty
automatic-leading storage has inactive manual-leading0.01.

`writer::native_text::tests::boxed_automatic_leading_matches_native_without_inherited_layout`
pins the source, asserts these native style defaults and fresh cache-free export.
It failed before the fix (21.6vs0.01) and passed afterward. Independent native
cache-removal experiment retained exact nonempty Box getters, job
dfe0439e9c8442c48513e0fba84adade. This fixture is semantic/native-acceptance
proof, not independently rendered/Asset-published visual fidelity or alpha proof.
Import implementation is unchanged. See the support ledger's P047 repair entry.
