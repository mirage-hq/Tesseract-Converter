# fx_keyframe_bake

Shared, format-neutral primitives for baking evaluated FX animation into editable
keyframes.

The package owns:

- the existing adaptive scalar curve fitter;
- typed-value millisecond sampling with held-step preservation and joint linear
  reduction (bounded working windows, caller-owned interpolation error and native
  key-field limit); continuous window boundaries can retain additional keys;
- the shared Boa JavaScript execution core; and
- deterministic random seeds and converted-key identities.

Host adapters own document graphs, property types, input construction, and
owner-local clocks. Boa's loop-iteration, recursion, and VM stack limits are
defensive execution bounds; they are not a wall-clock or heap sandbox.

This package makes no claim of Adobe fidelity. Each format adapter is responsible
for validating and documenting the behavior of its conversion.
