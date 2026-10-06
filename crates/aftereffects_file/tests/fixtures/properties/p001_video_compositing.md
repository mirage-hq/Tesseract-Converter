# P001 Video luma / inverted-luma / Screen controls

Independent managed AE26.5x89 author/save/reopen job
`025a96b0d62e4948850a8bd5c533887d`; source SHA256
`653e0fcdbd8d580689481cbde1241b17f38fedc1baac227c1622fc34c6e0731b`.
Exact CompItem ID2/name `p001-video-compositing`, 320x180, square pixels,
24fps, full2seconds, five layers. Public source movie:
`../media_native_panel/media/movie.mov`, SHA256
`a383d0058de7ce9723e263611acd35a438cf1154fbbfe8e0f1a1f78e74546292`.
The accompanying managed JSX authors only private context paths; never invoke
it outside the headless-adobe managed API. Relink to the same public movie bytes.

Native numerical readback: Luma video uses LUMA5015 / Luma matte;
Inverted video uses LUMA_INVERTED5016 / Inverted matte; providers disabled;
Screen video uses SCREEN5222. Luma video has Linear Opacity100@0s→50@1s,
with midpoint75. No image inspection/model scoring was used.

Executable source-hash/native-read/fresh-import assertions:
`export_document::tests::pr4442_media_cases::p001_video_compositing::p001_native_video_luma_screen_source_and_edited_export`.
Import implementation unchanged: imported occurrence wrappers retain editable
Luma/LumaInverted/Screen semantics. Export regression uses explicit current FX
Video owners and Rect providers for those independently established native
controls; fresh native output retains footage links and disabled matte providers.
This is not donor replay or flattened output. RED2309 omitted all3Videos;
GREEN2318 retained all3 and both editable providers.

Independent generated acceptance job `2c01802224b24895b669d39732fc45b1`
opened freshly generated original and edited CompItem1/`Comp 1`, five layers,
320x180/24fps/2seconds, same hash-pinned public footage. Matte relations,
provider switches, Screen mode and Opacity keys are natively verified.
Actual FX end-value edit50→20 changes midpoint75→60; start100 is retained.
Generated original SHA256
`2e410a0a9a7d96449fa4deeeec6cbeb6c42a3c1211a1586fad3e00139279ff55`, edited
`c2be0a585239f5616f42ae6242ff8215b768cc29e4e3cea352ad78a4aebee752`.
Sanitized numeric readback is in the accompanying JSON. Both calls published
only after verified cleanup and fresh READY.

Actual private P001 full10s/1080x1350/30fps native render job
`dd047178cd1748c7ba95a678b1bb48cc` also returned READY. Full decode300frames
verified. Canonical fullresolution RGB24/rgb-hybrid at0.25s,41samples against
unchanged source reference: before mean0.18347855044688954/min0.1761758728743271,
after mean0.2141610938806818/min0.20201688819476776. Before is the unchanged-main
score-eval8eb7daafe/build2250 cohort; after is latest473da79c5 plus the bounded
admission repair/build2325. This is measured omission improvement, NOT a0.95
or0.99 scene-fidelity pass. CustomShader remains deliberately unsupported.
Private source/media/movie artifacts are not published here.

Remaining proof: no minimal-profile independent30fps render/long-term Asset,
lossless RGB/alpha/audio proof, human UI inspection, or general Video compositing
coverage. Add/AlphaInverted Video profiles and other existing unsupported guards
remain excluded. Structural/native numeric acceptance is not visual equality.
