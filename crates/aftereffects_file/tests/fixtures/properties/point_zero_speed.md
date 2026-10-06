# Independent native zero-speed Point control

Adobe AE26.5x89 managed `Client.run_jsx` author/save/reopen, job
`3ff0735957534a60a6c9e69758f0669b`, request
`accuracy-w02-point-zero-speed-20261004-v1`; successful cleanup/READY.
Source SHA256 `340f2fd47d09d9452ae1c511847d511ca222787b8611735e7f99f4b49ab81ef6`.
Script SHA256 `1aba957d0d9815d1b59a93ee4df1fb5732433fb2f446524a6899c0aab219c1cf`.
The JSX is a managed function body, not a standalone Adobe runner. Execute only
through the supported managed API. No external assets or private source are used.

Pinned composition ID1/name `point-zero-speed-controls`,160x120,24fps,2s;
one `point-owner` solid120x80 and Radial Blur Point `ADBE Radial Blur-0002`.
Two native Bezier keys at0.5/1.5s ([500,1500] FX milliseconds), values
[60,40]/[72,33] pixels; zero incoming/outgoing speed and spatial tangents,
100/3 temporal influence. Reopened native samples:

| Seconds | Point pixels |
|---|---|
|0,0.5|[60,40]|
|0.75|[61.8750201311365,38.906238256837]|
|1|[66.0000932824578,36.4999455852329]|
|1.25|[70.1250018808083,34.0937489028618]|
|1.5,1.75|[72,33]|

Native sampling contains Adobe's numerical spatial evaluation; do not replace
these readbacks with exact analytical values or call them RGB render proof.
CPU assertion: `point_zero_speed_native_source_exports_and_edits_without_static_fallback`.
Fresh import asserts normalized scalar controls/time/easing; full fresh export
and end-point input edit assert keys survive, without original AEP replay.
Generated-output independent Adobe acceptance/readback completed in managed
job `bfc8cc763ef5403987f1bed1b6484cfa`, cleanup/READY succeeded. The sanitized
`point_zero_speed.readback.json` retains native-source and both generated-output
controls/samples and hashes. Exact generated CompItem IDs1/13/name, Point matchName,
120x80 owner,500/1500ms keys, zero speeds/one-third influence and end-point edit [72,33]→[96,56]
were independently read. Unused boundary interpolation flags are normalized.
30fps native reference,long-term Asset,RGB/alpha/audio fidelity and original49
comparison remain unmeasured. Public fixture/native control acceptance alone is
not a completed feature test; Radial Blur kernel/mode remains approximate.
