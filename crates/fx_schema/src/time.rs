//! Canonical time vocabulary for portable FX documents.
//!
//! Four persisted newtypes plus runtime-only precise remapping types:
//!
//! - [`Time`] — a point on the project timeline (integer milliseconds).
//! - [`TimeOffset`] — a signed displacement from a clock origin (integer
//!   milliseconds).
//! - [`Duration`] — a non-negative interval (`std::time::Duration`-style
//!   semantics, integer milliseconds).
//! - [`TimeRange`] — a half-open `[start, end)` interval over `Time`s.
//!
//! All four are `#[serde(transparent)]` (or transparent-equivalent for
//! `TimeRange`) and `#[ts(type = "number")]` where applicable, so they cross
//! JSON, ts-rs, serde-wasm-bindgen, pyo3, and uniffi boundaries as plain
//! integers (for `Time`/`TimeOffset`/`Duration`) or a small object (for
//! `TimeRange`).
//!
//! # Why not `std::time::Duration` directly
//!
//! `Duration`'s default serde impl is a `{secs, nanos}` struct, and ts-rs
//! emits the same shape. The whole point of these newtypes is *transparent
//! integer at every FFI boundary*, which `Duration` blocks. We provide
//! `From<Duration>`/`Into<Duration>` interop for the rare case where Rust
//! code wants `Duration`'s arithmetic.

use std::fmt;
use std::ops::{Add, AddAssign, Mul, Sub, SubAssign};

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

pub use crate::time_remap::TimeRemapProperty;

/// Expectation message emitted when a negative *integer* reaches the
/// millisecond deserializers below. Shared as a constant so downstream
/// diagnostics that match on it (e.g. `project_mutation::wire`'s
/// negative-field scan, JRB-1083) break at compile time if it is reworded.
pub const NON_NEGATIVE_MS_EXPECTED: &str = "a non-negative integer number of milliseconds";

const fn canonical_millis_u64(value: u64) -> u64 {
    if value > i64::MAX as u64 {
        u64::MAX
    } else {
        value
    }
}

/// Shared visitor body for [`Time`] and [`Duration`] — accepts any numeric
/// JSON form (integer or float) and routes through a caller-supplied
/// `f64` constructor for the float case. Floats are rounded to the nearest
/// millisecond (see [`Time::from_millis_f64`]); negatives clamp to zero.
///
/// This buys robustness against client-side rounding bugs: a JS caller that
/// sends `27092.19999998808` instead of `27092` would otherwise hit
/// `invalid type: floating point ..., expected u64` (JRB-967).
fn deserialize_millis_lenient<'de, D, T, FU, FF>(
    d: D,
    from_u64: FU,
    from_f64: FF,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    FU: Fn(u64) -> T,
    FF: FnOnce(f64) -> T,
{
    #[derive(Debug)]
    struct V<T, FU, FF> {
        from_u64: FU,
        from_f64: FF,
        _t: std::marker::PhantomData<T>,
    }
    impl<'de, T, FU, FF> Visitor<'de> for V<T, FU, FF>
    where
        FU: Fn(u64) -> T,
        FF: FnOnce(f64) -> T,
    {
        type Value = T;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a non-negative number of milliseconds (integer or float)")
        }
        fn visit_u64<E>(self, v: u64) -> Result<T, E> {
            // Collapse the whole high-bit range to the canonical `MAX`
            // sentinel (`u64::MAX`), mirroring the float path's `>= i64::MAX`
            // clamp. A wire integer in `(i64::MAX, u64::MAX)` would otherwise
            // deserialize into a *non-canonical* near-`u64::MAX` value that
            // `is_negative_sentinel()` flags yet escapes every exact
            // `== MAX` check — the JRB-1186 crash re-entered via the integer
            // path. No genuine timeline value approaches `i64::MAX` ms.
            Ok((self.from_u64)(canonical_millis_u64(v)))
        }
        fn visit_i64<E>(self, v: i64) -> Result<T, E>
        where
            E: de::Error,
        {
            // Keep prior schema strictness for whole integers: a negative
            // integer on the wire is a real producer bug, not the JS-float
            // jitter this lenient path exists to absorb (JRB-967). The float
            // path continues to clamp via `from_millis_f64`.
            if v < 0 {
                Err(E::invalid_value(
                    de::Unexpected::Signed(v),
                    &NON_NEGATIVE_MS_EXPECTED,
                ))
            } else {
                Ok((self.from_u64)(v as u64))
            }
        }
        fn visit_u128<E>(self, v: u128) -> Result<T, E>
        where
            E: de::Error,
        {
            let v = u64::try_from(v).unwrap_or(u64::MAX);
            Ok((self.from_u64)(canonical_millis_u64(v)))
        }
        fn visit_i128<E>(self, v: i128) -> Result<T, E>
        where
            E: de::Error,
        {
            if v < 0 {
                Err(E::invalid_value(
                    de::Unexpected::Other("negative integer"),
                    &NON_NEGATIVE_MS_EXPECTED,
                ))
            } else {
                let v = u64::try_from(v).unwrap_or(u64::MAX);
                Ok((self.from_u64)(canonical_millis_u64(v)))
            }
        }
        fn visit_f64<E>(self, v: f64) -> Result<T, E> {
            Ok((self.from_f64)(v))
        }
        fn visit_f32<E>(self, v: f32) -> Result<T, E> {
            Ok((self.from_f64)(f64::from(v)))
        }
    }
    d.deserialize_any(V {
        from_u64,
        from_f64,
        _t: std::marker::PhantomData,
    })
}

// ─────────────────────────────────────────────────────────────────────
// TimeOffset — a signed displacement from a clock origin.
// ─────────────────────────────────────────────────────────────────────

/// A signed displacement from a clock origin, in integer milliseconds.
///
/// Use this for local coordinates that may precede their origin, such as an
/// authored keyframe before a layer's current in-point. Use [`Time`] for an
/// absolute point on the non-negative project timeline and [`Duration`] for a
/// non-negative interval.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct TimeOffset(i64);

impl<'de> Deserialize<'de> for TimeOffset {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Debug)]
        struct TimeOffsetVisitor;

        impl Visitor<'_> for TimeOffsetVisitor {
            type Value = TimeOffset;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a signed number of milliseconds (integer or float)")
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(TimeOffset::from_millis(value))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(TimeOffset::from_millis(
                    i64::try_from(value).unwrap_or(i64::MAX),
                ))
            }

            fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E> {
                Ok(TimeOffset::from_millis(
                    i64::try_from(value).unwrap_or(if value < 0 { i64::MIN } else { i64::MAX }),
                ))
            }

            fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E> {
                Ok(TimeOffset::from_millis(
                    i64::try_from(value).unwrap_or(i64::MAX),
                ))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                Ok(TimeOffset::from_millis_f64(value))
            }

            fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E> {
                Ok(TimeOffset::from_millis_f64(f64::from(value)))
            }
        }

        deserializer.deserialize_any(TimeOffsetVisitor)
    }
}

impl TimeOffset {
    pub const MIN: Self = Self(i64::MIN);
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(i64::MAX);

    /// Construct from an exact signed millisecond displacement.
    pub const fn from_millis(milliseconds: i64) -> Self {
        Self(milliseconds)
    }

    /// Round milliseconds to the nearest integer, saturating finite and
    /// infinite overflow and mapping NaN to [`Self::ZERO`].
    pub fn from_millis_f64(milliseconds: f64) -> Self {
        if milliseconds.is_nan() {
            return Self::ZERO;
        }
        let milliseconds = milliseconds.round();
        if milliseconds <= i64::MIN as f64 {
            Self::MIN
        } else if milliseconds >= i64::MAX as f64 {
            Self::MAX
        } else {
            Self(milliseconds as i64)
        }
    }

    /// Return the exact signed millisecond displacement.
    pub const fn as_millis(self) -> i64 {
        self.0
    }

    /// Convert this displacement to fractional seconds.
    pub fn as_secs(self) -> f64 {
        self.0 as f64 / 1000.0
    }

    /// Convert a non-negative timeline point into the signed domain,
    /// saturating values beyond [`Self::MAX`].
    pub const fn saturating_from_time(time: Time) -> Self {
        if time.as_millis() > i64::MAX as u64 {
            Self::MAX
        } else {
            Self(time.as_millis() as i64)
        }
    }
}

impl From<i64> for TimeOffset {
    fn from(milliseconds: i64) -> Self {
        Self::from_millis(milliseconds)
    }
}

impl From<TimeOffset> for i64 {
    fn from(offset: TimeOffset) -> Self {
        offset.as_millis()
    }
}

// ─────────────────────────────────────────────────────────────────────
// Time — a point on the project timeline.
// ─────────────────────────────────────────────────────────────────────

/// A point on the project timeline, in integer milliseconds since timeline
/// start. Serialised wire format is a plain non-negative integer; the
/// deserialiser is intentionally lenient — JSON floats round to the nearest
/// millisecond (see [`deserialize_millis_lenient`] / JRB-967). Negative
/// integers are still rejected; negative floats clamp to [`Time::ZERO`]
/// per [`Time::from_millis_f64`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct Time(u64);

impl<'de> Deserialize<'de> for Time {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        deserialize_millis_lenient(d, Time::from_millis, Time::from_millis_f64)
    }
}

impl Time {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(u64::MAX);

    /// Construct from an exact millisecond count.
    pub const fn from_millis(ms: u64) -> Self {
        Self(ms)
    }

    /// Round milliseconds to the nearest integer millisecond. NaN and
    /// negative inputs clamp to [`Time::ZERO`]; values at or above
    /// [`i64::MAX`] ms (and `+∞`) clamp to [`Time::MAX`].
    ///
    /// The upper guard is `>= i64::MAX as f64`, **not** `>= u64::MAX as f64`.
    /// `u64::MAX as f64` rounds up to `2^64`, so the naive guard left a gap:
    /// any finite `f64` in `[2^63, 2^64)` passed the check and then
    /// `as u64`-truncated into a non-canonical near-`u64::MAX` value (e.g.
    /// `2^64 - 8192`) instead of the canonical [`Time::MAX`] sentinel. Such a
    /// value has its high bit set ([`Time::is_negative_sentinel`]) yet escapes
    /// every exact `== Time::MAX` check, leaking out of the wasm boundary as a
    /// raw `u64 > Number.MAX_SAFE_INTEGER` and crashing JS project load
    /// (JRB-1186). No real timeline position approaches `i64::MAX` ms
    /// (~292 million years), so collapsing the whole over-range to the
    /// sentinel is lossless for genuine timestamps.
    pub fn from_millis_f64(ms: f64) -> Self {
        if ms.is_nan() || ms <= 0.0 {
            return Self::ZERO;
        }
        let ms = ms.round();
        if !ms.is_finite() || ms >= i64::MAX as f64 {
            Self::MAX
        } else {
            Self(ms as u64)
        }
    }

    /// Round seconds to the nearest millisecond. NaN and negative inputs
    /// clamp to [`Time::ZERO`]; values at or above [`i64::MAX`] ms (and `+∞`)
    /// clamp to [`Time::MAX`]. See [`Time::from_millis_f64`] for why the upper
    /// guard is `i64::MAX`, not `u64::MAX` (JRB-1186).
    pub fn from_secs(s: f64) -> Self {
        if s.is_nan() || s <= 0.0 {
            return Self::ZERO;
        }
        let ms = (s * 1000.0).round();
        if !ms.is_finite() || ms >= i64::MAX as f64 {
            Self::MAX
        } else {
            Self(ms as u64)
        }
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }

    /// True when this `Time` lies in the "leaked-negative / overflow" region —
    /// the high bit is set (`self.0 > i64::MAX as u64`), i.e. the value equals
    /// a negative `i64` reinterpreted as `u64`.
    ///
    /// [`Time::MAX`] (`-1`) is the canonical ENG-807 negative sentinel, but an
    /// unsigned underflow (`0u64.wrapping_sub(n)`) or an `f64`-overflow cast
    /// can also produce a *near*-`u64::MAX` value such as `2^64 - 8192`
    /// (`= -8192`). Those non-canonical values used to slip past every exact
    /// `== Time::MAX` check and serialize as a raw `u64 > Number.MAX_SAFE_INTEGER`,
    /// crashing JS project load (JRB-1186). No genuine timeline position
    /// approaches `i64::MAX` ms (~292 million years), so treat the entire
    /// high-bit range as the negative sentinel.
    pub const fn is_negative_sentinel(self) -> bool {
        self.0 > i64::MAX as u64
    }

    /// Saturating cast to `u32`. Used at the video/PAG decoder boundary
    /// where the underlying API takes `u32` ms.
    pub const fn as_millis_u32(self) -> u32 {
        if self.0 > u32::MAX as u64 {
            u32::MAX
        } else {
            self.0 as u32
        }
    }

    /// Convert to `f64` seconds. Used at hot-path entries (animation
    /// interpolation, audio sample arithmetic) where downstream math is
    /// already `f64`.
    pub fn as_secs(self) -> f64 {
        self.0 as f64 / 1000.0
    }

    /// Subtract another tick, returning the elapsed duration.
    /// Returns `None` if `rhs > self`.
    pub fn checked_sub(self, rhs: Time) -> Option<Duration> {
        self.0.checked_sub(rhs.0).map(Duration)
    }

    /// Subtract another tick, saturating to zero on underflow.
    pub fn saturating_sub(self, rhs: Time) -> Duration {
        Duration(self.0.saturating_sub(rhs.0))
    }

    /// Subtract a duration from this tick.
    /// Returns `None` if the result would be before timeline start.
    pub fn checked_sub_duration(self, dur: Duration) -> Option<Time> {
        self.0.checked_sub(dur.0).map(Time)
    }

    /// Subtract a duration from this tick, saturating to zero on underflow.
    pub fn saturating_sub_duration(self, dur: Duration) -> Time {
        Time(self.0.saturating_sub(dur.0))
    }

    /// Add a duration to this tick.
    /// Returns `None` if the result would overflow `u64` milliseconds.
    pub fn checked_add_duration(self, dur: Duration) -> Option<Time> {
        self.0.checked_add(dur.0).map(Time)
    }
}

impl From<u64> for Time {
    fn from(ms: u64) -> Self {
        Self(ms)
    }
}

impl From<Time> for u64 {
    fn from(t: Time) -> Self {
        t.0
    }
}

/// `Time + Duration → Time`. Panics on u64 overflow (per
/// `std::ops::Add` convention; use `Time::checked_add_duration` or
/// `Time::saturating_add` if you expect overflow at the timeline edge).
impl Add<Duration> for Time {
    type Output = Time;
    fn add(self, rhs: Duration) -> Time {
        Time(self.0.checked_add(rhs.0).expect("Time + Duration overflow"))
    }
}

/// `Time += Duration`. Panics on u64 overflow (debug + release), mirroring
/// `Add<Duration> for Time`. Use `Time::saturating_add` (combined with
/// reassignment) when overflow at the timeline edge is plausible.
impl AddAssign<Duration> for Time {
    fn add_assign(&mut self, rhs: Duration) {
        self.0 = self
            .0
            .checked_add(rhs.0)
            .expect("Time += Duration overflow");
    }
}

/// `Time -= Duration`. Panics on underflow, mirroring `Sub<Duration>` on
/// `Duration`. Use `Time::checked_sub_duration` (or guard with a comparison
/// against `Time::ZERO`) when the result might be negative.
impl SubAssign<Duration> for Time {
    fn sub_assign(&mut self, rhs: Duration) {
        self.0 = self
            .0
            .checked_sub(rhs.0)
            .expect("Time -= Duration underflow");
    }
}

impl Time {
    pub fn saturating_add(self, rhs: Duration) -> Time {
        Time(self.0.saturating_add(rhs.0))
    }
}

// `Time + Time` is intentionally NOT implemented: adding two
// timeline positions has no meaning. Use `Time + Duration`.

// `Time - Time` is intentionally NOT implemented: callers must
// pick `checked_sub` or `saturating_sub` based on whether underflow is
// expected.

// ─────────────────────────────────────────────────────────────────────
// Duration — a non-negative interval.
// ─────────────────────────────────────────────────────────────────────

/// A non-negative interval in integer milliseconds (`std::time::Duration`-
/// style semantics). Serialised wire format is a plain non-negative integer;
/// the deserialiser is intentionally lenient — JSON floats round to the
/// nearest millisecond (see [`deserialize_millis_lenient`] / JRB-967).
/// Negative integers are still rejected; negative floats clamp to
/// [`Duration::ZERO`] per [`Duration::from_millis_f64`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct Duration(u64);

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        deserialize_millis_lenient(d, Duration::from_millis, Duration::from_millis_f64)
    }
}

impl Duration {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(u64::MAX);

    pub const fn from_millis(ms: u64) -> Self {
        Self(ms)
    }

    /// Round milliseconds to the nearest integer millisecond. NaN and
    /// negative inputs clamp to [`Duration::ZERO`]; values at or above
    /// [`i64::MAX`] ms (and `+∞`) clamp to [`Duration::MAX`].
    ///
    /// The upper guard is `>= i64::MAX as f64`, **not** `>= u64::MAX as f64`,
    /// mirroring the [`Time`] constructors (JRB-1186): `u64::MAX as f64`
    /// rounds up to `2^64`, so the naive guard let any finite `f64` in
    /// `[2^63, 2^64)` through, producing a non-canonical near-`u64::MAX`
    /// duration that escapes every `== Duration::MAX` check and crosses the
    /// wasm/JS boundary as a raw `u64 > Number.MAX_SAFE_INTEGER`. No genuine
    /// duration approaches `i64::MAX` ms (~292 million years), so collapsing
    /// the whole over-range to the sentinel is lossless for real values.
    pub fn from_millis_f64(ms: f64) -> Self {
        if ms.is_nan() || ms <= 0.0 {
            return Self::ZERO;
        }
        let ms = ms.round();
        if !ms.is_finite() || ms >= i64::MAX as f64 {
            Self::MAX
        } else {
            Self(ms as u64)
        }
    }

    /// Round seconds to the nearest millisecond. NaN and negative inputs
    /// clamp to [`Duration::ZERO`]; values at or above [`i64::MAX`] ms (and
    /// `+∞`) clamp to [`Duration::MAX`]. See [`Duration::from_millis_f64`] for
    /// why the upper guard is `i64::MAX`, not `u64::MAX` (JRB-1186).
    pub fn from_secs(s: f64) -> Self {
        if s.is_nan() || s <= 0.0 {
            return Self::ZERO;
        }
        let ms = (s * 1000.0).round();
        if !ms.is_finite() || ms >= i64::MAX as f64 {
            Self::MAX
        } else {
            Self(ms as u64)
        }
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }

    pub fn as_secs(self) -> f64 {
        self.0 as f64 / 1000.0
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn checked_sub(self, rhs: Duration) -> Option<Duration> {
        self.0.checked_sub(rhs.0).map(Duration)
    }

    pub fn saturating_sub(self, rhs: Duration) -> Duration {
        Duration(self.0.saturating_sub(rhs.0))
    }
}

/// `Duration + Duration → Duration`. Panics on u64 overflow per
/// `std::ops::Add` convention; use `Duration::saturating_add` when overflow
/// is plausible.
impl Add<Duration> for Duration {
    type Output = Duration;
    fn add(self, rhs: Duration) -> Duration {
        Duration(
            self.0
                .checked_add(rhs.0)
                .expect("Duration + Duration overflow"),
        )
    }
}

/// `Duration += Duration`. Panics on u64 overflow per `std::ops::AddAssign`
/// convention.
impl AddAssign<Duration> for Duration {
    fn add_assign(&mut self, rhs: Duration) {
        self.0 = self
            .0
            .checked_add(rhs.0)
            .expect("Duration += Duration overflow");
    }
}

impl Duration {
    pub fn saturating_add(self, rhs: Duration) -> Duration {
        Duration(self.0.saturating_add(rhs.0))
    }
}

/// `Duration - Duration → Duration`. Panics on underflow per
/// `std::ops::Sub` convention; use `checked_sub` or `saturating_sub` when
/// the result might be negative.
impl Sub<Duration> for Duration {
    type Output = Duration;
    fn sub(self, rhs: Duration) -> Duration {
        Duration(
            self.0
                .checked_sub(rhs.0)
                .expect("Duration subtraction underflow"),
        )
    }
}

/// `Duration * u32 → Duration`. Saturates at [`Duration::MAX`] on overflow
/// to match the saturating-on-overflow convention of the other arithmetic
/// helpers (`saturating_add`, `from_millis_f64`). Use `saturating_mul`
/// explicitly when the saturation behaviour is load-bearing.
impl Mul<u32> for Duration {
    type Output = Duration;
    fn mul(self, rhs: u32) -> Duration {
        Duration(self.0.saturating_mul(u64::from(rhs)))
    }
}

impl Duration {
    /// Multiply by a floating-point factor, saturating to
    /// [`Duration::MAX`] on overflow. Modeled on
    /// `std::time::Duration::mul_f64`. Negative or non-finite factors
    /// clamp to [`Duration::ZERO`].
    pub fn mul_f64(self, rhs: f64) -> Duration {
        if !rhs.is_finite() || rhs <= 0.0 {
            return Duration::ZERO;
        }
        let scaled = self.0 as f64 * rhs;
        // Guard against the `[2^63, 2^64)` gap: `u64::MAX as f64` rounds up to
        // `2^64`, so any finite product in that range would cast to a
        // non-canonical near-MAX value instead of saturating. Clamp at
        // `i64::MAX as f64` to match the `Duration`/`Time` constructor
        // convention and keep [`Duration::MAX`] the single overflow sentinel.
        if !scaled.is_finite() || scaled >= i64::MAX as f64 {
            Duration::MAX
        } else {
            Duration(scaled as u64)
        }
    }

    /// Ratio of `self` to `rhs` as `f64`. Returns `f64::INFINITY` when
    /// `self` is non-zero and `rhs` is zero, and `f64::NAN` when both
    /// are zero (IEEE-754 0/0). Callers on hot animation paths should
    /// guard against `NaN` propagation — see [`Duration::is_zero`].
    pub fn div_duration_f64(self, rhs: Duration) -> f64 {
        self.0 as f64 / rhs.0 as f64
    }
}

// ─────────────────────────────────────────────────────────────────────
impl From<std::time::Duration> for Duration {
    fn from(d: std::time::Duration) -> Self {
        Self(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}

impl From<Duration> for std::time::Duration {
    fn from(d: Duration) -> Self {
        std::time::Duration::from_millis(d.0)
    }
}

// ─────────────────────────────────────────────────────────────────────
// serde_secs — fractional-seconds wire format adapters.
// ─────────────────────────────────────────────────────────────────────

/// Serde adapters for fields whose **wire format is fractional seconds**
/// (`f64`) rather than this crate's transparent integer-millisecond
/// encoding — e.g. the persisted `fx_composition` document schema
/// (`Composition.duration`, `MediaLayer.startTime`).
///
/// Apply with `#[serde(with = "time_types::serde_secs::time")]` (or
/// `::duration`). The field keeps the seconds-typed JSON the frontend and
/// stored documents already use, while the Rust side gets a typed
/// [`Time`] / [`Duration`].
///
/// **Precision contract:** values are stored as integer milliseconds, so
/// deserialization rounds to the nearest millisecond ([`Time::from_secs`] /
/// [`Duration::from_secs`], which also clamp NaN/negative to zero — matching
/// the `.max(0.0)` clamps the former f64 consumers applied). Round-tripping a
/// document therefore quantizes sub-millisecond values; millisecond-aligned
/// values round-trip exactly (`1.25` → 1250 ms → `1.25`).
pub mod serde_secs {
    use serde::{Deserialize, Deserializer, Serializer};

    /// `#[serde(with = "time_types::serde_secs::time")]` — [`super::Time`]
    /// as fractional seconds on the wire.
    pub mod time {
        use super::*;
        use crate::time::Time;

        pub fn serialize<S: Serializer>(value: &Time, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_f64(value.as_secs())
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Time, D::Error> {
            f64::deserialize(deserializer).map(Time::from_secs)
        }
    }

    /// `#[serde(with = "time_types::serde_secs::duration")]` —
    /// [`super::Duration`] as fractional seconds on the wire.
    pub mod duration {
        use super::*;
        use crate::time::Duration;

        pub fn serialize<S: Serializer>(
            value: &Duration,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            serializer.serialize_f64(value.as_secs())
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Duration, D::Error> {
            f64::deserialize(deserializer).map(Duration::from_secs)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// TimeRange — half-open [start, end) interval.
// ─────────────────────────────────────────────────────────────────────

/// Half-open `[start, end)` interval. `contains(t)` is true iff
/// `start <= t < end` — same convention as Vulcan's active-segment lookup,
/// and the project model's frame-on-boundary semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TimeRange {
    pub start: Time,
    pub end: Time,
}

impl TimeRange {
    pub const fn new(start: Time, end: Time) -> Self {
        Self { start, end }
    }

    pub fn from_millis(start_ms: u64, end_ms: u64) -> Self {
        debug_assert!(
            end_ms >= start_ms,
            "TimeRange::from_millis constructed inverted: start={start_ms} > end={end_ms}"
        );
        Self {
            start: Time(start_ms),
            end: Time(end_ms),
        }
    }

    /// Half-open: `[start, end)`. A frame at exactly `end` is NOT in the
    /// range; it belongs to the next segment.
    pub fn contains(&self, t: Time) -> bool {
        self.start <= t && t < self.end
    }

    /// Saturating duration. Returns zero if `end < start` (degenerate).
    pub fn duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }

    /// Half-open intersection. Returns `None` when the ranges only touch at an
    /// edge or do not overlap.
    pub fn intersection(&self, other: TimeRange) -> Option<TimeRange> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (end > start).then(|| TimeRange::new(start, end))
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

// ─────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_serde_transparent_integer() {
        let t = Time::from_millis(1500);
        assert_eq!(serde_json::to_string(&t).unwrap(), "1500");
        let parsed: Time = serde_json::from_str("1500").unwrap();
        assert_eq!(parsed, t);
    }

    #[test]
    fn time_offset_preserves_negative_milliseconds_and_lenient_floats() {
        let offset = TimeOffset::from_millis(-1500);
        assert_eq!(serde_json::to_string(&offset).unwrap(), "-1500");
        assert_eq!(serde_json::from_str::<TimeOffset>("-1500").unwrap(), offset);
        assert_eq!(
            serde_json::from_str::<TimeOffset>("-1500.4").unwrap(),
            offset
        );
        assert_eq!(
            serde_json::from_str::<TimeOffset>("1500.6").unwrap(),
            TimeOffset::from_millis(1501)
        );
    }

    #[test]
    fn time_offset_float_conversion_saturates_and_maps_nan_to_zero() {
        assert_eq!(TimeOffset::from_millis_f64(f64::NAN), TimeOffset::ZERO);
        assert_eq!(
            TimeOffset::from_millis_f64(f64::NEG_INFINITY),
            TimeOffset::MIN
        );
        assert_eq!(TimeOffset::from_millis_f64(f64::INFINITY), TimeOffset::MAX);
        assert_eq!(TimeOffset::saturating_from_time(Time::MAX), TimeOffset::MAX);
    }

    #[test]
    fn duration_serde_transparent_integer() {
        let d = Duration::from_millis(500);
        assert_eq!(serde_json::to_string(&d).unwrap(), "500");
        let parsed: Duration = serde_json::from_str("500").unwrap();
        assert_eq!(parsed, d);
    }

    #[test]
    fn range_serde_camelcase() {
        let r = TimeRange::from_millis(100, 500);
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"start":100,"end":500}"#);
        let parsed: TimeRange = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn range_intersection_is_half_open() {
        assert_eq!(
            TimeRange::from_millis(100, 500).intersection(TimeRange::from_millis(300, 700)),
            Some(TimeRange::from_millis(300, 500))
        );
        assert_eq!(
            TimeRange::from_millis(100, 500).intersection(TimeRange::from_millis(500, 700)),
            None
        );
        assert_eq!(
            TimeRange::from_millis(100, 500).intersection(TimeRange::from_millis(0, 100)),
            None
        );
    }

    #[test]
    fn from_secs_rounds_to_nearest_ms() {
        assert_eq!(Time::from_secs(1.5), Time::from_millis(1500));
        assert_eq!(Time::from_secs(0.0005), Time::from_millis(1));
        assert_eq!(Time::from_secs(0.0004), Time::ZERO);
    }

    #[test]
    fn from_secs_clamps_invalid_inputs() {
        assert_eq!(Time::from_secs(-1.0), Time::ZERO);
        assert_eq!(Time::from_secs(f64::NAN), Time::ZERO);
        assert_eq!(Time::from_secs(f64::INFINITY), Time::MAX);
        assert_eq!(Time::from_secs(f64::NEG_INFINITY), Time::ZERO);
    }

    #[test]
    fn from_millis_f64_rounds_and_clamps_invalid_inputs() {
        assert_eq!(Time::from_millis_f64(1.5), Time::from_millis(2));
        assert_eq!(Time::from_millis_f64(-1.0), Time::ZERO);
        assert_eq!(Time::from_millis_f64(f64::NAN), Time::ZERO);
        assert_eq!(Time::from_millis_f64(f64::INFINITY), Time::MAX);

        assert_eq!(Duration::from_millis_f64(1.5), Duration::from_millis(2));
        assert_eq!(Duration::from_millis_f64(-1.0), Duration::ZERO);
        assert_eq!(Duration::from_millis_f64(f64::NAN), Duration::ZERO);
        assert_eq!(Duration::from_millis_f64(f64::INFINITY), Duration::MAX);
    }

    #[test]
    fn is_negative_sentinel_covers_high_bit_region() {
        // Genuine timeline positions never set the high bit.
        assert!(!Time::ZERO.is_negative_sentinel());
        assert!(!Time::from_millis(60 * 60 * 1000).is_negative_sentinel());
        assert!(!Time::from_millis(i64::MAX as u64).is_negative_sentinel());
        // The canonical sentinel and any near-MAX underflow/overflow artifact do.
        assert!(Time::MAX.is_negative_sentinel());
        assert!(Time::from_millis(i64::MAX as u64 + 1).is_negative_sentinel());
        // 2^64 - 8192 == -8192 reinterpreted as u64 (the JRB-1186 value).
        assert!(Time::from_millis(18_446_744_073_709_543_424).is_negative_sentinel());
    }

    #[test]
    fn integer_deserialize_saturates_high_bit_to_canonical_max() {
        // JRB-1186 re-entered via the integer wire path: a raw u64 in
        // `(i64::MAX, u64::MAX)` must collapse to the canonical `MAX`
        // sentinel, not a non-canonical near-MAX value that
        // `is_negative_sentinel` flags yet `== MAX` misses.
        let gap = 18_446_744_073_709_543_424_u64; // 2^64 - 8192
        let t: Time = serde_json::from_str(&gap.to_string()).unwrap();
        assert_eq!(t, Time::MAX);
        let d: Duration = serde_json::from_str(&gap.to_string()).unwrap();
        assert_eq!(d, Duration::MAX);
        // A genuine in-range integer still deserializes exactly.
        let ok: Time = serde_json::from_str("1500").unwrap();
        assert_eq!(ok, Time::from_millis(1500));
        assert!(!ok.is_negative_sentinel());
    }

    #[test]
    fn wide_integer_deserialize_canonicalizes_high_bit_values() {
        use serde::de::value::{Error, I128Deserializer, U128Deserializer};

        fn time_from_u128(value: u128) -> Result<Time, Error> {
            Time::deserialize(U128Deserializer::new(value))
        }
        fn duration_from_u128(value: u128) -> Result<Duration, Error> {
            Duration::deserialize(U128Deserializer::new(value))
        }
        fn time_from_i128(value: i128) -> Result<Time, Error> {
            Time::deserialize(I128Deserializer::new(value))
        }
        fn duration_from_i128(value: i128) -> Result<Duration, Error> {
            Duration::deserialize(I128Deserializer::new(value))
        }

        let boundary_u128 = i64::MAX as u128;
        assert_eq!(
            time_from_u128(boundary_u128).unwrap(),
            Time::from_millis(i64::MAX as u64)
        );
        assert_eq!(
            duration_from_u128(boundary_u128).unwrap(),
            Duration::from_millis(i64::MAX as u64)
        );
        assert_eq!(time_from_u128(boundary_u128 + 1).unwrap(), Time::MAX);
        assert_eq!(
            duration_from_u128(boundary_u128 + 1).unwrap(),
            Duration::MAX
        );
        assert_eq!(time_from_u128(u128::MAX).unwrap(), Time::MAX);
        assert_eq!(duration_from_u128(u128::MAX).unwrap(), Duration::MAX);

        let boundary_i128 = i64::MAX as i128;
        assert_eq!(
            time_from_i128(boundary_i128).unwrap(),
            Time::from_millis(i64::MAX as u64)
        );
        assert_eq!(
            duration_from_i128(boundary_i128).unwrap(),
            Duration::from_millis(i64::MAX as u64)
        );
        assert_eq!(time_from_i128(boundary_i128 + 1).unwrap(), Time::MAX);
        assert_eq!(
            duration_from_i128(boundary_i128 + 1).unwrap(),
            Duration::MAX
        );
        assert_eq!(time_from_i128(i128::MAX).unwrap(), Time::MAX);
        assert_eq!(duration_from_i128(i128::MAX).unwrap(), Duration::MAX);

        assert!(time_from_i128(-1).is_err());
        assert!(duration_from_i128(-1).is_err());
    }

    #[test]
    fn float_constructors_saturate_overflow_gap_to_canonical_max() {
        // JRB-1186: `u64::MAX as f64` rounds up to 2^64, so the old
        // `>= u64::MAX as f64` guard let finite f64 values in [2^63, 2^64)
        // through, where `as u64` truncated them into a non-canonical
        // near-MAX value (e.g. 2^64 - 8192) instead of `Time::MAX`. That
        // value escaped every `== Time::MAX` check and crashed JS load.
        // The whole over-range must now saturate to the canonical sentinel.
        let gap_ms = 18_446_744_073_709_543_424.0_f64; // 2^64 - 8192
        assert_eq!(Time::from_millis_f64(gap_ms), Time::MAX);
        // And via the seconds entry points (a corrupt huge duration).
        let gap_secs = 1.8e16_f64;
        assert_eq!(Time::from_secs(gap_secs), Time::MAX);
        // Any f64 ms at/above the i64::MAX guard collapses to the canonical
        // sentinel — never a near-MAX value that slips past
        // `is_negative_sentinel`.
        for ms in [9.3e18_f64, 1e19_f64, gap_ms, 1.5e19_f64] {
            assert_eq!(Time::from_millis_f64(ms), Time::MAX);
        }
        // Values below the guard still cast cleanly to a real (non-sentinel)
        // timeline position.
        let safe_ms = (i64::MAX as f64) - 8192.0;
        let safe = Time::from_millis_f64(safe_ms);
        assert!(!safe.is_negative_sentinel());
        assert_ne!(safe, Time::MAX);
        assert!(!Time::from_millis_f64(1e18).is_negative_sentinel());
    }

    #[test]
    fn duration_float_constructors_saturate_overflow_gap_to_max() {
        // Same JRB-1186 gap on the `Duration` twin: finite f64 values in
        // [2^63, 2^64) must saturate to the canonical `Duration::MAX` rather
        // than producing a non-canonical near-MAX duration that escapes
        // `== Duration::MAX` and crosses the wasm boundary as a raw
        // `u64 > Number.MAX_SAFE_INTEGER`.
        let gap_ms = 18_446_744_073_709_543_424.0_f64; // 2^64 - 8192
        assert_eq!(Duration::from_millis_f64(gap_ms), Duration::MAX);
        for ms in [9.3e18_f64, 1e19_f64, gap_ms, 1.5e19_f64] {
            assert_eq!(Duration::from_millis_f64(ms), Duration::MAX);
        }
        assert_eq!(Duration::from_secs(1.8e16_f64), Duration::MAX);
        assert_eq!(Duration::from_secs(f64::INFINITY), Duration::MAX);
        // Below the guard still casts to a real (non-saturated) duration.
        let safe_ms = (i64::MAX as f64) - 8192.0;
        let safe = Duration::from_millis_f64(safe_ms);
        assert_ne!(safe, Duration::MAX);
        assert!(safe.as_millis() < i64::MAX as u64);
    }

    #[test]
    fn mul_f64_saturates_overflow_gap_to_max() {
        // JRB-1186 class: the product `(u64::MAX / 2) * 3.0` lands in the
        // `[2^63, 2^64)` gap where `u64::MAX as f64` rounds up to 2^64. The old
        // `>= u64::MAX as f64` guard let it through and `as u64` truncated it to
        // a non-canonical near-MAX value. It must saturate to `Duration::MAX`.
        assert_eq!(
            Duration::from_millis(u64::MAX / 2).mul_f64(3.0),
            Duration::MAX
        );
        // Directly exercise the `[2^63, 2^64)` gap: this product lands *inside*
        // the range the old `>= u64::MAX as f64` guard let through, so it fails
        // without the `>= i64::MAX as f64` fix (would cast to a non-canonical
        // near-MAX Duration instead of saturating).
        let gap = Duration::from_millis(u64::MAX / 2).mul_f64(1.5);
        assert_eq!(gap, Duration::MAX);
        // Non-finite / non-positive factors clamp to zero per the contract.
        assert_eq!(
            Duration::from_millis(1000).mul_f64(f64::NAN),
            Duration::ZERO
        );
        assert_eq!(
            Duration::from_millis(1000).mul_f64(f64::INFINITY),
            Duration::ZERO
        );
        assert_eq!(Duration::from_millis(1000).mul_f64(-1.0), Duration::ZERO);
        assert_eq!(Duration::from_millis(1000).mul_f64(0.0), Duration::ZERO);
        // In-range factors scale normally without touching the sentinel.
        let scaled = Duration::from_millis(1000).mul_f64(2.5);
        assert_eq!(scaled, Duration::from_millis(2500));
        assert_ne!(scaled, Duration::MAX);
    }

    #[test]
    fn div_duration_f64_ratio_and_zero_contract() {
        assert_eq!(
            Duration::from_millis(3000).div_duration_f64(Duration::from_millis(1000)),
            3.0
        );
        // Non-zero / zero → +inf; 0 / 0 → NaN (IEEE-754), per the doc contract.
        assert!(Duration::from_millis(1000)
            .div_duration_f64(Duration::ZERO)
            .is_infinite());
        assert!(Duration::ZERO.div_duration_f64(Duration::ZERO).is_nan());
    }

    #[test]
    fn as_millis_u32_saturates() {
        let big = Time::from_millis(u64::from(u32::MAX) + 1);
        assert_eq!(big.as_millis_u32(), u32::MAX);
        assert_eq!(Time::from_millis(1500).as_millis_u32(), 1500);
    }

    #[test]
    fn tick_minus_tick_returns_duration() {
        let a = Time::from_millis(2000);
        let b = Time::from_millis(500);
        assert_eq!(a.checked_sub(b), Some(Duration::from_millis(1500)));
        assert_eq!(b.checked_sub(a), None);
        assert_eq!(b.saturating_sub(a), Duration::ZERO);
    }

    #[test]
    fn tick_plus_duration_returns_tick() {
        let t = Time::from_millis(500);
        let d = Duration::from_millis(750);
        assert_eq!(t + d, Time::from_millis(1250));
    }

    #[test]
    fn duration_arithmetic() {
        let a = Duration::from_millis(300);
        let b = Duration::from_millis(200);
        assert_eq!(a + b, Duration::from_millis(500));
        assert_eq!(a - b, Duration::from_millis(100));
        assert_eq!(a * 3, Duration::from_millis(900));
        assert_eq!(b.checked_sub(a), None);
        assert_eq!(b.saturating_sub(a), Duration::ZERO);
    }

    #[test]
    #[should_panic(expected = "Duration subtraction underflow")]
    fn duration_subtraction_panics_on_underflow() {
        let _ = Duration::from_millis(100) - Duration::from_millis(200);
    }

    #[test]
    fn std_duration_interop() {
        let our = Duration::from_millis(1234);
        let std: std::time::Duration = our.into();
        assert_eq!(std.as_millis(), 1234);
        let back: Duration = std.into();
        assert_eq!(back, our);
    }

    #[test]
    fn duration_mul_saturates_on_overflow() {
        assert_eq!(Duration::MAX * 2, Duration::MAX);
        assert_eq!(Duration::from_millis(u64::MAX) * u32::MAX, Duration::MAX);
        assert_eq!(Duration::from_millis(10) * 3, Duration::from_millis(30));
    }

    #[test]
    fn duration_from_std_clamps_on_u128_overflow() {
        let huge = std::time::Duration::from_secs(u64::MAX);
        assert_eq!(Duration::from(huge), Duration::MAX);
    }

    #[test]
    fn range_contains_half_open() {
        let r = TimeRange::from_millis(100, 500);
        assert!(!r.contains(Time::from_millis(99)));
        assert!(r.contains(Time::from_millis(100)));
        assert!(r.contains(Time::from_millis(499)));
        assert!(!r.contains(Time::from_millis(500)));
    }

    #[test]
    fn range_duration() {
        let r = TimeRange::from_millis(100, 500);
        assert_eq!(r.duration(), Duration::from_millis(400));
        assert!(!r.is_empty());

        let degenerate = TimeRange::from_millis(500, 500);
        assert_eq!(degenerate.duration(), Duration::ZERO);
        assert!(degenerate.is_empty());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn time_deserializes_fractional_milliseconds_jrb_967() {
        // JRB-967: studio clients occasionally serialize timing values as
        // floats (e.g. `27092.19999998808` from float-ms arithmetic). The
        // canonical wire shape is still an integer, but we round on the way
        // in so a client-side rounding bug stops cascading into a hard
        // `invalid type: floating point ..., expected u64` failure.
        assert_eq!(
            serde_json::from_str::<Time>("27092.19999998808").unwrap(),
            Time::from_millis(27092)
        );
        assert_eq!(
            serde_json::from_str::<Time>("42572.90000003576").unwrap(),
            Time::from_millis(42573)
        );
        assert_eq!(
            serde_json::from_str::<Time>("6958.300000011921").unwrap(),
            Time::from_millis(6958)
        );
        // Integer wire form still works (no behavior regression).
        assert_eq!(
            serde_json::from_str::<Time>("27092").unwrap(),
            Time::from_millis(27092)
        );
        // Negative *floats* clamp to zero, mirroring `Time::from_millis_f64`
        // (the lenient path exists to absorb JS float jitter, including
        // sub-zero rounding noise).
        assert_eq!(serde_json::from_str::<Time>("-5.5").unwrap(), Time::ZERO);
        // Negative *integers* on the wire are still rejected: that's a real
        // producer bug, not float jitter, and we keep the prior schema's
        // strictness for whole integers (cap-code-bot review on JRB-967).
        let err = serde_json::from_str::<Time>("-5")
            .expect_err("negative integer ms should fail to deserialize");
        assert!(
            err.to_string().contains("non-negative"),
            "expected non-negative-millisecond error, got: {err}"
        );
        // Same for Duration.
        let err = serde_json::from_str::<Duration>("-1")
            .expect_err("negative integer ms should fail to deserialize");
        assert!(
            err.to_string().contains("non-negative"),
            "expected non-negative-millisecond error, got: {err}"
        );
    }

    #[test]
    fn duration_deserializes_fractional_milliseconds_jrb_967() {
        assert_eq!(
            serde_json::from_str::<Duration>("27092.19999998808").unwrap(),
            Duration::from_millis(27092)
        );
        assert_eq!(
            serde_json::from_str::<Duration>("500").unwrap(),
            Duration::from_millis(500)
        );
        assert_eq!(
            serde_json::from_str::<Duration>("-1.0").unwrap(),
            Duration::ZERO
        );
    }

    #[test]
    fn time_range_deserializes_fractional_milliseconds_jrb_967() {
        // The wrapper struct should inherit the lenient behavior.
        let r: TimeRange = serde_json::from_str(r#"{"start":100.7,"end":500.4}"#).unwrap();
        assert_eq!(r, TimeRange::from_millis(101, 500));
    }

    #[test]
    fn round_trip_through_u64_for_ffi() {
        let t = Time::from_millis(42);
        let raw: u64 = t.into();
        let back: Time = raw.into();
        assert_eq!(t, back);
    }

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct SecsDoc {
        #[serde(with = "crate::time::serde_secs::time")]
        start: Time,
        #[serde(with = "crate::time::serde_secs::duration")]
        duration: Duration,
    }

    #[test]
    fn serde_secs_uses_fractional_seconds_on_the_wire() {
        let doc = SecsDoc {
            start: Time::from_millis(1250),
            duration: Duration::from_millis(2500),
        };
        let json = serde_json::to_value(&doc).unwrap();
        assert_eq!(json, serde_json::json!({ "start": 1.25, "duration": 2.5 }));

        let back: SecsDoc = serde_json::from_value(json).unwrap();
        assert_eq!(back, doc);
    }

    #[test]
    fn serde_secs_accepts_integer_seconds_and_rounds_to_millis() {
        // Stored documents write whole seconds as integers (`"duration": 1`).
        let doc: SecsDoc = serde_json::from_str(r#"{"start":2,"duration":1}"#).unwrap();
        assert_eq!(doc.start, Time::from_millis(2000));
        assert_eq!(doc.duration, Duration::from_millis(1000));

        // Sub-millisecond input quantizes to the nearest millisecond.
        let doc: SecsDoc =
            serde_json::from_str(r#"{"start":0.0014999,"duration":0.0015}"#).unwrap();
        assert_eq!(doc.start, Time::from_millis(1));
        assert_eq!(doc.duration, Duration::from_millis(2));
    }

    #[test]
    fn serde_secs_clamps_negative_to_zero() {
        // Mirrors the `.max(0.0)` clamp the former f64 consumers applied.
        let doc: SecsDoc = serde_json::from_str(r#"{"start":-1.0,"duration":-2.5}"#).unwrap();
        assert_eq!(doc.start, Time::ZERO);
        assert_eq!(doc.duration, Duration::ZERO);
    }
}
