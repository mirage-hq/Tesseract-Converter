# Native AEP compositing inputs

Unmodified MIT `forticheprod/py-aep` sources at
`e12a451c35bacd3f34a080090265f9370e66162b`; SHA-256/size/source paths are in
`provenance.json` and verified by a Rust test. License: `../LICENSE`.
JSON files are the pinned upstream Adobe property projections, not converter
outputs. No Adobe UI session or native render was run for this change.

| Native source | Exact target | Editable assertions |
| --- | --- | --- |
| `blendingMode.aep` | comp 30 / layer 43; comp 1 / layer 15; comp 16 / layer 29 | Add, Multiply, Screen respectively, not Normal |
| `trackMatteType.aep` | comp 1 / target 17 / provider 15; comp 18 / target 33 / provider 31 | Alpha/Luma source references resolve within each occurrence; AE-disabled providers remain sampleable through independent editable helper copies |
| `shutterAngle.aep` | comps 1 and 13 | Angle 180/360, phase 0, samples 16, adaptive limit 128, master disabled; authored settings survive even with master disabled |

All three native regression tests failed before mapping: Normal instead of Add,
missing matte, and default phase -90 instead of authored 0. All passed after
mapping. Supplemental record mutations test inverted modes, legacy adjacency,
invalid edges and a visible matte provider; they are not new Adobe evidence.
Every blend table entry is source-code-derived except the three native cases.

Evidence is editable-structure only. Blend pixels, alpha/luma fidelity, temporal
motion-blur sampling and host-FPS parity remain unverified. Independent helper
copies preserve the provider's display switch but do not retain linked editing.
