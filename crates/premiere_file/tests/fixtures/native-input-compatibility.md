# Native input compatibility excerpts

These fixtures contain unchanged records extracted from independently authored
Premiere projects in the local intake corpus. No Adobe UI, open, playback or
render was performed for this change. They establish saved syntax and structural
reader behavior, not visual fidelity or complete conversion of these projects.

## Plain XML framing

`native-plain-xml-sequence.prproj` keeps the original XML declaration,
`PremiereData` opening tag and byte-exact `Sequence` record from:

- Corpus case `141-openbionics-prosthetic-hands`.
- `openbionics-prosthetic-hands/OpenBionics-Prosthetic-Hands-639aca3/Assembly Guide/tex/figures/paper_images/Untitled.prproj`.
- Original: 320370 bytes, SHA-256
  `00399249776d0274464c96d58c4f5b590857b9925396e97c35a2836d4d4d8a73`.
- Sequence `5944a579-ddf0-4cd0-a38a-aab6bf1f2819`, name `Sequence 01`.
- Fixture: 2475 bytes, SHA-256
  `acc294eb53f538933cfaf2c32f31850e2dd4fc0c26fb2cace2f7817b6b4a2361`.

Other root records were removed. The sequence's references remain unchanged and
can be dangling in this discovery-only excerpt; it is not an importable media
timeline. `native_plain_xml_and_gzip_list_the_same_sequence` reads the native
plain bytes, then the same bytes in gzip, and checks the selected identity/name.
The baseline reader fails on the native plain form with `invalid gzip header`.

The unchanged complete corpus cases 066/067 (Apress AlphaEx CS3), 074/075 (H3VR)
and 141 reproduce the same framing failure. Reading plain XML does not give CS3
sequences missing an `ObjectUID` a selectable GUID or unlock unsupported content.

## Zero rendered-to-original offset

`native-zero-rendered-offset.xml` is the byte-exact `Node` of master clip
`de9e23ae-1915-4de4-ac14-508fdf81c481`, `Logo Placeholder`, extracted from:

- Corpus case `106-mixkit-541`, `mixkit-541/541/Corporate Logo.prproj`.
- Original gzip project: 98106 bytes, SHA-256
  `78457d777ca8469b059c429a6b24d55dfa23f467995291f597eb1ed3cb1777e6`.
- A newline was added after the extracted record; record bytes were unchanged.
- Fixture: 163 bytes, SHA-256
  `3c15da867e1b37d4378de8a2ff32168600bb521c8f8d55148317bc4c662b703a`.

The original master lists AudioClip 42 → AudioSequenceSource 77 and VideoClip
43 → VideoSequenceSource 80. Both refer to the original `Logo Placeholder`
sequence. The stored `BE.MasterClip.Rendered.OffsetToOriginal` is literal `0`.
All fourteen occurrences of this field in cases 106, 135 (`motionarray-293393`)
and 139 (`motionarray-93438`) are literal zero; the first two projects use sequence
sources, and the third uses legacy title `VideoMediaSource` records. The other
original hashes are:

- 135, `motionarray-293393/Big Stretch Titles_1/Project/MA_ Big Stretch Titles_1.prproj`:
  122755 bytes, `34ba2991a27b6c57b3654e9b1e4a6a9adb1a9d3ca1318c7cf8718e4056661774`.
- 139, `motionarray-93438/Travel Stories/Adobe Premiere/Travel Stories_1.prproj`:
  129424 bytes, `65d54d3e32b81bab9fe4aa404d83924c2e93135631c528203e4673efc91416ce`.

`native_zero_rendered_offset_reads_but_does_not_serialize` fails on the unchanged
native record before the fix. Only literal zero is admitted: no source-time
translation is required. Nonzero, malformed and unknown properties remain
unsupported. The field is not replayed into a newly authored export.
`zero_rendered_offset_keeps_occurrence_ranges_and_source_identity` binds this
unchanged Node into the synthetic one-clip graph as supplementary evidence:
master In/Out marks cannot shift or constrain the occurrence, and a mismatched
master source still rejects. This is not proof of nonzero render-and-replace
semantics or native title/sequence fidelity.
