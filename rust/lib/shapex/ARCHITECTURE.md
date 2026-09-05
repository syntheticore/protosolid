# Shapex kernel architecture

Shapex is a standalone Rust boundary-representation modeling kernel. It does not
contain an application document/feature graph, UI, JavaScript, WebAssembly, or
platform integration code. Those systems consume the kernel through ordinary
Rust APIs. The kernel does own automatic entity identity, immutable result
revisions, and operation provenance because those are part of modeling integrity.
It does not own application feature keys, parameters, expressions, or dependencies.

## Non-negotiable invariants

1. Safe public APIs never publish invalid topology or a physically invalid body.
2. Connectivity changes are expressed as Euler operators with explicit inverses.
3. An Euler operation is atomic. Failure restores topology, names, and provenance.
4. Construction topology is validated after every Euler operation.
5. A construction transaction becomes visible only after closed-volume geometric
   validation succeeds.
6. Every edge use on a face owns an oriented 2D p-curve. A 3D curve alone is not
   a valid trimmed-surface boundary.
7. Every geometric assertion has an achieved tolerance governed by the model's
   scale-aware tolerance policy.
8. Every topology-changing operation emits complete generated, modified, split,
   merged, and deleted provenance.
9. Entity identity is assigned when topology is created and is never reconstructed
   later through geometric coincidence.
10. Unsupported geometry returns a structured error. It is never interpreted as
    an empty intersection or successful no-op.

## Ownership

The advanced `Kernel` API owns generational arenas. Ordinary modeling operations
return immutable `Solid` and `Compound` snapshots with lightweight entity values.
No topology entity owns another through `Rc`, and no public API exposes interior
mutability.

Owned operations import their operands into an isolated internal topology, remap
all arena handles, and preserve exact geometry and persistent names. A result
retains a structurally shared lineage graph containing identities and operation
deltas only; it never retains earlier BREP bodies merely to resolve references.

The topological hierarchy is:

```text
Body -> Shell -> Face -> Loop -> Coedge -> Edge -> Vertex
```

An edge stores its exact 3D curve and parameter range. Every coedge stores its own
orientation, exact UV p-curve, and parameter range on one face. This represents
periodic seams and differing parameterizations without special cases.

## Editing and publication

`Kernel::begin_operation` clones a lightweight kernel state into an isolated Euler
transaction. Operators validate construction invariants before returning. Dropping
the transaction rolls it back. Committing runs published-body validation and swaps
the state into the kernel only on success.

Bulk algorithms such as booleans may compose many Euler operators inside one
transaction. Their intermediate open shells are valid cell-complex construction
states but are never observable as finished bodies.

Disconnected boundary shells use reversible body/shell Euler operators. This is
how a Boolean cavity becomes a negatively oriented closed shell of its containing
material body without a direct topology rewrite.

## Geometry

Basis geometry retains natural parameter domains, periodicity, and analytic types.
Trimming and orientation belong to topology uses. General curves and surfaces use
rational B-splines with validated knots, degrees, control nets, and weights.

Intersections use analytic solvers where a solver is certified. Results carry 3D
geometry, p-curves on both operands, parameters, tangency multiplicity, and achieved
error bounds. A pair without a certified solver returns `UnsupportedGeometry`; it
is never reported as an empty intersection. General NURBS subdivision and interval
certification remain deliberately outside the supported set until they can provide
the same completeness guarantee.

Faces, edges, solids, and compounds all implement entity-level tessellation.
Surface tessellation begins from one shared adaptive polyline per topological edge,
then performs conforming, local refinement in face parameter space. Exact boundary
samples are reused by adjacent faces, preventing face-by-face cracks. Whole-body
meshes compose the same face tessellations rather than using a second meshing path.

## Certified publication set

The current publisher certifies:

- arbitrary closed planar, linear-edge manifold shells, including cavities and
  genus introduced by through-holes;
- analytical BREP Boolean union, intersection, and difference, including curved
  supports, disconnected material bodies, and fully enclosed cavity shells;
- exact right cylinders, cones, spheres, and regular ring tori constructed by the
  kernel's Euler compositions;
- normal extrusions of profiles bounded by exact lines and circular arcs,
  including multiple holes;
- exact inward and outward shelling of manifold analytical BREP bodies, including
  planar, cylindrical, and toroidal fillet/protrusion combinations, with optional
  removed-face openings;
- full and partial profile revolutions, retaining primitive supports where
  possible and exact revolved surfaces for other meridians;
- smooth rational section lofts through line, circle, and NURBS profiles, with
  exact subdivision and degree elevation for section correspondence;
- equal-distance chamfers of planar edges and cylinder/plane rims, and neutral
  plane drafts of planar and pull-aligned cylindrical/conical faces;
- plane splits retaining either or both sides, including disconnected solids;
- rigid transforms of those bodies.

Planar bodies receive intersection/contact checks and mass properties from their
exact boundary triangulation. Recognized analytic bodies receive closed-form volume,
area, centroid, and inertia checks. General curved BREP combinations are certified by
direct divergence-theorem integration over exact trimmed parametric faces; periodic
p-curve loops are lifted into a continuous parameter chart before winding and physical
integration.


Before sewing, unmatched face boundaries are split at existing vertices on their
exact curves so adjacent faces can share an edge despite different subdivisions.
Boolean side probes use distances to retained curve intervals; coincident supports
use oriented trims to determine material sides. These operations retain exact
supports and curves, including when a chamfer cuts across earlier fillets. Periodic
seam continuity and display bands use a consistent parameter chart, with retained
seams selecting the intended sweep interval at and beyond a half turn.

## Parametric systems

The kernel is not an application-level dependency graph, expression evaluator, or
constraint solver. It provides the lower-level facilities such a system needs:
automatic persistent entity identity, immutable result revisions, complete
topology provenance, stable output ordering, and diagnostics that identify affected
entities. Applications retain their own feature identity and associate it with
kernel results. Entity resolution uses provenance, never array position or a
geometrical `IsSame` guess.

Resolution is transitive across arbitrary descendant operations and distinguishes
preserved, modified, split, merged, deleted, and unrelated entities. Cross-type
generation remains available separately, such as faces generated from an edge.
Semantic creation roles identify extrusion caps and the profile segment behind
each swept face without requiring caller-supplied kernel names.

## Legacy code

The pre-v2 curve, surface, and solid modules remain temporarily available while
their sound mathematical portions are migrated. New modeling algorithms target
`kernel` topology only. Once feature parity is reached, legacy topology and its
`Rc<RefCell<_>>` graph will be removed rather than maintained as a second kernel.
