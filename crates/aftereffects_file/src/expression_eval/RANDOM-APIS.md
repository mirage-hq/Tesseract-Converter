# Diagnosed random expression approximations (import only)

`seedRandom`, `random`, `gaussRandom`, `wiggle` and `noise` are admitted only in
our existing numeric property profile. Every invoked API emits an owner/property
`AE-PROPERTIES` **approximated** diagnostic, including random dependencies of a
deterministic consumer. Samples are baked through the existing editable-key route;
no runtime JS is written to FX. Captured Adobe sidecars still take precedence.
Successful evaluation does not guarantee that the existing fitter can represent
the samples; existing unsupported/fitting diagnostics remain authoritative.

These are our own approximations of the AE APIs, **not Adobe's PRNG/kernel**:

| API | Implemented replacement and limitations |
|---|---|
| seedRandom(offset, timeless=false) | FNV-1a hash of native composition/layer/property identity, offset and expression clock seeds a fresh xorshift32 stream. `timeless=true` excludes the clock. Calling again resets the stream. No ambient randomness/state leaks across property evaluations. Seed offsets accept finite numbers; timeless must be Boolean. AE random sequences differ. |
| random(), random(max), random(min,max) | Uniform scalar or component-wise vector interpolation; scalar bounds broadcast over vector bounds. Equal bounds return that value; descending bounds interpolate in descending order. Clock changes (including posterizeTime) reseed non-timeless streams. |
| gaussRandom([min,]max) | Custom Box-Muller normal transform; midpoint mean, standard deviation `abs(max-min)/6`, unbounded tails. Same zero/one/two-argument and vector interface as random. This is not AE's tail distribution. |
| wiggle(freq,amp,octaves=1,amp_mult=.5,t=time) | Custom coherent seeded value-lattice perturbation added to the pre-expression property evaluated at t. Frequency is respected; scalar amplitude, per-component independent offsets; octave frequency doubles and amplitude multiplies. Frequency0 returns the authored value. Nonnegative frequency and integer1..16 octaves are required; excess/unsafe lattice coordinates fail explicitly. Negative times work. Unlike AE fractal noise, this uses smoothstep interpolation. |
| noise(scalar\|[x,y]\|[x,y,z]) | Custom coherent1D/2D/3D value noise in[-1,1], smoothstep interpolation over seeded hash lattice. Independent of expression clock unless supplied in coordinates; seedRandom offset affects the lattice. Not AE Perlin noise. |

Random samples are deterministic for the same source/occurrence inputs, but not
bit-identical to Adobe. Random approximation approval does **not** admit unknown
deterministic APIs, unsupported interpolation, Shape targets, textIndex/textTotal,
per-character selectors or native Math.random. No FX→AEP expression authoring is
added; existing editable-key export is unchanged. Native RGB/alpha/audio fidelity
and independent30fps reference/Asset proof remain unmeasured.
