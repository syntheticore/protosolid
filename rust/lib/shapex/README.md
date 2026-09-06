# Shapex

Shapex is a full BREP cad kernel with fully analytical/implicit/nurbs surfaces and watertight topology guarantees, based on euler operators for internal consistency and mathematical elegance. Its safe API returns a solid only after topology, trimming geometry, manifoldness, and physical volume have been
validated. It's pleasant to use directly in any rust application, but also make it easy to use in parametrical cad applications, which need to deal with the topological naming problem and re-identify faces and edges across operations. Geometry is always represented using the simplest possible mathematical form. For instance, a cylindrical surface can be described as a surface of revolution, instead of using NURBS. Neighboring surfaces of the same type get unified automatically if possible. (Two planar surfaces of the same orientation for instance)

The old implementation is available only through
the isolated `legacy` compatibility feature while callers migrate.


```rust
use shapex::{
    ModelTolerance, Plane3, Point2, Profile, TessellationOptions, TrimmedCurve2,
    Vec3, Wire2,
};

let tolerance = ModelTolerance::default();
let outer = Wire2::new(
    [
        TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(20.0, 0.0))?,
        TrimmedCurve2::line(Point2::new(20.0, 0.0), Point2::new(20.0, 10.0))?,
        TrimmedCurve2::line(Point2::new(20.0, 10.0), Point2::new(0.0, 10.0))?,
        TrimmedCurve2::line(Point2::new(0.0, 10.0), Point2::new(0.0, 0.0))?,
    ],
    tolerance,
)?;
let hole = Wire2::new(
    [TrimmedCurve2::circle(Point2::new(5.0, 5.0), 2.0)?],
    tolerance,
)?;
let profile = Profile::new(outer, [hole], tolerance)?;

let extrusion = profile.extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0))?;
println!("volume: {}", extrusion.volume());
let mesh = extrusion.tessellate(TessellationOptions::default())?;
let first_face = extrusion.faces().next().unwrap();
let face_mesh = first_face.tessellate(TessellationOptions::default())?;
# Ok::<(), shapex::KernelError>(())
```

Entity identities are generated and retained by Shapex. Returned `Solid` and
`Compound` values carry compact transitive lineage, not previous BREP
versions. An entity from any ancestor result can therefore be resolved after
several later operations:

```rust,ignore
let face = solid.faces().next().unwrap().clone();
let first_cut = solid.difference(&first_tool)?;
let second_cut = first_cut.difference(&second_tool)?;
match second_cut.resolve(&face) {
    Resolution::Preserved(face) => { /* same semantic entity and revision */ }
    Resolution::Modified(face) => { /* same semantic entity, new revision */ }
    Resolution::Split(faces) => { /* one source has several descendants */ }
    Resolution::Merged(face) => { /* several sources contributed */ }
    Resolution::Deleted => {}
    Resolution::Unrelated => { /* the face never belonged to this lineage */ }
}
```

The latest operation also exposes typed semantic creation roles and cross-type
generation queries. For example, an extrusion reports its start, end, and swept
side faces, while a future fillet can report faces generated from its input edges.
Applications may associate those roles with their own document references without
assigning names inside Shapex.

Advanced clients can use `Kernel`, `EulerTransaction`, and the classical Euler
operators directly. Each operator validates construction topology; dropping a
transaction rolls it back, and committing runs the complete physical publication
suite atomically.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the invariants, ownership model,
certification boundary, and current supported geometry.
