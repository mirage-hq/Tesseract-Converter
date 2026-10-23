//! Native ACES colour-management vocabulary shared by the Premiere and After
//! Effects converters.
//!
//! An FX ACES document (`colorManagement` `acescct` / `aces2.0`) declares each
//! camera-log source with a LUT-less `cameraInput` Input Transform. The FX
//! renderer converts only the cameras whose input transform the ACES reference
//! (studio) OCIO config defines; other registered camera encodings (Fujifilm
//! F-Log2, Nikon N-Log, DJI D-Log M) fail an ACES render, so a converter must
//! never author them as `cameraInput`.

/// FX `cameraInput` encodings the ACES pipeline converts, each with the
/// colourspace name of its input transform in the ACES OCIO studio config.
/// After Effects 26.5's built-in "ACES 2.0 Studio v4.0" config
/// (`studio-config-v4.0.0_aces-v2.0_ocio-v2.5`) uses the same names.
const ACES_CAMERA_INPUTS: [(&str, &str); 9] = [
    ("camera:apple-log:v1", "Apple Log"),
    ("camera:arri-logc3:v1", "ARRI LogC3 (EI800)"),
    ("camera:arri-logc4:v1", "ARRI LogC4"),
    ("camera:blackmagic-film-gen5:v1", "BMDFilm WideGamut Gen5"),
    (
        "camera:canon-log3-cinema-gamut:v1",
        "CanonLog3 CinemaGamut D55",
    ),
    ("camera:panasonic-vlog:v1", "V-Log V-Gamut"),
    ("camera:red-log3g10:v1", "Log3G10 REDWideGamutRGB"),
    ("camera:sony-slog3-sgamut3:v1", "S-Log3 S-Gamut3"),
    ("camera:sony-slog3-sgamut3-cine:v1", "S-Log3 S-Gamut3.Cine"),
];

/// Whether an FX ACES document may declare `encoding` as a `cameraInput`.
#[must_use]
pub fn is_aces_camera_input(encoding: &str) -> bool {
    ACES_CAMERA_INPUTS
        .iter()
        .any(|(camera, _)| *camera == encoding)
}

/// The ACES OCIO studio-config colourspace of camera encoding `encoding`.
#[must_use]
pub fn aces_ocio_colorspace(encoding: &str) -> Option<&'static str> {
    ACES_CAMERA_INPUTS
        .iter()
        .find(|(camera, _)| *camera == encoding)
        .map(|(_, colorspace)| *colorspace)
}

/// The FX camera encoding whose ACES input transform is OCIO colourspace `name`.
#[must_use]
pub fn camera_for_aces_ocio_colorspace(name: &str) -> Option<&'static str> {
    ACES_CAMERA_INPUTS
        .iter()
        .find(|(_, colorspace)| *colorspace == name)
        .map(|(camera, _)| *camera)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aces_camera_inputs_round_trip_through_their_ocio_colorspace() {
        for (camera, colorspace) in ACES_CAMERA_INPUTS {
            assert!(is_aces_camera_input(camera));
            assert_eq!(aces_ocio_colorspace(camera), Some(colorspace));
            assert_eq!(camera_for_aces_ocio_colorspace(colorspace), Some(camera));
        }
        for unconverted in [
            "camera:fujifilm-flog2:v1",
            "camera:nikon-nlog:v1",
            "camera:dji-dlog-m:v1",
        ] {
            assert!(!is_aces_camera_input(unconverted));
            assert_eq!(aces_ocio_colorspace(unconverted), None);
        }
    }
}
