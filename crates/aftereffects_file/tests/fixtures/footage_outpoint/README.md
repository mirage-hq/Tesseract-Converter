# Footage outpoints beyond composition duration

Independent Adobe-authored control, not converter output. `author.jsx` is a
managed `Client.run_jsx` function body; never invoke it through direct Adobe
transport. It imports an opaque 32x32 PNG, a 13-second 30fps 32x32 MP4, and a
13-second mono 48kHz PCM16 WAVE. These synthetic inputs carry no private content.
The native AEP references retained worker snapshots; media pixels are not used
by this CPU clock regression. This is control-readback proof, not an image,
alpha, audible-audio or render-fidelity reference.

- Native application: After Effects 26.5.0.89.
- Managed request: `w09-p038-outpoint-native-control-v1`.
- Native job: `9c6179d5b65c4d44b9394b1ea95dad29`; cleanup and fresh READY passed.
- Source: `beyond_duration.aep`, 108303 bytes, SHA-256
  `98f7471fb25d545351fe56f6462a36d12e4536487a93d775928f6c6699f583d4`.
- Composition ID 1, `W09 P038 beyond-duration footage native control`, 32x32,
  square pixels, 30fps, duration 365/30 = 12.1666666666667 seconds.
- Three imported layers named `image`, `movie`, `audio`; source IDs 13, 15, 17.
  Source dimensions/type/range, composition duration and start/in points are
  independent native observations, not inferred from our reader.
- Native authored outpoint 12.167 seconds survives save/reopen as
  **12.1669921875**, still strictly beyond composition duration. All start/in
  points remain 0. Movie and audio source durations remain 13 seconds.
- Actual native edit: all three outpoints changed to **12.2**, saved/reopened,
  read back; restored to 12.167, saved/reopened and read back again. Root duration,
  source identities/ranges and start/in points remain unchanged. The committed
  AEP is the restored independent source, not the edited variant.

Input SHA-256 values are recorded in the task's immutable managed request and
receipt. Reproduction requires generating equivalent small synthetic sources and
supplying them as hash-pinned `AssetInput`s named `image`, `movie`, `audio` to the
managed author script; it does not require original P038 campaign assets.
No expected/reference movie is committed or claimed.

P038 motivated the regression: authored 12.167s rounds to 365 frames at 30fps.
The old validator incorrectly dropped entire footage occurrences ending at
12167ms because their native outpoints exceed the rounded composition duration.
Adobe supports these ordinary outpoints; its composition render range need not
contain every layer's authored active range. Preserve the layer/source clocks,
not a clipped or shifted substitute. Native sampling is deliberately absent.

Generated acceptance initially remained incomplete. Reviewer request
`r01-pr4913-generated-accept-tick-v1` (native job
`bea52e4158c24e9daae9eb55bf480a63`) replaced exact outpoint equality with the
native 24,576-Hz tick comparison and used manual result serialization. It failed
with `Object is invalid`; cleanup and fresh READY passed. The final target still
referenced a composition handle invalidated by close/reopen. Reacquire that handle
before a separately authorized retry. No raw phase result or acceptance PASS is
claimed for this failed request.

The separately authorized corrected request
`r01-pr4913-generated-accept-tick-v2` (native job
`f9fb7d3184574a36832e0cc852f178fa`, queue 2435) passed with verified READY.
Raw before/reopened image/movie/audio outpoints were
`[12.1669921875, 12.167, 12.167]`; edited values were `[12.2, 12.2, 12.2]`;
restored values were `[12.1669921875, 12.1669921875, 12.1669921875]`.
Offline validation of all returned phases confirmed unchanged root FPS/duration,
layer/source identities, source durations/types and start/in points, with every
outpoint within one 24,576-Hz tick of its authored expectation and beyond the root
duration. This closes generated editable acceptance, not render/alpha/audio proof.
