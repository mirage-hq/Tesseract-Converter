# Animated coincident Path seams

FX → AEP retains authored terminal vertices when a closed animated contour
repeats its first point at some keys but not others. Previously, independently
folding each key's repeated endpoint changed the native vertex count, and the
validator rejected the otherwise matching command topology. No vertex is added:
the authored closing segment, including its zero-length state, remains intact.
Static paths and consistently folded animation tracks retain their existing
compact encoding. Native coordinate, command, timing, list-field and easing
guards remain; this does not admit unequal command topology or pad contours.

## Bounded native evidence

Two managed Adobe operations established this profile on AE 26.5:

- Independent author/save/reopen control: one closed four-vertex Path,
  `[[0,0],[80,0],[80,80],[0,y]]`, two Linear keys at 0 and 1 second,
  with `y=0` and `y=80`; zero incoming/outgoing tangents. Adobe retained
  four vertices at 0, .25, .5, .75 and 1 second, with terminal Y
  `0,20,40,60,80`. Author job `fefe84301c854e06866cc52175013900`.
- Fresh converter-generated BASE and actual input EDIT (`80→60`) both opened
  with editable Path keys and four vertices. Native samples matched the control;
  EDIT terminal Y was `0,15,30,45,60`, including a −10 midpoint change.
  Acceptance job `e3cafab443e14b69b162ca324636fd5d`. Both calls completed READY.

Regression `native_path_keys_keep_authored_coincident_seam_vertices` pins the
four-vertex serialization and unchanged consistently folded encoding;
`native_path_keys_reject_unproved_speed_and_morph_but_allow_hold_topology`
retains negative unsupported-easing/unequal-topology assertions. The latter's
new seam assertion failed before the fix (Rust job 2688) and passed afterward
(job 2690).

This is numeric editable/native acceptance, not a general topology, alpha,
audio or raster-fidelity pass. The independent native source/render has not
been published as a long-term Asset. Owner-local JS fitting remains its existing
sampled-grid approximation; seam preservation neither tightens that tolerance
nor proves intersample source equivalence. Full P047 render/comparison evidence
is tracked separately and must not be inferred from this bounded control.
