# B-frame presentation-origin export control

This is an **FX → AEP media-admission repair**, not a P043 fidelity pass.
AEP → FX import behavior/proof is unchanged. `movie.mov` is a public, newly
generated moving test input, not expected/reference video and not a transcode
of any private source. No original49 footage or references were changed.

## Pinned source and native control

- `movie.mov`: SHA256 `5f0894d6b46532e36052ad5ba230a615e002823e6e619fc12ad6eb5b59b7bd1a`,
  H264,160×90,24fps,24frames,1s,2 B-frames. Generation command (FFmpeg7.1.5;
  byte identity is pinned, not promised across encoder versions):

  ```sh
  ffmpeg -f lavfi -i 'testsrc2=size=160x90:rate=24:duration=1' -an \
    -c:v libx264 -crf 18 -bf 2 -g 24 -sc_threshold 0 -pix_fmt yuv420p movie.mov
  ```

- `native-control.aep`: independently authored via managed `Client.run_jsx`,
  using pinned `author.jsx` and this exact MOV, **not our AEP writer**.
  SHA256 `d4f1ec2351cf22e11ba7ec9de59631c08c73cce28e931c445097d92c7e55dd64`.
  AE26.5x89; native job `1b71059c835e49109a3d988705c7e151`;
  cleanup/fresh READY verified. `provenance.json` retains exact source/script
  hashes, target IDs and native control readback.
- Original target2 (`B-frame original`):1s/30fps,layer0–1s,start0,stretch100%.
  Edited target15 (`B-frame edited rate`):.5s/30fps,layer0–.5s,start0,stretch50%.
  Both use the same complete1s footage,24fps,conformFrameRate0.
  Both targets support the single executable export assertion below; neither
  is silently counted as an independent RGB-reference case.

## Runnable regression and actual edit response

```sh
make -C opensource/conv test-aftereffects-file filter=b_frame
make -C opensource/conv test-aftereffects-file filter=presentation
```

`b_frame_mov_exports_unchanged_and_responds_to_authored_rate_edit` freshly
packages this MOV through `TesseractFileBuilder` and exports **two explicit FX
inputs**: full-source original1s and edited.5s. It asserts one editable Video,
source24fps/full1s, untouched packaged media bytes, exact layer source clock,
and native stretch1/.5, checked against the independent100%/50% readback.
Source MOV and native AEP hashes are checked. There is no flattening or media
sample edit. Existing source-zero rounded-millisecond visibility and zero-gain
diagnostics remain explicit; their behavior is outside this metadata repair.

The metadata regression was RED on current base (job1308:3 failed) and GREEN
(job1320). The **same pinned MOV export assertion** was also RED on exact
base7d720ad45 plus only test/fixture additions (job1440, native-case omission:
`QuickTime edit lists are not representable by the native source profile`),
then GREEN with the repair (job1389 and the branch's final CPU run1413).
The final suite's12 unrelated failures reproduced identically on untouched
base (job1423): branch1655passed/base1650passed, both12failed/409ignored.
Formatting and converter-workspace clippy passed (job1425).
Malformed/unsupported tables retain targeted exclusions: shifted edits,
missing/extra/duplicate samples, presentation holes, signed offsets, nonzero
flags, empty runs, invalid lengths and duplicate `ctts` atoms.

## Native generated-output evidence and limits

One fresh **edited FX export** from job1389 opened/rendered through managed
`render_aep`, job `d65e5b71310c4698a6d95af107c7648f`, with cleanup/READY.
AEP SHA `939f8c827a2578c77dcec7b120be748a952cbe3fae08d4732c28a2c3195ed46a`;
MP4 SHA `f98c6991e27aa1dd7ddf5aac241db6591c4720d47767257d0ff7d23ad10571fa`.
Output160×90/30fps,15decoded frames/full.5s,audiooff. The explicit FX position
[80,45] offsets the source from centered coordinates, so only its top-left
80×45 region is visible at the output's bottom-right; **full-canvas fidelity
was not established**. Diagnostic frame identification in that region found
source frames0,1,3,4,6,8,9,11,12,14,16,17,19,20,22 at output frames0–14,
consistent with the declared2× rate. This is sample identification/native
acceptance, **not** a canonical RGB24/.99 similarity pass. No scoring threshold
or reference was changed.

Reviewer follow-up at `7d3efb1f572c56e5cb4a9561229a41e07e8e950b` changed only
the explicit FX proof input to identity position[0,0]/anchor[0,0], preserving
full-canvas placement. Queued targeted tests1742/1743 passed and freshly exported
both inputs. Managed readback `737fe6bbd84b402ba1d215d4b8d0bc36` compared these
outputs with the unchanged independently authored source: full1s24fps footage,
original1s/100% and edited.5s/50%, online media, native30fps composition and
full-canvas position-minus-anchor[0,0]/scale100/rotation0 all held. Generated and
native controls use different but equivalent anchor origins. An earlier overly
specific anchor assertion failed in job6614cb1a61ac4c29836742ff145c636d with
verified READY; that invalid proof attempt is retained, not a product failure.

Managed render `ef0eb38e3f0c4afbb1727f8f886fefc5` produced the complete generated/
independent panel (3s,90frames,30fps,MOV,audiooff, cleanup/fresh READY). MOV SHA256
`7e48edc4bdebf9093bb0febcc08bc7ec9f5094f5bc48873e3bc7988fef22de5b`.
All corresponding full-resolution canonical decoded RGB24 frames were inspected:
30 original and15 edited, each nonblank/moving. Maximum channel difference was
1LSB, **not byte equality**. The unchanged `rgb-hybrid`/.99 frame-image gate
**FAILED**: original mean.923735/min.916603 (30/30 below.99), edited
mean.923948/min.917811 (15/15 below.99). The pinned validation tool SHA256 is
`efe27bf526803a2dab9ccf1ac7d3f898e2038abd5fc30201f5515e4935b8bae4`.
No threshold, sampling oracle or media bytes were changed; native acceptance and
correct full-canvas clocks are separate from this unresolved strict RGB result.

Independent native-reference rendering is now executed locally, but strict RGB
is **failed**, not passed. Long-term Asset publication/fresh hash-verified download,
alpha, audio and generalized frame-selection equivalence remain **unverified**. The output MP4 is
ignored local evidence, never Git. Private P043 remains blocked on current main
by unrelated MP4 source admission; this PR does not duplicate4807 or claim
P043 fixed. Native rate/duration controls of its untouched MP4 are supplementary
only and cannot replace this distinct MOV case's missing RGB reference.
