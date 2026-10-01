//! Three measured neutral Film Impact Stroke profiles, not a general plug-in law.
use super::film_impact::{self, read_profile_with, ProfileParam};
use crate::{
    error::{unsupported, Result},
    format::{Graph, Record},
    schema::PrFilmImpactStroke,
};

const PROFILE: [ProfileParam; 31] = [
    film_impact::ERROR_OCCURRED,
    ProfileParam {
        id: "8220",
        kind: "VideoComponentParam",
        name: Some("Pre Transform"),
        control: Some("11"),
        start: Some("-91445760000000000,false,0,0,0,0,0,0"),
        lower: None,
        upper: Some("false"),
        discontinuous: None,
    },
    ProfileParam {
        id: "8222",
        kind: "VideoComponentParam",
        name: Some("Apply Prescale"),
        control: None,
        start: Some("-91445760000000000,true,0,0,0,0,0,0"),
        lower: None,
        upper: None,
        discontinuous: None,
    },
    ProfileParam {
        id: "8223",
        kind: "VideoComponentParam",
        name: Some("Scale"),
        control: None,
        start: Some("-91445760000000000,99.,0,0,0,0,0,0"),
        lower: Some("0"),
        upper: Some("100"),
        discontinuous: None,
    },
    ProfileParam {
        id: "8221",
        kind: "VideoComponentParam",
        name: Some("Pre Transform"),
        control: Some("12"),
        start: Some("-91445760000000000,false,0,0,0,0,0,0"),
        lower: None,
        upper: Some("false"),
        discontinuous: None,
    },
    ProfileParam {
        id: "1",
        kind: "VideoComponentParam",
        name: Some("Controls"),
        control: Some("11"),
        start: Some("-91445760000000000,false,0,0,0,0,0,0"),
        lower: None,
        upper: Some("false"),
        discontinuous: None,
    },
    film_impact::CONTROL_8040,
    ProfileParam {
        id: "8041",
        kind: "VideoComponentParam",
        name: Some("Seed"),
        control: None,
        start: Some("-91445760000000000,0.,0,0,0,0,0,0"),
        lower: Some("0"),
        upper: Some("99999"),
        discontinuous: None,
    },
    ProfileParam {
        id: "2",
        kind: "VideoComponentParam",
        name: Some("Size"),
        control: None,
        start: Some("-91445760000000000,6.,0,0,0,0,0,0"),
        lower: Some("0"),
        upper: Some("1000"),
        discontinuous: None,
    },
    ProfileParam {
        id: "3",
        kind: "VideoComponentParam",
        name: Some("Position"),
        control: None,
        start: Some("-91445760000000000,100.,0,0,0,0,0,0"),
        lower: Some("-100"),
        upper: Some("100"),
        discontinuous: None,
    },
    ProfileParam {
        id: "14",
        kind: "VideoComponentParam",
        name: Some("Roundness"),
        control: None,
        start: Some("-91445760000000000,0.,0,0,0,0,0,0"),
        lower: Some("0"),
        upper: Some("100"),
        discontinuous: None,
    },
    ProfileParam {
        id: "4",
        kind: "VideoComponentParam",
        name: Some("Color"),
        control: None,
        start: Some("-91445760000000000,18374966859414961920,0,0,0,0,0,0"),
        lower: None,
        upper: None,
        discontinuous: None,
    },
    ProfileParam {
        id: "5",
        kind: "VideoComponentParam",
        name: Some("Colorize"),
        control: None,
        start: Some("-91445760000000000,100.,0,0,0,0,0,0"),
        lower: Some("0"),
        upper: Some("100"),
        discontinuous: None,
    },
    ProfileParam {
        id: "6",
        kind: "VideoComponentParam",
        name: Some("Duo Color"),
        control: None,
        start: Some("-91445760000000000,18374686479671623680,0,0,0,0,0,0"),
        lower: None,
        upper: None,
        discontinuous: None,
    },
    ProfileParam {
        id: "7",
        kind: "VideoComponentParam",
        name: Some("Duo Amount"),
        control: None,
        start: Some("-91445760000000000,0.,0,0,0,0,0,0"),
        lower: Some("0"),
        upper: Some("100"),
        discontinuous: None,
    },
    ProfileParam {
        id: "8",
        kind: "VideoComponentParam",
        name: Some("Invert"),
        control: None,
        start: Some("-91445760000000000,false,0,0,0,0,0,0"),
        lower: None,
        upper: None,
        discontinuous: None,
    },
    ProfileParam {
        id: "9",
        kind: "VideoComponentParam",
        name: Some("Alpha Falloff"),
        control: None,
        start: Some("-91445760000000000,0.,0,0,0,0,0,0"),
        lower: Some("0"),
        upper: Some("100"),
        discontinuous: None,
    },
    ProfileParam {
        id: "10",
        kind: "VideoComponentParam",
        name: Some("Opacity"),
        control: None,
        start: Some("-91445760000000000,100.,0,0,0,0,0,0"),
        lower: Some("0"),
        upper: Some("100"),
        discontinuous: None,
    },
    ProfileParam {
        id: "11",
        kind: "VideoComponentParam",
        name: Some("Hide Source"),
        control: None,
        start: Some("-91445760000000000,false,0,0,0,0,0,0"),
        lower: None,
        upper: None,
        discontinuous: None,
    },
    ProfileParam {
        id: "8240",
        kind: "VideoComponentParam",
        name: None,
        control: Some("16"),
        start: Some("-91445760000000000,false,0,0,0,0,0,0"),
        lower: None,
        upper: None,
        discontinuous: None,
    },
    ProfileParam {
        id: "13",
        kind: "VideoComponentParam",
        name: Some("Controls"),
        control: Some("12"),
        start: Some("-91445760000000000,false,0,0,0,0,0,0"),
        lower: None,
        upper: Some("false"),
        discontinuous: None,
    },
    film_impact::OVERLAY_MODE,
    film_impact::OVERLAY_INFO,
    film_impact::CONTROL_8141,
    ProfileParam {
        id: "8140",
        kind: "VideoComponentParam",
        name: Some("_ Applied Version"),
        control: None,
        start: Some("-91445760000000000,260300.,0,0,0,0,0,0"),
        lower: Some("0"),
        upper: Some("999999"),
        discontinuous: None,
    },
    film_impact::CONTROL_8300,
    film_impact::CONTROL_8301,
    film_impact::OVERLAY_ENABLED,
    film_impact::SEQUENCE_WIDTH,
    film_impact::SEQUENCE_HEIGHT,
    film_impact::SEQUENCE_PIXEL_RATIO,
];

pub(super) fn read(graph: &Graph<'_>, record: Record<'_>) -> Result<PrFilmImpactStroke> {
    for (profile, scale, size) in [
        (
            PrFilmImpactStroke::Outline99,
            "-91445760000000000,99.,0,0,0,0,0,0",
            "-91445760000000000,6.,0,0,0,0,0,0",
        ),
        (
            PrFilmImpactStroke::Outline100,
            "-91445760000000000,100.,0,0,0,0,0,0",
            "-91445760000000000,6.,0,0,0,0,0,0",
        ),
        (
            PrFilmImpactStroke::Frame99,
            "-91445760000000000,99.,0,0,0,0,0,0",
            "-91445760000000000,66.,0,0,0,0,0,0",
        ),
    ] {
        let mut controls = PROFILE;
        controls
            .iter_mut()
            .find(|p| p.id == "8223")
            .expect("profile contains Prescale")
            .start = Some(scale);
        controls
            .iter_mut()
            .find(|p| p.id == "2")
            .expect("profile contains Size")
            .start = Some(size);
        if read_profile_with(graph, record, "AE.Impact_Stroke_FX", &controls).is_ok() {
            return Ok(profile);
        }
    }
    Err(unsupported("Film Impact Stroke requires the measured static white neutral 6/99, 6/100 or 66/99 profile"))
}
