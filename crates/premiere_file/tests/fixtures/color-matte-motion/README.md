# Color Matte Motion host regression

`native-controls.xml` derives intrinsic Motion/Opacity records from pinned public
Adobe-native fixtures, not the converter writer:

- `feature_motion_opacity_26_5_strict.prproj`, SHA256
  `a8a966779cf61d4e2547d011b465a791d8b4b7f0a4c8bb06889551eb28df2ff0`,
  components211/212 and their parameter records.
- `feature_motion_position_path_adobe.prproj`, SHA256
  `7608e9de691406509b4c48251206051d5dbf0f773d912b2d7fa1e498ce54ccdc`,
  component111 and its parameter records.

Only ObjectID/ObjectRef values are increased by10000 and key timestamps are
translated to the generator origin914457600000000. The spatial case subtracts
its original914449132800000 origin first. Values, interpolation and handles are
unchanged. Tests attach these records to the red Color Matte of the pinned
`feature_color_matte_strict.prproj` (SHA256
`4449d85b321b65d085dc47c9a3d444b70e9574f8b60c74b84bf975c0ae222d24`,
sequence `c8acf9c1-34b2-4086-9f55-d528950a7059`) through the normal importer.
The sharp Crop controls reuse the separate public native Crop fixture.

This is native-derived editable-structure evidence for the host mapping, not
an independently Adobe-authored animated-matte case, Adobe UI inspection or new
native RGB/alpha fidelity proof. No native reference is replaced or promoted.
