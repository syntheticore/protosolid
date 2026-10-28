//! AP203/AP214/AP242 reader for BREP solids.
//!
//! Every solid instance of the file's representation graph, placed by its
//! assembly transformations, becomes a body of one compound. Faces are
//! converted to exact patches and sewn like the results of global modeling
//! operations. STEP files rarely carry usable p-curves, so each boundary is
//! lifted to its support by exact projection, guided by sampled parameters
//! that are kept continuous around the whole loop.
//!
//! Files written by other kernels are consistent only to their own
//! precision. The measured gaps between vertices, edge curves and supports
//! set a staging tolerance for sewing; the result then moves into a kernel
//! with the requested tolerance, its entities keeping the staging tolerance.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use cgmath::prelude::*;

use super::parser::{Model, Record, Value};
use super::{StepAssembly, StepOccurrence, StepProduct};
use crate::sew::{BrepPatch, BrepPatchLoop, BrepPatchSegment};
use crate::{
    invalid, unsupported, Compound, Cone3, Curve2, Curve3, Cylinder3, DomainBounds, FaceOrientation,
    Frame3, Kernel, KernelResult, Line3, LoopKind, Matrix3, ModelTolerance, NurbsCurve2,
    NurbsCurve3, NurbsSurface3, ParameterRange, Plane3, Point2, Point3, RigidTransform3,
    RevolvedSurface3, Sphere3, Surface3, SurfaceProjectionPcurve2, Torus3, Vec3, Circle3,
};

/// Parameter samples per boundary segment for the projection guides.
const GUIDE_SAMPLES: usize = 16;

pub(super) fn read(text: &str, tolerance: ModelTolerance) -> KernelResult<Compound> {
    let model = super::parser::parse(text)?;
    let reader = Reader::new(&model, tolerance);
    let solids = reader.solids()?;
    if solids.is_empty() {
        return Err(unsupported("STEP file contains no BREP solids"));
    }
    let plans = reader.plan(&solids)?;
    sew(&plans, reader.staging(), tolerance)
}

/// Every product's solids are sewn in its own coordinates, at the staging
/// tolerance of the whole file.
pub(super) fn read_assembly(text: &str, tolerance: ModelTolerance) -> KernelResult<StepAssembly> {
    let model = super::parser::parse(text)?;
    let reader = Reader::new(&model, tolerance);
    let Some(products) = reader.products()? else {
        let product = StepProduct {
            name: String::new(),
            bodies: Some(read(text, tolerance)?),
            colors: Vec::new(),
            occurrences: Vec::new(),
        };
        return Ok(StepAssembly { products: vec![product], roots: vec![0], ..Default::default() });
    };
    let colors = reader.colors();
    if products.iter().all(|product| product.found.solids.is_empty()) {
        return Err(unsupported("STEP file contains no BREP solids"));
    }
    let plans = products
        .iter()
        .map(|product| reader.plan(&product.found.solids))
        .collect::<KernelResult<Vec<_>>>()?;
    let staging = reader.staging();
    let mut assembly = StepAssembly::default();
    for (product, plans) in products.iter().zip(&plans) {
        let occurrences = product
            .found
            .occurrences
            .iter()
            .map(|(child, placement)| {
                Ok(StepOccurrence {
                    product: *child,
                    placement: RigidTransform3::new(placement.matrix, placement.translation, tolerance)?,
                })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let bodies = if plans.is_empty() { None } else { Some(sew(plans, staging, tolerance)?) };
        // Sewing keeps one body per found solid, in order.
        let colors = product.found.solids.iter().map(|(solid, _)| colors.get(solid).copied()).collect();
        assembly.products.push(StepProduct { name: product.name.clone(), bodies, colors, occurrences });
    }
    let placed = assembly
        .products
        .iter()
        .flat_map(|product| product.occurrences.iter().map(|occurrence| occurrence.product))
        .collect::<HashSet<_>>();
    // Definitions without solids or occurrences describe nothing to place.
    assembly.roots = (0..assembly.products.len())
        .filter(|index| !placed.contains(index))
        .filter(|index| assembly.products[*index].bodies.is_some() || !assembly.products[*index].occurrences.is_empty())
        .collect();
    Ok(assembly)
}

fn sew(plans: &[Vec<Vec<FacePlan>>], staging: ModelTolerance, tolerance: ModelTolerance) -> KernelResult<Compound> {
    let mut kernel = Kernel::new(staging)?;
    let mut transaction = kernel.begin_operation();
    for shells in plans {
        let mut bodies = Vec::new();
        for faces in shells {
            let patches = faces
                .iter()
                .map(|face| Reader::patch(face, staging))
                .collect::<KernelResult<Vec<_>>>()?;
            bodies.push(transaction.brep_shell(&patches)?);
        }
        transaction.nest_brep_shells(&bodies)?;
    }
    let committed = transaction.finish()?;
    let compound = Compound::from_operation(kernel, committed, [])?;
    if staging == tolerance {
        Ok(compound)
    } else {
        compound.with_tolerance(tolerance)
    }
}

/// A rigid placement of one representation in world coordinates.
#[derive(Debug, Clone, Copy)]
struct Placement {
    matrix: Matrix3,
    translation: Vec3,
}

impl Placement {
    fn identity() -> Self {
        Self { matrix: Matrix3::identity(), translation: Vec3::zero() }
    }

    fn point(&self, point: Point3) -> Point3 {
        Point3::from_vec(self.matrix * point.to_vec() + self.translation)
    }

    fn vector(&self, vector: Vec3) -> Vec3 {
        self.matrix * vector
    }

    /// This placement followed by `outer`.
    fn then(&self, outer: &Self) -> Self {
        Self {
            matrix: outer.matrix * self.matrix,
            translation: outer.matrix * self.translation + outer.translation,
        }
    }

    fn inverse(&self) -> Self {
        let matrix = self.matrix.transpose();
        Self { matrix, translation: -(matrix * self.translation) }
    }
}

/// One oriented boundary use before it is lifted to its face.
#[derive(Clone)]
struct Segment {
    curve: Curve3,
    range: ParameterRange,
}

/// A face whose boundary uses are lifted to continuous charts of its support.
struct FacePlan {
    surface: Surface3,
    orientation: FaceOrientation,
    loops: Vec<(LoopKind, Vec<(Segment, Vec<Point2>)>)>,
}

type EdgeCache = HashMap<usize, (Curve3, ParameterRange)>;

struct Reader<'a> {
    model: &'a Model,
    tolerance: ModelTolerance,
    /// Millimetres per file length unit.
    length: f64,
    /// Radians per file plane-angle unit.
    angle: f64,
    /// The largest distance between a vertex and its edge curves, or between
    /// a boundary curve and its face support.
    gap: Cell<f64>,
}

fn reference(value: &Value) -> KernelResult<usize> {
    match value {
        Value::Reference(id) => Ok(*id),
        _ => Err(invalid("STEP attribute is not an entity reference")),
    }
}

fn number(value: &Value) -> KernelResult<f64> {
    match value {
        Value::Number(number) => Ok(*number),
        Value::Typed(values) if values.len() == 1 => number(&values[0]),
        _ => Err(invalid("STEP attribute is not a number")),
    }
}

fn boolean(value: &Value) -> KernelResult<bool> {
    match value {
        Value::Enumeration(name) if name == "T" => Ok(true),
        Value::Enumeration(name) if name == "F" => Ok(false),
        _ => Err(invalid("STEP attribute is not a boolean")),
    }
}

fn list(value: &Value) -> KernelResult<&[Value]> {
    match value {
        Value::List(values) => Ok(values),
        _ => Err(invalid("STEP attribute is not a list")),
    }
}

fn argument(record: &Record, index: usize) -> KernelResult<&Value> {
    record
        .values
        .get(index)
        .ok_or_else(|| invalid(format!("STEP {} has too few attributes", record.name)))
}

fn numbers(value: &Value) -> KernelResult<Vec<f64>> {
    list(value)?.iter().map(number).collect()
}

/// Expand distinct knots and multiplicities into a full knot vector.
fn expand_knots(multiplicities: &Value, knots: &Value) -> KernelResult<Vec<f64>> {
    let multiplicities = numbers(multiplicities)?;
    let knots = numbers(knots)?;
    if multiplicities.len() != knots.len() {
        return Err(invalid("STEP knot multiplicities do not match its knots"));
    }
    Ok(knots
        .iter()
        .zip(multiplicities)
        .flat_map(|(knot, count)| std::iter::repeat_n(*knot, count as usize))
        .collect())
}

/// Knots implied by the B-spline subtypes without explicit knots.
fn implicit_knots(kind: &str, degree: usize, count: usize) -> KernelResult<Vec<f64>> {
    Ok(match kind {
        "UNIFORM" => (0..count + degree + 1).map(|index| index as f64 - degree as f64).collect(),
        "QUASI_UNIFORM" => {
            let spans = count - degree;
            let mut knots = vec![0.0; degree + 1];
            knots.extend((1..spans).map(|index| index as f64));
            knots.extend(vec![spans as f64; degree + 1]);
            knots
        }
        "BEZIER" => {
            let segments = (count - 1) / degree;
            let mut knots = vec![0.0; degree + 1];
            for index in 1..segments {
                knots.extend(vec![index as f64; degree]);
            }
            knots.extend(vec![segments as f64; degree + 1]);
            knots
        }
        _ => return Err(unsupported(format!("STEP B-spline form {kind} is not supported"))),
    })
}

/// The B-spline attributes after the name, their knot attributes, the
/// implicit knot form when there are none, and rational weights.
struct BsplineParts<'a> {
    base: &'a [Value],
    knots: Option<&'a [Value]>,
    form: &'a str,
    weights: Option<&'a Value>,
}

impl<'a> Reader<'a> {
    fn new(model: &'a Model, tolerance: ModelTolerance) -> Self {
        let mut reader = Self { model, tolerance, length: 1.0, angle: 1.0, gap: Cell::new(0.0) };
        let context = model
            .instances
            .iter()
            .filter_map(|(id, records)| {
                Some((id, records.iter().find(|record| record.name == "GLOBAL_UNIT_ASSIGNED_CONTEXT")?))
            })
            .min_by_key(|(id, _)| **id)
            .map(|(_, record)| record);
        if let Some(units) = context.and_then(|context| context.values.first()).and_then(|value| list(value).ok()) {
            for unit in units.iter().filter_map(|unit| reference(unit).ok()) {
                let factor = reader.unit_factor(unit);
                if reader.record(unit, "LENGTH_UNIT").is_some() {
                    reader.length = factor.unwrap_or(1.0);
                }
                if reader.record(unit, "PLANE_ANGLE_UNIT").is_some() {
                    reader.angle = factor.unwrap_or(1.0);
                }
            }
        }
        reader
    }

    fn records(&self, id: usize) -> KernelResult<&'a [Record]> {
        self.model
            .instances
            .get(&id)
            .map(Vec::as_slice)
            .ok_or_else(|| invalid(format!("STEP instance #{id} is missing")))
    }

    fn record(&self, id: usize, name: &str) -> Option<&'a Record> {
        self.records(id).ok()?.iter().find(|record| record.name == name)
    }

    fn simple(&self, id: usize) -> KernelResult<&'a Record> {
        Ok(&self.records(id)?[0])
    }

    /// Millimetres or radians per unit `id`.
    fn unit_factor(&self, id: usize) -> Option<f64> {
        if let Some(si) = self.record(id, "SI_UNIT") {
            let prefix = match si.values.first() {
                Some(Value::Enumeration(prefix)) => match prefix.as_str() {
                    "KILO" => 1.0e3,
                    "HECTO" => 1.0e2,
                    "DECA" => 1.0e1,
                    "DECI" => 1.0e-1,
                    "CENTI" => 1.0e-2,
                    "MILLI" => 1.0e-3,
                    "MICRO" => 1.0e-6,
                    "NANO" => 1.0e-9,
                    _ => 1.0,
                },
                _ => 1.0,
            };
            let base = match si.values.get(1) {
                Some(Value::Enumeration(name)) if name == "METRE" => 1.0e3,
                _ => 1.0,
            };
            return Some(prefix * base);
        }
        let conversion = self.record(id, "CONVERSION_BASED_UNIT")?;
        let measure = reference(conversion.values.get(1)?).ok()?;
        let measure = self.records(measure).ok()?.iter().find(|record| record.values.len() >= 2)?;
        Some(number(&measure.values[0]).ok()? * self.unit_factor(reference(&measure.values[1]).ok()?)?)
    }

    fn length(&self, value: &Value) -> KernelResult<f64> {
        Ok(number(value)? * self.length)
    }

    fn cartesian_point(&self, id: usize) -> KernelResult<Point3> {
        let record = self.simple(id)?;
        let coordinates = numbers(argument(record, 1)?)?;
        let coordinate = |index: usize| coordinates.get(index).copied().unwrap_or(0.0) * self.length;
        Ok(Point3::new(coordinate(0), coordinate(1), coordinate(2)))
    }

    fn direction(&self, id: usize) -> KernelResult<Vec3> {
        let ratios = numbers(argument(self.simple(id)?, 1)?)?;
        let ratio = |index: usize| ratios.get(index).copied().unwrap_or(0.0);
        let direction = Vec3::new(ratio(0), ratio(1), ratio(2));
        if direction.magnitude2() == 0.0 {
            return Err(invalid("STEP direction has zero length"));
        }
        Ok(direction.normalize())
    }

    /// An `axis2_placement_3d` as a local placement.
    fn axis_placement(&self, id: usize) -> KernelResult<Placement> {
        let record = self.simple(id)?;
        let origin = self.cartesian_point(reference(argument(record, 1)?)?)?;
        let z = match record.values.get(2) {
            Some(Value::Reference(axis)) => self.direction(*axis)?,
            _ => Vec3::unit_z(),
        };
        let reference_x = match record.values.get(3) {
            Some(Value::Reference(axis)) => self.direction(*axis)?,
            _ => Vec3::unit_x(),
        };
        let mut x = reference_x - z * reference_x.dot(z);
        if x.magnitude2() <= 1.0e-24 {
            x = Frame3::from_z(origin, z)?.x();
        }
        let x = x.normalize();
        Ok(Placement { matrix: Matrix3::from_cols(x, z.cross(x), z), translation: origin.to_vec() })
    }

    /// The world origin and right-handed axes of an `axis2_placement_3d`.
    fn frame(&self, id: usize, placement: &Placement) -> KernelResult<(Point3, Vec3, Vec3, Vec3)> {
        let local = self.axis_placement(id)?.then(placement);
        Ok((Point3::from_vec(local.translation), local.matrix.x, local.matrix.y, local.matrix.z))
    }

    fn bspline_parts(&self, id: usize) -> KernelResult<BsplineParts<'a>> {
        let records = self.records(id)?;
        let first = &records[0];
        let rational = |name: &str| self.record(id, name).and_then(|record| record.values.first());
        if records.len() == 1 {
            let curve = first.name.contains("CURVE");
            let base_count = if curve { 5 } else { 7 };
            let base = first.values.get(1..).unwrap_or(&[]);
            let form = first.name.trim_end_matches("_CURVE").trim_end_matches("_SURFACE");
            return Ok(match first.name.as_str() {
                "B_SPLINE_CURVE_WITH_KNOTS" | "B_SPLINE_SURFACE_WITH_KNOTS" if base.len() > base_count => {
                    BsplineParts { base: &base[..base_count], knots: Some(&base[base_count..]), form: "", weights: None }
                }
                _ => BsplineParts { base, knots: None, form, weights: None },
            });
        }
        let base = records
            .iter()
            .find(|record| record.name == "B_SPLINE_CURVE" || record.name == "B_SPLINE_SURFACE")
            .ok_or_else(|| invalid("STEP complex instance is not a B-spline"))?;
        let knots = records
            .iter()
            .find(|record| record.name.ends_with("_WITH_KNOTS"))
            .map(|record| record.values.as_slice());
        let form = ["UNIFORM", "QUASI_UNIFORM", "BEZIER"]
            .into_iter()
            .find(|form| records.iter().any(|record| record.name.starts_with(&format!("{form}_"))))
            .unwrap_or("");
        Ok(BsplineParts {
            base: &base.values,
            knots,
            form,
            weights: rational("RATIONAL_B_SPLINE_CURVE").or_else(|| rational("RATIONAL_B_SPLINE_SURFACE")),
        })
    }

    fn bspline_curve(&self, id: usize, placement: &Placement) -> KernelResult<NurbsCurve3> {
        let parts = self.bspline_parts(id)?;
        let degree = number(&parts.base[0])? as usize;
        let points = list(&parts.base[1])?
            .iter()
            .map(|point| Ok(placement.point(self.cartesian_point(reference(point)?)?)))
            .collect::<KernelResult<Vec<_>>>()?;
        let knots = match parts.knots {
            Some(knots) => expand_knots(&knots[0], &knots[1])?,
            None => implicit_knots(parts.form, degree, points.len())?,
        };
        let weights = match parts.weights {
            Some(weights) => numbers(weights)?,
            None => vec![1.0; points.len()],
        };
        NurbsCurve3::new(degree, points, weights, knots)
    }

    fn bspline_surface(&self, id: usize, placement: &Placement) -> KernelResult<NurbsSurface3> {
        let parts = self.bspline_parts(id)?;
        let degrees = (number(&parts.base[0])? as usize, number(&parts.base[1])? as usize);
        // STEP lists controls by u, then v; the kernel stores rows along v.
        let columns = list(&parts.base[2])?
            .iter()
            .map(|column| {
                list(column)?
                    .iter()
                    .map(|point| Ok(placement.point(self.cartesian_point(reference(point)?)?)))
                    .collect::<KernelResult<Vec<_>>>()
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let (count_u, count_v) = (columns.len(), columns.first().map_or(0, Vec::len));
        let controls = (0..count_v).map(|v| (0..count_u).map(|u| columns[u][v]).collect()).collect();
        let weights = match parts.weights {
            Some(weights) => {
                let columns = list(weights)?.iter().map(numbers).collect::<KernelResult<Vec<_>>>()?;
                (0..count_v).map(|v| (0..count_u).map(|u| columns[u][v]).collect()).collect()
            }
            None => vec![vec![1.0; count_u]; count_v],
        };
        let knots = match parts.knots {
            Some(knots) => (expand_knots(&knots[0], &knots[2])?, expand_knots(&knots[1], &knots[3])?),
            None => (
                implicit_knots(parts.form, degrees.0, count_u)?,
                implicit_knots(parts.form, degrees.1, count_v)?,
            ),
        };
        NurbsSurface3::new(degrees, controls, weights, knots)
    }

    /// An exact rational ellipse from conjugate semi-diameters, starting at
    /// `center + first`.
    fn ellipse(center: Point3, first: Vec3, second: Vec3) -> KernelResult<NurbsCurve3> {
        let corner = std::f64::consts::FRAC_1_SQRT_2;
        let offsets = [
            (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0), (-1.0, 0.0),
            (-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (1.0, 0.0),
        ];
        NurbsCurve3::new(
            2,
            offsets.iter().map(|(a, b)| center + first * *a + second * *b).collect(),
            (0..9).map(|index| if index % 2 == 1 { corner } else { 1.0 }).collect(),
            vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0],
        )
    }

    /// The 3D curve of an edge. Closed ellipses start at `start`.
    fn curve(&self, id: usize, placement: &Placement, start: Option<Point3>) -> KernelResult<Curve3> {
        let record = self.simple(id)?;
        Ok(match record.name.as_str() {
            "LINE" => {
                let origin = placement.point(self.cartesian_point(reference(argument(record, 1)?)?)?);
                let vector = self.simple(reference(argument(record, 2)?)?)?;
                let direction = self.direction(reference(argument(vector, 1)?)?)?;
                let magnitude = self.length(argument(vector, 2)?)?;
                Line3::new(origin, placement.vector(direction) * magnitude)?.into()
            }
            "CIRCLE" => {
                let (center, x, y, _) = self.frame(reference(argument(record, 1)?)?, placement)?;
                let radius = self.length(argument(record, 2)?)?;
                Circle3::new(center, x * radius, y * radius, self.tolerance)?.into()
            }
            "ELLIPSE" => {
                let (center, x, y, _) = self.frame(reference(argument(record, 1)?)?, placement)?;
                let (a, b) = (self.length(argument(record, 2)?)?, self.length(argument(record, 3)?)?);
                let phase = start.map_or(0.0, |start| {
                    let offset = start - center;
                    (offset.dot(y) / b).atan2(offset.dot(x) / a)
                });
                let (sin, cos) = phase.sin_cos();
                Self::ellipse(center, x * (a * cos) + y * (b * sin), -x * (a * sin) + y * (b * cos))?.into()
            }
            "SURFACE_CURVE" | "SEAM_CURVE" | "BOUNDED_SURFACE_CURVE" | "INTERSECTION_CURVE" | "TRIMMED_CURVE" => {
                self.curve(reference(argument(record, 1)?)?, placement, start)?
            }
            "POLYLINE" => {
                let points = list(argument(record, 1)?)?
                    .iter()
                    .map(|point| Ok(placement.point(self.cartesian_point(reference(point)?)?)))
                    .collect::<KernelResult<Vec<_>>>()?;
                let mut knots = vec![0.0];
                knots.extend((0..points.len()).map(|index| index as f64));
                knots.push((points.len() - 1) as f64);
                NurbsCurve3::new(1, points.clone(), vec![1.0; points.len()], knots)?.into()
            }
            "BEZIER_CURVE" | "UNIFORM_CURVE" | "QUASI_UNIFORM_CURVE" => self.bspline_curve(id, placement)?.into(),
            _ if self.records(id)?.iter().any(|record| record.name.starts_with("B_SPLINE_CURVE")) =>
            {
                self.bspline_curve(id, placement)?.into()
            }
            other => return Err(unsupported(format!("STEP curve type {other} is not supported"))),
        })
    }

    fn vertex(&self, id: usize, placement: &Placement) -> KernelResult<Point3> {
        let record = self.simple(id)?;
        Ok(placement.point(self.cartesian_point(reference(argument(record, 1)?)?)?))
    }

    /// The curve of an `edge_curve` and its parameter range from its start
    /// vertex to its end vertex. Both face uses share this exact pair.
    fn edge(&self, id: usize, placement: &Placement, cache: &mut EdgeCache) -> KernelResult<(Curve3, ParameterRange)> {
        if let Some(edge) = cache.get(&id) {
            return Ok(edge.clone());
        }
        let record = self.simple(id)?;
        if record.name != "EDGE_CURVE" {
            return Err(unsupported(format!("STEP edge type {} is not supported", record.name)));
        }
        let (start_id, end_id) = (reference(argument(record, 1)?)?, reference(argument(record, 2)?)?);
        let start = self.vertex(start_id, placement)?;
        let end = self.vertex(end_id, placement)?;
        let same_sense = boolean(argument(record, 4)?)?;
        let closed = start_id == end_id || start.distance(end) <= self.tolerance.linear * 8.0;
        let curve = self.curve(reference(argument(record, 3)?)?, placement, Some(start))?;
        let domain = curve.domain();
        let first = self.project_curve(&curve, start)?;
        let range = match (domain.period, domain.bounds) {
            (Some(period), _) if closed => ParameterRange::new(first, first + if same_sense { period } else { -period })?,
            (Some(period), _) => {
                let delta = (self.project_curve(&curve, end)? - first).rem_euclid(period);
                ParameterRange::new(first, first + if same_sense { delta } else { delta - period })?
            }
            (None, DomainBounds::Bounded(bounds)) if closed => {
                let ends = [bounds.start, bounds.end]
                    .iter()
                    .map(|parameter| Ok(curve.evaluate(*parameter)?.point.distance(start)))
                    .collect::<KernelResult<Vec<_>>>()?;
                self.measure(ends[0].max(ends[1]) * 2.0);
                if same_sense { bounds } else { bounds.reversed() }
            }
            (None, _) if closed => return Err(invalid("STEP closed edge lies on an unbounded open curve")),
            (None, _) => ParameterRange::new(first, self.project_curve(&curve, end)?)?,
        };
        cache.insert(id, (curve.clone(), range));
        Ok((curve, range))
    }

    /// Plan the faces of every shell of each placed solid, measuring gaps.
    fn plan(&self, solids: &[(usize, Placement)]) -> KernelResult<Vec<Vec<Vec<FacePlan>>>> {
        let mut plans = Vec::with_capacity(solids.len());
        for (solid, placement) in solids {
            let mut edges = HashMap::new();
            let shells = self
                .shells(*solid)?
                .into_iter()
                .map(|(shell, reversed)| self.shell(shell, reversed, placement, &mut edges))
                .collect::<KernelResult<Vec<_>>>()?;
            plans.push(shells);
        }
        Ok(plans)
    }

    /// Sewing tolerance for the gaps measured so far.
    fn staging(&self) -> ModelTolerance {
        ModelTolerance { linear: self.tolerance.linear.max(self.gap.get() * 4.0), ..self.tolerance }
    }

    fn measure(&self, gap: f64) {
        self.gap.set(self.gap.get().max(gap));
    }

    /// The parameter of a vertex on its edge curve. Edges meeting at the
    /// vertex can each miss it, so twice the distance must weld.
    fn project_curve(&self, curve: &Curve3, point: Point3) -> KernelResult<f64> {
        let projection = curve.project(point, self.tolerance)?;
        self.measure(projection.distance * 2.0);
        Ok(projection.parameter)
    }

    /// The `edge_curve` of an oriented edge and whether the use follows it.
    fn oriented_edge(&self, id: usize) -> KernelResult<(usize, bool)> {
        let record = self.simple(id)?;
        if record.name != "ORIENTED_EDGE" {
            return Ok((id, true));
        }
        let (edge, forward) = self.oriented_edge(reference(argument(record, 3)?)?)?;
        Ok((edge, forward == boolean(argument(record, 4)?)?))
    }

    /// The support of a face and whether the kernel's natural normal opposes
    /// the STEP support's natural normal. `extent` samples the face boundary.
    fn surface(&self, id: usize, placement: &Placement, extent: &[Point3]) -> KernelResult<(Surface3, bool)> {
        let record = self.simple(id)?;
        let tolerance = self.tolerance;
        let frame = || self.frame(reference(argument(record, 1)?)?, placement);
        Ok(match record.name.as_str() {
            "PLANE" => {
                let (origin, x, y, _) = frame()?;
                (Plane3::new(origin, x, y, tolerance)?.into(), false)
            }
            "CYLINDRICAL_SURFACE" => {
                let (origin, x, y, z) = frame()?;
                let radius = self.length(argument(record, 2)?)?;
                (Cylinder3::new(origin, z, x * radius, y * radius, tolerance)?.into(), false)
            }
            "CONICAL_SURFACE" => {
                let (origin, x, y, z) = frame()?;
                let radius = self.length(argument(record, 2)?)?;
                let slope = (number(argument(record, 3)?)? * self.angle).tan();
                let cone = if radius > tolerance.linear {
                    let height = radius / slope;
                    Cone3::new(origin - z * height, z * height, x * radius, y * radius, tolerance)?
                } else {
                    Cone3::new(origin, z, x * slope, y * slope, tolerance)?
                };
                (cone.into(), false)
            }
            "SPHERICAL_SURFACE" => {
                let (center, x, y, z) = frame()?;
                let radius = self.length(argument(record, 2)?)?;
                (Sphere3::new(center, x * radius, y * radius, z * radius, tolerance)?.into(), false)
            }
            "TOROIDAL_SURFACE" => {
                let (center, x, y, _) = frame()?;
                let major = self.length(argument(record, 2)?)?;
                let minor = self.length(argument(record, 3)?)?;
                (Torus3::new(center, x * major, y * major, minor, tolerance)?.into(), false)
            }
            "RECTANGULAR_TRIMMED_SURFACE" => self.surface(reference(argument(record, 1)?)?, placement, extent)?,
            "OFFSET_SURFACE" => {
                let (base, flipped) = self.surface(reference(argument(record, 1)?)?, placement, extent)?;
                let distance = self.length(argument(record, 2)?)? * if flipped { -1.0 } else { 1.0 };
                let surface = base.offset(distance, tolerance).or_else(|_| {
                    crate::OffsetSurface3::new(base, distance, tolerance).map(Surface3::Offset)
                })?;
                (surface, flipped)
            }
            "SURFACE_OF_REVOLUTION" => {
                let curve = self.curve(reference(argument(record, 1)?)?, placement, None)?;
                let axis = self.simple(reference(argument(record, 2)?)?)?;
                let origin = placement.point(self.cartesian_point(reference(argument(axis, 1)?)?)?);
                let direction = match axis.values.get(2) {
                    Some(Value::Reference(direction)) => placement.vector(self.direction(*direction)?),
                    _ => placement.vector(Vec3::unit_z()),
                };
                let surface = self.revolution(&curve, origin, direction, extent)?;
                let flipped = self.revolution_flipped(&surface, &curve, origin, direction, extent)?;
                (surface, flipped)
            }
            "SURFACE_OF_LINEAR_EXTRUSION" => {
                let curve = self.curve(reference(argument(record, 1)?)?, placement, None)?;
                let vector = self.simple(reference(argument(record, 2)?)?)?;
                let direction = placement.vector(self.direction(reference(argument(vector, 1)?)?)?)
                    * self.length(argument(vector, 2)?)?;
                (self.extrusion(&curve, direction, extent)?, false)
            }
            "BEZIER_SURFACE" | "UNIFORM_SURFACE" | "QUASI_UNIFORM_SURFACE" => (self.bspline_surface(id, placement)?.into(), false),
            _ if self.records(id)?.iter().any(|record| record.name.starts_with("B_SPLINE_SURFACE")) =>
            {
                (self.bspline_surface(id, placement)?.into(), false)
            }
            other => return Err(unsupported(format!("STEP surface type {other} is not supported"))),
        })
    }

    /// A support of revolution in its canonical kernel form where one exists.
    fn revolution(&self, curve: &Curve3, origin: Point3, axis: Vec3, extent: &[Point3]) -> KernelResult<Surface3> {
        let tolerance = self.tolerance;
        let axis = axis.normalize();
        let foot = |point: Point3| origin + axis * (point - origin).dot(axis);
        let parallel = |vector: Vec3| vector.normalize().cross(axis).magnitude() <= tolerance.angular * 16.0;
        let linear = |point: Point3| tolerance.linear_at_scale(point.to_vec().magnitude());
        match curve {
            Curve3::Line(line) if parallel(line.direction()) => {
                let radial = line.origin() - foot(line.origin());
                let x = radial.normalize();
                let radius = radial.magnitude();
                return Ok(Cylinder3::new(foot(line.origin()), axis, x * radius, axis.cross(x) * radius, tolerance)?.into());
            }
            Curve3::Line(line) => {
                // A generator meeting the axis sweeps a cone, or a plane when
                // it is perpendicular.
                let direction = line.direction().normalize();
                let normal = direction.cross(axis);
                if (line.origin() - origin).dot(normal.normalize()).abs() > linear(line.origin()) {
                    return Err(unsupported("STEP hyperboloid of revolution is not supported"));
                }
                if direction.dot(axis).abs() <= tolerance.angular * 16.0 {
                    let x = direction;
                    return Ok(Plane3::new(foot(line.origin()), x, axis.cross(x), tolerance)?.into());
                }
                let along = (foot(line.origin()) - line.origin()).dot(direction)
                    / (1.0 - direction.dot(axis).powi(2));
                let apex = line.origin() + direction * along;
                let far = extent
                    .iter()
                    .copied()
                    .max_by(|a, b| a.distance(apex).total_cmp(&b.distance(apex)))
                    .ok_or_else(|| invalid("surface of revolution has no extent"))?;
                let point = apex + direction * (far - apex).dot(direction);
                let height = (point - apex).dot(axis);
                let radial = point - (apex + axis * height);
                let x = radial.normalize();
                let radius = radial.magnitude();
                return Ok(Cone3::new(apex, axis * height, x * radius, axis.cross(x) * radius, tolerance)?.into());
            }
            Curve3::Circle(circle) if circle.normal().dot(axis).abs() <= tolerance.angular * 16.0 => {
                let center = foot(circle.center());
                let offset = circle.center() - center;
                if offset.magnitude() <= linear(circle.center()) {
                    let x = Frame3::from_z(center, axis)?.x();
                    let radius = circle.radius();
                    return Ok(Sphere3::new(center, x * radius, axis.cross(x) * radius, axis * radius, tolerance)?.into());
                }
                let x = offset.normalize();
                return Ok(Torus3::new(center, x * offset.magnitude(), axis.cross(x) * offset.magnitude(),
                    circle.radius(), tolerance)?.into());
            }
            _ => {}
        }
        let nurbs = self.nurbs_curve(curve, extent)?;
        let sample = curve.evaluate(match curve.domain().bounds {
            DomainBounds::Bounded(range) => (range.start + range.end) * 0.5,
            _ => 0.0,
        })?.point;
        let radial = (sample - origin) - axis * (sample - origin).dot(axis);
        let frame = Frame3::new(origin, radial, axis.cross(radial), tolerance)?;
        let profile = nurbs
            .control_points()
            .iter()
            .map(|point| {
                let offset = *point - origin;
                if offset.dot(frame.y()).abs() > linear(*point) {
                    return Err(unsupported("STEP surface of revolution has a non-planar meridian"));
                }
                Ok(Point2::new(offset.dot(frame.x()), offset.dot(frame.z())))
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let profile = NurbsCurve2::new(nurbs.degree(), profile, nurbs.weights().to_vec(), nurbs.knots().to_vec())?;
        let DomainBounds::Bounded(range) = profile.domain().bounds else {
            unreachable!()
        };
        Ok(RevolvedSurface3::new(frame, profile.into(), range, tolerance)?.into())
    }

    /// Whether the kernel support's natural normal opposes that of the STEP
    /// revolution, whose first parameter turns about `axis`.
    fn revolution_flipped(
        &self,
        surface: &Surface3,
        curve: &Curve3,
        origin: Point3,
        axis: Vec3,
        extent: &[Point3],
    ) -> KernelResult<bool> {
        let axis = axis.normalize();
        let radial = |point: Point3| (point - origin) - axis * (point - origin).dot(axis);
        let point = extent
            .iter()
            .copied()
            .max_by(|a, b| radial(*a).magnitude2().total_cmp(&radial(*b).magnitude2()))
            .ok_or_else(|| invalid("surface of revolution has no extent"))?;
        // Turn the point back onto the swept curve to find its meridian tangent.
        let reference = radial(curve.evaluate(match curve.domain().bounds {
            DomainBounds::Bounded(range) => (range.start + range.end) * 0.5,
            _ => 0.0,
        })?.point);
        let (x, y) = (reference.normalize(), axis.cross(reference.normalize()));
        let angle = radial(point).dot(y).atan2(radial(point).dot(x));
        let rotate = |vector: Vec3, angle: f64| {
            let (sin, cos) = angle.sin_cos();
            axis * vector.dot(axis) + (vector - axis * vector.dot(axis)) * cos + axis.cross(vector) * sin
        };
        let meridian = origin + rotate(point - origin, -angle);
        let parameter = curve.project(meridian, self.tolerance)?.parameter;
        let tangent = rotate(curve.evaluate(parameter)?.first_derivative, angle);
        let step_normal = axis.cross(point - origin).cross(tangent);
        let (u, v) = surface.project(point, self.tolerance)?.parameters;
        Ok(surface.normal(u, v)?.dot(step_normal) < 0.0)
    }

    /// A curve as a B-spline. Lines are bounded by the projection of `extent`.
    fn nurbs_curve(&self, curve: &Curve3, extent: &[Point3]) -> KernelResult<NurbsCurve3> {
        Ok(match curve {
            Curve3::Nurbs(curve) => curve.clone(),
            Curve3::Line(line) => {
                let parameters = extent
                    .iter()
                    .map(|point| Ok(curve.project(*point, self.tolerance)?.parameter))
                    .collect::<KernelResult<Vec<_>>>()?;
                let low = parameters.iter().copied().fold(f64::INFINITY, f64::min);
                let high = parameters.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let margin = (high - low).max(1.0) * 0.01;
                let (low, high) = (low - margin, high + margin);
                NurbsCurve3::new(
                    1,
                    vec![line.origin() + line.direction() * low, line.origin() + line.direction() * high],
                    vec![1.0; 2],
                    vec![low, low, high, high],
                )?
            }
            Curve3::Circle(circle) => Self::ellipse(circle.center(), circle.x_radius(), circle.y_radius())?,
            _ => return Err(unsupported("STEP swept curve type is not supported")),
        })
    }

    /// A support of linear extrusion: a plane, cylinder or ruled B-spline.
    /// The swept curve is the first parameter, as in STEP.
    fn extrusion(&self, curve: &Curve3, direction: Vec3, extent: &[Point3]) -> KernelResult<Surface3> {
        let tolerance = self.tolerance;
        match curve {
            Curve3::Line(line) => return Ok(Plane3::new(line.origin(), line.direction(), direction, tolerance)?.into()),
            Curve3::Circle(circle) if circle.normal().cross(direction.normalize()).magnitude() <= tolerance.angular * 16.0 => {
                return Ok(Cylinder3::new(circle.center(), direction, circle.x_radius(), circle.y_radius(), tolerance)?.into());
            }
            _ => {}
        }
        let nurbs = self.nurbs_curve(curve, extent)?;
        // Bound the ruling by every boundary point against the curve's hull.
        let unit = direction.normalize();
        let along = |point: &Point3| point.to_vec().dot(unit);
        let (point_low, point_high) = extent.iter().map(along).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        let (hull_low, hull_high) = nurbs.control_points().iter().map(along).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        let magnitude = direction.magnitude();
        let (low, high) = ((point_low - hull_high) / magnitude, (point_high - hull_low) / magnitude);
        let margin = (high - low).max(1.0) * 0.01;
        let (low, high) = (low - margin, high + margin);
        let rows = [low, high]
            .iter()
            .map(|v| nurbs.control_points().iter().map(|point| *point + direction * *v).collect())
            .collect();
        Ok(NurbsSurface3::new(
            (nurbs.degree(), 1),
            rows,
            vec![nurbs.weights().to_vec(); 2],
            (nurbs.knots().to_vec(), vec![low, low, high, high]),
        )?
        .into())
    }

    /// Projection samples of one boundary use. Each sample continues from its
    /// neighbor, so the chart keeps to one side of a closed support's seam.
    /// Without a `seed` for its start, the use's middle is projected globally.
    fn chart(&self, surface: &Surface3, segment: &Segment, seed: Option<Point2>) -> KernelResult<Vec<Point2>> {
        let range = segment.range;
        let at = |fraction: f64| range.start + range.span() * fraction / GUIDE_SAMPLES as f64;
        let points = (0..=GUIDE_SAMPLES)
            .map(|index| Ok(segment.curve.evaluate(at(index as f64))?.point))
            .collect::<KernelResult<Vec<_>>>()?;
        let domains = surface.domains();
        let project = |point: Point3, neighbor: Point2| -> KernelResult<Point2> {
            let near = surface.project_near(point, neighbor, self.tolerance)?;
            // Leave the neighbor's branch only where its refinement failed.
            // Across a closed support's seam both branches lie on the curve.
            let scale = point.to_vec().magnitude().max(1.0);
            if near.distance > scale * 1.0e-4 {
                let global = surface.project(point, self.tolerance)?;
                if global.distance < near.distance * 0.5 {
                    self.measure(global.distance);
                    return Ok(crate::nearest_uv(domains, Point2::new(global.parameters.0, global.parameters.1), neighbor));
                }
            }
            self.measure(near.distance);
            Ok(Point2::new(near.parameters.0, near.parameters.1))
        };
        let mut chart = vec![Point2::origin(); points.len()];
        let order = match seed {
            Some(seed) => {
                chart[0] = project(points[0], seed)?;
                (1..points.len()).map(|index| (index, index - 1)).collect::<Vec<_>>()
            }
            None => {
                let middle = GUIDE_SAMPLES / 2;
                let projection = surface.project(points[middle], self.tolerance)?;
                self.measure(projection.distance);
                chart[middle] = Point2::new(projection.parameters.0, projection.parameters.1);
                (middle + 1..points.len())
                    .map(|index| (index, index - 1))
                    .chain((0..middle).rev().map(|index| (index, index + 1)))
                    .collect()
            }
        };
        for (index, neighbor) in order {
            chart[index] = project(points[index], chart[neighbor])?;
        }
        // Interpolated curves meet their support at regular nodes, often at
        // these very samples, and deviate most near their ends. Probe between
        // samples at irrational offsets, refined from the traced chart.
        let ends = [1.0e-3, 3.0e-3, 1.0e-2, 3.0e-2].into_iter().flat_map(|end| [end, GUIDE_SAMPLES as f64 - end]);
        let probes = (0..GUIDE_SAMPLES)
            .flat_map(|index| (1..4).map(move |probe| index as f64 + (probe as f64 * 0.618_033_988_75).fract()))
            .chain(ends);
        for probe in probes {
            let between = segment.curve.evaluate(at(probe))?.point;
            // A seed at a pole cannot move; the global projection is unique there.
            let seed = chart[(probe.round() as usize).min(GUIDE_SAMPLES)];
            let mut distance = surface.project_near(between, seed, self.tolerance)?.distance;
            if distance > between.to_vec().magnitude().max(1.0) * 1.0e-4 {
                distance = distance.min(surface.project(between, self.tolerance)?.distance);
            }
            self.measure(distance);
        }
        // At a pole one coordinate is free; continue it from the regular
        // neighbors so the guide follows the meridian.
        let degenerate = chart
            .iter()
            .map(|uv| {
                let (_, du, dv) = surface.first_order(uv.x, uv.y)?;
                let scale = du.magnitude().max(dv.magnitude()).max(1.0);
                Ok([du.magnitude() <= scale * 1.0e-9, dv.magnitude() <= scale * 1.0e-9])
            })
            .collect::<KernelResult<Vec<_>>>()?;
        for axis in 0..2 {
            for index in 0..chart.len() {
                if !degenerate[index][axis] {
                    continue;
                }
                let neighbor = (0..chart.len())
                    .filter(|other| !degenerate[*other][axis])
                    .min_by_key(|other| other.abs_diff(index));
                if let Some(neighbor) = neighbor {
                    chart[index][axis] = chart[neighbor][axis];
                }
            }
        }
        Ok(chart)
    }

    /// Lift a loop's samples into one continuous chart, traced from the use
    /// whose middle lies deepest inside the support's bounded domain. The
    /// charts are in traversal order from that use, which is returned too.
    /// Where consecutive uses meet at a pole, the free coordinate's period is
    /// undetermined; those bridges are returned as `(use, axis, period)`.
    fn loop_charts(
        &self,
        surface: &Surface3,
        segments: &[Segment],
    ) -> KernelResult<(Vec<Vec<Point2>>, usize, Vec<(usize, usize, f64)>)> {
        let domains = surface.domains();
        let depth = |uv: (f64, f64)| {
            (0..2)
                .map(|axis| match (domains[axis].period, domains[axis].bounds) {
                    (None, DomainBounds::Bounded(range)) => {
                        let value = if axis == 0 { uv.0 } else { uv.1 };
                        let (low, high) = (range.start.min(range.end), range.start.max(range.end));
                        (value - low).min(high - value) / (high - low)
                    }
                    _ => f64::INFINITY,
                })
                .fold(f64::INFINITY, f64::min)
        };
        let mut depths = Vec::with_capacity(segments.len());
        for segment in segments {
            let middle = segment.curve.evaluate(segment.range.start + segment.range.span() * 0.5)?.point;
            depths.push(depth(surface.project(middle, self.tolerance)?.parameters));
        }
        let first = (0..segments.len()).max_by(|a, b| depths[*a].total_cmp(&depths[*b])).unwrap();
        let mut charts = Vec::<Vec<Point2>>::with_capacity(segments.len());
        let mut bridges = Vec::new();
        for step in 0..segments.len() {
            let seed = charts.last().map(|chart| *chart.last().unwrap());
            if let Some(previous) = seed {
                let (_, du, dv) = surface.first_order(previous.x, previous.y)?;
                for (axis, rate) in [du.magnitude(), dv.magnitude()].into_iter().enumerate() {
                    if let Some(period) = domains[axis].period {
                        if rate <= self.tolerance.linear {
                            bridges.push((step, axis, period));
                        }
                    }
                }
            }
            charts.push(self.chart(surface, &segments[(first + step) % segments.len()], seed)?);
        }
        Ok((charts, first, bridges))
    }

    /// Choose the chart copy after every pole bridge so the loop winds with
    /// `sign`, preferring the smallest enclosed parameter area.
    fn bridged_charts(&self, surface: &Surface3, segments: &[Segment], sign: f64) -> KernelResult<(Vec<Vec<Point2>>, f64)> {
        let (charts, first, bridges) = self.loop_charts(surface, segments)?;
        let bridges = &bridges[..bridges.len().min(4)];
        let mut best = None::<(bool, f64, Vec<Vec<Point2>>)>;
        for combination in 0..3usize.pow(bridges.len() as u32) {
            let mut candidate = charts.clone();
            let mut code = combination;
            for (index, axis, period) in bridges {
                let turns = (code % 3) as f64 - 1.0;
                code /= 3;
                for chart in &mut candidate[*index..] {
                    for point in chart {
                        point[*axis] += turns * period;
                    }
                }
            }
            let area = signed_area(&candidate);
            let better = best.as_ref().is_none_or(|(winds, best_area, _)| {
                (area * sign > 0.0, -area.abs()) > (*winds, -best_area.abs())
            });
            if better {
                best = Some((area * sign > 0.0, area, candidate));
            }
        }
        let (_, area, mut charts) = best.unwrap();
        charts.rotate_right(first);
        Ok((charts, area))
    }

    fn plan_loops(
        &self,
        surface: &Surface3,
        orientation: FaceOrientation,
        loops: Vec<(bool, Vec<Segment>)>,
    ) -> KernelResult<Vec<(LoopKind, Vec<(Segment, Vec<Point2>)>)>> {
        let sign = orientation.sign();
        let mut lifted = Vec::new();
        for (outer, segments) in &loops {
            let (charts, area) = self.bridged_charts(surface, segments, sign)?;
            lifted.push((*outer, charts, area));
        }
        let outer = lifted.iter().position(|(outer, ..)| *outer).unwrap_or_else(|| {
            let candidates = (0..lifted.len()).filter(|index| lifted[*index].2 * sign > 0.0).collect::<Vec<_>>();
            let pool = if candidates.is_empty() { (0..lifted.len()).collect() } else { candidates };
            pool.into_iter().max_by(|a, b| lifted[*a].2.abs().total_cmp(&lifted[*b].2.abs())).unwrap()
        });
        Ok(loops
            .into_iter()
            .zip(lifted)
            .enumerate()
            .map(|(index, ((_, segments), (_, charts, _)))| {
                let kind = if index == outer { LoopKind::Outer } else { LoopKind::Inner };
                (kind, segments.into_iter().zip(charts).collect())
            })
            .collect())
    }

    fn patch(face: &FacePlan, tolerance: ModelTolerance) -> KernelResult<BrepPatch> {
        let loops = face
            .loops
            .iter()
            .map(|(kind, segments)| {
                let segments = segments
                    .iter()
                    .map(|(segment, chart)| {
                        Ok(BrepPatchSegment {
                            pcurve: Self::pcurve(&face.surface, segment, chart, tolerance)?,
                            curve: segment.curve.clone(),
                            range: segment.range,
                            provenance: Default::default(),
                        })
                    })
                    .collect::<KernelResult<Vec<_>>>()?;
                Ok(BrepPatchLoop { kind: *kind, segments })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        Ok(BrepPatch {
            sources: Vec::new(),
            preserved: Vec::new(),
            surface: face.surface.clone(),
            orientation: face.orientation,
            loops,
        })
    }

    fn pcurve(surface: &Surface3, segment: &Segment, chart: &[Point2], tolerance: ModelTolerance) -> KernelResult<Curve2> {
        if let Surface3::Plane(plane) = surface {
            if let Some(pcurve) = plane.flatten_curve(&segment.curve, tolerance)? {
                return Ok(pcurve);
            }
        }
        let range = segment.range;
        let increasing = ParameterRange::new(range.start.min(range.end), range.start.max(range.end))?;
        let guide = crate::intersection::smooth_nurbs_guide(chart, range)?;
        Ok(SurfaceProjectionPcurve2::new(segment.curve.clone(), surface.clone(), guide.into(), increasing, tolerance)?.into())
    }

    fn face(&self, id: usize, reversed: bool, placement: &Placement, edges: &mut EdgeCache) -> KernelResult<FacePlan> {
        let record = self.simple(id)?;
        if record.name == "ORIENTED_FACE" {
            let forward = boolean(argument(record, 3)?)?;
            return self.face(reference(argument(record, 2)?)?, reversed == forward, placement, edges);
        }
        if record.name != "ADVANCED_FACE" && record.name != "FACE_SURFACE" {
            return Err(unsupported(format!("STEP face type {} is not supported", record.name)));
        }
        let mut loops = Vec::new();
        for bound in list(argument(record, 1)?)? {
            let bound = self.simple(reference(bound)?)?;
            let edge_loop = self.simple(reference(argument(bound, 1)?)?)?;
            match edge_loop.name.as_str() {
                "EDGE_LOOP" => {}
                "VERTEX_LOOP" => continue,
                other => return Err(unsupported(format!("STEP loop type {other} is not supported"))),
            }
            let mut segments = list(argument(edge_loop, 1)?)?
                .iter()
                .map(|oriented| {
                    let (edge, forward) = self.oriented_edge(reference(oriented)?)?;
                    let (curve, range) = self.edge(edge, placement, edges)?;
                    Ok(Segment { curve, range: if forward { range } else { range.reversed() } })
                })
                .collect::<KernelResult<Vec<_>>>()?;
            if boolean(argument(bound, 2)?)? == reversed {
                segments.reverse();
                for segment in &mut segments {
                    segment.range = segment.range.reversed();
                }
            }
            loops.push((bound.name == "FACE_OUTER_BOUND", segments));
        }
        if loops.is_empty() {
            return Err(unsupported("STEP face has no edge loop"));
        }
        let mut extent = Vec::new();
        for (_, segments) in &loops {
            for segment in segments {
                for fraction in [0.0, 0.5] {
                    extent.push(segment.curve.evaluate(segment.range.start + segment.range.span() * fraction)?.point);
                }
            }
        }
        let (surface, flipped) = self.surface(reference(argument(record, 2)?)?, placement, &extent)?;
        let same_sense = boolean(argument(record, 3)?)? != reversed;
        let orientation = if same_sense != flipped { FaceOrientation::Forward } else { FaceOrientation::Reversed };
        let loops = self.plan_loops(&surface, orientation, loops)?;
        Ok(FacePlan { surface, orientation, loops })
    }

    fn shell(&self, id: usize, reversed: bool, placement: &Placement, edges: &mut EdgeCache) -> KernelResult<Vec<FacePlan>> {
        let record = self.simple(id)?;
        list(argument(record, 1)?)?
            .iter()
            .map(|face| self.face(reference(face)?, reversed, placement, edges))
            .collect()
    }

    /// The shells of one solid and whether each is used reversed.
    fn shells(&self, id: usize) -> KernelResult<Vec<(usize, bool)>> {
        let record = self.simple(id)?;
        Ok(match record.name.as_str() {
            "MANIFOLD_SOLID_BREP" => vec![self.oriented_shell(reference(argument(record, 1)?)?)?],
            "BREP_WITH_VOIDS" => {
                let mut shells = vec![self.oriented_shell(reference(argument(record, 1)?)?)?];
                for void in list(argument(record, 2)?)? {
                    shells.push(self.oriented_shell(reference(void)?)?);
                }
                shells
            }
            _ => vec![self.oriented_shell(id)?],
        })
    }

    fn oriented_shell(&self, id: usize) -> KernelResult<(usize, bool)> {
        let record = self.simple(id)?;
        if record.name == "ORIENTED_CLOSED_SHELL" {
            let (shell, reversed) = self.oriented_shell(reference(argument(record, 2)?)?)?;
            return Ok((shell, reversed == boolean(argument(record, 3)?)?));
        }
        Ok((id, false))
    }

    /// The items and the representation record of a representation.
    fn representation(&self, id: usize) -> Option<&'a [Value]> {
        self.records(id).ok()?.iter().find_map(|record| {
            (record.name.ends_with("REPRESENTATION") && record.values.len() == 3)
                .then(|| list(&record.values[1]).ok())
                .flatten()
        })
    }

    fn transformation(&self, id: usize) -> KernelResult<Placement> {
        let record = self.simple(id)?;
        if record.name != "ITEM_DEFINED_TRANSFORMATION" {
            return Err(unsupported(format!("STEP transformation {} is not supported", record.name)));
        }
        let origin = self.axis_placement(reference(argument(record, 2)?)?)?;
        let target = self.axis_placement(reference(argument(record, 3)?)?)?;
        Ok(origin.inverse().then(&target))
    }

    /// Collect solids of a representation item, placed in its representation.
    /// Mapped representations that `product` assigns to another product are
    /// recorded as occurrences of it instead.
    fn collect(
        &self,
        item: usize,
        local: Placement,
        found: &mut Collected,
        product: &dyn Fn(usize) -> Option<usize>,
        depth: usize,
    ) -> KernelResult<()> {
        let record = self.simple(item)?;
        match record.name.as_str() {
            "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS" => found.solids.push((item, local)),
            "SHELL_BASED_SURFACE_MODEL" => {
                for shell in list(argument(record, 1)?)? {
                    let shell = reference(shell)?;
                    if self.simple(self.oriented_shell(shell)?.0)?.name == "CLOSED_SHELL" {
                        found.solids.push((shell, local));
                    }
                }
            }
            "MAPPED_ITEM" if depth < 32 => {
                let map = self.simple(reference(argument(record, 1)?)?)?;
                let origin = self.axis_placement(reference(argument(map, 0)?)?)?;
                let target = self.axis_placement(reference(argument(record, 2)?)?)?;
                let mapping = origin.inverse().then(&target).then(&local);
                let mapped = reference(argument(map, 1)?)?;
                if let Some(product) = product(mapped) {
                    found.occurrences.push((product, mapping));
                    return Ok(());
                }
                for item in self.representation(mapped).unwrap_or(&[]) {
                    self.collect(reference(item)?, mapping, found, product, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Representations, grouped into shared coordinate spaces, and the
    /// transformed links between those spaces.
    fn graph(&self) -> KernelResult<Graph> {
        let mut ids = self.model.instances.keys().copied().collect::<Vec<_>>();
        ids.sort_unstable();
        let representations = ids.iter().copied().filter(|id| self.representation(*id).is_some()).collect::<Vec<_>>();

        // Representations of each product definition, to orient assembly links.
        let mut definition_shapes = HashMap::<usize, Vec<usize>>::new();
        for id in &ids {
            let Some(record) = self.record(*id, "SHAPE_DEFINITION_REPRESENTATION") else { continue };
            let (Ok(shape), Ok(representation)) = (reference(&record.values[0]), reference(&record.values[1])) else { continue };
            if let Some(definition) = self.record(shape, "PRODUCT_DEFINITION_SHAPE").and_then(|shape| shape.values.get(2)) {
                if let Ok(definition) = reference(definition) {
                    definition_shapes.entry(definition).or_default().push(representation);
                }
            }
        }
        // Assembly links relate a component's representation to its
        // parent's. They never share one space, even without a transform:
        // mapped items then place the component.
        let mut assembly_links = HashSet::new();
        let mut reversed_links = HashSet::new();
        for id in &ids {
            let Some(record) = self.record(*id, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION") else { continue };
            let (Ok(link), Ok(shape)) = (reference(&record.values[0]), reference(&record.values[1])) else { continue };
            assembly_links.insert(link);
            let usage = self
                .record(shape, "PRODUCT_DEFINITION_SHAPE")
                .and_then(|shape| reference(shape.values.get(2)?).ok())
                .and_then(|usage| self.records(usage).ok()?.iter().find(|record| record.values.len() >= 5));
            let link_record = self.records(link).ok().and_then(|records| {
                records.iter().find(|record| record.name.ends_with("REPRESENTATION_RELATIONSHIP") && record.values.len() >= 4)
            });
            if let (Some(usage), Some(link_record)) = (usage, link_record) {
                let parent = reference(&usage.values[3]).ok().and_then(|id| definition_shapes.get(&id));
                let first = reference(&link_record.values[2]).ok();
                if parent.is_some_and(|parent| first.is_some_and(|first| parent.contains(&first))) {
                    reversed_links.insert(link);
                }
            }
        }

        // Untransformed links share one coordinate space.
        let mut group = representations.iter().map(|id| (*id, *id)).collect::<HashMap<_, _>>();
        fn root(group: &mut HashMap<usize, usize>, id: usize) -> usize {
            let parent = group.get(&id).copied().unwrap_or(id);
            if parent == id {
                return id;
            }
            let root_id = root(group, parent);
            group.insert(id, root_id);
            root_id
        }
        let mut links = Vec::new();
        for id in &ids {
            let records = self.records(*id)?;
            let Some(link) = records.iter().find(|record| {
                record.name.ends_with("REPRESENTATION_RELATIONSHIP") && record.values.len() >= 4
            }) else {
                continue;
            };
            let (Ok(mut child), Ok(mut parent)) = (reference(&link.values[2]), reference(&link.values[3])) else { continue };
            if !group.contains_key(&child) || !group.contains_key(&parent) {
                continue;
            }
            let transform = records
                .iter()
                .find(|record| record.name == "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")
                .map(|record| self.transformation(reference(argument(record, 0)?)?))
                .transpose()?;
            match transform {
                Some(mut transform) => {
                    if reversed_links.contains(id) {
                        std::mem::swap(&mut child, &mut parent);
                        transform = transform.inverse();
                    }
                    links.push((child, parent, transform));
                }
                None if assembly_links.contains(id) => {}
                None => {
                    let (a, b) = (root(&mut group, child), root(&mut group, parent));
                    group.insert(a, b);
                }
            }
        }
        for id in &representations {
            root(&mut group, *id);
        }
        let links = links
            .into_iter()
            .map(|(child, parent, transform)| (group[&child], group[&parent], transform))
            .collect();
        let mapped = ids
            .iter()
            .filter_map(|id| self.record(*id, "REPRESENTATION_MAP"))
            .filter_map(|map| reference(map.values.get(1)?).ok())
            .collect();
        let mut members = HashMap::<usize, Vec<usize>>::new();
        for id in &representations {
            members.entry(group[id]).or_default().push(*id);
        }
        Ok(Graph { group, members, links, mapped, definition_shapes })
    }

    /// The solids of every representation in a coordinate space.
    fn space(&self, graph: &Graph, space: usize, found: &mut Collected, product: &dyn Fn(usize) -> Option<usize>) -> KernelResult<()> {
        let start = found.solids.len();
        for representation in &graph.members[&space] {
            for item in self.representation(*representation).unwrap_or(&[]) {
                if let Ok(item) = reference(item) {
                    self.collect(item, Placement::identity(), found, product, 0)?;
                }
            }
        }
        let mut seen = HashSet::new();
        let mut index = 0;
        found.solids.retain(|(item, placement)| {
            index += 1;
            index <= start || seen.insert((*item, format!("{placement:?}")))
        });
        Ok(())
    }

    /// Every solid instance with its world placement.
    fn solids(&self) -> KernelResult<Vec<(usize, Placement)>> {
        let graph = self.graph()?;
        fn placements(
            space: usize,
            links: &[(usize, usize, Placement)],
            memo: &mut HashMap<usize, Vec<Placement>>,
            depth: usize,
        ) -> Vec<Placement> {
            if let Some(result) = memo.get(&space) {
                return result.clone();
            }
            let parents = links.iter().filter(|(child, ..)| *child == space).collect::<Vec<_>>();
            let result = if parents.is_empty() || depth > 32 {
                vec![Placement::identity()]
            } else {
                parents
                    .into_iter()
                    .flat_map(|(_, parent, transform)| {
                        placements(*parent, links, memo, depth + 1)
                            .into_iter()
                            .map(|outer| transform.then(&outer))
                            .collect::<Vec<_>>()
                    })
                    .collect()
            };
            memo.insert(space, result.clone());
            result
        }

        // Mapped representations are placed only through their mapped items.
        let mut spaces = graph.members.keys().copied().collect::<Vec<_>>();
        spaces.sort_unstable();
        let mut memo = HashMap::new();
        let mut solids = Vec::new();
        for space in spaces {
            if graph.members[&space].iter().any(|member| graph.mapped.contains(member)) {
                continue;
            }
            let mut found = Collected::default();
            self.space(&graph, space, &mut found, &|_| None)?;
            for placement in placements(space, &graph.links, &mut memo, 0) {
                for (item, local) in &found.solids {
                    solids.push((*item, local.then(&placement)));
                }
            }
        }
        Ok(solids)
    }

    /// The product definitions that own coordinate spaces, with their own
    /// solids and their occurrences in each other. `None` without products.
    fn products(&self) -> KernelResult<Option<Vec<ProductPlan>>> {
        let graph = self.graph()?;
        let mut definitions = graph.definition_shapes.keys().copied().collect::<Vec<_>>();
        definitions.sort_unstable();
        let mut owner = HashMap::new();
        for (index, definition) in definitions.iter().enumerate() {
            for representation in &graph.definition_shapes[definition] {
                if let Some(space) = graph.group.get(representation) {
                    owner.entry(*space).or_insert(index);
                }
            }
        }
        if owner.is_empty() {
            return Ok(None);
        }
        let mut products = Vec::with_capacity(definitions.len());
        for (index, definition) in definitions.iter().enumerate() {
            let mut found = Collected::default();
            let product = |representation: usize| {
                graph.group.get(&representation).and_then(|space| owner.get(space)).copied().filter(|other| *other != index)
            };
            let mut spaces = owner.iter().filter(|(_, owner)| **owner == index).map(|(space, _)| *space).collect::<Vec<_>>();
            spaces.sort_unstable();
            for space in &spaces {
                self.space(&graph, *space, &mut found, &product)?;
            }
            for (child, parent, transform) in &graph.links {
                if spaces.contains(parent) {
                    if let Some(child) = owner.get(child).filter(|child| **child != index) {
                        found.occurrences.push((*child, *transform));
                    }
                }
            }
            products.push(ProductPlan { name: self.product_name(*definition), found });
        }
        Ok(Some(products))
    }

    /// The name of a product definition's product, or its identifier.
    fn product_name(&self, definition: usize) -> String {
        let text = |record: &Record, index: usize| match record.values.get(index) {
            Some(Value::Text(text)) if !text.is_empty() => Some(text.clone()),
            _ => None,
        };
        (|| {
            let formation = self.simple(reference(self.simple(definition).ok()?.values.get(2)?).ok()?).ok()?;
            let product = self.simple(reference(formation.values.get(2)?).ok()?).ok()?;
            text(product, 1).or_else(|| text(product, 0))
        })()
        .unwrap_or_default()
    }

    /// Surface colours of styled representation items by item id.
    fn colors(&self) -> HashMap<usize, [f64; 3]> {
        let mut colors = HashMap::new();
        for record in self.model.instances.values().flatten() {
            if !matches!(record.name.as_str(), "STYLED_ITEM" | "OVER_RIDING_STYLED_ITEM") {
                continue;
            }
            let (Some(styles), Some(Value::Reference(item))) = (record.values.get(1), record.values.get(2)) else {
                continue;
            };
            if let Some(color) = self.color(styles, 0) {
                colors.entry(*item).or_insert(color);
            }
        }
        colors
    }

    /// The first surface colour a presentation style value leads to.
    fn color(&self, value: &Value, depth: usize) -> Option<[f64; 3]> {
        match value {
            Value::List(values) => values.iter().find_map(|value| self.color(value, depth)),
            Value::Reference(id) if depth < 8 => {
                let record = self.simple(*id).ok()?;
                match record.name.as_str() {
                    "COLOUR_RGB" => Some([
                        number(record.values.get(1)?).ok()?,
                        number(record.values.get(2)?).ok()?,
                        number(record.values.get(3)?).ok()?,
                    ]),
                    "DRAUGHTING_PRE_DEFINED_COLOUR" => match record.values.first()? {
                        Value::Text(name) => Some(match name.as_str() {
                            "red" => [1.0, 0.0, 0.0],
                            "green" => [0.0, 1.0, 0.0],
                            "blue" => [0.0, 0.0, 1.0],
                            "yellow" => [1.0, 1.0, 0.0],
                            "magenta" => [1.0, 0.0, 1.0],
                            "cyan" => [0.0, 1.0, 1.0],
                            "black" => [0.0, 0.0, 0.0],
                            "white" => [1.0, 1.0, 1.0],
                            _ => return None,
                        }),
                        _ => None,
                    },
                    // Edge colours do not colour surfaces.
                    "CURVE_STYLE" => None,
                    _ => record.values.iter().find_map(|value| self.color(value, depth + 1)),
                }
            }
            _ => None,
        }
    }
}

/// Solids found in a coordinate space with their local placements, and the
/// products it places.
#[derive(Default)]
struct Collected {
    solids: Vec<(usize, Placement)>,
    occurrences: Vec<(usize, Placement)>,
}

struct ProductPlan {
    name: String,
    found: Collected,
}

/// Representations by coordinate space. `group` maps each representation to
/// its space; `links` place one space in another.
struct Graph {
    group: HashMap<usize, usize>,
    members: HashMap<usize, Vec<usize>>,
    links: Vec<(usize, usize, Placement)>,
    mapped: HashSet<usize>,
    definition_shapes: HashMap<usize, Vec<usize>>,
}

/// Signed parameter-space area enclosed by consecutive chart samples.
fn signed_area(charts: &[Vec<Point2>]) -> f64 {
    let points = charts.iter().flatten().collect::<Vec<_>>();
    (0..points.len())
        .map(|index| {
            let (a, b) = (points[index], points[(index + 1) % points.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_offsets_retain_analytic_supports_and_normals() {
        let model = super::super::parser::parse("DATA;
            #1=CARTESIAN_POINT('',(1.,2.,3.));
            #2=DIRECTION('',(0.,0.,1.)); #3=DIRECTION('',(1.,0.,0.));
            #4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
            #5=PLANE('',#4); #6=CYLINDRICAL_SURFACE('',#4,4.);
            #7=SPHERICAL_SURFACE('',#4,4.); #8=TOROIDAL_SURFACE('',#4,6.,2.);
            #9=OFFSET_SURFACE('',#5,0.5,.F.); #10=OFFSET_SURFACE('',#6,-0.5,.F.);
            #11=OFFSET_SURFACE('',#7,0.5,.F.); #12=OFFSET_SURFACE('',#8,-0.5,.F.);
            #13=OFFSET_SURFACE('',#8,4.5,.F.); #14=OFFSET_SURFACE('',#6,-5.,.F.);
            #15=OFFSET_SURFACE('',#7,-5.,.F.); #16=OFFSET_SURFACE('',#7,-4.,.F.);
            #17=OFFSET_SURFACE('',#6,-4.,.F.);
            ENDSEC;").unwrap();
        let reader = Reader::new(&model, ModelTolerance::default());
        for (base, offset, distance) in [(5, 9, 0.5), (6, 10, -0.5), (7, 11, 0.5), (8, 12, -0.5)] {
            let placement = Placement::identity();
            let (base, _) = reader.surface(base, &placement, &[]).unwrap();
            let (offset, flipped) = reader.surface(offset, &placement, &[]).unwrap();
            assert_eq!(offset.kind_name(), base.kind_name());
            assert!(!flipped);
            for (u, v) in [(0.0, 0.0), (0.8, 0.4), (5.9, -0.3)] {
                let expected = base.evaluate(u, v).unwrap().point + base.normal(u, v).unwrap() * distance;
                assert!(offset.evaluate(u, v).unwrap().point.distance(expected) < 1.0e-12);
                assert!(offset.normal(u, v).unwrap().dot(base.normal(u, v).unwrap()) > 1.0 - 1.0e-12);
            }
        }
        for (base, offset, distance) in [(8, 13, 4.5), (6, 14, -5.0), (7, 15, -5.0),
            (7, 16, -4.0), (6, 17, -4.0)] {
            let placement = Placement::identity();
            let (base, _) = reader.surface(base, &placement, &[]).unwrap();
            let (offset, flipped) = reader.surface(offset, &placement, &[]).unwrap();
            assert!(matches!(offset, Surface3::Offset(_)));
            assert!(!flipped);
            for (u, v) in [(0.0, 0.0), (0.8, 0.4), (5.9, -0.3)] {
                let expected = base.evaluate(u, v).unwrap().point + base.normal(u, v).unwrap() * distance;
                assert!(offset.evaluate(u, v).unwrap().point.distance(expected) < 1.0e-12);
            }
        }
    }
}
