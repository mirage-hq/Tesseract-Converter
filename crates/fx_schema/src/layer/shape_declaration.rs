//! Shared rectangle and shape-layer persisted declarations.

/// Define rectangle and shape-layer records using the caller's effect reader.
#[doc(hidden)]
#[macro_export]
macro_rules! define_shape_layer_schema {
    () => {
        /// Rectangle shape layer data.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct RectLayer {
            /// Stable composition-unique layer id — the id space referenced by
            /// `parent`, track mattes, path masks, and text `pathOptions`.
            pub id: LayerId,
            /// User-visible name for the layer. This is distinct from `description`.
            pub name: String,
            /// Optional user-visible description for the layer.
            #[serde(default, skip_serializing_if = "String::is_empty")]
            pub description: String,
            /// Whether this layer is excluded from all rendered output.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub is_hidden: bool,
            /// `LayerId` of the containing Group layer, or `null` for a root
            /// layer — mirrors the layer's position in the tree.
            #[serde(skip_serializing_if = "Option::is_none")]
            pub parent: Option<LayerId>,
            /// How this layer composites over the stack below (see [`BlendMode`]).
            #[serde(default)]
            pub blend_mode: BlendMode,
            /// Optional inline track-matte source applied before blending.
            #[serde(alias = "layerMask", skip_serializing_if = "Option::is_none")]
            pub track_matte: Option<TrackMatte>,
            /// Stacked vector-path masks drawn directly on this layer, each carrying a
            /// stable [`FxItemId`] (JRB-1200). Distinct from the [`Self::track_matte`]
            /// track matte: these carry their own path geometry rather than
            /// referencing another layer's rendered alpha.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub masks: Vec<PathMask>,
            /// Immediate-parent-local span during which this layer is active. Root
            /// layers use composition time.
            pub active_range: TimeRangeProperty,
            /// AE-style Effects stack applied to this layer's rasterized content
            /// (the renderer wraps each lowered [`scene::NodeEffect`] in an
            /// `EffectGroup` around the rect raster, in authored stack order).
            /// [`LayerEffect::PersonMatte`] lowers to nothing here — it needs a
            /// source video frame to segment — so it is skipped on a rect layer.
            #[serde(
                default,
                skip_serializing_if = "Vec::is_empty",
                deserialize_with = "crate::effect::deserialize_effects"
            )]
            pub effects: Vec<EffectInstance>,
            /// Whether this layer participates in the composition's standard motion blur.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub motion_blur: bool,
            /// The layer's transform (anchor point, position, scale, rotation,
            /// opacity).
            pub transform: Transform,
            /// Rectangle geometry and paint settings.
            pub rect: RectShape,
        }

        /// Shape layer data backed by the same path/fill/stroke model as
        /// [`NodeContent::Shape`].
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ShapeLayer {
            /// Stable composition-unique layer id — the id space referenced by
            /// `parent`, track mattes, path masks, and text `pathOptions`.
            pub id: LayerId,
            /// User-visible name for the layer. This is distinct from `description`.
            pub name: String,
            /// Optional user-visible description for the layer.
            #[serde(default, skip_serializing_if = "String::is_empty")]
            pub description: String,
            /// Whether this layer is excluded from all rendered output.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub is_hidden: bool,
            /// `LayerId` of the containing Group layer, or `null` for a root
            /// layer — mirrors the layer's position in the tree.
            #[serde(skip_serializing_if = "Option::is_none")]
            pub parent: Option<LayerId>,
            /// How this layer composites over the stack below (see [`BlendMode`]).
            #[serde(default)]
            pub blend_mode: BlendMode,
            /// Optional inline track-matte source applied before blending.
            #[serde(alias = "layerMask", skip_serializing_if = "Option::is_none")]
            pub track_matte: Option<TrackMatte>,
            /// Stacked vector-path masks drawn directly on this layer, each carrying a
            /// stable [`FxItemId`] (JRB-1200). Distinct from the [`Self::track_matte`]
            /// track matte: these carry their own path geometry rather than
            /// referencing another layer's rendered alpha.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub masks: Vec<PathMask>,
            /// Immediate-parent-local span during which this layer is active. Root
            /// layers use composition time.
            pub active_range: TimeRangeProperty,
            /// AE-style Effects stack applied to this layer's rasterized content
            /// (the renderer wraps each lowered [`scene::NodeEffect`] in an
            /// `EffectGroup` around the shape raster, in authored stack order).
            /// [`LayerEffect::PersonMatte`] lowers to nothing here — it needs a
            /// source video frame to segment — so it is skipped on a shape layer.
            #[serde(
                default,
                skip_serializing_if = "Vec::is_empty",
                deserialize_with = "crate::effect::deserialize_effects"
            )]
            pub effects: Vec<EffectInstance>,
            /// Whether this layer participates in the composition's standard motion blur.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub motion_blur: bool,
            /// The layer's transform (anchor point, position, scale, rotation,
            /// opacity).
            pub transform: Transform,
            /// Generic vector path with fill and stroke styles.
            pub shape: ShapeContent,
        }

        /// Generic vector shape content matching [`NodeContent::Shape`].
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ShapeContent {
            /// Vector outline to fill and/or stroke.
            pub path: ShapePath,
            /// Fill styles painted for the path.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub fills: Vec<ShapeFillStyle>,
            /// Stroke styles painted for the path.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub strokes: Vec<ShapeStrokeStyle>,
            /// Optional RoundCorners modifier applied to [`Self::path`] before it is
            /// filled or stroked, mirroring AE's `Round Corners` shape modifier
            /// (JRB-1216). Absent (or a `0` radius) is a no-op.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub round_corners: Option<ShapeRoundCorners>,
            /// Optional OffsetPaths modifier: expand (positive `amount`) or contract
            /// (negative) the outline by an animatable distance, mirroring AE's
            /// `Offset Paths` shape modifier (JRB-1529). Runs after RoundCorners and
            /// before TrimPaths, matching the default AE operator insertion order.
            /// Absent (or a `0` amount) is a no-op.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub offset_paths: Option<ShapeOffsetPaths>,
            /// Optional TrimPaths modifier: reveal only a sub-range of the outline
            /// (the classic animated stroke "draw-on"). `None` == the whole outline is
            /// drawn (no modifier), matching a shape group with no Trim Paths operator.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub trim: Option<ShapeTrimPaths>,
            /// Optional PolyStar generator (star / regular polygon), mirroring AE's
            /// `Polystar Path` shape element (JRB-1215). When present, its generated
            /// outline *replaces* [`Self::path`] as the base geometry the
            /// RoundCorners / OffsetPaths / TrimPaths modifiers (and the fills /
            /// strokes) consume — a PolyStar is a path *source*, not a modifier.
            /// `None` keeps the explicit [`Self::path`] command list.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub poly_star: Option<ShapePolyStar>,
            /// Optional Ellipse generator, mirroring AE's `Ellipse Path` shape element
            /// (JRB-1530). A path *source* like [`Self::poly_star`]: when present, its
            /// generated outline replaces [`Self::path`]. When both generators are
            /// present, their contours combine into one compound path (AE semantics —
            /// every path source in a shape group contributes a contour).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub ellipse: Option<ShapeEllipse>,
        }

        /// RoundCorners shape modifier matching AE's `Round Corners` (a single
        /// non-negative Radius). Rounds sharp corners of [`ShapeContent::path`] into
        /// cubic arcs at lowering time via [`scene::Path::round_corners`]; the existing
        /// shape renderer then draws the rounded path with no shader changes
        /// (JRB-1216).
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ShapeRoundCorners {
            /// Corner radius in layer pixels. `0` is a no-op.
            #[serde(default)]
            pub radius: NonNegativeProperty,
        }

        /// OffsetPaths modifier matching AE's `Offset Paths` (JRB-1529): displace
        /// the shape outline by an animatable distance — positive `amount` expands
        /// the filled region, negative contracts it. Lowered at scene-build time via
        /// [`scene::offset::offset_path`], so the existing shape renderer draws the
        /// displaced geometry with no shader changes.
        ///
        /// `line_join` styles the corners on the side the displaced outline opens up
        /// (the contracting side always stays sharp, as in AE); `miter_limit` is the
        /// standard miter-length ÷ offset-distance cap beyond which a miter corner
        /// falls back to bevel (AE default `4`, same convention as
        /// [`ShapeStrokeStyle::miter_limit`]).
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ShapeOffsetPaths {
            /// Offset distance in layer pixels (positive = expand, negative =
            /// contract). `0` is a no-op. Animatable via
            /// [`crate::PropType::OffsetPathsAmount`].
            #[serde(default)]
            pub amount: ScalarProperty,
            /// Corner style where the displaced outline opens a gap (AE "Line Join").
            #[serde(default)]
            pub line_join: ShapeLineJoin,
            /// Miter-length ratio cap beyond which a miter corner falls back to
            /// bevel (AE "Miter Limit", default `4`).
            #[serde(default = "default_miter_limit")]
            pub miter_limit: ScalarProperty,
        }

        impl Default for ShapeOffsetPaths {
            fn default() -> Self {
                Self {
                    amount: 0.0,
                    line_join: ShapeLineJoin::default(),
                    miter_limit: default_miter_limit(),
                }
            }
        }

        /// TrimPaths modifier — reveal only a sub-range of a shape's outline, the
        /// classic animated stroke "draw-on". Strictly mirrors After Effects /
        /// libpag Trim Paths.
        ///
        /// `start` / `end` are percentages (`0..=100`, `end` defaults to a fully-drawn
        /// `100`) and `offset` is in degrees (`360` == one full loop); animating them
        /// (via the [`crate::PropType::TrimStart`] / `TrimEnd` / `TrimOffset`
        /// properties) produces the draw-on animation. `mode` follows AE's "Trim
        /// Multiple Shapes" toggle.
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ShapeTrimPaths {
            /// Start of the drawn range, as a percentage (`0..=100`).
            #[serde(default)]
            pub start: ScalarProperty,
            /// End of the drawn range, as a percentage (`0..=100`).
            #[serde(default = "default_hundred")]
            pub end: ScalarProperty,
            /// Phase offset in degrees (`360` == one full loop around the outline).
            #[serde(default)]
            pub offset: ScalarProperty,
            /// How multiple contours are trimmed (AE "Trim Multiple Shapes").
            #[serde(default)]
            pub mode: ShapeTrimMode,
        }

        impl Default for ShapeTrimPaths {
            fn default() -> Self {
                Self {
                    start: 0.0,
                    end: 100.0,
                    offset: 0.0,
                    mode: ShapeTrimMode::default(),
                }
            }
        }

        /// AE "Trim Multiple Shapes" mode, matching [`scene`]-level trimming and the
        /// PAG `TrimPathsType`.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum ShapeTrimMode {
            /// Every contour is trimmed independently by the same range — all contours
            /// draw together (AE "Simultaneously").
            #[default]
            Simultaneously,
            /// The contours are treated as one continuous compound path and the range
            /// is distributed across their combined length — contours draw one after
            /// another (AE "Individually").
            Individually,
        }

        /// PolyStar generator kind, mirroring AE's `Polystar Path` "Type" dropdown and
        /// the PAG [`crate::pag`]-side `PolyStarType`.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum ShapePolyStarType {
            /// Alternating outer / inner radius vertices — a star.
            #[default]
            Star,
            /// Every vertex at the outer radius — a regular polygon.
            Polygon,
        }

        /// PolyStar generator — emits a star or regular-polygon outline from a compact
        /// parameter set (AE's `Polystar Path` shape element / libpag's
        /// `PolyStarElement`, JRB-1215). Lowered to a [`Path`] at evaluation time and
        /// then consumed by the RoundCorners / TrimPaths modifiers and the fills /
        /// strokes, so the existing shape renderer draws it with no shader changes.
        ///
        /// The geometry is authored in layer-local pixels (a `position` center plus
        /// `outer_radius` / `inner_radius`); `rotation` is in degrees (0° points the
        /// first vertex straight up, matching AE); `points` is the vertex count
        /// (clamped to `3..=1000`). `outer_roundness` / `inner_roundness` are
        /// percentages (`0..=100`, AE convention) that round the corners into cubic
        /// arcs. `reversed` flips the contour winding.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ShapePolyStar {
            /// Star vs. regular polygon.
            #[serde(default)]
            pub star_type: ShapePolyStarType,
            /// Number of points / sides (clamped to `3..=1000` at lowering time).
            #[serde(default = "default_poly_star_points")]
            pub points: ScalarProperty,
            /// Center of the generated shape, in layer-local pixels.
            #[serde(default)]
            pub position: Vector2Property,
            /// Rotation in degrees (`0` == first vertex points straight up).
            #[serde(default)]
            pub rotation: ScalarProperty,
            /// Outer radius in layer pixels (the star tips / polygon vertices).
            #[serde(default = "default_poly_star_outer_radius")]
            pub outer_radius: ScalarProperty,
            /// Inner radius in layer pixels (the star's concave vertices). Ignored for
            /// [`ShapePolyStarType::Polygon`].
            #[serde(default)]
            pub inner_radius: ScalarProperty,
            /// Outer-corner roundness as a percentage (`0..=100`, AE convention).
            #[serde(default)]
            pub outer_roundness: ScalarProperty,
            /// Inner-corner roundness as a percentage (`0..=100`). Ignored for
            /// [`ShapePolyStarType::Polygon`].
            #[serde(default)]
            pub inner_roundness: ScalarProperty,
            /// Reverse the contour winding direction.
            #[serde(default)]
            pub reversed: bool,
        }

        impl Default for ShapePolyStar {
            fn default() -> Self {
                Self {
                    star_type: ShapePolyStarType::default(),
                    points: default_poly_star_points(),
                    position: [0.0, 0.0],
                    rotation: 0.0,
                    outer_radius: default_poly_star_outer_radius(),
                    inner_radius: 0.0,
                    outer_roundness: 0.0,
                    inner_roundness: 0.0,
                    reversed: false,
                }
            }
        }

        fn default_poly_star_points() -> ScalarProperty {
            5.0
        }

        fn default_poly_star_outer_radius() -> ScalarProperty {
            100.0
        }

        /// Ellipse generator — emits an ellipse outline from an animatable center and
        /// size (AE's `Ellipse Path` shape element / PAG's `EllipseShape`, JRB-1530).
        /// A path *source* like [`ShapePolyStar`]: lowered to a [`Path`] at evaluation
        /// time and then consumed by the RoundCorners / TrimPaths modifiers and the
        /// fills / strokes, so the existing shape renderer draws it with no shader
        /// changes.
        ///
        /// The geometry is authored in layer-local pixels: `position` is the center
        /// and `size` is the full `[width, height]` extent (AE convention — a
        /// `[100, 100]` size is a circle of radius 50, which is also how this
        /// subsumes a plain circle shape). `reversed` flips the contour winding.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ShapeEllipse {
            /// Full `[width, height]` extent in layer pixels.
            #[serde(default = "default_ellipse_size")]
            pub size: Vector2Property,
            /// Center of the generated ellipse, in layer-local pixels.
            #[serde(default)]
            pub position: Vector2Property,
            /// Reverse the contour winding direction.
            #[serde(default)]
            pub reversed: bool,
        }

        impl Default for ShapeEllipse {
            fn default() -> Self {
                Self {
                    size: default_ellipse_size(),
                    position: [0.0, 0.0],
                    reversed: false,
                }
            }
        }

        /// AE / PAG default Ellipse size (a 100×100 circle).
        fn default_ellipse_size() -> Vector2Property {
            [100.0, 100.0]
        }
    };
}
