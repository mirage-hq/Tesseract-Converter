//! FX `lookTransform` 3D LUT → native Apply Color LUT (`ADBE Apply Color LUT2`).
//!
//! A media source's `inputTransform` 3D LUT is lowered the same way, as the
//! first effect of its footage layer ([`lower_input_transform`]).
//!
//! Effect lowering has no archive access, so it emits the native effect with
//! its FX `.cube` asset id; [`bind`] then embeds each LUT from the archive
//! ([`ArchiveLuts`]) on the final layers, or omits that effect with a
//! diagnostic. AE stores the LUT inside the project (an ICC device link in
//! the instance's sequence data, see `effects::apply_color_lut`); the `.cube`
//! itself is also published beside the project for re-application.

use std::collections::{BTreeMap, BTreeSet};

use fx_schema::{
    AssetId, ColorEncodingId, ColorTransform, InputColorTransform, LayerEffect,
    PersistedInputTransform, SDR_REC709_DISPLAY_ENCODING_ID,
};
use tesseract_file::{TesseractFile, TesseractFileError};

use super::ExportDiagnostic;
use crate::{
    effects::apply_color_lut::{self, Lut3d},
    writer::{
        LayerSpec,
        effects::{NativeEffect, NativeLut, new_effect},
    },
};

/// Largest `.cube` asset read for export (a 255³ LUT is about 600 MB of text;
/// AE's own picker accepts far smaller files).
const MAX_CUBE_BYTES: u64 = 256 * 1024 * 1024;

/// A `.cube` archive asset prepared for embedding.
#[derive(Debug)]
pub(crate) struct ArchiveLut {
    /// The archive file name, shown by AE as `Using: <name>`.
    pub file_name: String,
    /// The asset bytes as stored, published beside the project.
    pub bytes: Vec<u8>,
    lut: Result<Lut3d, String>,
}

/// Every `.cube` asset that an FX `lookTransform` of the document references.
#[derive(Debug, Default)]
pub(crate) struct ArchiveLuts {
    luts: BTreeMap<String, ArchiveLut>,
}

impl ArchiveLuts {
    pub(crate) fn prepare(
        archive: &TesseractFile,
        document: &fx_schema::EditableFxCompositionDocument,
    ) -> Result<Self, TesseractFileError> {
        let mut luts = BTreeMap::new();
        for reference in document.composition().asset_refs() {
            if reference.kind != fx_schema::asset_refs::AssetKind::Lut
                || luts.contains_key(reference.asset_id)
            {
                continue;
            }
            let asset = archive.asset(reference.asset_id)?;
            let file_name = asset
                .descriptor()
                .path
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            let bytes = asset.read_verified_bytes(MAX_CUBE_BYTES)?;
            let lut = std::str::from_utf8(&bytes)
                .map_err(|_| "the asset is not UTF-8 .cube text".to_owned())
                .and_then(Lut3d::parse_cube);
            luts.insert(
                reference.asset_id.to_owned(),
                ArchiveLut {
                    file_name,
                    bytes,
                    lut,
                },
            );
        }
        Ok(Self { luts })
    }

    pub(crate) fn get(&self, asset_id: &AssetId) -> Option<&ArchiveLut> {
        self.luts.get(asset_id.as_str())
    }
}

/// The LUT asset of an exportable `lookTransform`, or why it cannot export.
fn exportable(effect: &LayerEffect) -> Result<&AssetId, String> {
    let LayerEffect::LookTransform { transform, .. } = effect else {
        return Err("not a lookTransform".into());
    };
    let Some(ColorTransform::Lut3d {
        asset_id,
        input_encoding,
        output_encoding,
        ..
    }) = transform
    else {
        return Err("a lookTransform without a transform is a bypass; nothing to export".into());
    };
    require_sdr_display([input_encoding, output_encoding])?;
    Ok(asset_id)
}

/// Apply Color LUT applies its table to the layer's display-referred SDR
/// code values (working space None), so both encodings must be that domain.
fn require_sdr_display(encodings: [&ColorEncodingId; 2]) -> Result<(), String> {
    for encoding in encodings {
        if encoding.as_str() != SDR_REC709_DISPLAY_ENCODING_ID {
            return Err(format!(
                "encoding {encoding} is not the display-referred SDR encoding that Apply Color LUT applies in (working space None)"
            ));
        }
    }
    Ok(())
}

/// Lower a media source's FX `inputTransform` 3D LUT to an Apply Color LUT
/// for the head of the footage layer's effect stack. `Err` is the reason the
/// footage is written ungraded.
///
/// AE footage has no per-source LUT, but FX evaluates an Input Transform
/// before every layer effect, which is where a head-of-stack per-pixel colour
/// effect sits in AE. The caller inserts the effect at index 0 so that
/// authored effects follow it. A re-import reads it back as the first
/// `lookTransform` of the layer, not as `source.inputTransform`.
pub(super) fn lower_input_transform(
    input: &PersistedInputTransform,
) -> Result<(NativeEffect, String), String> {
    let Some(input) = input.as_supported() else {
        return Err("the Input Transform uses an unsupported future semantic version".into());
    };
    let InputColorTransform::Lut3d {
        asset_id,
        input_encoding,
        output_encoding,
        ..
    } = &input.transform
    else {
        return Err(
            "a camera input transform has no Apply Color LUT equal (AE footage has no per-source camera interpretation)"
                .into(),
        );
    };
    require_sdr_display([input_encoding, output_encoding])?;
    let mut native = new_effect(apply_color_lut::MATCH_NAME, true, [1.0; 2])
        .map_err(|error| error.to_string())?;
    native.lut = Some(NativeLut {
        asset_id: asset_id.clone(),
        file_name: String::new(),
        sequence_data: None,
    });
    Ok((
        native,
        format!(
            "source.inputTransform written as a head-of-stack Apply Color LUT with LUT asset {asset_id} embedded (AE footage has no per-source LUT; a re-import reads it as the layer's first lookTransform); {}",
            apply_color_lut::LUT_INTERPOLATION
        ),
    ))
}

/// Why a `lookTransform` has no Apply Color LUT export, if it has none.
pub(super) fn unexportable_reason(effect: &LayerEffect) -> Option<String> {
    exportable(effect).err()
}

/// Lower one FX `lookTransform`. `Err` omits it with the reason.
pub(super) fn lower(
    effect: &LayerEffect,
    enabled: bool,
) -> Result<(NativeEffect, Vec<String>), String> {
    let asset_id = exportable(effect)?;
    let LayerEffect::LookTransform { mix, .. } = effect else {
        return Err("not a lookTransform".into());
    };
    let mut native = new_effect(apply_color_lut::MATCH_NAME, enabled, [1.0; 2])
        .map_err(|error| error.to_string())?;
    native.lut = Some(NativeLut {
        asset_id: asset_id.clone(),
        file_name: String::new(),
        sequence_data: None,
    });
    let mut notes = vec![format!(
        "written as Apply Color LUT with LUT asset {asset_id} embedded (AE stores it as a 16-bit device-link CLUT and re-reads no file); {}",
        apply_color_lut::LUT_INTERPOLATION
    )];
    if mix.get() != 1.0 {
        native.compositing.effect_opacity_percent = mix.get() * 100.0;
        notes.push(format!(
            "mix {} written as Effect Opacity {}% (AE blends in its working space)",
            mix.get(),
            mix.get() * 100.0
        ));
    }
    Ok((native, notes))
}

/// Embed each Apply Color LUT's archive LUT on the final layers; omit an
/// effect whose LUT cannot be embedded. Returns the embedded assets.
pub(super) fn bind(
    layers: &mut [LayerSpec],
    luts: Option<&ArchiveLuts>,
    diagnostics: &mut Vec<ExportDiagnostic>,
) -> BTreeSet<AssetId> {
    let mut embedded = BTreeSet::new();
    for layer in layers {
        bind_layer(layer, luts, diagnostics, &mut embedded);
    }
    embedded
}

fn bind_layer(
    layer: &mut LayerSpec,
    luts: Option<&ArchiveLuts>,
    diagnostics: &mut Vec<ExportDiagnostic>,
    embedded: &mut BTreeSet<AssetId>,
) {
    match layer {
        LayerSpec::Options(inner, options) => {
            let owner = options.fx_id;
            options.effects.retain_mut(|effect| {
                let Some(lut) = &mut effect.lut else {
                    return true;
                };
                match embed(lut, luts) {
                    Ok(warning) => {
                        if let Some(warning) = warning {
                            diagnostics.push(ExportDiagnostic {
                                layer_id: Some(owner),
                                message: format!("Effect lookTransform: {warning}"),
                            });
                        }
                        embedded.insert(lut.asset_id.clone());
                        true
                    }
                    Err(reason) => {
                        diagnostics.push(ExportDiagnostic {
                            layer_id: Some(owner),
                            message: format!(
                                "Effect lookTransform: LUT asset {}: {reason}; Apply Color LUT omitted, owner and other effects retained",
                                lut.asset_id
                            ),
                        });
                        false
                    }
                }
            });
            bind_layer(inner, luts, diagnostics, embedded);
        }
        LayerSpec::Timed(inner, _) => bind_layer(inner, luts, diagnostics, embedded),
        LayerSpec::Precomposition(value) => {
            for layer in &mut value.layers {
                bind_layer(layer, luts, diagnostics, embedded);
            }
        }
        _ => {}
    }
}

fn embed(lut: &mut NativeLut, luts: Option<&ArchiveLuts>) -> Result<Option<String>, String> {
    let archive = luts
        .and_then(|luts| luts.get(&lut.asset_id))
        .ok_or("the asset is not available to this export")?;
    let parsed = archive.lut.as_ref().map_err(Clone::clone)?;
    let (data, clamped) = apply_color_lut::encode_sequence_data(parsed, &archive.file_name);
    lut.file_name = archive.file_name.clone();
    lut.sequence_data = Some(data);
    Ok((clamped > 0).then(|| {
        format!(
            "LUT asset {}: {clamped} sample values outside 0..1 clamped (Apply Color LUT stores 16-bit codes)",
            lut.asset_id
        )
    }))
}

/// Published `.cube` files beside the project: `(package-relative name, asset)`,
/// one per embedded asset, names made unique.
pub(crate) fn published_files<'a>(
    luts: &'a ArchiveLuts,
    embedded: &BTreeSet<AssetId>,
) -> Vec<(String, &'a ArchiveLut)> {
    let mut names = BTreeSet::new();
    let mut files = Vec::new();
    for id in embedded {
        let Some(lut) = luts.get(id) else {
            continue;
        };
        let base = apply_color_lut::safe_cube_file_name(&lut.file_name);
        let stem = &base[..base.len() - ".cube".len()];
        let mut name = base.clone();
        let mut counter = 2;
        while !names.insert(name.to_ascii_lowercase()) {
            name = format!("{stem}-{counter}.cube");
            counter += 1;
        }
        files.push((name, lut));
    }
    files
}
