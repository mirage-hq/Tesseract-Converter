# Range Selector enum storage regression

Independent Adobe-native source, not converter-generated. Managed `Client.run_jsx`
request `accuracy-w03-selector-enum-20261004-v1`, job
`34b9ca9ee18a490882688c976b455634`, AE26.5x89; successful cleanup/fresh READY.
`author.jsx` is the exact supplied feature body. No source/private49 media reused.

`source.aep` SHA256 `af930a26ff398e33be705b2677bc977612a4887fd474976ef59513dd6188fa6d`,
composition1 `W03 selector enums`, 320x180, square pixels,30fps,1s, one ArialMT
Text layer with editable Position animator and Range Selector. Author readback:
Units2 (Index), BasedOn3 (Words), Mode2 (Subtract), Shape4 (Triangle).
Actual native input edit BasedOn3→4 (Lines); saved source/readback retains4.

`writer::text::tests::native_range_selector_enums_match_adobe_source` compares
all four native enum descriptors, storage selector and explicit values with fresh
export property records at24fps and30fps clocks. Only the owning property clock
may differ. Mode's native flags0x20004 differ from the other enums'0x20000.
Ordinals are unchanged; no guessed bounds/clamps or full-source replay.

This is independent native author/readback and structural export evidence, not
native acceptance of a generated AEP. Current-main Text acceptance fails with
`Error reading the text layer. Skipping the text layer.` on the prior minimal
baseline/candidate probe; that prerequisite is not repaired here. No native
export edit response,30fps reference MP4,long-term Asset,RGB/alpha/audio comparison
or original49 render is established. Import implementation remains unchanged.
