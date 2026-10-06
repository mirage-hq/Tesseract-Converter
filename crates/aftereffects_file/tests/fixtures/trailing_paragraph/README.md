# Native trailing-paragraph control

`native.aep` is a minimal, independently AE-authored source, not converter output.
SHA256: `7d194b82b6248ce18809210a39892eb8ad652ba80fcb0561102a40a7e70c9979`.
Composition native ID `1`, name `W03 trailing paragraph`, 320x180, square pixels,
30fps, one second. Adobe AE `26.5x89`.

`author.jsx` is the exact managed function-body authoring script (SHA256
`86f51f82ade53f0ed383cc554ba661190c0fe3bbcf57b91ee079960e741963c0`).
It must only run through `Client.run_jsx`, with `output_kind="aep"`; it is not a
direct Adobe execution script. No input assets are required.

Managed `Client.run_jsx` request `accuracy-w03-trailing-paragraph-20261004-v1`
(job `d1244884bbbf4ec1b7fd5d76029e6afe`) independently authored two point Text
layers using `addText('A')` and `addText('A\r')`. Native Source Text readback
returned exactly `['A', 'A\r']`. Editing the latter through `TextDocument.text`
and `setValue` to `A\r\r` returned exactly `A\r\r`, then the native project
was saved. The corresponding saved COS text is `A\r\r\r`; the other layer
stores `A\r`. Thus native storage adds one terminal paragraph return even when
editable input already ends in a return. Cleanup and fresh READY passed.

Runnable regressions: `trailing_paragraph_point_text_matches_native_authored_control`
and `trailing_paragraph_box_text_keeps_authored_breaks_and_terminal_run_unit`.
The point regression checks this source hash and saved native control directly;
the box regression applies the same terminal-marker grammar and checks UTF-16
style-run coverage. `trailing_paragraph_native_import_preserves_authored_empty_paragraphs`
freshly imports composition `1` and asserts distinct editable Text values `A`
and `A\n\n`, removing only the native storage marker. Import implementation is
unchanged; this assertion is structural evidence, not generated native acceptance. Box native readback, native rendering, visual equality,
alpha/audio and original49-case fidelity are **not established** by these tests.
No reference video or Asset publication is claimed.

Execution evidence: both regressions failed before the fix in shared Rust queue
job `1300`, then passed in job `1305` (2 passed, 0 ignored), alongside workspace
check, clippy `-D warnings`, and formatting. Immutable converter build job `1310`
(SHA256 `56ac006f898a641a3a8ddbbd97cbf688e4ffd0c40f6130117d89f6423acb652a`)
freshly imported this source and exported a new 30fps AEP. A second managed
readback request `accuracy-w03-generated-paragraph-20261004-v1` failed opening
that generated project: `After Effects error: Error reading the text layer.
Skipping the text layer.` (job `49df9fe320b94db5a31dd411f4f9554d`, READY true).
Consequently generated native acceptance/edit response remains **unproved**;
this local grammar repair does not fix or bypass the separate Text envelope/
font-cache acceptance work in PR4807. No original49 render was attempted after
this prerequisite failure. The independent native author/readback/control and
RED/GREEN payload correction must not be described as end-to-end visual proof.

## Independent current-main acceptance comparison

Reviewer build `1349` pins base `068aec6922b3bb97486ad8a0d48449e1aef4ae6a`
(binary SHA256 `42d5e12dc0d1f90833c2d675c690a8dde22dc33e876af759d1d1248ff053a285`).
Fresh import of the unchanged source followed by a 30fps baseline export emits
AEP SHA256 `74845c9f0a63e49651a3e8fb01b90564c9794db66a10ef0033d81cdf09b80d11`.
Exporting that exact same FX archive with fixed build `1310` emits SHA256
`9eeee44dbdae85b385822ce6de29afcc8ad4409613d9ca384135d82135b6e1a7`, identical
to the previously rejected fixed input. Build `1310`'s product source matches
reviewed PR HEAD `95d3f905fd73aebf4876c2d4ded29d269ce2b89a`; intervening changes
only retain the author script and documentation, not converter behavior.

Managed reviewer request `accuracy-r01-pr4825-main068-baseline-20261004-v1`
(job `d2bc71383751466886b2994a0060267e`, READY true) also rejects the **baseline**
AEP at open with the same `Error reading the text layer. Skipping the text layer.`
Thus the tested current-main Text acceptance failure predates this paragraph
change. It does not identify the precise envelope/font-cache cause or prove that
unmerged PR4807 resolves this particular fixture. No PR4807 code is stacked or
copied here. The fixed known-failed input was not replayed. Generated acceptance,
native edit response, Box-native paragraph readback, reference rendering/Asset
publication and original49 fidelity remain incomplete.

Reviewer queue `1369` passed all three paragraph regressions (3 passed, 0 failed,
0 ignored), standalone workspace all-target check/clippy with warnings denied,
and formatting on `2aea5dece9a8a470b204f3a906a1719acc0c07f3`. The later evidence
update is documentation-only; no new native acceptance is implied.

Before publication, main advanced to `e6ac82fe9759c8d02e21946b4e66f2611857db5a`
(Basic Text face preservation). The clean rebase retained all task patches;
queue `1378` passed the same three paragraph tests and check/clippy/fmt on
rebased `7efd031bffec92eee1dc4687a5319bbc410480ef`. The native acceptance pair
above remains evidence at baseline `068aec692`, not fresh native proof of this
later base. Final receipt edits are documentation-only.
