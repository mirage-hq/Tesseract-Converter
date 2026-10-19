//! Lower a bounded marker-driven hold/reverse program to four editable clock knots.
use crate::{
    properties::{NumericKeyframe, NumericProperty},
    structure::{Composition, Layer},
};

const PROGRAM: &str = r#"try {
  var k1 = key(1).time, k2 = key(2).time;
  var nm = thisComp.marker.numKeys;
  if (nm >= 2) {
    var mIn = thisComp.marker.key(1).time;
    var mOut = thisComp.marker.key(nm).time;
    if (time <= k1) { 0; }
    else if (time > k1 && time < k2 && time < mIn) { linear(time, 0, mIn, k1, k2); }
    else if (time >= mIn && time <= mOut) { k2; }
    else if (time > mOut) { linear(time, mOut, mOut + (mIn - k1), k2, k1); }
  } else { value; }
} catch(e) { value; }"#;

pub(super) fn lower(
    layer: &Layer,
    composition: &Composition,
    numeric: &mut NumericProperty,
) -> Result<bool, String> {
    if !numeric.expression_enabled {
        return Ok(false);
    }
    let property = super::time_remap_property(layer)
        .map_err(|error| error.to_string())?
        .ok_or("missing Time Remap property")?;
    let expression =
        super::super::control_links::expression(property).map_err(|error| error.to_string())?;
    if !expression
        .chars()
        .filter(|c| !c.is_whitespace())
        .eq(PROGRAM.chars().filter(|c| !c.is_whitespace()))
    {
        return Ok(false);
    }
    // key(n).time is composition time in AE. This profile has a complete,
    // continuous piecewise-linear graph only for a zero-origin unit clock and
    // k1=0, 0<mIn<=k2; other clocks/branch gaps must not silently become ramps.
    if layer.record.start_time() != Some(0.0) || layer.record.stretch() != Some(1.0) {
        return Err("marker hold/reverse program requires a zero-origin unit layer clock".into());
    }
    let markers = composition
        .marker_times
        .as_ref()
        .map_err(|error| format!("composition markers: {error}"))?;
    if markers.len() < 2 {
        // The program returns the authored property, not a synthesized clock.
        numeric.expression_enabled = false;
        return Ok(true);
    }
    let (Some(first), Some(second)) = (numeric.keyframes.first(), numeric.keyframes.get(1)) else {
        return Err("marker hold/reverse program requires two authored key times".into());
    };
    let k2 = second.time_secs;
    let m_in = markers[0];
    let m_out = markers[markers.len() - 1];
    let end = m_out + m_in;
    if first.time_secs != 0.0
        || !k2.is_finite()
        || !m_in.is_finite()
        || !m_out.is_finite()
        || !end.is_finite()
        || m_in <= 0.0
        || m_in > k2
        || m_out <= m_in
    {
        return Err(
            "marker hold/reverse program has unsupported key/marker order or branch gaps".into(),
        );
    }
    numeric.keyframes = [(0.0, 0.0), (m_in, k2), (m_out, k2), (end, 0.0)]
        .into_iter()
        .map(|(time_secs, value)| NumericKeyframe {
            time_secs,
            values: vec![value],
            in_interpolation: 1,
            out_interpolation: 1,
            in_speed: Vec::new(),
            in_influence: Vec::new(),
            out_speed: Vec::new(),
            out_influence: Vec::new(),
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        })
        .collect();
    numeric.expression_enabled = false;
    Ok(true)
}
