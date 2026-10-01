//! One declaration of the two persisted turbulent-noise popup ordinals.

/// Define the fixed ordered choices used by the effect payload.
#[doc(hidden)]
#[macro_export]
macro_rules! define_noise_popup_schema {
    () => {
        /// AE "Noise Type" popup for [`super::LayerEffect::TurbulentNoise`] — how the
        /// generator interpolates between lattice points (JRB-1716).
        ///
        /// Lowered to the preset's `noiseType` uniform slot as its declaration ordinal
        /// (see [`Self::uniform_code`]), which the WGSL branches on. An omitted field
        /// means [`Self::SoftLinear`], AE's default and the only mode v1 shipped, so
        /// existing documents keep their exact rendered output.
        ///
        /// `#[repr(u8)]` states the declaration-order discriminant
        /// [`Self::uniform_code`] reads; `#[non_exhaustive]` keeps a mode added later
        /// from breaking a downstream `match` (AE's four options are all modeled here,
        /// but [`FractalType`] has deferred ones and the two popups stay symmetric).
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        #[repr(u8)]
        #[non_exhaustive]
        pub enum NoiseType {
            /// No interpolation: every pixel takes the value of the lattice cell that
            /// contains it, so the field reads as hard rectangular blocks.
            Block,
            /// Bilinear blend on the raw cell fraction. Cheaper and crisper than
            /// `SoftLinear`, but the un-eased ramps make cell boundaries visible as
            /// faint creases.
            Linear,
            /// Bilinear blend on a smoothstep-eased fraction — continuous first
            /// derivative across cell boundaries, so the field looks organic. AE's
            /// default.
            #[default]
            SoftLinear,
            /// Bicubic (Catmull-Rom) blend over the 4×4 lattice neighbourhood: the
            /// smoothest, roundest field, at 16 lattice taps per octave instead of 4.
            Spline,
        }

        /// AE "Fractal Type" popup for [`super::LayerEffect::TurbulentNoise`] — how the
        /// octaves of the fractal are combined (JRB-1716).
        ///
        /// Lowered to the preset's `fractalType` uniform slot as its declaration
        /// ordinal (see [`Self::uniform_code`]). An omitted field means [`Self::Basic`],
        /// the plain signed-fBm sum v1 shipped, so existing documents keep their exact
        /// rendered output.
        ///
        /// `Basic` and the three turbulence modes reproduce AE's documented behavior.
        /// AE's `Strings`, `Rocky` and `Cloudy` algorithms are proprietary and are
        /// approximated here — see the preset doc comment in
        /// `crate::custom_shader_presets` for the divergence note.
        ///
        /// `#[non_exhaustive]` because AE has further modes this does not model yet (the
        /// Dynamic family, Smeary, …) and appending one must not break a downstream
        /// `match`. `#[repr(u8)]` states the declaration-order discriminant
        /// [`Self::uniform_code`] reads.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        #[repr(u8)]
        #[non_exhaustive]
        pub enum FractalType {
            /// Plain signed fBm: octaves sum around mid-gray into soft billows.
            #[default]
            Basic,
            /// Smoothed turbulence — the absolute-value fold of `TurbulentBasic` eased
            /// at both ends, so the creases are rounded rather than pinched.
            TurbulentSmooth,
            /// Classic turbulence: the absolute value of each octave, which folds
            /// mid-gray into dark creases and doubles the apparent detail.
            TurbulentBasic,
            /// Ridged (inverted) turbulence: the folds become bright sharp ridges.
            TurbulentSharp,
            /// Max-combine instead of sum: the strongest octave wins at each pixel. The
            /// coarsest octave carries the most amplitude, so it dominates and the finer
            /// ones only punch through where it is weak — broad bulbous ridges rather
            /// than the even detail the summing modes give.
            Max,
            /// Approximation of AE "Strings": ridges sharpened into thin bright
            /// filaments over a dark field.
            Strings,
            /// Approximation of AE "Rocky": high-contrast turbulence pushed into hard
            /// light/dark facets.
            Rocky,
            /// Approximation of AE "Cloudy": extra per-octave damping and reduced
            /// contrast, for a soft low-detail overcast wash.
            Cloudy,
        }
    };
}
