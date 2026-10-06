# Noise native records

Offline verbatim record extraction from human-authored `human-inputs-20261004.prproj`
(SHA-256 `e74d088116570ddb7178b127129036755be2f8e553dea80f98aa59b601585b76`),
saved interactively in Premiere Pro 2026. Sequence `effects`, UID
`b3aecac7-c452-48ba-a5e1-737807cb32ee`, one `subject.mov` owner, chain 388.
No Adobe execution or source mutation was used to derive this fixture.

`noise-native-records.xml` SHA-256:
`321ed5cad07858271bac1ea8e36d7babd36d7c8de5538eeab8b4f9c6c3c15084`.
It retains chain 388, components 549/550 and parameters 730–757 without media.
Tests attach these original effect records to the existing one-clip harness.
Chain Index 0 is Legacy 549; Index 1 is modern 550, which renders first.
Legacy saves Amount 5 (0–100), Noise Type true and Clipping true.
Modern saves Intensity 50, Seed 0, Shadows/Midtones/Highlights 75,
Uniform Intensity true, Saturation 50, Blend Mode 4, Master 100,
Preserve Alpha true, and UI/version sentinels including Applied Version 260501.

`noise_native_siblings_keep_owner_and_import_editable_grain` exercises both native
siblings and the editable import; `noise_monochrome_and_wrapping_retain_both_effects`
uses exact-control mutations for approximate mode retention.
`noise_modern_exact_record_strength_seed_keys_and_malformed_controls` targets individual
Modern parameter records for editable controls and malformed-input rejection. The supplementary
`noise_amount_keys_bypass_and_current_edits_export_as_legacy` checks modeled keys,
bypass, current FX edits and writer readback. No native keyframe fixture, Adobe
export acceptance, RGB/alpha comparison or visual fidelity pass is claimed.
