//! The certified topology of an FX shape path of several contours, which
//! exports as one Premiere piece per contour: a Premiere Path holds one
//! contour (`format::shape_payload`), so each hole becomes an inverted Mask
//! with Shape above the piece
//! that it cuts, inside the SubGroup that bounds it (`convert::graphic`).
//!
//! That construction fills what FX fills only when the contours are simple,
//! apart, and nested as the fill rule reads them. Each fact is certified, or
//! the path is refused:
//! - A cubic lies inside the bounding box of its control points, and de
//!   Casteljau subdivision halves a piece into two cubics whose boxes
//!   converge on it. Two pieces whose boxes lie `gap` apart are at least
//!   `gap` apart, and two whose boxes lie nearer than `gap` less both
//!   diagonals are nearer, so refinement decides any separation that is not
//!   within rounding of the one asked for, down to [`MAX_DEPTH`] halvings
//!   and [`BUDGET`] comparisons; closer, touching or crossing pieces, and
//!   undecided ones, are refused. No curve is flattened: the exported pieces
//!   keep the original cubics.
//! - Two consecutive pieces of one contour meet at their shared point. A
//!   cubic leaves its start inside the cone of its derivative's control
//!   vectors, so pieces that leave the shared point in disjoint cones meet
//!   only there; a cusp or a segment that doubles back is refused.
//! - A contour that is simple and apart from another lies entirely inside or
//!   outside it, so one of its points decides containment: the signed
//!   crossings of a horizontal ray with the other contour's y-monotone
//!   pieces, each resolved by subdividing its box until the box lies on one
//!   side of the point. Orientation is the sign of the enclosed area, the
//!   exact Green integral of each cubic by three-point Gauss-Legendre
//!   quadrature, refused when rounding could flip it.
//!
//! Coordinates are FX layer pixels in f64. Every box is widened by
//! [`rounding`], which covers the error of repeated f64 subdivision and the
//! narrowing of Premiere's f32 coordinates (at most 2^-24 of a coordinate;
//! a cubic moves at most as far as its control points). This box margin alone
//! does not protect adjacent edges or orientation from narrowing. The actual
//! native path, including endpoint welding, is certified separately and must
//! retain every orientation, containment relation and fill role. Transforms
//! are affine and shared by every piece, and a stroke scales with its path, so every transform keeps
//! these facts and the caller's separation, its strokes' reach in layer
//! pixels. Only antialiasing depends on the device scale: the caller's
//! clearance, one device pixel's worth of layer pixels, finds contours whose
//! edges may share a pixel ([`Contours::near`]).

use fx_schema::{ShapeFillRule, ShapePath, ShapePathCommand};

type Point = [f64; 2];

/// Halvings before an undecided comparison is refused: 2^-30 of a piece.
const MAX_DEPTH: u32 = 30;
/// Box comparisons before a path is refused as too close to decide, which
/// bounds the work of pieces that run near the asked separation for long.
const BUDGET: usize = 1 << 20;

/// What one contour exports as, from the fill status of the region just
/// inside it and of the region just outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContourRole {
    /// Filled inside, unfilled outside: a piece with the shape's fill.
    Filled,
    /// Unfilled inside, filled outside: an inverted mask of the piece
    /// around it.
    Hole,
    /// The same on both sides: only its stroke draws.
    Boundary,
}

/// One contour, as its commands from its `moveTo` (a `close` included) and
/// its role, with the depth of its nesting.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Contour {
    pub(super) path: ShapePath,
    pub(super) role: ContourRole,
    pub(super) depth: usize,
}

/// The certified contours of a path, and the first two that may come within
/// the caller's clearance of each other.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Contours {
    /// Deepest first.
    pub(super) contours: Vec<Contour>,
    /// The one-based numbers of the first two contours not certified
    /// `separation + clearance` apart, whose antialiased edges may share a
    /// device pixel.
    pub(super) near: Option<[usize; 2]>,
}

/// The contours of `path`, deepest first, each with its role under
/// `fill_rule`, or why they cannot be certified. Every two contours must lie
/// at least `separation` apart in layer pixels, so that the caller's strokes
/// cannot meet another contour; two that are not also `clearance` further
/// apart are [`Contours::near`]. `fill_rule` is `None` for a path without
/// fill, whose contours are all boundaries.
pub(super) fn contours(
    path: &ShapePath,
    fill_rule: Option<ShapeFillRule>,
    separation: f64,
    clearance: f64,
) -> Result<Contours, String> {
    let original = certify(path, fill_rule, separation, clearance)?;
    // Use exactly the writer's conversion and the reader's interpretation:
    // rounding coordinates alone would miss the closing-endpoint weld.
    let mut native_commands = Vec::new();
    for (commands, _) in split_contours(path)? {
        let native = super::premiere_path(&ShapePath { commands })
            .map_err(|error| format!("native contour topology cannot be certified: {error}"))?;
        native_commands.extend(super::fx_path(&native).commands);
    }
    let native = certify(
        &ShapePath {
            commands: native_commands,
        },
        fill_rule,
        separation,
        clearance,
    )
    .map_err(|reason| {
        format!("native contour topology after f32 narrowing and endpoint welding: {reason}")
    })?;
    if original.orientations != native.orientations
        || original.containers != native.containers
        || original
            .result
            .contours
            .iter()
            .map(|contour| (contour.role, contour.depth))
            .ne(native
                .result
                .contours
                .iter()
                .map(|contour| (contour.role, contour.depth)))
    {
        return Err(
            "native contour topology changes after f32 narrowing and endpoint welding".to_owned(),
        );
    }
    Ok(Contours {
        near: original.result.near.or(native.result.near),
        ..original.result
    })
}

/// Relations use source contour order, before the output's deepest-first sort.
struct Certificate {
    result: Contours,
    orientations: Vec<i32>,
    containers: Vec<Vec<usize>>,
}

/// One bounded pass on either the original coordinates or actual native geometry.
fn certify(
    path: &ShapePath,
    fill_rule: Option<ShapeFillRule>,
    separation: f64,
    clearance: f64,
) -> Result<Certificate, String> {
    let split = split_contours(path)?;
    let pieces: Vec<Vec<Cubic>> = split
        .iter()
        .map(|(_, segments)| monotone_pieces(segments))
        .collect::<Result<_, _>>()?;
    let scale = pieces
        .iter()
        .flatten()
        .flat_map(|piece| piece.0)
        .flatten()
        .fold(1.0_f64, |largest, value| largest.max(value.abs()));
    let eps = rounding(scale);
    let mut budget = BUDGET;
    // The clearance only reports, so it has its own budget and cannot starve
    // a certificate.
    let mut near_budget = BUDGET;
    let mut near = None;
    for (index, contour) in pieces.iter().enumerate() {
        simple(contour, eps, &mut budget)
            .map_err(|reason| format!("contour {} of the shape path {reason}", index + 1))?;
        for (other_index, other) in pieces.iter().enumerate().skip(index + 1) {
            let numbers = [index + 1, other_index + 1];
            for piece in contour {
                for other_piece in other {
                    if apart(piece, other_piece, separation + 2.0 * eps, eps, &mut budget)
                        != Some(true)
                    {
                        let [first, second] = numbers;
                        return Err(if separation > 0.0 {
                            format!(
                                "contours {first} and {second} of the shape path are not certified {separation} px apart"
                            )
                        } else {
                            format!(
                                "contours {first} and {second} of the shape path are not certified apart"
                            )
                        });
                    }
                    // Undecided counts as near.
                    if near.is_none()
                        && apart(
                            piece,
                            other_piece,
                            separation + clearance + 2.0 * eps,
                            eps,
                            &mut near_budget,
                        ) != Some(true)
                    {
                        near = Some(numbers);
                    }
                }
            }
        }
    }
    let orientations = pieces
        .iter()
        .enumerate()
        .map(|(index, contour)| {
            orientation(contour).ok_or_else(|| {
                format!(
                    "contour {} of the shape path encloses too little area to orient",
                    index + 1
                )
            })
        })
        .collect::<Result<Vec<i32>, String>>()?;
    // `inside[j]` lists the contours that contain contour j.
    let mut inside = vec![Vec::new(); pieces.len()];
    for (j, contour) in pieces.iter().enumerate() {
        for (i, other) in pieces.iter().enumerate() {
            if i != j && winding(contour, other, eps)? != 0 {
                inside[j].push(i);
            }
        }
    }
    let filled = |winding: i32, depth: usize| match fill_rule {
        None => false,
        Some(ShapeFillRule::NonZeroWinding) => winding != 0,
        Some(ShapeFillRule::EvenOdd) => depth % 2 == 1,
    };
    let mut result: Vec<Contour> = split
        .into_iter()
        .enumerate()
        .map(|(j, (commands, _))| {
            let outside: i32 = inside[j].iter().map(|&i| orientations[i]).sum();
            let depth = inside[j].len();
            let role = match (
                filled(outside + orientations[j], depth + 1),
                filled(outside, depth),
            ) {
                (true, false) => ContourRole::Filled,
                (false, true) => ContourRole::Hole,
                _ => ContourRole::Boundary,
            };
            Contour {
                path: ShapePath { commands },
                role,
                depth,
            }
        })
        .collect();
    result.sort_by(|a, b| b.depth.cmp(&a.depth));
    Ok(Certificate {
        result: Contours {
            contours: result,
            near,
        },
        orientations,
        containers: inside,
    })
}

/// The largest error that f64 subdivision and Premiere's f32 coordinates
/// add to a box of a path whose coordinates reach `scale`: 2^-24 of it for
/// the narrowing, and far more than [`MAX_DEPTH`] exact-to-half-ulp
/// subdivisions add.
fn rounding(scale: f64) -> f64 {
    scale * 2f64.powi(-23)
}

/// One cubic: its control points; a line has its control points on it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cubic([Point; 4]);

impl Cubic {
    fn line(from: Point, to: Point) -> Self {
        let third = |t: f64| {
            [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ]
        };
        Self([from, third(1.0 / 3.0), third(2.0 / 3.0), to])
    }

    fn start(&self) -> Point {
        self.0[0]
    }

    fn end(&self) -> Point {
        self.0[3]
    }

    /// The point at `t` and the two cubics that `t` splits this one into,
    /// which share it exactly.
    fn split(&self, t: f64) -> (Self, Self) {
        let lerp = |a: Point, b: Point| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let [p0, p1, p2, p3] = self.0;
        let (a, b, c) = (lerp(p0, p1), lerp(p1, p2), lerp(p2, p3));
        let (d, e) = (lerp(a, b), lerp(b, c));
        let middle = lerp(d, e);
        (Self([p0, a, d, middle]), Self([middle, e, c, p3]))
    }

    /// The bounding box of the control points, `[min, max]`, widened by
    /// `eps`, which contains the curve.
    fn bounds(&self, eps: f64) -> [Point; 2] {
        let mut min = self.0[0];
        let mut max = self.0[0];
        for point in &self.0[1..] {
            for axis in 0..2 {
                min[axis] = min[axis].min(point[axis]);
                max[axis] = max[axis].max(point[axis]);
            }
        }
        [[min[0] - eps, min[1] - eps], [max[0] + eps, max[1] + eps]]
    }

    /// The derivative's control vectors, which bound its directions.
    fn hodograph(&self) -> [Point; 3] {
        let [p0, p1, p2, p3] = self.0;
        let difference = |a: Point, b: Point| [b[0] - a[0], b[1] - a[1]];
        [difference(p0, p1), difference(p1, p2), difference(p2, p3)]
    }

    fn reversed(&self) -> Self {
        let [p0, p1, p2, p3] = self.0;
        Self([p3, p2, p1, p0])
    }
}

/// One contour's commands, from its `moveTo`, and its segments.
type SplitContour = (Vec<ShapePathCommand>, Vec<Cubic>);

/// The contours of `path`: the commands of each, from its `moveTo`, and its
/// segments, closed back to its start as FX fills it. Refuses rounded
/// anchors, which a Premiere vertex cannot hold, and a contour that goes on
/// after its `close`.
fn split_contours(path: &ShapePath) -> Result<Vec<SplitContour>, String> {
    let mut contours: Vec<SplitContour> = Vec::new();
    let mut current = [0.0; 2];
    let mut closed = false;
    for command in &path.commands {
        if command.corner_radius().is_some_and(|radius| radius != 0.0) {
            return Err("rounded shape path corners are unsupported".to_owned());
        }
        match *command {
            ShapePathCommand::MoveTo { x, y, .. } => {
                current = [x, y];
                closed = false;
                contours.push((vec![command.clone()], Vec::new()));
                continue;
            }
            _ if closed => {
                return Err("a shape path contour goes on after its close".to_owned());
            }
            _ => {}
        }
        let Some((commands, segments)) = contours.last_mut() else {
            return Err("a shape path must start with a move".to_owned());
        };
        commands.push(command.clone());
        match *command {
            ShapePathCommand::LineTo { x, y, .. } => {
                segments.push(Cubic::line(current, [x, y]));
                current = [x, y];
            }
            ShapePathCommand::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
                ..
            } => {
                segments.push(Cubic([current, [c1x, c1y], [c2x, c2y], [x, y]]));
                current = [x, y];
            }
            ShapePathCommand::Close => closed = true,
            ShapePathCommand::MoveTo { .. } => {}
        }
    }
    for (_, segments) in &mut contours {
        let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
            return Err("a shape path contour draws no segment".to_owned());
        };
        if first.start() != last.end() {
            let closing = Cubic::line(last.end(), first.start());
            segments.push(closing);
        }
        // A segment of one point draws nothing.
        segments.retain(|segment| segment.0.iter().any(|point| *point != segment.0[0]));
        if segments.is_empty() {
            return Err("a shape path contour encloses nothing".to_owned());
        }
    }
    Ok(contours)
}

/// The pieces of `segments` that are monotone in y, split where the
/// derivative of y is zero, so that each crosses a horizontal line at most
/// once. A split near an exact root leaves at most a rounding-sized
/// overshoot, which the boxes' widening covers.
fn monotone_pieces(segments: &[Cubic]) -> Result<Vec<Cubic>, String> {
    let mut pieces = Vec::new();
    for segment in segments {
        let mut roots: Vec<f64> = derivative_roots(segment, 1)
            .into_iter()
            .filter(|t| *t > 1e-9 && *t < 1.0 - 1e-9)
            .collect();
        roots.sort_by(f64::total_cmp);
        roots.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        let mut rest = *segment;
        let mut done = 0.0;
        for t in roots {
            let (piece, tail) = rest.split((t - done) / (1.0 - done));
            pieces.push(piece);
            rest = tail;
            done = t;
        }
        pieces.push(rest);
    }
    Ok(pieces)
}

/// The parameters in [0, 1] where the derivative of `cubic` along `axis`
/// is zero: the roots of a quadratic in Bernstein form.
fn derivative_roots(cubic: &Cubic, axis: usize) -> Vec<f64> {
    let [p0, p1, p2, p3] = cubic.0.map(|point| point[axis]);
    let (d0, d1, d2) = (p1 - p0, p2 - p1, p3 - p2);
    let a = d0 - 2.0 * d1 + d2;
    let b = 2.0 * (d1 - d0);
    let c = d0;
    if a.abs() <= 1e-12 * (d0.abs() + d1.abs() + d2.abs()) {
        return if b != 0.0 { vec![-c / b] } else { Vec::new() };
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return Vec::new();
    }
    let root = discriminant.sqrt();
    // The stable quadratic formula.
    let q = -0.5 * (b + b.signum() * root);
    let mut roots = vec![q / a];
    if q != 0.0 {
        roots.push(c / q);
    }
    roots
}

/// Whether the pieces are at least `gap` apart: `Some(true)` when their
/// boxes, or those of their halves, are that far apart, `Some(false)` when
/// two of them are provably nearer, and `None` when [`MAX_DEPTH`] or the
/// `budget` runs out first.
fn apart(a: &Cubic, b: &Cubic, gap: f64, eps: f64, budget: &mut usize) -> Option<bool> {
    fn visit(
        a: &Cubic,
        b: &Cubic,
        gap: f64,
        eps: f64,
        depth: u32,
        budget: &mut usize,
    ) -> Option<bool> {
        *budget = budget.checked_sub(1)?;
        let ([a_min, a_max], [b_min, b_max]) = (a.bounds(eps), b.bounds(eps));
        let dx = (b_min[0] - a_max[0]).max(a_min[0] - b_max[0]).max(0.0);
        let dy = (b_min[1] - a_max[1]).max(a_min[1] - b_max[1]).max(0.0);
        let distance = dx.hypot(dy);
        if distance >= gap && distance > 0.0 {
            return Some(true);
        }
        // Any two points of the pieces are nearer than this.
        let diagonal = |[min, max]: [Point; 2]| (max[0] - min[0]).hypot(max[1] - min[1]);
        if distance + diagonal(a.bounds(eps)) + diagonal(b.bounds(eps)) < gap {
            return Some(false);
        }
        if depth >= MAX_DEPTH {
            return None;
        }
        let (a0, a1) = a.split(0.5);
        let (b0, b1) = b.split(0.5);
        for (a, b) in [(&a0, &b0), (&a0, &b1), (&a1, &b0), (&a1, &b1)] {
            if !visit(a, b, gap, eps, depth + 1, budget)? {
                return Some(false);
            }
        }
        Some(true)
    }
    visit(a, b, gap, eps, 0, budget)
}

/// Why the closed chain of `pieces` is not certified simple, if it is not:
/// every two pieces that do not follow each other are apart, and every two
/// that do meet only at their shared point.
fn simple(pieces: &[Cubic], eps: f64, budget: &mut usize) -> Result<(), &'static str> {
    let count = pieces.len();
    if count == 1 {
        // One piece that closes on itself is a loop only if it turns back.
        return meet_once(&pieces[0], &pieces[0], eps, budget);
    }
    for i in 0..count {
        for j in i + 1..count {
            let (a, b) = (&pieces[i], &pieces[j]);
            let adjacent_after = j == i + 1;
            let adjacent_before = i == 0 && j == count - 1;
            match (adjacent_after, adjacent_before) {
                (false, false) => {
                    if apart(a, b, eps, eps, budget) != Some(true) {
                        return Err("crosses or touches itself");
                    }
                }
                (true, false) => meet_once(a, b, eps, budget)?,
                (false, true) => meet_once(b, a, eps, budget)?,
                // Two pieces that close a contour on both ends.
                (true, true) => {
                    meet_once(a, b, eps, budget)?;
                    meet_once(b, a, eps, budget)?;
                }
            }
        }
    }
    Ok(())
}

/// Why `a` and `b`, where `a` ends and `b` starts, are not certified to meet
/// only there, if they are not: the halves at the shared point leave it in
/// disjoint cones, and every other pair of halves is apart. For one piece
/// that closes on itself, `a` and `b` are that piece.
fn meet_once(a: &Cubic, b: &Cubic, eps: f64, budget: &mut usize) -> Result<(), &'static str> {
    // `a` reversed, so that both leave the shared point.
    let (mut a, mut b) = (a.reversed(), *b);
    let mut far = Vec::new();
    for _ in 0..MAX_DEPTH {
        if let (Some(a_cone), Some(b_cone)) = (cone(&a), cone(&b)) {
            if disjoint(a_cone, b_cone) {
                for (x, y) in &far {
                    if apart(x, y, eps, eps, budget) != Some(true) {
                        return Err("crosses or touches itself");
                    }
                }
                return Ok(());
            }
        }
        let (a_near, a_far) = a.split(0.5);
        let (b_near, b_far) = b.split(0.5);
        far.extend([(a_far, b_far), (a_near, b_far), (a_far, b_near)]);
        (a, b) = (a_near, b_near);
    }
    Err("turns back on itself at a cusp")
}

/// The directions that a piece leaves its start in, as an angle interval
/// `[from, to]` of width below half a turn: the cone of its derivative's
/// nonzero control vectors. `None` when they span half a turn or more.
fn cone(piece: &Cubic) -> Option<[f64; 2]> {
    let vectors: Vec<Point> = piece
        .hodograph()
        .into_iter()
        .filter(|vector| *vector != [0.0; 2])
        .collect();
    let first = vectors.first()?;
    let base = first[1].atan2(first[0]);
    let mut low: f64 = 0.0;
    let mut high: f64 = 0.0;
    for vector in &vectors {
        let mut angle = vector[1].atan2(vector[0]) - base;
        while angle <= -std::f64::consts::PI {
            angle += std::f64::consts::TAU;
        }
        while angle > std::f64::consts::PI {
            angle -= std::f64::consts::TAU;
        }
        low = low.min(angle);
        high = high.max(angle);
    }
    // A margin for the angles' rounding.
    const MARGIN: f64 = 1e-9;
    (high - low + 2.0 * MARGIN < std::f64::consts::PI)
        .then_some([base + low - MARGIN, base + high + MARGIN])
}

/// Whether two angle intervals of width below half a turn do not overlap.
fn disjoint(a: [f64; 2], b: [f64; 2]) -> bool {
    let centre = |[from, to]: [f64; 2]| (from + to) / 2.0;
    let half = |[from, to]: [f64; 2]| (to - from) / 2.0;
    let mut distance = (centre(a) - centre(b)).rem_euclid(std::f64::consts::TAU);
    if distance > std::f64::consts::PI {
        distance = std::f64::consts::TAU - distance;
    }
    distance > half(a) + half(b)
}

/// The orientation of a simple closed chain, the sign of its enclosed area
/// ∮ x dy (either sign is one of the two directions), or `None` when
/// rounding could flip it.
fn orientation(pieces: &[Cubic]) -> Option<i32> {
    // Three-point Gauss-Legendre is exact for x·y', of degree five.
    const NODES: [(f64, f64); 3] = [
        (0.112_701_665_379_258_31, 5.0 / 18.0),
        (0.5, 8.0 / 18.0),
        (0.887_298_334_620_741_7, 5.0 / 18.0),
    ];
    let mut area = 0.0;
    let mut magnitude = 0.0;
    for piece in pieces {
        let [p0, p1, p2, p3] = piece.0;
        for (t, weight) in NODES {
            let u = 1.0 - t;
            let point = |axis: usize| {
                u * u * u * p0[axis]
                    + 3.0 * u * u * t * p1[axis]
                    + 3.0 * u * t * t * p2[axis]
                    + t * t * t * p3[axis]
            };
            let tangent = |axis: usize| {
                3.0 * (u * u * (p1[axis] - p0[axis])
                    + 2.0 * u * t * (p2[axis] - p1[axis])
                    + t * t * (p3[axis] - p2[axis]))
            };
            let term = point(0) * tangent(1) - point(1) * tangent(0);
            area += weight * term;
            magnitude += weight * term.abs();
        }
    }
    (area.abs() > 1e-9 * magnitude).then_some(if area > 0.0 { 1 } else { -1 })
}

/// The winding number of `other`, a closed chain of monotone pieces, around
/// the chain `contour`, which lies entirely inside or outside it: the signed
/// crossings of a rightward ray from one of `contour`'s points, one whose y
/// is clear of every piece end of `other`.
fn winding(contour: &[Cubic], other: &[Cubic], eps: f64) -> Result<i32, String> {
    let ends: Vec<f64> = other.iter().map(|piece| piece.start()[1]).collect();
    let point = contour
        .iter()
        .flat_map(|piece| [piece.start(), piece.split(0.5).0.end()])
        .find(|point| ends.iter().all(|y| (y - point[1]).abs() > 4.0 * eps))
        .ok_or_else(|| "no point of a contour clears the other contour's ends".to_owned())?;
    let mut winding = 0;
    for piece in other {
        winding += crossing(piece, point, eps, 0)
            .ok_or_else(|| "a contour's containment is too close to decide".to_owned())?;
    }
    Ok(winding)
}

/// The signed crossing of a monotone piece with the ray rightward from
/// `point`: `1` upward in y, `-1` downward, `0` when it misses; the lower
/// end counts and the upper end does not. `None` when subdivision cannot
/// decide the side.
fn crossing(piece: &Cubic, point: Point, eps: f64, depth: u32) -> Option<i32> {
    let (y0, y1) = (piece.start()[1], piece.end()[1]);
    let (low, high, sign) = if y0 < y1 {
        (y0, y1, 1)
    } else if y1 < y0 {
        (y1, y0, -1)
    } else {
        return Some(0);
    };
    if point[1] < low || point[1] >= high {
        return Some(0);
    }
    let [min, max] = piece.bounds(eps);
    if min[0] > point[0] {
        return Some(sign);
    }
    if max[0] < point[0] {
        return Some(0);
    }
    if depth >= MAX_DEPTH {
        return None;
    }
    let (first, second) = piece.split(0.5);
    Some(crossing(&first, point, eps, depth + 1)? + crossing(&second, point, eps, depth + 1)?)
}

#[cfg(test)]
mod tests {
    use super::{contours, ContourRole, Contours};
    use fx_schema::{ShapeFillRule, ShapePath, ShapePathCommand};
    use ContourRole::{Boundary, Filled, Hole};

    const EVEN_ODD: Option<ShapeFillRule> = Some(ShapeFillRule::EvenOdd);
    const NON_ZERO: Option<ShapeFillRule> = Some(ShapeFillRule::NonZeroWinding);

    fn point(command: fn(f64, f64) -> ShapePathCommand, [x, y]: [f64; 2]) -> ShapePathCommand {
        command(x, y)
    }

    fn move_to(x: f64, y: f64) -> ShapePathCommand {
        ShapePathCommand::MoveTo {
            x,
            y,
            mirror: None,
            corner_radius: None,
        }
    }

    fn line_to(x: f64, y: f64) -> ShapePathCommand {
        ShapePathCommand::LineTo {
            x,
            y,
            mirror: None,
            corner_radius: None,
        }
    }

    /// A closed polygon through `points`.
    fn polygon(points: &[[f64; 2]]) -> Vec<ShapePathCommand> {
        let mut commands = vec![point(move_to, points[0])];
        commands.extend(points[1..].iter().map(|&vertex| point(line_to, vertex)));
        commands.push(ShapePathCommand::Close);
        commands
    }

    /// An axis-aligned square from `min` to `max`, clockwise on screen, or
    /// the other way with `reversed`.
    fn square(min: f64, max: f64, reversed: bool) -> Vec<ShapePathCommand> {
        let mut points = vec![[min, min], [max, min], [max, max], [min, max]];
        if reversed {
            points.reverse();
        }
        polygon(&points)
    }

    /// A circle of four cubic arcs about `centre`.
    fn circle(centre: [f64; 2], radius: f64) -> Vec<ShapePathCommand> {
        let k = 0.552_284_749_830_793_4 * radius;
        let [cx, cy] = centre;
        let arc = |c1: [f64; 2], c2: [f64; 2], end: [f64; 2]| ShapePathCommand::CubicTo {
            c1x: cx + c1[0],
            c1y: cy + c1[1],
            c2x: cx + c2[0],
            c2y: cy + c2[1],
            x: cx + end[0],
            y: cy + end[1],
            mirror: None,
            corner_radius: None,
        };
        vec![
            move_to(cx + radius, cy),
            arc([radius, k], [k, radius], [0.0, radius]),
            arc([-k, radius], [-radius, k], [-radius, 0.0]),
            arc([-radius, -k], [-k, -radius], [0.0, -radius]),
            arc([k, -radius], [radius, -k], [radius, 0.0]),
            ShapePathCommand::Close,
        ]
    }

    fn roles(
        contours_of: &[Vec<ShapePathCommand>],
        rule: Option<ShapeFillRule>,
    ) -> Result<Vec<(ContourRole, usize)>, String> {
        let path = ShapePath {
            commands: contours_of.concat(),
        };
        let Contours { contours, near } = contours(&path, rule, 0.0, 2.0)?;
        assert_eq!(near, None, "these contours are more than 2 px apart");
        // Each contour keeps its own commands.
        let mut kept: Vec<_> = contours
            .iter()
            .flat_map(|contour| contour.path.commands.clone())
            .collect();
        let mut original = path.commands.clone();
        let key = |command: &ShapePathCommand| format!("{command:?}");
        kept.sort_by_key(key);
        original.sort_by_key(key);
        assert_eq!(kept, original);
        Ok(contours
            .into_iter()
            .map(|contour| (contour.role, contour.depth))
            .collect())
    }

    #[test]
    fn native_narrowing_rejects_a_collapsed_triangle_but_keeps_a_representable_hole() {
        const A: f64 = 16_777_216.0;
        let path = |size| ShapePath {
            commands: [
                square(A - 100.0, A + 100.0, false),
                polygon(&[[A, A], [A + size, A], [A + size, A + size]]),
            ]
            .concat(),
        };
        let error = contours(&path(0.5), EVEN_ODD, 0.0, 2.0).unwrap_err();
        assert!(error.contains("native contour topology"), "{error}");
        let kept = contours(&path(32.0), EVEN_ODD, 0.0, 2.0).unwrap();
        assert_eq!(
            kept.contours
                .iter()
                .map(|contour| contour.role)
                .collect::<Vec<_>>(),
            [Hole, Filled]
        );
    }

    #[test]
    fn contours_take_the_role_their_fill_rule_and_nesting_give_them() {
        // A C whose bay holds a square: the C's bounding box contains the
        // square, the C does not.
        let c = polygon(&[
            [0.0, 0.0],
            [100.0, 0.0],
            [100.0, 20.0],
            [20.0, 20.0],
            [20.0, 80.0],
            [100.0, 80.0],
            [100.0, 100.0],
            [0.0, 100.0],
        ]);
        let flip = |commands: Vec<ShapePathCommand>| -> Vec<ShapePathCommand> {
            commands
                .into_iter()
                .map(|command| match command {
                    ShapePathCommand::MoveTo { x, y, .. } => move_to(-x * 1e4, y * 1e4),
                    ShapePathCommand::LineTo { x, y, .. } => line_to(-x * 1e4, y * 1e4),
                    other => other,
                })
                .collect()
        };
        for (case, contours_of, rule, expected) in [
            (
                "even-odd donut of one winding",
                vec![square(0.0, 100.0, false), square(30.0, 70.0, false)],
                EVEN_ODD,
                vec![(Hole, 1), (Filled, 0)],
            ),
            (
                "nonzero donut of opposite windings",
                vec![square(0.0, 100.0, false), square(30.0, 70.0, true)],
                NON_ZERO,
                vec![(Hole, 1), (Filled, 0)],
            ),
            (
                "nonzero contours of one winding",
                vec![square(0.0, 100.0, false), square(30.0, 70.0, false)],
                NON_ZERO,
                vec![(Boundary, 1), (Filled, 0)],
            ),
            (
                "even-odd island in a hole",
                vec![
                    square(0.0, 100.0, false),
                    square(20.0, 80.0, false),
                    square(40.0, 60.0, true),
                ],
                EVEN_ODD,
                vec![(Filled, 2), (Hole, 1), (Filled, 0)],
            ),
            (
                "nonzero island of the outer winding in a hole",
                vec![
                    square(0.0, 100.0, false),
                    square(20.0, 80.0, true),
                    square(40.0, 60.0, false),
                ],
                NON_ZERO,
                vec![(Filled, 2), (Hole, 1), (Filled, 0)],
            ),
            (
                "disjoint contours",
                vec![square(0.0, 10.0, false), square(20.0, 30.0, true)],
                NON_ZERO,
                vec![(Filled, 0), (Filled, 0)],
            ),
            (
                "a square in a C's bay",
                vec![c.clone(), square(50.0, 70.0, false)],
                EVEN_ODD,
                vec![(Filled, 0), (Filled, 0)],
            ),
            (
                "curved donut",
                vec![circle([0.0, 0.0], 50.0), circle([5.0, 0.0], 20.0)],
                EVEN_ODD,
                vec![(Hole, 1), (Filled, 0)],
            ),
            (
                "curved nonzero boundary",
                vec![circle([0.0, 0.0], 50.0), circle([5.0, 0.0], 20.0)],
                NON_ZERO,
                vec![(Boundary, 1), (Filled, 0)],
            ),
            (
                "reflected and scaled donut",
                vec![
                    flip(square(0.0, 100.0, false)),
                    flip(square(30.0, 70.0, true)),
                ],
                NON_ZERO,
                vec![(Hole, 1), (Filled, 0)],
            ),
            (
                "an unfilled path's contours",
                vec![square(0.0, 100.0, false), square(30.0, 70.0, false)],
                None,
                vec![(Boundary, 1), (Boundary, 0)],
            ),
        ] {
            assert_eq!(roles(&contours_of, rule), Ok(expected), "{case}");
        }
    }

    #[test]
    fn contours_that_cannot_be_certified_are_refused() {
        let open_triangle = |x: f64| {
            vec![
                move_to(x, 0.0),
                line_to(x + 50.0, 0.0),
                line_to(x + 50.0, 50.0),
            ]
        };
        for (case, contours_of, reason) in [
            (
                "crossing contours",
                vec![square(0.0, 10.0, false), square(5.0, 15.0, false)],
                "contours 1 and 2 of the shape path are not certified 2 px apart",
            ),
            (
                "contours nearer than the strokes' separation",
                vec![square(0.0, 10.0, false), square(11.0, 20.0, false)],
                "contours 1 and 2 of the shape path are not certified 2 px apart",
            ),
            (
                "a touching hole",
                vec![square(0.0, 100.0, false), square(0.0, 50.0, false)],
                "contours 1 and 2 of the shape path are not certified 2 px apart",
            ),
            (
                "a bow tie",
                vec![
                    polygon(&[[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]]),
                    square(20.0, 30.0, false),
                ],
                "contour 1 of the shape path crosses or touches itself",
            ),
            (
                "a retraced segment",
                vec![
                    polygon(&[[0.0, 0.0], [10.0, 0.0], [5.0, 0.0]]),
                    square(20.0, 30.0, false),
                ],
                "contour 1 of the shape path turns back on itself at a cusp",
            ),
            (
                "a near-degenerate sliver",
                vec![
                    polygon(&[[0.0, 0.0], [10.0, 0.0], [10.0, 1e-12]]),
                    square(20.0, 30.0, false),
                ],
                "contour 1 of the shape path",
            ),
            (
                "an open contour closed across another",
                vec![open_triangle(0.0), square(20.0, 30.0, false)],
                "contours 1 and 2 of the shape path are not certified 2 px apart",
            ),
        ] {
            let path = ShapePath {
                commands: contours_of.concat(),
            };
            let error = contours(&path, NON_ZERO, 2.0, 0.0).unwrap_err();
            assert!(error.contains(reason), "{case}: {error}");
        }
        // Without strokes, only contours that cross or touch are refused.
        for (case, contours_of) in [
            (
                "crossing contours",
                vec![square(0.0, 10.0, false), square(5.0, 15.0, false)],
            ),
            (
                "a touching hole",
                vec![square(0.0, 100.0, false), square(0.0, 50.0, false)],
            ),
        ] {
            let path = ShapePath {
                commands: contours_of.concat(),
            };
            assert_eq!(
                contours(&path, NON_ZERO, 0.0, 2.0),
                Err("contours 1 and 2 of the shape path are not certified apart".to_owned()),
                "{case}"
            );
        }
        // A contour that goes on after its close has no Premiere piece.
        let mut after_close = square(0.0, 10.0, false);
        after_close.push(line_to(5.0, 20.0));
        assert_eq!(
            contours(
                &ShapePath {
                    commands: after_close
                },
                NON_ZERO,
                2.0,
                0.0
            ),
            Err("a shape path contour goes on after its close".to_owned())
        );
    }

    #[test]
    fn contours_within_the_clearance_convert_and_are_near() {
        // Beyond the strokes' separation, contours within the clearance are
        // kept and reported; the first such pair is named.
        for (case, contours_of, separation, near) in [
            (
                "a hole 1 px inside its outline",
                vec![square(0.0, 100.0, false), square(1.0, 99.0, false)],
                0.0,
                Some([1, 2]),
            ),
            (
                "a hole 3 px inside its outline",
                vec![square(0.0, 100.0, false), square(3.0, 97.0, false)],
                0.0,
                None,
            ),
            (
                "stroked contours 1 px beyond their separation",
                vec![square(0.0, 100.0, false), square(5.0, 95.0, false)],
                4.0,
                Some([1, 2]),
            ),
            (
                "a curved band 0.25 px wide",
                vec![circle([0.0, 0.0], 200.0), circle([0.0, 0.0], 199.75)],
                0.0,
                Some([1, 2]),
            ),
            (
                "the second pair of three",
                vec![
                    square(0.0, 100.0, false),
                    square(10.0, 90.0, false),
                    square(11.0, 89.0, false),
                ],
                0.0,
                Some([2, 3]),
            ),
        ] {
            let path = ShapePath {
                commands: contours_of.concat(),
            };
            let result = contours(&path, EVEN_ODD, separation, 2.0).map(|found| found.near);
            assert_eq!(result, Ok(near), "{case}");
        }
    }
}
