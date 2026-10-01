//! PDF-compatible Illustrator artwork decoding.
//!
//! This intentionally implements a restricted single-page PDF profile. Eager
//! object/xref-stream expansion is disabled; content decoding is not bounded by
//! a converter quota. This is not a process or memory sandbox.

use std::collections::HashSet;

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId, content::Content};
use thiserror::Error;

const MATRIX_LIMIT: f64 = 1.0e12;
const MAX_FORM_DEPTH: usize = 16;
const MAX_FORM_OPERATIONS: usize = 100_000;

/// One decoded page of editable vector artwork.
#[derive(Clone, Debug)]
pub(crate) struct Artwork {
    pub(crate) dimensions: [f64; 2],
    pub(crate) shapes: Vec<VectorShape>,
    pub(crate) warnings: Vec<String>,
}

/// One PDF paint operation. Paint order is the order in `Artwork::shapes`.
#[derive(Clone, Debug)]
pub(crate) struct VectorShape {
    pub(crate) path: Vec<PathCommand>,
    pub(crate) transform: Affine,
    pub(crate) fill: Option<Fill>,
    pub(crate) stroke: Option<Stroke>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Affine {
    pub(crate) a: f64,
    pub(crate) b: f64,
    pub(crate) c: f64,
    pub(crate) d: f64,
    pub(crate) e: f64,
    pub(crate) f: f64,
}

impl Affine {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    fn concat(self, next: Self) -> Self {
        Self {
            a: self.a * next.a + self.c * next.b,
            b: self.b * next.a + self.d * next.b,
            c: self.a * next.c + self.c * next.d,
            d: self.b * next.c + self.d * next.d,
            e: self.a * next.e + self.c * next.f + self.e,
            f: self.b * next.e + self.d * next.f + self.f,
        }
    }

    fn point(self, point: [f64; 2]) -> [f64; 2] {
        [
            self.a * point[0] + self.c * point[1] + self.e,
            self.b * point[0] + self.d * point[1] + self.f,
        ]
    }

    fn inverse(self) -> Option<Self> {
        let determinant = self.a * self.d - self.b * self.c;
        if !determinant.is_finite() || determinant.abs() <= 1.0e-12 {
            return None;
        }
        let inverse = Self {
            a: self.d / determinant,
            b: -self.b / determinant,
            c: -self.c / determinant,
            d: self.a / determinant,
            e: (self.c * self.f - self.d * self.e) / determinant,
            f: (self.b * self.e - self.a * self.f) / determinant,
        };
        inverse.is_valid().then_some(inverse)
    }

    fn is_valid(self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .into_iter()
            .all(|value| value.is_finite() && value.abs() <= MATRIX_LIMIT)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PathCommand {
    Move([f64; 2]),
    Line([f64; 2]),
    Cubic([f64; 2], [f64; 2], [f64; 2]),
    Close,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Fill {
    pub(crate) color: [f64; 4],
    pub(crate) even_odd: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Stroke {
    pub(crate) color: [f64; 4],
    pub(crate) width: f64,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
    pub(crate) miter_limit: f64,
    pub(crate) dashes: Vec<f64>,
    pub(crate) dash_offset: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// A PDF is valid but outside the intentionally restricted editable-AI profile.
#[derive(Debug, Error)]
pub(crate) enum VectorMediaError {
    #[error("source is not a PDF-compatible AI file")]
    Signature,
    #[error("PDF-compatible AI uses unsupported {0}")]
    Unsupported(&'static str),
    #[error("malformed PDF-compatible AI {0}")]
    Malformed(&'static str),
    #[error("PDF-compatible AI conversion interrupted: {0}")]
    Interrupted(&'static str),
    #[error("PDF-compatible AI parser rejected the restricted PDF profile: {0}")]
    Pdf(#[from] lopdf::Error),
}

#[derive(Clone)]
struct GraphicsState {
    ctm: Affine,
    fill: Option<[f64; 4]>,
    stroke: Option<[f64; 4]>,
    width: f64,
    cap: LineCap,
    join: LineJoin,
    miter_limit: f64,
    dashes: Vec<f64>,
    dash_offset: f64,
    poisoned: bool,
}

impl Default for GraphicsState {
    fn default() -> Self {
        Self {
            ctm: Affine::IDENTITY,
            fill: Some([0.0, 0.0, 0.0, 1.0]),
            stroke: Some([0.0, 0.0, 0.0, 1.0]),
            width: 1.0,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 10.0,
            dashes: Vec::new(),
            dash_offset: 0.0,
            poisoned: false,
        }
    }
}

#[derive(Default)]
struct PathBuilder {
    raw: Vec<PathCommand>,
    flattened: Vec<PathCommand>,
    construction_ctm: Option<Affine>,
    consistent_ctm: bool,
    raw_current: Option<[f64; 2]>,
    flat_current: Option<[f64; 2]>,
    raw_start: Option<[f64; 2]>,
    flat_start: Option<[f64; 2]>,
}

impl PathBuilder {
    fn clear(&mut self) {
        *self = Self::default();
    }

    fn note_ctm(&mut self, ctm: Affine) {
        match self.construction_ctm {
            None => {
                self.construction_ctm = Some(ctm);
                self.consistent_ctm = true;
            }
            Some(existing) if existing == ctm => {}
            Some(_) => self.consistent_ctm = false,
        }
    }

    fn move_to(&mut self, point: [f64; 2], ctm: Affine) {
        self.note_ctm(ctm);
        let flat = ctm.point(point);
        self.raw.push(PathCommand::Move(point));
        self.flattened.push(PathCommand::Move(flat));
        self.raw_current = Some(point);
        self.flat_current = Some(flat);
        self.raw_start = Some(point);
        self.flat_start = Some(flat);
    }

    fn line_to(&mut self, point: [f64; 2], ctm: Affine) -> Result<(), VectorMediaError> {
        if self.raw_current.is_none() {
            return Err(VectorMediaError::Malformed(
                "path line without a current point",
            ));
        }
        self.note_ctm(ctm);
        let flat = ctm.point(point);
        self.raw.push(PathCommand::Line(point));
        self.flattened.push(PathCommand::Line(flat));
        self.raw_current = Some(point);
        self.flat_current = Some(flat);
        Ok(())
    }

    fn cubic_to(
        &mut self,
        first: [f64; 2],
        second: [f64; 2],
        end: [f64; 2],
        ctm: Affine,
    ) -> Result<(), VectorMediaError> {
        if self.raw_current.is_none() {
            return Err(VectorMediaError::Malformed(
                "path curve without a current point",
            ));
        }
        self.note_ctm(ctm);
        self.raw.push(PathCommand::Cubic(first, second, end));
        self.flattened.push(PathCommand::Cubic(
            ctm.point(first),
            ctm.point(second),
            ctm.point(end),
        ));
        self.raw_current = Some(end);
        self.flat_current = Some(ctm.point(end));
        Ok(())
    }

    fn cubic_v(
        &mut self,
        second: [f64; 2],
        end: [f64; 2],
        ctm: Affine,
    ) -> Result<(), VectorMediaError> {
        let first_raw = self
            .raw_current
            .ok_or(VectorMediaError::Malformed("v without a current point"))?;
        let first_flat = self
            .flat_current
            .ok_or(VectorMediaError::Malformed("v without a current point"))?;
        self.note_ctm(ctm);
        self.raw.push(PathCommand::Cubic(first_raw, second, end));
        self.flattened.push(PathCommand::Cubic(
            first_flat,
            ctm.point(second),
            ctm.point(end),
        ));
        self.raw_current = Some(end);
        self.flat_current = Some(ctm.point(end));
        Ok(())
    }

    fn close(&mut self) {
        if self.raw_current.is_some() {
            self.raw.push(PathCommand::Close);
            self.flattened.push(PathCommand::Close);
            self.raw_current = self.raw_start;
            self.flat_current = self.flat_start;
        }
    }
}

struct DecodeContext<'a> {
    document: &'a Document,
    form_operations: usize,
    forms: HashSet<ObjectId>,
    warnings: Vec<String>,
}

impl DecodeContext<'_> {
    fn warning(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    fn charge_form_operations(&mut self, count: usize) -> Result<(), VectorMediaError> {
        self.form_operations = self
            .form_operations
            .checked_add(count)
            .ok_or(VectorMediaError::Interrupted("Form expansion work limit"))?;
        if self.form_operations > MAX_FORM_OPERATIONS {
            return Err(VectorMediaError::Interrupted("Form expansion work limit"));
        }
        Ok(())
    }
}

struct Interpreter<'a, 'b> {
    context: &'a mut DecodeContext<'b>,
    state: GraphicsState,
    stack: Vec<GraphicsState>,
    path: PathBuilder,
    shapes: Vec<VectorShape>,
    page_map: Affine,
    resources: Vec<&'b Dictionary>,
    depth: usize,
}

/// Decode one PDF-compatible `.ai` byte stream under the restricted profile.
pub(crate) fn decode(bytes: &[u8]) -> Result<Artwork, VectorMediaError> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(VectorMediaError::Signature);
    }
    // Disable eager object/xref-stream expansion in the parser itself. PDF
    // names may be escaped and whitespace is arbitrary: byte-string rejection
    // cannot enforce this restriction. Content streams decode on demand without
    // a converter quota; this is not a process memory sandbox.
    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            password: None,
            filter: None,
            strict: true,
            max_decompressed_size: Some(0),
        },
    )?;
    if document.is_encrypted() || document.encryption_state.is_some() {
        return Err(VectorMediaError::Unsupported("encrypted content"));
    }
    if document.catalog()?.get(b"OCProperties").is_ok() {
        return Err(VectorMediaError::Unsupported(
            "optional-content groups / Illustrator layer visibility",
        ));
    }
    let pages = document.get_pages();
    if pages.len() != 1 {
        return Err(VectorMediaError::Unsupported(
            "multiple pages/artboards without a proven native selector",
        ));
    }
    let page_id = *pages
        .values()
        .next()
        .ok_or(VectorMediaError::Malformed("page tree"))?;
    let page = document.get_dictionary(page_id)?;
    let media_box = inherited_array(&document, page, b"MediaBox")?;
    let media_box = rectangle(media_box, "MediaBox")?;
    if let Some(crop) = inherited_optional(&document, page, b"CropBox")? {
        let crop = rectangle(crop.as_array()?, "CropBox")?;
        if crop != media_box {
            return Err(VectorMediaError::Unsupported(
                "a CropBox different from MediaBox without a proven native crop selector",
            ));
        }
    }
    let user_unit = page
        .get(b"UserUnit")
        .ok()
        .map(|value| document.dereference(value).map_err(VectorMediaError::from))
        .transpose()?
        .map(|(_, value)| number(value))
        .transpose()?
        .unwrap_or(1.0);
    if !user_unit.is_finite() || !(0.01..=100.0).contains(&user_unit) {
        return Err(VectorMediaError::Malformed("UserUnit"));
    }
    let rotation = inherited_optional(&document, page, b"Rotate")?
        .map(|value| value.as_i64().map_err(VectorMediaError::from))
        .transpose()?
        .unwrap_or(0)
        .rem_euclid(360);
    if rotation % 90 != 0 {
        return Err(VectorMediaError::Unsupported(
            "non-right-angle page rotation",
        ));
    }
    let (page_map, dimensions) = page_mapping(media_box, user_unit, rotation);
    let resources = inherited_resources(&document, page)?;
    check_color_resources(&document, &resources)?;
    if page.has(b"Group") {
        return Err(VectorMediaError::Unsupported("page transparency Group"));
    }
    let content_ids = document.get_page_contents(page_id);
    let mut context = DecodeContext {
        document: &document,
        form_operations: 0,
        forms: HashSet::new(),
        warnings: vec![
            "PDF-compatible AI imported through the restricted single-page whole-artwork profile; no native AE page/layer selector has been established, so selector fidelity is not claimed".into(),
        ],
    };
    let mut interpreter = Interpreter {
        context: &mut context,
        state: GraphicsState::default(),
        stack: Vec::new(),
        path: PathBuilder::default(),
        shapes: Vec::new(),
        page_map,
        resources,
        depth: 0,
    };
    // A /Contents array is one logical stream: even an operator's operands
    // can straddle stream boundaries.
    let mut page_content = Vec::new();
    for id in content_ids {
        let stream = document.get_object(id)?.as_stream()?;
        let bytes = stream.get_plain_content()?;
        page_content.extend_from_slice(&bytes);
        page_content.push(b'\n');
    }
    let operations = Content::decode_strict(&page_content)?.operations;
    interpreter.run(&operations)?;
    if !interpreter.stack.is_empty() {
        return Err(VectorMediaError::Malformed("unbalanced q operator"));
    }
    if !interpreter.path.raw.is_empty() {
        interpreter
            .context
            .warning("unterminated PDF path was not painted and was omitted");
    }
    Ok(Artwork {
        dimensions,
        shapes: interpreter.shapes,
        warnings: context.warnings,
    })
}

impl Interpreter<'_, '_> {
    fn run(&mut self, operations: &[lopdf::content::Operation]) -> Result<(), VectorMediaError> {
        for operation in operations {
            let operands = operation.operands.as_slice();
            match operation.operator.as_str() {
                "q" => {
                    exact_operands(operands, 0, "q")?;
                    self.stack.push(self.state.clone());
                }
                "Q" => {
                    exact_operands(operands, 0, "Q")?;
                    self.state = self
                        .stack
                        .pop()
                        .ok_or(VectorMediaError::Malformed("unbalanced Q operator"))?;
                    // The current path is deliberately not restored: PDF q/Q
                    // save graphics state, never the path under construction.
                }
                "cm" => {
                    let values = numbers(operands, 6, "cm")?;
                    let matrix = Affine {
                        a: values[0],
                        b: values[1],
                        c: values[2],
                        d: values[3],
                        e: values[4],
                        f: values[5],
                    };
                    if !matrix.is_valid() {
                        return Err(VectorMediaError::Malformed("cm matrix"));
                    }
                    self.state.ctm = self.state.ctm.concat(matrix);
                    if !self.state.ctm.is_valid() {
                        return Err(VectorMediaError::Malformed("concatenated CTM"));
                    }
                }
                "m" => {
                    let point = point(operands, "m")?;
                    self.path.move_to(point, self.state.ctm);
                }
                "l" => {
                    let point = point(operands, "l")?;
                    self.path.line_to(point, self.state.ctm)?;
                }
                "c" => {
                    let values = numbers(operands, 6, "c")?;
                    self.path.cubic_to(
                        [values[0], values[1]],
                        [values[2], values[3]],
                        [values[4], values[5]],
                        self.state.ctm,
                    )?;
                }
                "v" => {
                    let values = numbers(operands, 4, "v")?;
                    self.path.cubic_v(
                        [values[0], values[1]],
                        [values[2], values[3]],
                        self.state.ctm,
                    )?;
                }
                "y" => {
                    let values = numbers(operands, 4, "y")?;
                    let end = [values[2], values[3]];
                    self.path
                        .cubic_to([values[0], values[1]], end, end, self.state.ctm)?;
                }
                "h" => {
                    exact_operands(operands, 0, "h")?;
                    self.path.close();
                }
                "re" => {
                    let values = numbers(operands, 4, "re")?;
                    let [x, y, width, height] = values.as_slice() else {
                        unreachable!()
                    };
                    self.path.move_to([*x, *y], self.state.ctm);
                    self.path.line_to([*x + *width, *y], self.state.ctm)?;
                    self.path
                        .line_to([*x + *width, *y + *height], self.state.ctm)?;
                    self.path.line_to([*x, *y + *height], self.state.ctm)?;
                    self.path.close();
                }
                "f" | "F" => self.paint(true, false, false)?,
                "f*" => self.paint(true, false, true)?,
                "S" => self.paint(false, true, false)?,
                "s" => {
                    self.path.close();
                    self.paint(false, true, false)?;
                }
                "B" => self.paint(true, true, false)?,
                "B*" => self.paint(true, true, true)?,
                "b" => {
                    self.path.close();
                    self.paint(true, true, false)?;
                }
                "b*" => {
                    self.path.close();
                    self.paint(true, true, true)?;
                }
                "n" => {
                    exact_operands(operands, 0, "n")?;
                    self.path.clear();
                }
                "rg" => self.state.fill = Some(rgb(operands, "rg")?),
                "RG" => self.state.stroke = Some(rgb(operands, "RG")?),
                "g" => self.state.fill = Some(gray(operands, "g")?),
                "G" => self.state.stroke = Some(gray(operands, "G")?),
                "w" => {
                    let value = scalar(operands, "w")?;
                    if value < 0.0 {
                        return Err(VectorMediaError::Malformed("negative line width"));
                    }
                    self.state.width = value;
                }
                "J" => {
                    self.state.cap = match integer(operands, "J")? {
                        0 => LineCap::Butt,
                        1 => LineCap::Round,
                        2 => LineCap::Square,
                        _ => return Err(VectorMediaError::Malformed("line cap")),
                    };
                }
                "j" => {
                    self.state.join = match integer(operands, "j")? {
                        0 => LineJoin::Miter,
                        1 => LineJoin::Round,
                        2 => LineJoin::Bevel,
                        _ => return Err(VectorMediaError::Malformed("line join")),
                    };
                }
                "M" => {
                    let value = scalar(operands, "M")?;
                    if value < 1.0 {
                        return Err(VectorMediaError::Malformed("miter limit"));
                    }
                    self.state.miter_limit = value;
                }
                "d" => self.set_dash(operands)?,
                "Do" => self.xobject(operands)?,
                "W" | "W*" => {
                    self.state.poisoned = true;
                    self.context.warning(
                        "PDF clipping-path mapping is not implemented in this restricted interpreter; the affected graphics-state scope is omitted until restore",
                    );
                }
                "k" | "cs" | "sc" | "scn" => {
                    self.state.fill = None;
                    self.context.warning(format!(
                        "PDF {} fill color is unsupported; stale RGB was invalidated and affected fill paints are omitted",
                        operation.operator
                    ));
                }
                "K" | "CS" | "SC" | "SCN" => {
                    self.state.stroke = None;
                    self.context.warning(format!(
                        "PDF {} stroke color is unsupported; stale RGB was invalidated and affected stroke paints are omitted",
                        operation.operator
                    ));
                }
                "Tr" => {
                    let mode = integer(operands, "Tr")?;
                    if !(0..=7).contains(&mode) {
                        return Err(VectorMediaError::Malformed("text rendering mode"));
                    }
                    if mode >= 4 {
                        self.state.poisoned = true;
                        self.context.warning("PDF text clipping is unsupported; affected graphics-state scope omitted until restore");
                    }
                }
                "BT" | "ET" | "Tf" | "Tm" | "Td" | "TD" | "T*" | "Tj" | "TJ" | "'" | "\""
                | "Tc" | "Tw" | "Tz" | "TL" | "Ts" => {
                    self.context.warning(
                        "PDF text operation was omitted; text is not silently outlined or rasterized",
                    );
                }
                "sh" | "BI" | "ID" | "EI" => {
                    self.context.warning(
                        "PDF gradient or inline-image paint was omitted; no raster fallback was emitted",
                    );
                }
                "i" | "ri" | "d0" | "d1" => {
                    self.context.warning(format!(
                        "PDF {} operator has no editable FX mapping and was ignored",
                        operation.operator
                    ));
                }
                _ => {
                    self.state.poisoned = true;
                    self.context.warning(format!(
                        "unknown PDF operator {:?} may affect render state; affected graphics-state scope is omitted until restore",
                        operation.operator
                    ));
                }
            }
        }
        Ok(())
    }

    fn paint(
        &mut self,
        fill_requested: bool,
        stroke_requested: bool,
        even_odd: bool,
    ) -> Result<(), VectorMediaError> {
        if self.path.raw.is_empty() {
            self.path.clear();
            return Ok(());
        }
        if self.state.poisoned {
            self.context.warning(
                "PDF paint in an unsupported render-state scope was omitted; independent restored siblings remain convertible",
            );
            self.path.clear();
            return Ok(());
        }
        let fill = fill_requested
            .then_some(self.state.fill)
            .flatten()
            .map(|color| Fill { color, even_odd });
        let mut stroke = stroke_requested
            .then_some(self.state.stroke)
            .flatten()
            .map(|color| Stroke {
                color,
                width: self.state.width,
                cap: self.state.cap,
                join: self.state.join,
                miter_limit: self.state.miter_limit,
                dashes: self.state.dashes.clone(),
                dash_offset: self.state.dash_offset,
            });
        let stroke_space = stroke.as_ref().and_then(|_| self.state.ctm.inverse());
        if stroke.is_some() && stroke_space.is_none() {
            stroke = None;
            self.context.warning(
                "PDF stroke painted under a singular CTM was omitted; convertible fill geometry was retained",
            );
        }
        if fill.is_none() && stroke.is_none() {
            self.context.warning(
                "PDF paint used only unsupported color/state semantics; path paint was omitted",
            );
            self.path.clear();
            return Ok(());
        }
        let (path, transform) = if let Some(inverse) = stroke_space {
            // PDF path points are fixed when constructed, but stroke width and
            // dash geometry use the CTM active at paint time. Move the fixed
            // points back into that paint space, then retain its CTM as the
            // editable FX Transform.
            (
                map_path(&self.path.flattened, inverse),
                self.page_map.concat(self.state.ctm),
            )
        } else if self.path.consistent_ctm {
            (
                std::mem::take(&mut self.path.raw),
                self.page_map
                    .concat(self.path.construction_ctm.unwrap_or(Affine::IDENTITY)),
            )
        } else {
            (std::mem::take(&mut self.path.flattened), self.page_map)
        };
        self.shapes.push(VectorShape {
            path,
            transform,
            fill,
            stroke,
        });
        self.path.clear();
        Ok(())
    }

    fn set_dash(&mut self, operands: &[Object]) -> Result<(), VectorMediaError> {
        exact_operands(operands, 2, "d")?;
        let array = operands[0]
            .as_array()
            .map_err(|_| VectorMediaError::Malformed("dash array"))?;
        let mut dashes = array.iter().map(number).collect::<Result<Vec<_>, _>>()?;
        if dashes.iter().any(|value| *value < 0.0)
            || (!dashes.is_empty() && dashes.iter().all(|value| *value == 0.0))
        {
            return Err(VectorMediaError::Malformed("dash pattern"));
        }
        if dashes.len() % 2 == 1 {
            let copy = dashes.clone();
            dashes.extend(copy);
        }
        self.state.dashes = dashes;
        self.state.dash_offset = number(&operands[1])?;
        Ok(())
    }

    fn xobject(&mut self, operands: &[Object]) -> Result<(), VectorMediaError> {
        exact_operands(operands, 1, "Do")?;
        let name = operands[0]
            .as_name()
            .map_err(|_| VectorMediaError::Malformed("Do name"))?;
        let Some((id, stream)) = find_xobject(self.context.document, &self.resources, name)? else {
            self.context.warning(format!(
                "PDF XObject {:?} was missing; paint omitted",
                String::from_utf8_lossy(name)
            ));
            return Ok(());
        };
        let subtype = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .unwrap_or_default();
        if subtype != b"Form" {
            self.context.warning(format!(
                "PDF {:?} XObject was omitted; no image/raster fallback was emitted",
                String::from_utf8_lossy(subtype)
            ));
            return Ok(());
        }
        if stream.dict.has(b"Group") || stream.dict.has(b"OC") {
            self.context.warning("PDF Form Group/transparency or optional-content visibility is unsupported; this Form was omitted");
            return Ok(());
        }
        if self.depth >= MAX_FORM_DEPTH {
            return Err(VectorMediaError::Interrupted(
                "Form XObject recursion depth",
            ));
        }
        if !self.context.forms.insert(id) {
            return Err(VectorMediaError::Malformed("cyclic Form XObject"));
        }
        let matrix = stream
            .dict
            .get(b"Matrix")
            .ok()
            .map(|value| matrix(value, "Form Matrix"))
            .transpose()?
            .unwrap_or(Affine::IDENTITY);
        let bbox = stream
            .dict
            .get(b"BBox")
            .map_err(|_| VectorMediaError::Malformed("Form BBox"))?;
        let bbox = rectangle(
            bbox.as_array()
                .map_err(|_| VectorMediaError::Malformed("Form BBox"))?,
            "Form BBox",
        )?;
        let resources = stream
            .dict
            .get(b"Resources")
            .ok()
            .map(|value| {
                self.context
                    .document
                    .dereference(value)
                    .and_then(|(_, value)| value.as_dict())
            })
            .transpose()?
            .map_or_else(|| self.resources.clone(), |value| vec![value]);
        if let Err(error) = check_color_resources(self.context.document, &resources) {
            self.context.warning(format!(
                "Form color resources rejected: {error}; this Form was omitted"
            ));
            self.context.forms.remove(&id);
            return Ok(());
        }
        let bytes = stream.get_plain_content()?;
        let operations = Content::decode_strict(&bytes)?.operations;
        self.context.charge_form_operations(operations.len())?;
        let mut child = Interpreter {
            context: self.context,
            state: GraphicsState {
                ctm: self.state.ctm.concat(matrix),
                ..self.state.clone()
            },
            stack: Vec::new(),
            path: PathBuilder::default(),
            shapes: Vec::new(),
            page_map: self.page_map,
            resources,
            depth: self.depth + 1,
        };
        child.run(&operations)?;
        if !child.stack.is_empty() {
            return Err(VectorMediaError::Malformed("unbalanced q in Form XObject"));
        }
        let form_map = self.page_map.concat(self.state.ctm.concat(matrix));
        let inverse = form_map.inverse();
        for shape in child.shapes {
            if inverse.is_some_and(|inverse| shape_within(&shape, inverse, bbox)) {
                self.shapes.push(shape);
            } else {
                self.context.warning(
                    "Form XObject paint could cross its implicit BBox clip; that paint was omitted rather than emitted unbounded, while bounded siblings remain editable",
                );
            }
        }
        self.context.forms.remove(&id);
        Ok(())
    }
}

fn check_color_resources(
    document: &Document,
    resources: &[&Dictionary],
) -> Result<(), VectorMediaError> {
    for resources in resources {
        if let Ok(value) = resources.get(b"ColorSpace") {
            let spaces = document.dereference(value)?.1.as_dict()?;
            if spaces.has(b"DefaultRGB") || spaces.has(b"DefaultGray") {
                return Err(VectorMediaError::Unsupported(
                    "DefaultRGB/DefaultGray color-space overrides",
                ));
            }
        }
    }
    Ok(())
}

fn find_xobject<'a>(
    document: &'a Document,
    resources: &[&'a Dictionary],
    name: &[u8],
) -> Result<Option<(ObjectId, &'a lopdf::Stream)>, VectorMediaError> {
    for resource in resources {
        let Ok(xobjects) = resource
            .get_deref(b"XObject", document)
            .and_then(Object::as_dict)
        else {
            continue;
        };
        let Ok(object) = xobjects.get(name) else {
            continue;
        };
        let id = object
            .as_reference()
            .map_err(|_| VectorMediaError::Unsupported("direct Form XObject streams"))?;
        let stream = document.get_object(id)?.as_stream()?;
        return Ok(Some((id, stream)));
    }
    Ok(None)
}

fn inherited_resources<'a>(
    document: &'a Document,
    page: &'a Dictionary,
) -> Result<Vec<&'a Dictionary>, VectorMediaError> {
    let mut resources = Vec::new();
    let mut current = page;
    let mut seen = HashSet::new();
    loop {
        if let Ok(value) = current.get(b"Resources") {
            let value = document.dereference(value)?.1;
            resources.push(value.as_dict()?);
            // Resources are inherited as a whole dictionary, not merged with
            // ancestors when a nearer dictionary is present.
            return Ok(resources);
        }
        let Ok(parent) = current.get(b"Parent").and_then(Object::as_reference) else {
            return Ok(resources);
        };
        if !seen.insert(parent) {
            return Err(VectorMediaError::Malformed("cyclic page tree"));
        }
        current = document.get_dictionary(parent)?;
    }
}

fn inherited_object<'a>(
    document: &'a Document,
    page: &'a Dictionary,
    key: &[u8],
) -> Result<&'a Object, VectorMediaError> {
    inherited_optional(document, page, key)?.ok_or(VectorMediaError::Malformed(
        "missing inherited page property",
    ))
}

fn inherited_optional<'a>(
    document: &'a Document,
    page: &'a Dictionary,
    key: &[u8],
) -> Result<Option<&'a Object>, VectorMediaError> {
    let mut current = page;
    let mut seen = HashSet::new();
    loop {
        if let Ok(value) = current.get(key) {
            return Ok(Some(document.dereference(value)?.1));
        }
        let Ok(parent) = current.get(b"Parent") else {
            return Ok(None);
        };
        let parent = parent.as_reference()?;
        if !seen.insert(parent) {
            return Err(VectorMediaError::Malformed("cyclic page tree"));
        }
        current = document.get_dictionary(parent)?;
    }
}

fn inherited_array<'a>(
    document: &'a Document,
    page: &'a Dictionary,
    key: &[u8],
) -> Result<&'a Vec<Object>, VectorMediaError> {
    inherited_object(document, page, key)?
        .as_array()
        .map_err(VectorMediaError::from)
}

fn page_mapping(box_: [f64; 4], unit: f64, rotation: i64) -> (Affine, [f64; 2]) {
    let [x0, y0, x1, y1] = box_;
    let width = (x1 - x0) * unit;
    let height = (y1 - y0) * unit;
    match rotation {
        90 => (
            Affine {
                a: 0.0,
                b: unit,
                c: unit,
                d: 0.0,
                e: -y0 * unit,
                f: -x0 * unit,
            },
            [height, width],
        ),
        180 => (
            Affine {
                a: -unit,
                b: 0.0,
                c: 0.0,
                d: unit,
                e: x1 * unit,
                f: -y0 * unit,
            },
            [width, height],
        ),
        270 => (
            Affine {
                a: 0.0,
                b: -unit,
                c: -unit,
                d: 0.0,
                e: y1 * unit,
                f: x1 * unit,
            },
            [height, width],
        ),
        _ => (
            Affine {
                a: unit,
                b: 0.0,
                c: 0.0,
                d: -unit,
                e: -x0 * unit,
                f: y1 * unit,
            },
            [width, height],
        ),
    }
}

fn map_path(path: &[PathCommand], transform: Affine) -> Vec<PathCommand> {
    path.iter()
        .map(|command| match *command {
            PathCommand::Move(point) => PathCommand::Move(transform.point(point)),
            PathCommand::Line(point) => PathCommand::Line(transform.point(point)),
            PathCommand::Cubic(first, second, end) => PathCommand::Cubic(
                transform.point(first),
                transform.point(second),
                transform.point(end),
            ),
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

fn shape_within(shape: &VectorShape, inverse_clip: Affine, bounds: [f64; 4]) -> bool {
    let transform = inverse_clip.concat(shape.transform);
    let mut points = Vec::new();
    for command in &shape.path {
        match command {
            PathCommand::Move(point) | PathCommand::Line(point) => points.push(*point),
            PathCommand::Cubic(first, second, end) => {
                points.extend([*first, *second, *end]);
            }
            PathCommand::Close => {}
        }
    }
    let expansion = shape.stroke.as_ref().map_or(0.0, |stroke| {
        let scale_bound =
            (transform.a.powi(2) + transform.b.powi(2) + transform.c.powi(2) + transform.d.powi(2))
                .sqrt();
        stroke.width * 0.5 * stroke.miter_limit.max(2.0) * scale_bound
    });
    points.into_iter().all(|point| {
        let point = transform.point(point);
        point[0] - expansion >= bounds[0]
            && point[1] - expansion >= bounds[1]
            && point[0] + expansion <= bounds[2]
            && point[1] + expansion <= bounds[3]
    })
}

fn rectangle(values: &[Object], label: &'static str) -> Result<[f64; 4], VectorMediaError> {
    let values = numbers(values, 4, label)?;
    let rectangle = [values[0], values[1], values[2], values[3]];
    if rectangle[2] <= rectangle[0] || rectangle[3] <= rectangle[1] {
        return Err(VectorMediaError::Malformed(label));
    }
    Ok(rectangle)
}

fn matrix(value: &Object, label: &'static str) -> Result<Affine, VectorMediaError> {
    let values = value
        .as_array()
        .map_err(|_| VectorMediaError::Malformed(label))?;
    let values = numbers(values, 6, label)?;
    let matrix = Affine {
        a: values[0],
        b: values[1],
        c: values[2],
        d: values[3],
        e: values[4],
        f: values[5],
    };
    matrix
        .is_valid()
        .then_some(matrix)
        .ok_or(VectorMediaError::Malformed(label))
}

fn exact_operands(
    operands: &[Object],
    count: usize,
    label: &'static str,
) -> Result<(), VectorMediaError> {
    if operands.len() != count {
        return Err(VectorMediaError::Malformed(label));
    }
    Ok(())
}

fn number(value: &Object) -> Result<f64, VectorMediaError> {
    let value = f64::from(value.as_float()?);
    if !value.is_finite() || value.abs() > MATRIX_LIMIT {
        return Err(VectorMediaError::Malformed("numeric operand"));
    }
    Ok(value)
}

fn numbers(
    operands: &[Object],
    count: usize,
    label: &'static str,
) -> Result<Vec<f64>, VectorMediaError> {
    exact_operands(operands, count, label)?;
    operands.iter().map(number).collect()
}

fn point(operands: &[Object], label: &'static str) -> Result<[f64; 2], VectorMediaError> {
    let values = numbers(operands, 2, label)?;
    Ok([values[0], values[1]])
}

fn scalar(operands: &[Object], label: &'static str) -> Result<f64, VectorMediaError> {
    let values = numbers(operands, 1, label)?;
    Ok(values[0])
}

fn integer(operands: &[Object], label: &'static str) -> Result<i64, VectorMediaError> {
    exact_operands(operands, 1, label)?;
    operands[0]
        .as_i64()
        .map_err(|_| VectorMediaError::Malformed(label))
}

fn rgb(operands: &[Object], label: &'static str) -> Result<[f64; 4], VectorMediaError> {
    let values = numbers(operands, 3, label)?;
    if values.iter().any(|value| !(0.0..=1.0).contains(value)) {
        return Err(VectorMediaError::Malformed(label));
    }
    Ok([values[0], values[1], values[2], 1.0])
}

fn gray(operands: &[Object], label: &'static str) -> Result<[f64; 4], VectorMediaError> {
    let value = scalar(operands, label)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(VectorMediaError::Malformed(label));
    }
    Ok([value, value, value, 1.0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specification_built_cases_decode_editable_paints_and_transforms() {
        let cases = [
            include_bytes!("../../tests/fixtures/vector_media/spec_case_1.ai").as_slice(),
            include_bytes!("../../tests/fixtures/vector_media/spec_case_2.ai").as_slice(),
            include_bytes!("../../tests/fixtures/vector_media/spec_case_3.ai").as_slice(),
        ];
        let decoded = cases
            .into_iter()
            .map(|bytes| decode(bytes).expect("specification-built PDF case decodes"))
            .collect::<Vec<_>>();
        assert!(
            decoded
                .iter()
                .all(|artwork| artwork.dimensions == [192.0, 128.0])
        );
        assert!(decoded.iter().all(|artwork| !artwork.shapes.is_empty()));
        assert!(decoded[0].shapes.iter().any(|shape| shape.fill.is_some()));
        assert!(decoded[1].shapes.iter().any(|shape| shape.stroke.is_some()));
        assert!(
            decoded[2]
                .shapes
                .iter()
                .any(|shape| shape.transform != Affine::IDENTITY)
        );
    }

    #[test]
    fn q_restore_does_not_restore_the_current_path() {
        let bytes = minimal_pdf(b"0 0 m q 10 0 l Q 10 10 l 1 0 0 rg f");
        let artwork = decode(&bytes).expect("bounded PDF decodes");
        assert_eq!(artwork.shapes.len(), 1);
        assert_eq!(artwork.shapes[0].path.len(), 3);
    }

    #[test]
    fn stroke_uses_paint_time_ctm_while_q_restore_keeps_constructed_path() {
        let bytes = minimal_pdf(b"0 0 m q 2 0 0 2 0 0 cm 10 0 l Q 2 w S");
        let artwork = decode(&bytes).expect("mixed-construction path decodes");
        assert_eq!(artwork.shapes.len(), 1);
        assert!(matches!(
            artwork.shapes[0].path.as_slice(),
            [PathCommand::Move([x0, y0]), PathCommand::Line([x1, y1])]
                if *x0 == 0.0 && *y0 == 0.0 && *x1 == 20.0 && *y1 == 0.0
        ));
        assert_eq!(artwork.shapes[0].stroke.as_ref().unwrap().width, 2.0);
        assert_eq!(artwork.shapes[0].transform, artwork_page_map());
    }

    #[test]
    fn unsupported_color_invalidates_stale_rgb_and_preserves_stroke_sibling() {
        let bytes = minimal_pdf(b"1 0 0 rg 0 0 10 10 re f 0 1 0 RG 0 0 0 1 k 20 0 10 10 re B");
        let artwork = decode(&bytes).expect("bounded PDF decodes");
        assert_eq!(artwork.shapes.len(), 2);
        assert!(artwork.shapes[1].fill.is_none());
        assert!(artwork.shapes[1].stroke.is_some());
        assert!(
            artwork
                .warnings
                .iter()
                .any(|warning| warning.contains("stale RGB was invalidated"))
        );
    }

    #[test]
    fn bounded_form_is_lowered_but_bbox_crossing_form_is_omitted() {
        let safe = pdf_with_form(b"1 1 8 8 re 1 0 0 rg f", b"0 0 10 10");
        let artwork = decode(&safe).expect("safe bounded Form decodes");
        assert_eq!(artwork.shapes.len(), 1);
        assert!((artwork.shapes[0].transform.e - 20.0).abs() < 0.001);
        assert!((artwork.shapes[0].transform.f - 80.0).abs() < 0.001);

        let crossing = pdf_with_form(b"-1 -1 12 12 re 1 0 0 rg f", b"0 0 10 10");
        let artwork = decode(&crossing).expect("unsafe Form is diagnosed, not fatal");
        assert!(artwork.shapes.is_empty());
        assert!(
            artwork
                .warnings
                .iter()
                .any(|warning| warning.contains("implicit BBox clip"))
        );
    }

    #[test]
    fn unbalanced_graphics_state_is_rejected_without_restoring_a_path() {
        let error = decode(&minimal_pdf(b"q 0 0 10 10 re f"))
            .expect_err("unbalanced graphics state must be rejected");
        assert!(error.to_string().contains("unbalanced q"));
    }

    #[test]
    fn review_escaped_object_stream_type_cannot_bypass_loader_limits() {
        let bytes = build_pdf(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 4 0 R >>".to_vec(),
            stream("", b"10 10 20 20 re f"),
            stream("/Type /Obj#53tm /N 1 /First 4", b"6 0 << >>"),
        ]);
        assert!(
            decode(&bytes).is_err(),
            "eager object-stream decoding must be disabled structurally, not by a byte substring"
        );
    }

    #[test]
    fn review_text_clip_does_not_expose_unclipped_paints() {
        let artwork = decode(&minimal_pdf(
            b"q BT 7 Tr ET 10 10 20 20 re f Q 50 50 10 10 re f",
        ))
        .unwrap();
        assert_eq!(
            artwork.shapes.len(),
            1,
            "unsupported text clip must suppress its scope, not restored siblings"
        );
    }

    #[test]
    fn review_form_bbox_is_checked_in_form_space_not_its_rotated_aabb() {
        let artwork = decode(&pdf_with_form(
            b"-1 5 0.5 0.5 re f",
            b"0 0 10 10] /Matrix [1 .5 .5 1 0 0",
        ))
        .unwrap();
        assert!(
            artwork.shapes.is_empty(),
            "paint outside a skewed Form BBox cannot be retained merely because its AABB contains it"
        );
    }

    #[test]
    fn review_transparency_form_is_not_rendered_as_ordinary_opaque_artwork() {
        let bytes = pdf_with_form(
            b"1 1 8 8 re f",
            b"0 0 10 10] /Group << /S /Transparency >> /Unused [0",
        );
        let artwork = decode(&bytes).unwrap();
        assert!(artwork.shapes.is_empty());
        assert!(
            artwork
                .warnings
                .iter()
                .any(|warning| warning.contains("Group"))
        );
    }

    #[test]
    fn content_array_operands_can_cross_stream_boundaries() {
        let bytes = build_pdf(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents [4 0 R 5 0 R] >>".to_vec(),
            stream("", b"1 0"),
            stream("", b"0 rg 10 10 20 20 re f"),
        ]);
        let artwork = decode(&bytes).unwrap();
        assert_eq!(artwork.shapes.len(), 1);
        assert_eq!(artwork.shapes[0].fill.unwrap().color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn default_color_overrides_are_not_silently_treated_as_device_rgb() {
        let bytes = build_pdf(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /ColorSpace << /DefaultRGB [/CalRGB << /WhitePoint [0.95 1 1.09] >>] >> >> /Contents 4 0 R >>".to_vec(),
            stream("", b"1 0 0 rg 10 10 20 20 re f"),
        ]);
        assert!(matches!(
            decode(&bytes),
            Err(VectorMediaError::Unsupported(
                "DefaultRGB/DefaultGray color-space overrides"
            ))
        ));
    }

    #[test]
    fn stroke_controls_keep_numeric_values_and_repeat_odd_dash_patterns() {
        let artwork = decode(&minimal_pdf(
            b"[3 2 1] .5 d 2 w 2 J 1 j 4 M 0 1 0 RG 10 10 m 20 20 l S",
        ))
        .unwrap();
        let stroke = artwork.shapes[0].stroke.as_ref().unwrap();
        assert_eq!(stroke.color, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(stroke.width, 2.0);
        assert!(matches!(stroke.cap, LineCap::Square));
        assert!(matches!(stroke.join, LineJoin::Round));
        assert_eq!(stroke.miter_limit, 4.0);
        assert_eq!(stroke.dashes, [3.0, 2.0, 1.0, 3.0, 2.0, 1.0]);
        assert_eq!(stroke.dash_offset, 0.5);
    }

    #[test]
    fn compressed_content_beyond_former_stream_quota_retains_paint() {
        use std::io::Write;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&vec![b' '; 4 * 1024 * 1024 + 1]).unwrap();
        encoder.write_all(b"10 10 20 20 re f").unwrap();
        let content = encoder.finish().unwrap();
        let bytes = build_pdf(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 4 0 R >>".to_vec(),
            stream("/Filter /FlateDecode", &content),
        ]);
        assert_eq!(decode(&bytes).unwrap().shapes.len(), 1);
    }

    #[test]
    fn source_beyond_former_eight_megabyte_quota_retains_paint() {
        let mut content = vec![b' '; 8 * 1024 * 1024];
        content.extend_from_slice(b"10 10 20 20 re f");
        let bytes = minimal_pdf(&content);
        assert!(bytes.len() > 8 * 1024 * 1024);
        assert_eq!(decode(&bytes).unwrap().shapes.len(), 1);
    }

    #[test]
    fn graphics_stack_dash_and_diagnostics_have_no_policy_quota() {
        let mut content = "q ".repeat(129);
        content.push_str(&"BT ET ".repeat(65));
        content.push_str(&"Q ".repeat(129));
        content.push_str(&format!("[{}] 0 d ", "1 ".repeat(33)));
        content.push_str("10 10 20 20 re S");
        let artwork = decode(&minimal_pdf(content.as_bytes())).unwrap();
        assert_eq!(artwork.shapes.len(), 1);
        assert_eq!(artwork.shapes[0].stroke.as_ref().unwrap().dashes.len(), 66);
        assert_eq!(artwork.warnings.len(), 131);
    }

    #[test]
    fn operation_path_and_shape_counts_beyond_former_quotas_are_editable() {
        let mut content = "q Q ".repeat(50_001);
        content.push_str(&"10 10 20 20 re f ".repeat(10_001));
        let artwork = decode(&minimal_pdf(content.as_bytes())).unwrap();
        assert_eq!(artwork.shapes.len(), 10_001);
        assert!(artwork.shapes.iter().all(|shape| shape.path.len() == 5));
    }

    #[test]
    fn object_count_beyond_former_quota_retains_paint() {
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 4 0 R >>".to_vec(),
            stream("", b"10 10 20 20 re f"),
        ];
        objects.resize(20_001, b"null".to_vec());
        assert_eq!(decode(&build_pdf(objects)).unwrap().shapes.len(), 1);
    }

    #[test]
    fn excessive_acyclic_form_nesting_interrupts_conversion() {
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Fm 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
            stream("", b"/Fm Do"),
        ];
        for id in 5..=22 {
            let resources = if id < 22 {
                format!("/Resources << /XObject << /Fm {} 0 R >> >>", id + 1)
            } else {
                String::new()
            };
            let content = if id < 22 {
                b"/Fm Do".as_slice()
            } else {
                b"10 10 20 20 re f".as_slice()
            };
            objects.push(stream(
                &format!("/Type /XObject /Subtype /Form /BBox [0 0 100 100] {resources}"),
                content,
            ));
        }
        assert!(matches!(
            decode(&build_pdf(objects)),
            Err(VectorMediaError::Interrupted(
                "Form XObject recursion depth"
            ))
        ));
    }

    #[test]
    fn excessive_form_expansion_work_interrupts_conversion() {
        let repeated = "/Leaf Do ".repeat(MAX_FORM_OPERATIONS + 1);
        let bytes = build_pdf(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Fm 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
            stream("", b"/Fm Do"),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /XObject << /Leaf 6 0 R >> >>",
                repeated.as_bytes(),
            ),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 100 100]",
                b"10 10 20 20 re f",
            ),
        ]);
        assert!(matches!(
            decode(&bytes),
            Err(VectorMediaError::Interrupted("Form expansion work limit"))
        ));
    }

    #[test]
    fn inherited_page_properties_beyond_former_depth_quota_are_read() {
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 4 0 R /Contents 22 0 R >>".to_vec(),
        ];
        for id in 4..=21 {
            objects.push(if id == 21 {
                b"<< /MediaBox [0 0 100 100] /Resources << >> >>".to_vec()
            } else {
                format!("<< /Parent {} 0 R >>", id + 1).into_bytes()
            });
        }
        objects.push(stream("", b"10 10 20 20 re f"));
        assert_eq!(decode(&build_pdf(objects)).unwrap().shapes.len(), 1);
    }

    #[test]
    fn cyclic_form_is_still_rejected() {
        let bytes = build_pdf(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Fm 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
            stream("", b"/Fm Do"),
            stream("/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /XObject << /Fm 5 0 R >> >>", b"/Fm Do"),
        ]);
        assert!(matches!(
            decode(&bytes),
            Err(VectorMediaError::Malformed("cyclic Form XObject"))
        ));
    }

    fn artwork_page_map() -> Affine {
        Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: -1.0,
            e: 0.0,
            f: 100.0,
        }
    }

    fn minimal_pdf(content: &[u8]) -> Vec<u8> {
        build_pdf(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 4 0 R >>".to_vec(),
            stream("", content),
        ])
    }

    fn pdf_with_form(form_content: &[u8], bbox: &[u8]) -> Vec<u8> {
        let page_content = b"q 1 0 0 1 20 20 cm /Fm Do Q";
        let form_dictionary = format!(
            "/Type /XObject /Subtype /Form /BBox [{}]",
            std::str::from_utf8(bbox).expect("ASCII bbox")
        );
        build_pdf(vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Fm 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
            stream("", page_content),
            stream(&form_dictionary, form_content),
        ])
    }

    fn stream(dictionary: &str, content: &[u8]) -> Vec<u8> {
        let mut output =
            format!("<< {dictionary} /Length {} >>\nstream\n", content.len()).into_bytes();
        output.extend_from_slice(content);
        output.extend_from_slice(b"\nendstream");
        output
    }

    fn build_pdf(objects: Vec<Vec<u8>>) -> Vec<u8> {
        let object_count = objects.len();
        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.4\n");
        let mut offsets = Vec::new();
        for object in objects {
            offsets.push(pdf.len());
            let id = offsets.len();
            pdf.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
            pdf.extend_from_slice(&object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                object_count + 1
            )
            .as_bytes(),
        );
        pdf
    }
}
