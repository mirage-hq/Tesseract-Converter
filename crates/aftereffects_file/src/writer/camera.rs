//! A fresh two-node camera for the existing FX root perspective projection.
//!
//! AE's principal point is the composition center, not the camera's world XY
//! position. Cropped camera compositions must therefore be centered around the
//! original FX principal point; moving the camera alone is not a lens shift.

use crate::{rifx::Chunk, schema::layer_records::LayerRecord, timing::Duration24};

use super::{AepWriteError, views};
use views::ValueKind;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeCameraSpec {
    /// World XY of both camera and point of interest, in this composition.
    pub center: [f64; 2],
    /// Distance to Z=0 and zoom in pixels. No depth of field is introduced.
    pub distance: f64,
}

impl NativeCameraSpec {
    /// Mirrors the existing FX implicit camera without introducing a camera
    /// model into FX. The ratio is pinned by fx_composition/value_validation.
    pub(crate) fn root(width: u32, height: u32) -> Self {
        Self {
            center: [f64::from(width) * 0.5, f64::from(height) * 0.5],
            distance: f64::from(width) * 1.388,
        }
    }

    pub(super) fn validate(&self) -> Result<(), AepWriteError> {
        if self.center.iter().any(|value| !value.is_finite())
            || !self.distance.is_finite()
            || self.distance <= 0.0
        {
            return Err(AepWriteError::Invalid("invalid native root camera"));
        }
        Ok(())
    }

    pub(super) fn translate(&mut self, offset: [f64; 2]) -> Result<(), AepWriteError> {
        let translated = Self {
            center: [self.center[0] + offset[0], self.center[1] + offset[1]],
            distance: self.distance,
        };
        translated.validate()?;
        *self = translated;
        Ok(())
    }
}

/// Adds a camera only when an emitted root actually needs perspective.
/// The caller must make the composition principal point equal `camera.center`.
pub(crate) fn append_if_needed(
    layers: &mut Vec<super::LayerSpec>,
    camera: NativeCameraSpec,
) -> Result<(), AepWriteError> {
    for layer in layers.iter() {
        if layer.has_three_d_root()? {
            camera.validate()?;
            layers.push(super::LayerSpec::Camera(camera));
            return Ok(());
        }
    }
    Ok(())
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(super) fn timeline_layer(
    camera: &NativeCameraSpec,
    id: u32,
    duration: Duration24,
) -> Result<Chunk, AepWriteError> {
    timeline_layer_with_clock(
        camera,
        id,
        duration,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(super) fn timeline_layer_with_clock(
    camera: &NativeCameraSpec,
    id: u32,
    duration: Duration24,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    camera.validate()?;
    let [x, y] = camera.center;
    let transform = views::group(
        1,
        "",
        vec![
            (
                "ADBE Anchor Point",
                views::property_with_clock(ValueKind::Spatial, &[x, y, 0.0], None, None, clock)?,
            ),
            (
                "ADBE Position",
                views::property_with_clock(
                    ValueKind::Spatial,
                    &[x, y, -camera.distance],
                    None,
                    None,
                    clock,
                )?,
            ),
        ],
    )?;
    let options = views::group(
        1,
        "",
        vec![(
            "ADBE Camera Zoom",
            // The independently authored essential_media_replace camera/view
            // record pins this scalar descriptor and the required zero bounds.
            views::property_with_clock(
                ValueKind::Scalar,
                &[camera.distance],
                Some((0.0, 0.0)),
                None,
                clock,
            )?,
        )],
    )?;
    let properties = views::group(
        1,
        "",
        vec![
            ("ADBE Transform Group", transform),
            ("ADBE Camera Options Group", options),
        ],
    )?;
    Ok(Chunk::list(
        *b"Layr",
        vec![
            Chunk::data(*b"ldta", LayerRecord::camera_ae26(id, duration)?.encode())?,
            Chunk::data(*b"Utf8", b"FX root projection".to_vec())?,
            properties,
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        properties,
        structure::{ItemKind, read_project},
    };

    #[test]
    fn fresh_camera_has_native_envelope_and_requested_projection() {
        let pinned = read_project(include_bytes!("../../tests/fixtures/layers/type.aep")).unwrap();
        let native = pinned
            .items
            .iter()
            .find_map(|item| {
                let ItemKind::Composition(comp) = &item.kind else {
                    return None;
                };
                comp.layers
                    .iter()
                    .find(|layer| layer.record.layer_type() == 2)
            })
            .expect("independently authored native camera");
        let camera = NativeCameraSpec {
            center: [410.0, 260.0],
            distance: 2664.96,
        };
        let bytes = super::super::write_composition(
            &super::super::CompositionSpec {
                name: "Camera".into(),
                width: 820,
                height: 520,
                duration_frames: 24,
            },
            &[super::super::LayerSpec::Camera(camera.clone())],
        )
        .unwrap();
        let project = read_project(&bytes).unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("composition");
        };
        let generated = &comp.layers[0];
        assert_eq!(generated.record.layer_type(), native.record.layer_type());
        assert_eq!(generated.record.source_id(), 0);
        assert_eq!(
            generated.record.flags().three_d_layer,
            native.record.flags().three_d_layer
        );
        assert_eq!(generated.record.auto_orient(), native.record.auto_orient());
        assert_eq!(
            generated.record.unknown_ae26_flag(),
            native.record.unknown_ae26_flag()
        );
        assert_eq!(
            generated.record.unknown_ae26_float().to_bits(),
            native.record.unknown_ae26_float().to_bits()
        );
        let transform = properties::read_transform(&generated.content).unwrap();
        for (name, expected) in [
            ("ADBE Anchor Point", vec![410.0, 260.0, 0.0]),
            ("ADBE Position", vec![410.0, 260.0, -2664.96]),
        ] {
            let numeric = transform
                .iter()
                .find(|value| value.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap();
            assert_eq!(numeric.values, expected);
            assert!(!numeric.animated);
        }
        let roots = properties::root_runs(&generated.content).unwrap();
        let options = roots
            .iter()
            .find(|(name, _)| *name == "ADBE Camera Options Group")
            .unwrap()
            .1;
        let options = properties::unique_list(options, *b"tdgp").unwrap();
        let zoom = properties::runs(options)
            .unwrap()
            .into_iter()
            .find(|(name, _)| *name == "ADBE Camera Zoom")
            .unwrap()
            .1;
        let zoom = properties::unique_list(zoom, *b"tdbs").unwrap();
        assert_eq!(
            properties::read_numeric(zoom).unwrap().values,
            vec![camera.distance]
        );
    }

    #[test]
    fn thirty_fps_camera_uses_composition_property_clock() {
        fn descriptors(chunk: &Chunk, output: &mut Vec<u32>) {
            if chunk.id() == *b"tdb4" {
                let bytes = chunk.data_payload().unwrap();
                output.push(u32::from_be_bytes(bytes[12..16].try_into().unwrap()));
            }
            if let Some(children) = chunk.children() {
                for child in children {
                    descriptors(child, output);
                }
            }
        }
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let camera = timeline_layer_with_clock(
            &NativeCameraSpec::root(640, 480),
            1,
            Duration24::from_frames(60).unwrap(),
            clock,
        )
        .unwrap();
        let mut ticks = Vec::new();
        descriptors(&camera, &mut ticks);
        assert_eq!(ticks, vec![30_720; 3]);
    }

    #[test]
    fn invalid_camera_translation_is_atomic() {
        let mut camera = NativeCameraSpec::root(1920, 1080);
        let before = camera.clone();
        assert!(camera.translate([f64::INFINITY, 0.0]).is_err());
        assert_eq!(camera, before);
        camera.distance = 0.0;
        assert!(camera.validate().is_err());
    }
}
