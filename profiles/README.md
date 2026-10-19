# Owned BT.2100 ICC target-policy profiles

These profiles derive from standards equations and primary/white chromaticities,
**not from Adobe or another vendor's ICC data**. Their generated payloads are
CC0-1.0, as declared by the copyright tags. Generator code uses this workspace's
MIT license. No third-party ICC data is redistributed.

| File | Size | SHA-256 |
| --- | ---: | --- |
| `jerboa-pq.icc` (W203 v2) | 25212 | `0d76b1ceab273f8293a092bb8defd97a1bc4a2386a5e8fcc6b377b4ae5fd0aad` |
| `jerboa-hlg.icc` (W203 v2 float) | 13620736 | `c76f97e1e765af063387ef397feae9f41938841b3c5e334efe780e3ffaa85ed7` |

Regenerate with `python3 profiles/generate_owned_icc.py`; verify without writing
with `python3 profiles/generate_owned_icc.py --check`. Offline controls are in
`scripts/test_owned_icc.py`, included by `make test-offline`. Profile filenames,
embedded descriptions and payload bytes retain their established identities.

## Standards and approximations

- ITU-R [BT.2100](https://www.itu.int/rec/R-REC-BT.2100): PQ/ST 2084 EOTF and HLG
  inverse OETF with the luminance-coupled reference-display OOTF. PQ normalizes
  to 10000 cd/m²; HLG uses 1000 cd/m², zero black and gamma 1.2.
- ITU-R [BT.2020](https://www.itu.int/rec/R-REC-BT.2020): RGB primaries and D65
  white determine RGB→XYZ coefficients. No per-channel HLG power approximation.
- ICC [v4 specification](https://www.color.org/specification/ICC.1-2022-05.pdf):
  D50 XYZ PCS, Bradford adaptation, fixed date, sorted aligned tags, profile ID
  algorithm and lut16 XYZ encoding (32768 represents 1.0).
- **PQ W203 v2:** 4097-entry sampled ST2084 TRCs remain normalized to 10000 nits.
  Adapted XYZ matrix columns scale by 10000/203, so relative PCS Y=1 represents
  203 nits and full-scale PQ has Y≈49.2611. `lumi` is 203; `wtpt` stays normalized
  D50. s15Fixed16 coefficients retain this extended PCS without an HDR clamp.
  Finite 16-bit TRC quantization is approximate (about 0.153 nit per code).
- **HLG W203 v2:** `mpet` D2B0/B2D0 and D2B1/B2D1 cover perceptual/relative
  intent. Forward 65³ float CLUTs evaluate the coupled OOTF; an unclipped matrix
  maps display RGB to D50 XYZ with 1000/203 PCS gain. Reverse transforms use the
  inverse matrix, float square-root curves and a 65³ inverse CLUT sampled on
  squared display RGB. Float PCS retains negative adapted XYZ and full-scale
  Y≈4.926. Interpolation remains approximate; CMM support controls tag selection.
- **HLG integer fallback:** conventional A2B0/B2A0 use 33³ lut16 grids and
  identity 256-entry tables with the same gain/inverse gain. Graphics above
  405.994 nits clip; negative adapted XYZ also clips (8191/35937 forward nodes).
  Export reports this limitation. Full-range fallback fidelity is not claimed.
- **Finite PCS gamut clamp:** unsigned XYZ16 cannot represent saturated BT.2020
  red's negative adapted Z. Only finite coordinates outside [0,65535/32768] are
  clamped before quantization. The original unscaled v1 affected 307/35937 nodes
  near red (minimum Z −0.0014799001216323653). Float transforms do not use this
  clamp. Nonfinite values reject. Reverse transforms clip out-of-device RGB to
  [0,1]. Neither clamp preserves color exactly.

These are not arbitrary-display profiles, an ACES configuration, a source
interpretation policy or proof of linear-light compositing or render precision.
No reference clock, pixel threshold or fidelity gate changes.

## Native calibration — PQ v1 failure and versioned correction

Independent AE 26.5x89 built-in `Rec.2100 PQ W203` controls used opaque .4/.5/.75
solids, 300×180, 30fps, 1s, 32bpc/nonlinear working space and AE→AME ProRes PQ
with white 203. Built-in output Y10 codes 414/502/721 correspond to
32.285806/92.245709/983.377856 nits. Owned v1 instead produced codes 178/226/377
(0.664926/1.892188/20.068045 nits): its `lumi=10000` tag did not establish an
absolute 10000-nit mapping. No vendor ICC bytes were extracted or redistributed.

W203 v2 corrects matrix scaling, not only the label. Its serialized regression
failed on v1 and after a lumi-only change, then passed with matrix scaling.
Native acceptance and the bounded three-grey comparison passed. Exact pins:

- Owned source, composition 14: `e62db1b303792fcdef07ad67c96c07cbb7025e164b42a411352974ad4670c77a`.
- Owned reference: `47e58f23799b521a0bb264d5904d3aff19e4cf69f627c58f3cc688e03e68f316`.
- Independent built-in reference: `0c72a4b5fcfb43171374a0a0956b92da9e9e24ad3eb9d47c5e97949d548b3042`.
- Imported/rendered candidate: `7fd1e087dfa14ca3585f33935cc73d37f1c90ecb7fcccb3b075b0b3572ee6fe3`.

Fresh import retained three editable Rects, no JsScript and W609. Independent
ST2084 assertions passed. The unchanged strict comparison covered all 30 frames
and three patches: worst error 0.40664804297305324 nits and p99 normalized
0.20332402148652662 against the owned reference; zero error against the built-in
reference. This is bounded neutral-point evidence, not complete edited FX→AEP,
chromatic, alpha or ≥1024-level precision proof. Old v1 oracles remain failures;
imports do not silently treat v1 bytes as calibrated v2.

## HLG native calibration — float v2 bounded native proof

Built-in HLG grey outputs were 29.594291/50.697028/203.152145 nits (Y10
414/502/721), matching coupled HLG1000/gamma1.2 quantization. Owned v1 instead
produced 6.079575/10.249028/41.265505 nits. Original built-in and owned references:
`16784e82dd864331b667135246a62fd4c42caa9eedad723a6c16309bea4fba56` and
`4a9a6aae0fae8abfcebd5139426e26041b42cd4c8ffebf5f9a3a1d34f37d60f6`.

Scaling lut16 alone would require peak PCS Y=4.926108374 above its maximum
1.999969482, clipping above 405.993804932 nits. An mAB matrix-output route also
clips by ICC.1:2022 §10.12.5. V2's float route addresses that representation limit;
the integer fallback remains an explicit approximation.

V2 source (composition 14): `451bf301a38b0b971e8cb448aa59afd525726dea84b52189eb10332e77583b8a`;
reference: `cd150201e0efec814a845a9b3cdba2428e8e303791b100fa98679734e17d8cb9`.
The same grey control passed every frozen full-frame/p99/patch gate over all 30
frames: worst 0.407489 nits, p99 normalized 0.203745. Additional .4/.75/1.0
controls distinguished float selection: both owned and built-in peaks decoded
1000.000029 nits, above the integer fallback bound. Exact pins:

- Owned peak source: `058e1c832124ebabf49432b8b7529e3ce1cfbc0aff1fbcf73f48905c0d8e857e`.
- Owned peak reference: `13457a459db2efe756873d01184c573f96cdf08f75529b83f3c2f16f2bbac8c2`.
- Built-in peak source: `d08b53d0056dc4ccd6912e027875b4f39bd055e89b2910c2f2a9ac8e25ccee43`.
- Built-in peak reference: `b9210cd2f0c57567ccd5f21611d24e0074910165756e09c0f2d4fe17170674f3`.

Fresh import retained three editable Rects, no JsScript and W609. Independent
inverse-HLG assertions passed. Candidate
`2441c52d6699ed0ee6467b86f41519098b885425d0fdc4353a81a43f450433ac`
matched the built-in grey reference exactly across all 30 frames and patches.
`test_hlg_float_pcs_native_luminance_and_unclipped_peak` separately checks float
layout, forward luminance, unclipped peak/negative XYZ and sampled reverse math.

## Historical chromatic precision experiment (not admitted)

An experiment used finer float PQ curves/matrices and a 129³ HLG forward CLUT;
normal regeneration and converter bindings still retain exact v2 bytes. It did
not fit primaries, white, Bradford adaptation or native gates. Numerical budgets
(PQ 0.002-nit XYZ / inverse 2e-6 signal; HLG 0.2-nit XYZ) were offline controls,
not native fidelity thresholds.

The frozen 30-frame native comparisons **failed**: experimental PQ worst
423.301494 nits / p99 5.391277 (v2 worst 107.227827), HLG worst 19.631793 nits /
p99 2.237796 (unchanged). Better serialized math did not fix the native route.
See [measured evidence](../crates/aftereffects_file/tests/fixtures/hdr/chromatic-continuation-evidence-v3.json).

Complete edited FX→AEP proof, reverse/chromatic/alpha controls, ≥1024-level
precision and publicly committed independent HDR references remain incomplete.
Native acceptance alone does not establish CMM interpolation or renderer fidelity.
See the [limitation ledger](../docs/after-effects-support.md).
