# Native Black Video occurrence

`occurrence-1172.xml` retains 11 unchanged top-level records from an independently
Adobe-authored Premiere project. No customer media or other production content is
included. Four minimal surrounding sequence/track records are a synthetic test
harness; their canvas and rate copy the source VideoTrackGroup 787. The harness
selects only occurrence 1172, not the full native sequence.

- Original compressed project SHA-256:
  `dff8bc0a167fb5882413a1190d819f9e530cc038334d04f9676a749b11806721`.
- Original decompressed XML SHA-256:
  `af7dce057ab5ec45ef16438a95a0b83e48af609ae3ddf5131c0d8ddbcd885c05`.
- Derived fixture SHA-256:
  `826f4a1f034272a74b6f5f545f7ce427f294cf90a25439c92cbfe9ee3affcdb7`.
- Source sequence UID: `d40c25d5-0359-473e-974e-24f423007763`.
- Native record IDs: 71, 72, 73, 389, 930, 1172, 1374, 1375, 1671;
  MasterClip `2160ab06-9d6a-4b1a-ad9a-9ffdc9e5e81b`;
  Media `e2bcda88-84c4-4736-b083-7430bd7d928e`.

The Media record has the shared generator ImplementationID
`42008e7a-de6f-4270-96de-7e287abb9b4b`, `FilePath` and `ActualMediaFilePath`
`1112293707` (`BLAK`), `Infinite=true`, no ImporterPrefs, no file or audio.
VideoStream 930 is `IsStill=true`, 2160×3840, FrameRate 8475667200 ticks/frame
(30000/1001 fps), Duration 10973491200000000 ticks (12 hours). The source and
sequence clocks are separate records with the same rate. Occurrence 1172 spans
7662003148800..8094262176000 timeline ticks (frames 904..955), with source range
914456685542400..914888944569600. Its chain 1374 saves default Motion and Opacity;
neither placement nor master is an adjustment layer. Existing import diagnostics
retain the occurrence's unconverted new-color-management flag and nondefault tone
mapping metadata; they do not omit its solid.

Tests assert native identity/ranges, the editable solid and canonical identity
transform, and fresh conversion/export of current edited content. Supplemental
XML mutations of this fixture test static opacity and malformed generator
admission. Separate synthetic `adjustment_xml` regressions test adjustment flags;
these mutations and synthetic controls are not independently authored. Black
Video exports via the existing Color Matte writer, not by replaying native
records. Nondefault Motion,
keys and other edits retain the existing matte limitations. Adobe UI inspection,
current-candidate reopening, independent render fidelity and alpha comparison are
unrun/unmeasured; this fixture is structural evidence only.
