# Native AEP source collection for the #4379 feature inventory

## Source inventory, not current converter support
Source-collection milestone only: Adobe-native `.aep` sources for the implemented import controls in #4379. User accepts grayscale gradients; custom color-stop and nonuniform-alpha cases are not required for this milestone. Time Remap remains explicitly deferred. This is not an import/editability/render-fidelity assessment.

The 37 merged-ledger categories are reconciled below. 32 source-bearing categories have identified native cases; five categories are deferred/excluded or converter safety rather than authorable render features. The final authoring batch supplied the 11 missing mapped-control cases (three parametric reverse directions, keyed Gradient Stroke Miter Limit, frame-blend master-off, six selector animation targets). No known missing named implemented control remains in this reconciliation. This is not every malformed-input boundary, arbitrary cross-product, or approximation case.

All paths below are relative to `crates/aftereffects_file/tests/fixtures/`. Numbers are composition item IDs in those native files. Files may contain multiple independently named cases. Existing sources are reused where identified; no requirement for one file per enum or one source per test assertion. Source metadata comes from Adobe authoring reports and existing pinned sidecars, not from running the converter.

This inventory was authored against #4379. The PR now uses main's #4422 implementation unchanged; source availability does not override its support or omission policy. In particular, native Shape/Mask Path keyframes are currently omitted with diagnostics, not implemented by this PR.

## Feature → source → target

| # | Feature | Native AEP and composition IDs | Status |
|---|---|---|---|
|1|Items/folders, comp instances, nesting, order|`layers/import_precomp_structure.aep`: source1 in SOURCE_FOLDER, two instances17, nested31, sibling order44|Source ready|
|2|Solid RGB / source dimensions|Existing `render/export_static_color.aep`: comp1 layer17, RGB[.875,.3125,.0625],448×192; `render/solid_color_1080.aep`: comp1 layer15|Source ready|
|3|Static Transform|`properties/import_transform_components.aep`: AnchorX1/Y18, PositionX34/Y50, ScaleX66/Y82, RotationZ98, Opacity114, separatedPositionX130/Y146, 3DPositionZ162, RotationX178/Y194, OrientationX210/Y226/Z242|Source ready|
|4|Numeric animation / interpolation / clocks|`properties/import_numeric_animation_cases.aep`: Opacity Linear1/Hold18/Bezier34, Anchor50, Position66, Scale82, RotationZ98/X114/Y130, Orientation146/162/178, PositionZ194, separatedXYZ210/226/242, spatialBezier258; `properties/import_temporal_clock_cases.aep`: identity1,start/stretch16,reverseBezier30,shape44,mask57|Source ready|
|5|Orientation|Static `properties/import_transform_components.aep`210/226/242; keyed `properties/import_numeric_animation_cases.aep`146/162/178|Source ready|
|6|Start/In/Out/Stretch|`layers/import_timing_controls.aep`: Start1,In18,Out34,Stretch50,Reverse66; clock cases in row4. AE setters can couple bounds; these are control cases, not one-serialized-field mutation claims|Source ready|
|7|Time Remap|No new source required under explicit user defer|Deferred|
|8|Transform parenting|`parenting/import_parenting_cases.aep`: parentPosition1,Scale20,Rotation38,two-nullChain56,solidParent76|Source ready|
|9|Layer blend modes|Existing `compositing/blendingMode.aep`: Add30/Multiply1/Screen16; `compositing/import_remaining_blend_modes.aep`: Normal1,Overlay18,SoftLight34,HardLight50,Darken66,Lighten82,ClassicDifference98,Hue114,Saturation130,Color146,Luminosity162,ClassicDodge178,ClassicBurn194,Exclusion210,Difference226,Dodge242,Burn258,LinearBurn274,LinearLight290,Vivid306,Pin322,HardMix338,LighterColor354,DarkerColor370,Subtract386,Divide402|All 29 modeled layer modes sourced|
|10|Shape group / paint / Boolean paint blend owners|`shapes/import_shape_blend_ownership.aep`: Group1,Fill17,Stroke32; `shapes/import_boolean_paint_blend.aep`: BooleanFill1/Stroke17 (Multiply)|Source ready; owner cases, not all-mode × owner cross-product|
|11|Track matte|`compositing/import_track_matte_cases.aep`: Alpha1,AlphaInverted20,Luma38,LumaInverted56|Source ready|
|12|Motion blur / shutter|`compositing/import_motion_blur_cases.aep`: masterOff1,layerOff18,bothOn34,shutter270/phase−90:50|Source ready|
|13|Eye/audio/solo/guide|`layers/import_layer_switches.aep`: EyeOff1,Solo18,Guide34,AudioOn51/Off64; `media/import_audio_media_controls.aep`: AudioOff78,VisualOff93|Source ready|
|14|Native shared static paths|`shapes/import_path_direction_cases.aep`: openForward1/openReverse17,closedForward32/closedReverse47,sharedPathTwoPaints92|Source ready|
|15|Path animation|`path-animation/import_path_key_cases.aep`: Linear1,Hold17,Bezier32; `masks/import_mask_controls.aep`: keyed mask path226|Source ready; current main omits native Shape/Mask Path animation with diagnostics|
|16|Rectangle|`shapes/import_rectangle_controls.aep`: Size1,Position17,Roundness32, keyedSize47/Position62/Roundness77; existing `geometry/geometry_probe.aep` comp14 layer40 static100×50|Source ready|
|17|Ellipse/Star/Polygon|`shapes/import_parametric_shape_controls.aep`: EllipseSize1/Position17; StarPoints32/Position47/Rotation62/InnerRadius77/OuterRadius92/InnerRoundness107/OuterRoundness122; PolygonPoints137/Position152/Rotation167/Radius182/Roundness197. Keyed: `shapes/import_shape_control_animation.aep` Ellipse167/182,Star197/212/227/242/257/272/287; `shapes/import_polygon_animation.aep` Points1/Position14/Rotation27/Radius40/Roundness53|Source ready; fractional outline fidelity excluded|
|18|Direction / rectangle traversal|`shapes/import_path_direction_cases.aep`: open1/17,closed32/47,rectangle62/77; `shapes/import_remaining_mapped_controls.aep`: EllipseReverse1,StarReverse17,PolygonReverse32|Source ready|
|19|Fill/Stroke/Gradient controls|`shapes/import_solid_paint_controls.aep`: FillColor1/Opacity17,StrokeColor32/Width47/Opacity62,Cap77/92/107,Join122/137/152,Miter167. `shapes/import_stroke_dash_caps.aep`: open caps1/17/32,acute joins47/62/77,miter92/107,dashGap122,offset137,keyedOffset152. `shapes/import_shape_control_animation.aep`: keyed FillColor1/Opacity17,StrokeColor32/Width47/Opacity62/Miter77. `shapes/import_gradient_controls.aep`: FillLinear1/Radial17,StrokeLinear32/Radial47,start62,end77,opacity92,width107. `shapes/import_isolated_native_gradients.aep`: nativeFill14/Stroke27. `shapes/import_gradient_stroke_details.aep`: Opacity1,DashGap14,Offset27,keyedOffset40. `shapes/import_remaining_mapped_controls.aep`: keyedGradientStrokeMiter47|Source ready; grayscale accepted, nonuniform alpha not claimed|
|20|Ordered paint ownership / composite order|`shapes/import_shape_flags_order.aep`: FillThenStroke77,StrokeThenFill92,prefixPaint107,Composite1:122/2:137; `shapes/import_modifier_order_cases.aep`: partialRound17,nestedOwnPaint107; sharedPathTwoPaints row14|Source ready|
|21|Vector enable flags|`shapes/import_shape_flags_order.aep`: GroupOff1,PathOff17,FillOff32,StrokeOff47,ModifierOff62|Source ready; malformed/collapse-header proofs not required|
|22|Text enable flags|`text/import_text_enable_cases.aep`: AnimatorOff1,RangeOff14,PathOptionsOff27|Source ready|
|23|Repeater|Explicitly deferred by #4379 scope|Excluded|
|24|Shape group Transform / opacity|`shapes/import_group_transform_controls.aep`: static/keyed pairs Anchor1/17,Position32/47,Scale62/77,Rotation92/107,Skew122/137,SkewAxis152/167,Opacity182/197|Source ready|
|25|Round/Offset/Trim|`shapes/import_modifier_controls.aep`: Round1,Offset17,TrimStart32/End47/Offset62; `shapes/import_shape_control_animation.aep`: keyedRound92,Offset107,TrimStart122/End137/Offset152; `shapes/import_modifier_order_cases.aep`: repeatedRound1,partial17,nested32 plus diagnostic mixed stages62/77/92|Source ready|
|26|Individual Trim|`shapes/import_modifier_controls.aep`: complete-set Individual77; `shapes/import_modifier_order_cases.aep`: prefix diagnostic47|Source ready|
|27|Merge Paths|`shapes/import_modifier_controls.aep`: Append92,Union107,Subtract122,Intersect137,Exclude152; Boolean paint row10|Source ready|
|28|Shape bounds / command and memory budgets|Converter safety limits, not additional editable Adobe feature|Not an authoring requirement|
|29|Masks|`masks/import_mask_controls.aep`: Add1/Subtract18/Intersect34/Difference50/Lighten66/Darken82/None98,Inverted114,Feather130/Opacity146/Expansion162,keyedFeather178/Opacity194/Expansion210/Path226; mask clock row4|Source ready; extra source modes may be approximated|
|30|Intrinsic Text|Detailed field and selector map below|Source ready|
|31|Essential Properties|`essential/import_occurrence_overrides.aep`: Opacity source1/override16,Position32/46,Rotation62/76; `essential/import_color_nested_overrides.aep`: Color1/14,multiple28/43; `essential/import_nested_override_precedence.aep`: source1,middle16(55),outer29(25 plus unchanged sibling)|Source ready; source-path types/precedence, not every Transform × override permutation|
|32|Essential media replacement|`media-replacement/import_media_replacement_cases.aep`: source3,replacedOccurrence16,existing blue→red source and untouched sibling|Source ready|
|33|Image/video/audio and frame blending|`media/import_image_source_controls.aep`: Image3,reused16,distinct30; `media/import_audio_media_controls.aep`: Audio2 and VideoNoBlend109/FrameMix124/PixelMotion139; `media/import_frame_blend_master_off.aep`: masterOff2|Source ready|
|34|Local assets / publication inputs|Image reused/distinct references row33; WAV/MP4 references rows32/35|Source inputs ready; filesystem failure/publication algorithms are not separate AEP features|
|35|Audio gain / mute|`media/import_audio_media_controls.aep`: Unity2,Left18,Right33,Stereo48,keyed63,AudioOff78,VisualOff93|Source ready|
|36|Effects/Layer Styles|Excluded by user|Excluded|
|37|Camera/Light/mesh/AE expressions|Unsupported engine/runtime content|Excluded|

### Row 30: Text field targets
- `text/import_text_document_controls.aep`: point1,box17,fontSize32,fillColor47,fillOff62,strokeColor77,width92,strokeOverFill107,justifyLeft122/Center137/Right152,tracking167,manualLeading182,baseline197,SourceTextHold212. Font identity is explicitly ArialMT throughout, not inferred from names. A contrasting-font performance/fidelity test is not part of source collection.
- `text/import_text_additional_controls.aep`: AllCaps1,anchorAlign17/keyed32,pathFirstMarginKeyed47/Last62,autoLeading77,boxSize92,fullJustifyLastLeft108.
- `text/import_full_justification_variants.aep`: fullJustifyLastCenter1/Right17/Full32.
- `text/import_text_animator_channels.aep`: static/keyed pairs Anchor1/17,Position32/47,Scale62/77,Rotation92/107,Skew122/137,SkewAxis152/167,Tracking182/197,StrokeWidth212/227,Blur242/257,Opacity272/287,FillColor302/317,StrokeColor332/347,LineSpacing362/377,LineAnchor392/407,CharOffset422/437,CharReplace452/467.
- `text/import_text_selector_controls.aep`: PercentageStart1/End17/Offset32,IndexStart47/End62/Offset77,Amount92,EaseHigh107/Low122,RandomOrder137/Seed152,Shape1..6 at167/182/197/212/227/242,Mode1..6 at257/272/287/302/317/332,BasedOn1..4 at347/362/377/392,WigglySpeed407/Max422/Min437/Seed452/Mode467.
- `text/import_selector_animation.aep`: keyed PercentageStart1/End14/Offset27,IndexStart40/End53/Offset66,Amount79 (Adobe authoring report).
- `text/import_remaining_selector_animation.aep`: keyed RangeEaseHigh1/Low17/RandomSeed32,WigglySpeed47/Amount62/Seed77.
- `text/import_selector_order_cases.aep`: twoRanges1,Range→Wiggly17,Wiggly→Range32 (ordering approximation recorded in importer, not claimed faithful).
- `text/import_text_path_options.aep`: Path1,FirstMargin17,LastMargin32,Perpendicular47,Reverse62,ForceAlign77,AnchorGrouping1..4 at92/107/122/137.

## Evidence and exclusions
- [import_sources.json](import_sources.json) pins paths, byte counts and SHA-256 hashes of the 46 grouped native sources. Composition IDs above come from the original Adobe authoring reports, not from a converter run during cleanup.
- The additional 16 earlier Adobe-native sources and their sidecars are documented in [render/README.md](render/README.md). The [AE expected-video manifest](aep_video_references.json) records 30fps Adobe reference generation and long-term Asset publication for all 415 compositions, including replacements for the 11 legacy local 24fps videos. Only entries marked `verified` have completed publication/download verification. Video binaries are not tracked in Git. The 16 import assertions are retained separately from the 46-file collection; those assertions do not cover every case in this table.
- This PR adds source/test data only. Main's landed importer, writer, CLI and support policy are unchanged. See [the current support ledger](../../../../docs/after-effects-support.md).
- Accepted exclusion: custom-color/nonuniform-alpha gradient UI authoring. Other exclusions: Time Remap, Repeater, Effects/Layer Styles, unsupported runtime features; malformed graph/enum, budget/error and full source×modifier×clock permutations are not native feature authoring requirements.
- Font/media portability remains unestablished. Shared asset references may contain absolute checkout paths; this collection is not a self-contained media bundle. Text sources use ArialMT; availability on other hosts is not established.
- Source presence does not establish conversion, editable-value, rendering, alpha or audio results. No execution gate or new assertion for the 46-file collection was added during this reconciliation. Import assertions, missing visual/alpha/audio proof and portable media remain separate work.
