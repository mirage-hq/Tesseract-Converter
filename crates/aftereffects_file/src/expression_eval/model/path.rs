use super::*;

impl Model {
    pub(super) fn push_path(
        &mut self,
        comp_id: u32,
        layer: &Layer,
        identity: PropertyIdentity,
        run: &[Chunk],
        scale: [f64; 2],
        mask: bool,
    ) -> usize {
        let slot = self.properties.len();
        let mut property = Property {
            comp_id,
            layer_id: layer.record.id(),
            identity,
            initial: Vec::new(),
            keys: Vec::new(),
            expression: None,
            error: None,
            expression_enabled: false,
            notes: Vec::new(),
            range: None,
            text: None,
            text_style: None,
            text_keys: Vec::new(),
            text_bounds: None,
            path: None,
            path_keys: Vec::new(),
        };
        let decoded = (|| -> Result<(), String> {
            if scale.iter().any(|size| !size.is_finite() || *size <= 0.0) {
                return Err("invalid Path API source dimensions".into());
            }
            let storage = properties::unique_list(run, *b"om-s").map_err(|e| e.to_string())?;
            let leaf = properties::unique_list(storage, *b"tdbs").map_err(|e| e.to_string())?;
            let metadata = properties::read_path_metadata(leaf).map_err(|e| e.to_string())?;
            property.expression_enabled = metadata.expression_enabled;
            if mask {
                let flags = properties::data(leaf, *b"tdsb").map_err(|e| e.to_string())?;
                if flags.len() != 4 || flags[0] != 0 {
                    return Err("RotoBezier Path API geometry is unsupported".into());
                }
            }
            let shapes = properties::unique_list(storage, *b"omks").map_err(|e| e.to_string())?;
            if shapes.len() != metadata.keyframes.len().max(1)
                || shapes
                    .iter()
                    .any(|shape| shape.list_kind() != Some(*b"shap"))
            {
                return Err(
                    "Path timing/geometry counts or variable feather are unsupported".into(),
                );
            }
            let values = shapes
                .iter()
                .map(|shape| {
                    properties::read_native_path(
                        shape.children().ok_or("missing Path shape")?,
                        scale,
                    )
                    .map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let first = values.first().ok_or("missing native Path value")?.clone();
            property.path = Some(first.clone());
            let start = layer
                .record
                .start_time()
                .ok_or("invalid Path dependency start time")?;
            let stretch = layer
                .record
                .stretch()
                .ok_or("invalid Path dependency stretch")?;
            if !start.is_finite() || !stretch.is_finite() || stretch <= 0.0 {
                return Err("unsupported Path dependency clock".into());
            }
            let mut previous = None;
            for (index, (key, value)) in metadata.keyframes.iter().zip(values).enumerate() {
                if value.closed != first.closed || value.vertices.len() != first.vertices.len() {
                    return Err("changing native Path dependency topology is unsupported".into());
                }
                if index > 0 {
                    let from = &metadata.keyframes[index - 1];
                    if from.out_interpolation != 3
                        && !(from.out_interpolation == 1 && key.in_interpolation == 1)
                    {
                        return Err("Path dependency temporal easing outside Linear/Hold is not yet evaluated".into());
                    }
                }
                let time = start + key.time_secs * stretch;
                if !time.is_finite() || previous.is_some_and(|previous| time <= previous) {
                    return Err("invalid native Path key clock".into());
                }
                previous = Some(time);
                property.path_keys.push(PathKey {
                    time,
                    value,
                    hold: key.out_interpolation == 3,
                });
            }
            if metadata.expression_enabled {
                let source = std::str::from_utf8(
                    properties::expression_data(leaf).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                property.expression = Some(syntax::compile(source).map_err(|e| e.to_string())?);
            }
            Ok(())
        })();
        if let Err(error) = decoded {
            property.error = Some(error);
        }
        self.properties.push(property);
        slot
    }
}
