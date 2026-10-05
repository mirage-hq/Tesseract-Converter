//! Export-only sampling at four times the native output frame rate.

use crate::{AfterEffectsExportOptions, timing::FrameRate, writer::AepWriteError};

#[derive(Clone, Copy, Debug)]
pub(super) struct Sampling(FrameRate);

impl Sampling {
    pub(super) fn new(options: &AfterEffectsExportOptions) -> Result<Self, AepWriteError> {
        Ok(Self(FrameRate::new(options.fps)?))
    }

    /// Includes zero and the exact owner endpoint. The native 16.16 FPS and
    /// integer arithmetic avoid accumulating fractional-frame drift. At the
    /// maximum supported 240fps, adjacent samples still round to distinct ms.
    pub(super) fn offsets(self, duration_ms: u64) -> impl Iterator<Item = u64> {
        let (integer, fraction) = self.0.parts();
        let fixed = (u128::from(integer) << 16) + u128::from(fraction);
        let denominator = fixed * 4;
        let mut index = 0_u64;
        let mut done = false;
        std::iter::from_fn(move || {
            if done {
                return None;
            }
            let numerator = u128::from(index) * 65_536 * 1_000;
            // Clamp before narrowing; index * the scale fits in u128.
            let offset =
                ((numerator + denominator / 2) / denominator).min(u128::from(duration_ms)) as u64;
            done = offset == duration_ms;
            if !done {
                index += 1;
            }
            Some(offset)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampling(fps: f64) -> Sampling {
        Sampling::new(&AfterEffectsExportOptions { fps }).unwrap()
    }

    #[test]
    fn default_samples_at_four_times_output_fps() {
        let sampling = Sampling::new(&AfterEffectsExportOptions::default()).unwrap();
        assert_eq!(
            sampling.offsets(42).collect::<Vec<_>>(),
            [0, 10, 21, 31, 42]
        );
    }

    #[test]
    fn grid_includes_rounded_frames_and_exact_endpoint() {
        assert_eq!(
            sampling(30.0).offsets(34).collect::<Vec<_>>(),
            [0, 8, 17, 25, 33, 34]
        );
        assert_eq!(sampling(30.0).offsets(0).collect::<Vec<_>>(), [0]);
        assert_eq!(sampling(30.0).offsets(1).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(sampling(30.0).offsets(1_000).count(), 121);
    }

    #[test]
    fn fractional_and_extreme_rates_keep_strictly_increasing_offsets() {
        for fps in [1.0 / 65_536.0, 23.976, 29.97, 59.94, 240.0] {
            let offsets = sampling(fps).offsets(1_003).collect::<Vec<_>>();
            assert_eq!(offsets.first(), Some(&0));
            assert_eq!(offsets.last(), Some(&1_003));
            assert!(offsets.windows(2).all(|pair| pair[0] < pair[1]));
        }
        // No duration-sized allocation or floating-point step accumulation.
        assert_eq!(
            sampling(30.0).offsets(u64::MAX).take(3).collect::<Vec<_>>(),
            [0, 8, 17]
        );
    }

    #[test]
    fn grid_rejects_invalid_rates() {
        for fps in [0.0, f64::NAN, f64::INFINITY, 241.0] {
            assert!(Sampling::new(&AfterEffectsExportOptions { fps }).is_err());
        }
    }
}
