# Native parenting projection

`layer_misc.json.gz` stores the independent AE JSON projection for the existing
`../layers/layer_misc.aep`, pinned to MIT `forticheprod/py-aep` revision
`e12a451c35bacd3f34a080090265f9370e66162b`. See `provenance.json` and the
native AEP hash/provenance in `../layers/` (license: `../LICENSE`).

Target: composition 44, child 59 (`ChildLayer`), transform parent 57
(`ParentNull`). The parent has anchor `[0,0,0]`, position `[50,50,0]`, and
opacity 0; the child has anchor `[50,50,0]`, position `[0,0,0]`, opacity 100.
The editable ancestor copy must not inherit the null's zero opacity.

This is a mixed native layer-settings fixture, not newly authored minimal
feature proof. Tests check native source parsing, these projected values,
editable transform-only ancestry, preserved stacking, and serialization.
No Adobe UI inspection, native render comparison, or alpha proof was run.
