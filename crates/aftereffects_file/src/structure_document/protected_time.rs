//! Bounded leading protected-region clocks for stretched precomposition occurrences.
use fx_schema::Time;

use crate::{
    properties::{data, root_runs, unique_list},
    structure::Layer,
};

/// Preserve a single leading protected region at unit speed; distribute the
/// occurrence's remaining duration over the unprotected source tail. Marker
/// names are display-only and must not select the mapping.
pub(super) fn clock(layer: &Layer) -> Result<Option<Vec<(Time, Time)>>, String> {
    lower(layer)
        .map_err(|error| format!("protected-region source clock: {error}; affine timing retained"))
}

fn lower(layer: &Layer) -> Result<Option<Vec<(Time, Time)>>, String> {
    let runs = root_runs(&layer.content).map_err(|error| error.to_string())?;
    let mut markers = runs.iter().filter(|(name, _)| *name == "ADBE Marker");
    let Some((_, run)) = markers.next() else {
        return Ok(None);
    };
    if markers.next().is_some() {
        return Err("duplicate marker property".into());
    }
    let wrapper = unique_list(run, *b"mrst").map_err(|error| error.to_string())?;
    let records = unique_list(wrapper, *b"mrky").map_err(|error| error.to_string())?;
    let records: Vec<_> = records
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"Nmrd"))
        .collect();
    let mut protected = Vec::new();
    for record in &records {
        let header = data(record.children().ok_or("opaque marker value")?, *b"NmHd")
            .map_err(|error| error.to_string())?;
        if header.len() != 20 {
            return Err("unsupported marker value layout".into());
        }
        let flags = u32::from_be_bytes(header[..4].try_into().expect("checked marker length"));
        if flags & 2 != 0 {
            if flags != 2 {
                return Err("unsupported protected marker flags".into());
            }
            let ticks =
                i32::from_be_bytes(header[8..12].try_into().expect("checked marker length"));
            let divisor =
                u32::from_be_bytes(header[12..16].try_into().expect("checked marker length"));
            if ticks <= 0 || divisor == 0 {
                return Err("invalid protected marker duration".into());
            }
            protected.push(f64::from(ticks) / f64::from(divisor));
        }
    }
    if protected.is_empty() {
        return Ok(None);
    }
    if records.len() != 1 || protected.len() != 1 {
        return Err("only a single leading protected region is currently mapped".into());
    }
    let property = unique_list(wrapper, *b"tdbs").map_err(|error| error.to_string())?;
    let meta = data(property, *b"tdb4").map_err(|error| error.to_string())?;
    let list = unique_list(property, *b"list").map_err(|error| error.to_string())?;
    let header = data(list, *b"lhd3").map_err(|error| error.to_string())?;
    let keys = data(list, *b"ldat").map_err(|error| error.to_string())?;
    if meta.len() != 124
        || meta[..8] != [0xdb, 0x99, 0, 1, 0, 0, 0, 0]
        || meta[12..16] == [0; 4]
        || header.len() != 52
        || header[..4] != [0, 0xd0, 0x0b, 0xee]
        || header[10..12] != [0, 1]
        || header[18..20] != [0, 16]
        || header[23] != 4
        || keys.len() != 16
    {
        return Err("unsupported protected marker clock/list layout".into());
    }
    // The first four bytes of each 16-byte marker key are signed time ticks,
    // not an envelope prefix. Only source-zero markers fit this bounded mapping.
    let marker_ticks = i32::from_be_bytes(keys[..4].try_into().expect("checked marker key length"));
    if marker_ticks != 0 {
        return Err("nonleading protected marker is not mapped".into());
    }
    let (Some(start), Some(input), Some(output), Some(stretch)) = (
        layer.record.start_time(),
        layer.record.in_point(),
        layer.record.out_point(),
        layer.record.stretch(),
    ) else {
        return Err("invalid occurrence clock".into());
    };
    let protected = protected[0];
    let end = start + output * stretch;
    if start < 0.0
        || input != 0.0
        || stretch <= 0.0
        || output <= protected
        || end <= start + protected
    {
        return Err("trimmed, reversed or over-compressed protected occurrence".into());
    }
    let knots: Vec<_> = [(start, 0.0), (start + protected, protected), (end, output)]
        .into_iter()
        .map(|(time, value)| (Time::from_secs(time), Time::from_secs(value)))
        .collect();
    if knots
        .windows(2)
        .any(|pair| pair[0].0 >= pair[1].0 || pair[0].1 >= pair[1].1)
    {
        return Err("protected clock knots collapse at millisecond precision".into());
    }
    Ok(Some(knots))
}
