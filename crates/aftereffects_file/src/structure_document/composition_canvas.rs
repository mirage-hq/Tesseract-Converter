use super::*;

impl Converter<'_> {
    pub(super) fn retain_blend_canvas(
        &mut self,
        comp_id: u32,
        comp: &Composition,
        target: &mut GroupLayer,
    ) -> Result<(), DocumentError> {
        if !comp.layers.iter().any(|layer| {
            compositing::blend_mode(layer.record.blend_mode())
                .is_some_and(|mode| mode != fx_schema::BlendMode::Normal)
        }) {
            return Ok(());
        }
        self.retain_raster_canvas(comp_id, comp, target)
    }

    pub(super) fn retain_raster_canvas(
        &mut self,
        comp_id: u32,
        comp: &Composition,
        target: &mut GroupLayer,
    ) -> Result<(), DocumentError> {
        if comp.width == 0
            || comp.height == 0
            || comp.pixel_aspect.0 != comp.pixel_aspect.1
            || comp.layers.iter().any(|layer| {
                layer.record.flags().three_d_layer || matches!(layer.record.layer_type(), 1 | 2)
            })
        {
            self.warn(Limitation::GroupBounds, Some(comp_id), None,
                "composition blend-canvas isolation omitted for a nonplanar, nonsquare-pixel or empty source canvas; existing best-effort Group retained".into());
            return Ok(());
        }
        let Some(first_id) = reserve_ids(&mut self.next_id, 2) else {
            self.warn(Limitation::ExpansionLimit, Some(comp_id), None,
                "composition blend-canvas mask omitted: generated identifier space exhausted; editable children retained".into());
            return Ok(());
        };
        let guide_id = LayerId::new(first_id + 1);
        let source = crate::structure::SolidSource {
            width: comp.width,
            height: comp.height,
            pixel_aspect: comp.pixel_aspect,
            color: [1.0; 3],
        };
        let transform = Transform {
            anchor_point: [0.0, 0.0],
            position: fx_schema::Position::TwoD([0.0, 0.0]),
            scale: [100.0, 100.0],
            rotation: 0.0,
            skew: 0.0,
            skew_axis: 0.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            orientation: [0.0, 0.0, 0.0],
            opacity: fx_schema::PercentageProperty::new(100.0).expect("100 is a finite percentage"),
        };
        let mut guide = transform::solid_rect(&source, target, guide_id, transform);
        guide.active_range = fx_schema::TimeRangeProperty::new(
            fx_schema::Time::ZERO,
            fx_schema::Duration::from_secs(comp.duration_secs),
        );
        guide.name = "Composition canvas guide".into();
        guide.description = "Editable source-canvas boundary; mask-only geometry clips the raster precomposition on its transparent backdrop".into();
        target
            .layers
            .push(fx_schema::Layer::from_data(&FxLayer::Rect(guide))?);
        // Raster precompositions clip before the occurrence's masks, effects and Transform.
        target.masks.push(fx_schema::layer::PathMask {
            id: fx_schema::FxItemId::new(first_id),
            mode: fx_schema::layer::MaskMode::Add,
            inverted: false,
            layer: Some(guide_id),
            legacy_path: None,
            feather: [0.0, 0.0],
            expansion: 0.0,
            opacity: fx_schema::NonNegativeProperty::new(1.0).expect("one is nonnegative"),
        });
        self.warn(Limitation::GroupBounds, Some(comp_id), None,
            "planar raster composition retains an editable source-canvas mask and transparent local backdrop before occurrence effects and geometry; collapsed occurrences keep their existing passthrough. Native clipping-edge rasterization and alpha fidelity remain unverified".into());
        Ok(())
    }
}
