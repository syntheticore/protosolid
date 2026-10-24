//! AP214 writer for exact BREP solids.
//!
//! Analytic supports, lines, circles and B-splines are written exactly.
//! Procedural supports and implicit edge curves are interpolated by
//! B-splines. Each face's sense is derived from its material normal, so the
//! written supports need not share the kernel's natural normals.
//!
//! Assemblies write every product once. Its occurrences in other products
//! are next assembly usages placed by representation relationships, as
//! AP214 and AP242 importers expect. Solid colours are styled items of one
//! presentation representation.

use std::collections::BTreeMap;

use cgmath::prelude::*;

use super::{fit, StepAssembly, StepLengthUnit, StepOccurrence};
use crate::{
    invalid, BodyId, Compound, Curve2, Curve3, EdgeId, FaceId, FaceOrientation, KernelResult,
    LoopKind, ModelTolerance, NurbsCurve3, NurbsSurface3, Point2, Point3, RevolvedSurface3,
    ShellId, Surface3, Topology, Vec2, Vec3, VertexId,
};

pub(super) fn write(compound: &Compound, name: &str, timestamp: &str) -> KernelResult<String> {
    let mut document = Document::new(StepLengthUnit::Millimetre);
    let solids = document.solids(compound)?;
    Ok(document.finish(&[ProductShape { name, solids, colors: &[], occurrences: &[] }], name, timestamp))
}

pub(super) fn write_assembly(assembly: &StepAssembly, timestamp: &str) -> KernelResult<String> {
    let mut document = Document::new(assembly.unit);
    let mut products = Vec::with_capacity(assembly.products.len());
    for product in &assembly.products {
        products.push(ProductShape {
            name: &product.name,
            solids: match &product.bodies {
                Some(bodies) => document.solids(bodies)?,
                None => Vec::new(),
            },
            colors: &product.colors,
            occurrences: &product.occurrences,
        });
    }
    let name = assembly.roots.first().map_or("", |root| assembly.products[*root].name.as_str());
    Ok(document.finish(&products, name, timestamp))
}

/// A product's written solids, their colours and the occurrences it places.
struct ProductShape<'a> {
    name: &'a str,
    solids: Vec<usize>,
    colors: &'a [Option<[f64; 3]>],
    occurrences: &'a [StepOccurrence],
}

/// The entities of one exchange file. Every compound's geometry is written
/// by its own `Writer`; products and their contexts are written last.
struct Document {
    entities: Vec<String>,
    /// The coarsest fit tolerance of the written compounds.
    fit_tolerance: f64,
    unit: StepLengthUnit,
}

/// How the written support's natural normal relates to the face.
enum Sense {
    /// The support keeps the kernel parameterization and its natural normal.
    Parameterization,
    /// The support's natural normal at the face sample point.
    Normal(Vec3),
}

struct Writer<'a> {
    topology: &'a Topology,
    tolerance: ModelTolerance,
    fit_tolerance: f64,
    /// File length units per millimetre.
    scale: f64,
    entities: &'a mut Vec<String>,
    vertices: BTreeMap<VertexId, usize>,
    edges: BTreeMap<EdgeId, usize>,
}

fn add(entities: &mut Vec<String>, entity: String) -> usize {
    entities.push(entity);
    entities.len()
}

fn point(entities: &mut Vec<String>, point: Point3, scale: f64) -> usize {
    let point = point * scale;
    add(entities, format!("CARTESIAN_POINT('',({},{},{}))", real(point.x), real(point.y), real(point.z)))
}

fn direction(entities: &mut Vec<String>, direction: Vec3) -> usize {
    let direction = direction.normalize();
    add(entities, format!("DIRECTION('',({},{},{}))", real(direction.x), real(direction.y), real(direction.z)))
}

fn placement(entities: &mut Vec<String>, origin: Point3, z: Vec3, x: Vec3, scale: f64) -> usize {
    let z = z.normalize();
    let x = x - z * x.dot(z);
    let origin = point(entities, origin, scale);
    let z = direction(entities, z);
    let x = direction(entities, x);
    add(entities, format!("AXIS2_PLACEMENT_3D('',#{origin},#{z},#{x})"))
}

impl Writer<'_> {
    fn add(&mut self, entity: String) -> usize {
        add(self.entities, entity)
    }

    fn point(&mut self, value: Point3) -> usize {
        point(self.entities, value, self.scale)
    }

    fn direction(&mut self, value: Vec3) -> usize {
        direction(self.entities, value)
    }

    fn placement(&mut self, origin: Point3, z: Vec3, x: Vec3) -> usize {
        placement(self.entities, origin, z, x, self.scale)
    }

    fn line(&mut self, origin: Point3, direction: Vec3) -> usize {
        let origin = self.point(origin);
        let unit = self.direction(direction);
        let vector = self.add(format!("VECTOR('',#{unit},{})", real(direction.magnitude() * self.scale)));
        self.add(format!("LINE('',#{origin},#{vector})"))
    }

    fn circle(&mut self, center: Point3, normal: Vec3, x: Vec3, radius: f64) -> usize {
        let placement = self.placement(center, normal, x);
        self.add(format!("CIRCLE('',#{placement},{})", real(radius * self.scale)))
    }

    fn bspline_curve(&mut self, curve: &NurbsCurve3) -> usize {
        let points = curve
            .control_points()
            .iter()
            .map(|point| format!("#{}", self.point(*point)))
            .collect::<Vec<_>>()
            .join(",");
        let (multiplicities, knots) = knot_runs(curve.knots());
        let degree = curve.degree();
        if curve.weights().iter().all(|weight| *weight == 1.0) {
            return self.add(format!(
                "B_SPLINE_CURVE_WITH_KNOTS('',{degree},({points}),.UNSPECIFIED.,.F.,.F.,({multiplicities}),({knots}),.UNSPECIFIED.)"
            ));
        }
        let weights = reals(curve.weights());
        self.add(format!(
            "(BOUNDED_CURVE()B_SPLINE_CURVE({degree},({points}),.UNSPECIFIED.,.F.,.F.)\
             B_SPLINE_CURVE_WITH_KNOTS(({multiplicities}),({knots}),.UNSPECIFIED.)CURVE()\
             GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_CURVE(({weights}))REPRESENTATION_ITEM(''))"
        ))
    }

    fn bspline_surface(&mut self, surface: &NurbsSurface3) -> usize {
        // The kernel stores rows along v; STEP lists controls by u, then v.
        let controls = surface.control_points();
        let weights = surface.weights();
        let (columns, rows) = (controls[0].len(), controls.len());
        let points = (0..columns)
            .map(|u| {
                let column = (0..rows)
                    .map(|v| format!("#{}", self.point(controls[v][u])))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("({column})")
            })
            .collect::<Vec<_>>()
            .join(",");
        let (degree_u, degree_v) = surface.degrees();
        let (knots_u, knots_v) = surface.knots();
        let (multiplicities_u, knots_u) = knot_runs(knots_u);
        let (multiplicities_v, knots_v) = knot_runs(knots_v);
        if weights.iter().flatten().all(|weight| *weight == 1.0) {
            return self.add(format!(
                "B_SPLINE_SURFACE_WITH_KNOTS('',{degree_u},{degree_v},({points}),.UNSPECIFIED.,.F.,.F.,.F.,\
                 ({multiplicities_u}),({multiplicities_v}),({knots_u}),({knots_v}),.UNSPECIFIED.)"
            ));
        }
        let weights = (0..columns)
            .map(|u| format!("({})", reals(&(0..rows).map(|v| weights[v][u]).collect::<Vec<_>>())))
            .collect::<Vec<_>>()
            .join(",");
        self.add(format!(
            "(BOUNDED_SURFACE()B_SPLINE_SURFACE({degree_u},{degree_v},({points}),.UNSPECIFIED.,.F.,.F.,.F.)\
             B_SPLINE_SURFACE_WITH_KNOTS(({multiplicities_u}),({multiplicities_v}),({knots_u}),({knots_v}),.UNSPECIFIED.)\
             GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_SURFACE(({weights}))REPRESENTATION_ITEM('')SURFACE())"
        ))
    }

    fn vertex(&mut self, vertex: VertexId) -> KernelResult<usize> {
        if let Some(id) = self.vertices.get(&vertex) {
            return Ok(*id);
        }
        let point = self.point(self.topology.vertex(vertex)?.point());
        let id = self.add(format!("VERTEX_POINT('',#{point})"));
        self.vertices.insert(vertex, id);
        Ok(id)
    }

    fn edge(&mut self, edge_id: EdgeId) -> KernelResult<usize> {
        if let Some(id) = self.edges.get(&edge_id) {
            return Ok(*id);
        }
        let edge = self.topology.edge(edge_id)?;
        let [start, end] = edge.vertices();
        let (start, end) = (self.vertex(start)?, self.vertex(end)?);
        let range = edge.range();
        let forward = range.span() > 0.0;
        let (curve, same_sense) = match edge.curve() {
            Curve3::Line(line) => (self.line(line.origin(), line.direction()), forward),
            Curve3::Circle(circle) => (
                self.circle(circle.center(), circle.normal(), circle.x_radius(), circle.radius()),
                forward,
            ),
            Curve3::Nurbs(curve) => (self.bspline_curve(curve), forward),
            // Curves are cheap to fit closely; keeping them on their exact
            // neighbours leaves the support fits as the only deviation.
            curve => (self.bspline_curve(&fit::curve(curve, range, self.fit_tolerance * 0.01)?), true),
        };
        let id = self.add(format!("EDGE_CURVE('',#{start},#{end},#{curve},{})", logical(same_sense)));
        self.edges.insert(edge_id, id);
        Ok(id)
    }

    /// A regular point of the face boundary and its material normal.
    fn face_sample(&self, face_id: FaceId) -> KernelResult<(Point3, Vec3)> {
        let face = self.topology.face(face_id)?;
        let mut best = None::<(f64, Point3, Vec3)>;
        for loop_id in face.loops() {
            for coedge_id in self.topology.loop_coedges(*loop_id)? {
                let coedge = self.topology.coedge(coedge_id)?;
                let range = coedge.range();
                let Ok(uv) = coedge.pcurve().evaluate(range.start + range.span() * 0.5) else {
                    continue;
                };
                let (u, v) = (uv.point.x, uv.point.y);
                let (Ok(value), Ok(normal)) = (face.surface().evaluate(u, v), face.oriented_normal(u, v)) else {
                    continue;
                };
                let regularity = value.du.cross(value.dv).magnitude();
                if best.map_or(true, |(current, ..)| regularity > current) {
                    best = Some((regularity, value.point, normal));
                }
            }
        }
        best.map(|(_, point, normal)| (point, normal))
            .ok_or_else(|| invalid("face has no regular boundary point"))
    }

    fn surface(&mut self, face_id: FaceId, sample: Point3) -> KernelResult<(usize, Sense)> {
        let surface = self.topology.face(face_id)?.surface();
        Ok(match surface {
            Surface3::Plane(plane) => {
                let placement = self.placement(plane.origin(), plane.normal(), plane.u());
                (self.add(format!("PLANE('',#{placement})")), Sense::Normal(plane.normal()))
            }
            Surface3::Cylinder(cylinder) => {
                let axis = cylinder.x_radius().cross(cylinder.y_radius()).normalize();
                let placement = self.placement(cylinder.origin(), axis, cylinder.x_radius());
                let radial = sample - cylinder.origin();
                let normal = radial - axis * radial.dot(axis);
                let id = self.add(format!("CYLINDRICAL_SURFACE('',#{placement},{})", real(cylinder.radius() * self.scale)));
                (id, Sense::Normal(normal))
            }
            Surface3::Cone(cone) => {
                let height = cone.axis().magnitude();
                let axis = cone.axis() / height;
                let radius = cone.reference_radius();
                let placement = self.placement(cone.apex() + cone.axis(), axis, cone.x_radius());
                let offset = sample - cone.apex();
                let axial = offset.dot(axis);
                let radial = (offset - axis * axial).normalize();
                let normal = (radial - axis * (radius / height)) * axial.signum();
                let id = self.add(format!(
                    "CONICAL_SURFACE('',#{placement},{},{})",
                    real(radius * self.scale),
                    real((radius / height).atan())
                ));
                (id, Sense::Normal(normal))
            }
            Surface3::Sphere(sphere) => {
                let placement = self.placement(sphere.center(), sphere.z_radius(), sphere.x_radius());
                let id = self.add(format!("SPHERICAL_SURFACE('',#{placement},{})", real(sphere.radius() * self.scale)));
                (id, Sense::Normal(sample - sphere.center()))
            }
            Surface3::Torus(torus) => {
                let axis = torus.normal();
                let placement = self.placement(torus.center(), axis, torus.x_major());
                let offset = sample - torus.center();
                let tube = torus.center() + (offset - axis * offset.dot(axis)).normalize() * torus.major_radius();
                let id = self.add(format!(
                    "TOROIDAL_SURFACE('',#{placement},{},{})",
                    real(torus.major_radius() * self.scale),
                    real(torus.minor_radius() * self.scale)
                ));
                (id, Sense::Normal(sample - tube))
            }
            Surface3::Nurbs(nurbs) => (self.bspline_surface(nurbs), Sense::Parameterization),
            Surface3::Revolved(revolved) => match self.profile_curve(revolved) {
                Some(curve) => {
                    let frame = revolved.frame();
                    let origin = self.point(frame.origin());
                    let direction = self.direction(frame.z());
                    let axis = self.add(format!("AXIS1_PLACEMENT('',#{origin},#{direction})"));
                    (self.add(format!("SURFACE_OF_REVOLUTION('',#{curve},#{axis})")), Sense::Parameterization)
                }
                None => self.fitted_surface(face_id)?,
            },
            _ => self.fitted_surface(face_id)?,
        })
    }

    fn fitted_surface(&mut self, face_id: FaceId) -> KernelResult<(usize, Sense)> {
        let surface = self.topology.face(face_id)?.surface();
        let bounds = crate::features::face_parameter_bounds(self.topology, face_id, self.tolerance)?;
        let fitted = fit::surface(surface, bounds, self.fit_tolerance)?;
        Ok((self.bspline_surface(&fitted), Sense::Parameterization))
    }

    /// The meridian of a revolved support as a curve with the profile's own
    /// parameter, so the revolution keeps the kernel parameterization.
    fn profile_curve(&mut self, surface: &RevolvedSurface3) -> Option<usize> {
        let frame = surface.frame();
        let lift = |point: Point2| frame.origin() + frame.x() * point.x + frame.z() * point.y;
        let lift_vector = |vector: Vec2| frame.x() * vector.x + frame.z() * vector.y;
        match surface.profile() {
            Curve2::Line(line) => Some(self.line(lift(line.origin()), lift_vector(line.direction()))),
            // A whole circle would sweep its support twice with opposite
            // normals; write only the retained arc.
            Curve2::Circle(circle) => {
                let range = surface.profile_range();
                let arc = arc(lift(circle.center()), frame.x() * circle.radius(), frame.z() * circle.radius(),
                    range.start.min(range.end), range.start.max(range.end)).ok()?;
                let arc = if range.span() < 0.0 { reversed(&arc).ok()? } else { arc };
                Some(self.bspline_curve(&arc))
            }
            Curve2::Nurbs(curve) => {
                let lifted = NurbsCurve3::new(
                    curve.degree(),
                    curve.control_points().iter().map(|point| lift(*point)).collect(),
                    curve.weights().to_vec(),
                    curve.knots().to_vec(),
                )
                .ok()?;
                Some(self.bspline_curve(&lifted))
            }
            _ => None,
        }
    }

    fn face(&mut self, face_id: FaceId, reversed: bool) -> KernelResult<usize> {
        let topology = self.topology;
        let face = topology.face(face_id)?;
        let (sample, material) = self.face_sample(face_id)?;
        let (surface, sense) = self.surface(face_id, sample)?;
        let same_sense = match sense {
            Sense::Parameterization => face.orientation() == FaceOrientation::Forward,
            Sense::Normal(normal) => normal.dot(material) > 0.0,
        };
        let mut bounds = Vec::new();
        for loop_id in face.loops() {
            let mut edges = Vec::new();
            for coedge_id in topology.loop_coedges(*loop_id)? {
                let coedge = topology.coedge(coedge_id)?;
                let edge = self.edge(coedge.edge())?;
                edges.push(format!(
                    "#{}",
                    self.add(format!("ORIENTED_EDGE('',*,*,#{edge},{})", logical(coedge.is_forward())))
                ));
            }
            let edge_loop = self.add(format!("EDGE_LOOP('',({}))", edges.join(",")));
            let kind = match topology.loop_(*loop_id)?.kind() {
                LoopKind::Outer => "FACE_OUTER_BOUND",
                LoopKind::Inner => "FACE_BOUND",
            };
            bounds.push(format!("#{}", self.add(format!("{kind}('',#{edge_loop},{})", logical(!reversed)))));
        }
        Ok(self.add(format!(
            "ADVANCED_FACE('',({}),#{surface},{})",
            bounds.join(","),
            logical(same_sense != reversed)
        )))
    }

    /// Void shells are written with normals out of the cavity and referenced
    /// reversed, as `brep_with_voids` requires.
    fn shell(&mut self, shell: ShellId, reversed: bool) -> KernelResult<usize> {
        let faces = self
            .topology
            .shell(shell)?
            .faces()
            .iter()
            .map(|face| Ok(format!("#{}", self.face(*face, reversed)?)))
            .collect::<KernelResult<Vec<_>>>()?;
        Ok(self.add(format!("CLOSED_SHELL('',({}))", faces.join(","))))
    }

    fn body(&mut self, body: BodyId) -> KernelResult<usize> {
        let mut outer = None;
        let mut voids = Vec::new();
        for shell in self.topology.body(body)?.shells() {
            if crate::physical::shell_signed_volume(self.topology, *shell, self.tolerance)? > 0.0 {
                outer = Some(*shell);
            } else {
                voids.push(*shell);
            }
        }
        let outer = self.shell(outer.ok_or_else(|| invalid("solid has no outer shell"))?, false)?;
        if voids.is_empty() {
            return Ok(self.add(format!("MANIFOLD_SOLID_BREP('',#{outer})")));
        }
        let voids = voids
            .into_iter()
            .map(|shell| {
                let shell = self.shell(shell, true)?;
                Ok(format!("#{}", self.add(format!("ORIENTED_CLOSED_SHELL('',*,#{shell},.F.)"))))
            })
            .collect::<KernelResult<Vec<_>>>()?;
        Ok(self.add(format!("BREP_WITH_VOIDS('',#{outer},({}))", voids.join(","))))
    }
}

impl Document {
    fn new(unit: StepLengthUnit) -> Self {
        Self { entities: Vec::new(), fit_tolerance: 0.0, unit }
    }

    fn add(&mut self, entity: String) -> usize {
        add(&mut self.entities, entity)
    }

    /// Write a compound's solids in its own coordinates.
    fn solids(&mut self, compound: &Compound) -> KernelResult<Vec<usize>> {
        let scale = compound
            .vertices()
            .map(|vertex| vertex.point().to_vec().magnitude())
            .fold(1.0, f64::max);
        let fit_tolerance = (compound.tolerance().linear * 10.0).max(scale * 1.0e-9);
        self.fit_tolerance = self.fit_tolerance.max(fit_tolerance);
        let mut writer = Writer {
            topology: compound.topology(),
            tolerance: compound.tolerance(),
            fit_tolerance,
            scale: 1.0 / self.unit.millimetres(),
            entities: &mut self.entities,
            vertices: BTreeMap::new(),
            edges: BTreeMap::new(),
        };
        compound.solids().map(|solid| writer.body(solid.topology_id())).collect()
    }

    /// Products with solids are BREP shape representations; assemblies of
    /// other products only are plain shape representations. Occurrences
    /// relate a product's representation to its parent's by the
    /// occurrence's placement there.
    fn finish(mut self, products: &[ProductShape], name: &str, timestamp: &str) -> String {
        let application = self.add("APPLICATION_CONTEXT('automotive design')".into());
        self.add(format!(
            "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{application})"
        ));
        let product_context = self.add(format!("PRODUCT_CONTEXT('',#{application},'mechanical')"));
        let definition_context =
            self.add(format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{application},'design')"));
        let scale = 1.0 / self.unit.millimetres();
        let millimetre = self.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
        let length = match self.unit {
            StepLengthUnit::Millimetre => millimetre,
            StepLengthUnit::Inch => {
                let exponents = self.add("DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.)".into());
                let measure = self.add(format!("LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#{millimetre})"));
                self.add(format!("(CONVERSION_BASED_UNIT('INCH',#{measure})LENGTH_UNIT()NAMED_UNIT(#{exponents}))"))
            }
        };
        let angle = self.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
        let solid_angle = self.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
        let accuracy = if self.fit_tolerance > 0.0 { self.fit_tolerance } else { 1.0e-6 };
        let uncertainty = self.add(format!(
            "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({}),#{length},'distance_accuracy_value','confusion accuracy')",
            real(accuracy * scale)
        ));
        let context = self.add(format!(
            "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty}))\
             GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length},#{angle},#{solid_angle}))REPRESENTATION_CONTEXT('',''))"
        ));

        // (definition, representation, origin, occurrence placements)
        let mut written = Vec::with_capacity(products.len());
        for product in products {
            let name = string(product.name);
            let product_id = self.add(format!("PRODUCT({name},{name},'',(#{product_context}))"));
            self.add(format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{product_id}))"));
            let formation = self.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product_id})"));
            let definition =
                self.add(format!("PRODUCT_DEFINITION('design','',#{formation},#{definition_context})"));
            let shape = self.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
            let origin = placement(&mut self.entities, Point3::origin(), Vec3::unit_z(), Vec3::unit_x(), scale);
            let placements = product
                .occurrences
                .iter()
                .map(|occurrence| {
                    let rotation = occurrence.placement.rotation();
                    let origin = Point3::from_vec(occurrence.placement.translation());
                    placement(&mut self.entities, origin, rotation.z, rotation.x, scale)
                })
                .collect::<Vec<_>>();
            let items = std::iter::once(&origin)
                .chain(&placements)
                .chain(&product.solids)
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(",");
            let kind = if product.solids.is_empty() { "SHAPE_REPRESENTATION" } else { "ADVANCED_BREP_SHAPE_REPRESENTATION" };
            let representation = self.add(format!("{kind}({name},({items}),#{context})"));
            self.add(format!("SHAPE_DEFINITION_REPRESENTATION(#{shape},#{representation})"));
            written.push((definition, representation, origin, placements));
        }
        let mut usage = 0;
        for (product, (definition, representation, _, placements)) in products.iter().zip(&written) {
            for (occurrence, placement) in product.occurrences.iter().zip(placements) {
                let (child_definition, child_representation, child_origin, _) = &written[occurrence.product];
                usage += 1;
                let child_name = string(products[occurrence.product].name);
                let nauo = self.add(format!(
                    "NEXT_ASSEMBLY_USAGE_OCCURRENCE('{usage}',{child_name},'',#{definition},#{child_definition},$)"
                ));
                let shape = self.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{nauo})"));
                let transformation =
                    self.add(format!("ITEM_DEFINED_TRANSFORMATION('','',#{child_origin},#{placement})"));
                let relationship = self.add(format!(
                    "(REPRESENTATION_RELATIONSHIP('','',#{child_representation},#{representation})\
                     REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{transformation})SHAPE_REPRESENTATION_RELATIONSHIP())"
                ));
                self.add(format!("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{relationship},#{shape})"));
            }
        }

        // One surface style per distinct colour, shared by its solids.
        let mut styles = Vec::<([f64; 3], usize)>::new();
        let mut styled = Vec::new();
        for product in products {
            for (solid, color) in product.solids.iter().zip(product.colors) {
                let Some(color) = color else { continue };
                let style = match styles.iter().find(|(existing, _)| existing == color) {
                    Some((_, style)) => *style,
                    None => {
                        let style = self.surface_style(*color);
                        styles.push((*color, style));
                        style
                    }
                };
                styled.push(format!("#{}", self.add(format!("STYLED_ITEM('color',(#{style}),#{solid})"))));
            }
        }
        if !styled.is_empty() {
            self.add(format!(
                "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',({}),#{context})",
                styled.join(",")
            ));
        }

        let name = string(name);
        let mut text = String::from("ISO-10303-21;\nHEADER;\n");
        text += "FILE_DESCRIPTION(('ProtoSolid BREP'),'2;1');\n";
        text += &format!("FILE_NAME({name},{},(''),(''),'ProtoSolid','ProtoSolid','');\n", string(timestamp));
        text += "FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n";
        for (index, entity) in self.entities.iter().enumerate() {
            text += &format!("#{}={entity};\n", index + 1);
        }
        text += "ENDSEC;\nEND-ISO-10303-21;\n";
        text
    }

    /// A presentation style filling both sides of a surface with `color`.
    fn surface_style(&mut self, [red, green, blue]: [f64; 3]) -> usize {
        let color = self.add(format!("COLOUR_RGB('',{},{},{})", real(red), real(green), real(blue)));
        let fill_color = self.add(format!("FILL_AREA_STYLE_COLOUR('',#{color})"));
        let fill = self.add(format!("FILL_AREA_STYLE('',(#{fill_color}))"));
        let surface_fill = self.add(format!("SURFACE_STYLE_FILL_AREA(#{fill})"));
        let side = self.add(format!("SURFACE_SIDE_STYLE('',(#{surface_fill}))"));
        let usage = self.add(format!("SURFACE_STYLE_USAGE(.BOTH.,#{side})"));
        self.add(format!("PRESENTATION_STYLE_ASSIGNMENT((#{usage}))"))
    }
}

/// An exact rational arc of `center + x cos t + y sin t` over `[start, end]`,
/// in at most quarter-turn spans.
fn arc(center: Point3, x: Vec3, y: Vec3, start: f64, end: f64) -> KernelResult<NurbsCurve3> {
    let spans = ((end - start) / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = (end - start) / spans as f64;
    let at = |angle: f64, scale: f64| center + (x * angle.cos() + y * angle.sin()) * scale;
    let weight = (step * 0.5).cos();
    let mut points = vec![at(start, 1.0)];
    let mut weights = vec![1.0];
    let mut knots = vec![0.0; 3];
    for index in 0..spans {
        let angle = start + step * index as f64;
        points.extend([at(angle + step * 0.5, 1.0 / weight), at(angle + step, 1.0)]);
        weights.extend([weight, 1.0]);
        knots.extend(vec![(index + 1) as f64; 2]);
    }
    knots.push(spans as f64);
    NurbsCurve3::new(2, points, weights, knots)
}

fn reversed(curve: &NurbsCurve3) -> KernelResult<NurbsCurve3> {
    let last = *curve.knots().last().unwrap();
    let first = curve.knots()[0];
    NurbsCurve3::new(
        curve.degree(),
        curve.control_points().iter().rev().copied().collect(),
        curve.weights().iter().rev().copied().collect(),
        curve.knots().iter().rev().map(|knot| first + last - knot).collect(),
    )
}

/// Distinct knots and their multiplicities.
fn knot_runs(knots: &[f64]) -> (String, String) {
    let mut runs = Vec::<(f64, usize)>::new();
    for knot in knots {
        match runs.last_mut() {
            Some((value, count)) if (*knot - *value).abs() <= f64::EPSILON * value.abs().max(1.0) * 4.0 => {
                *count += 1
            }
            _ => runs.push((*knot, 1)),
        }
    }
    (
        runs.iter().map(|(_, count)| count.to_string()).collect::<Vec<_>>().join(","),
        reals(&runs.iter().map(|(value, _)| *value).collect::<Vec<_>>()),
    )
}

fn reals(values: &[f64]) -> String {
    values.iter().map(|value| real(*value)).collect::<Vec<_>>().join(",")
}

/// The shortest round-trip decimal in Part 21 syntax, which requires a
/// decimal point and an upper-case exponent.
pub(super) fn real(value: f64) -> String {
    let text = format!("{value:?}");
    match text.split_once('e') {
        Some((mantissa, exponent)) if mantissa.contains('.') => format!("{mantissa}E{exponent}"),
        Some((mantissa, exponent)) => format!("{mantissa}.E{exponent}"),
        None => text,
    }
}

fn logical(value: bool) -> &'static str {
    if value { ".T." } else { ".F." }
}

fn string(value: &str) -> String {
    let mut text = String::from("'");
    for character in value.chars() {
        match character {
            '\'' => text += "''",
            '\\' => text += "\\\\",
            ' '..='~' => text.push(character),
            _ => {
                text += "\\X2\\";
                for unit in character.encode_utf16(&mut [0; 2]) {
                    text += &format!("{unit:04X}");
                }
                text += "\\X0\\";
            }
        }
    }
    text + "'"
}
