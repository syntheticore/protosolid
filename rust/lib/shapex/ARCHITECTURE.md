# Shapex kernel architecture

Shapex is a standalone Rust boundary-representation modeling kernel. It does not
contain an application document/feature graph, UI, JavaScript, WebAssembly, or
platform integration code. Those systems consume the kernel through ordinary
Rust APIs. The kernel does own automatic entity identity, immutable result
revisions, and operation provenance because those are part of modeling integrity.
It does not own application feature keys, parameters, expressions, or dependencies.

## Non-negotiable invariants

1. Safe public APIs never publish invalid topology or a physically invalid body.
2. Topology changes happen only inside a transaction. A transaction is atomic:
   dropping it, or a failure to finish it, leaves the kernel unchanged.
3. A transaction becomes visible only after the complete published-body
   invariants (closed manifold shells, consistent p-curves and loop winding,
   physical volume) are proven for every body it touched.
4. Every edge use on a face owns an oriented 2D p-curve. A 3D curve alone is not
   a valid trimmed-surface boundary.
5. Every geometric assertion has an achieved tolerance governed by the model's
   scale-aware tolerance policy.
6. Every topology-changing operation emits complete generated, modified, split,
   merged, and deleted provenance.
7. Entity identity is assigned when topology is created and is never reconstructed
   later through geometric coincidence.
8. Unsupported geometry returns a structured error. It is never interpreted as
   an empty intersection or successful no-op.

## Ownership

A `Kernel` owns generational arenas. Ordinary modeling operations
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

Every operation edits an isolated copy of a kernel's topology inside a
`Transaction` (`transaction.rs`). Edits are not validated individually; an
operation may pass through open, partially built states. Finishing the
transaction runs the complete published-body validation once, certifies the
physical properties of the bodies it touched, and only then swaps the result into
the kernel. Dropping it discards every edit. Where an operation tries alternative
constructions, it does so under a transaction checkpoint (`atomic`), which
restores topology, names, and provenance when an attempt fails.

Operations build topology in one of two ways:

- **Patch sewing** (`sew.rs`). Global operations — Booleans, offsets and shells,
  face moves, drafts and replacements, sweeps, lofts, revolutions and face-limited
  extrusions — describe their result as exact trimmed face patches carrying
  segment provenance. Sewing conforms the patch boundaries to each other, welds
  them into shared edges and vertices, separates point-contact vertex fans, and
  seals the shell. It knows nothing about the operation that produced the
  patches. Disconnected component shells are then nested by signed volume and
  classification, so a cavity becomes a negatively oriented shell of its
  containing material body.
- **Euler operators** (`euler.rs`) and the low-level edits of `transaction.rs`.
  Primitive construction (`primitives.rs`), straight profile extrusion
  (`extrude.rs`), periodic seam relocation (`periodic.rs`), and edge blends
  (`edge_blend.rs`) edit topology locally.

After construction, `canonicalize.rs` removes boundaries without material
meaning: partitions of one periodic face support and degree-two subdivisions of
one curve. Periodic charts are normalized once more when a transaction finishes.

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

Spline supports sharing a parameter map are certified coincident in common boxes
using Bernstein coefficients of the homogeneous cross-products. This recognizes exact
continuations, different knot subdivisions, degrees, and rational weights without
relying on point samples. Shared-support Boolean arrangements retain chart cells
at singular boundaries, including poles, and publish each overlapping region once.

Faces, edges, solids, and compounds all implement entity-level tessellation.
Surface tessellation begins from one shared adaptive polyline per topological edge,
then performs conforming, local refinement in face parameter space. Exact boundary
samples are reused by adjacent faces, preventing face-by-face cracks. Whole-body
meshes compose the same face tessellations rather than using a second meshing path.

### Evaluation cost and reuse

Rational 2D and 3D curves share a local homogeneous evaluator for positions and
their derivatives through third order. It locates the active knot span and reads only
`degree + 1` controls, giving `O(log(control_count) + degree²)` evaluation cost.
Derivative control data is formed only for that span. Degrees up to 15 use stack
scratch space; higher degrees use scratch space proportional to the degree.
Endpoint selection and repeated-knot behavior match whole-curve differentiation.
All derivative orders use the same domain clamping, including endpoint roundoff.

Operations with an existing edge use analytic support intersections or continue
that edge locally. Trimmed-face intersections search only the supplied parameter
bounds. They do not first run an exhaustive search over an extended support.

Edge blends prepare their contact rails, circular frame and differential data once
per longitudinal parameter. Evaluations across a section reuse that preparation.
Intersection and blend evaluations share bounded caches through immutable geometry
clones. Cache keys use exact floating-point bits and cache contents do not affect
geometry identity. A four-way, 128-set cache replaces entries locally; collisions
cause recomputation, never approximate reuse. Sets allocate storage on demand.

To profile application scene reconstruction through the release WASM transport,
run from the repository root:

```sh
npm run build-shapex
node --cpu-prof scripts/profile-shapex-scene.mjs path/to/scene.cad
```

The harness uses saved sketch coordinates and excludes constraint solving,
runtime startup and UI work. It reports per-feature timings, separate display-mesh
timing, and geometry measurements for before/after comparisons. The CPU profile
includes Rust/WASM function names. Use fresh processes and the same build profile
and scene when comparing changes; verify an ordinary application load separately.

## Certified publication set

The current publisher certifies:

- arbitrary closed planar, linear-edge manifold shells, including cavities and
  genus introduced by through-holes;
- analytical BREP Boolean union, intersection, and difference, including curved
  supports, disconnected material bodies, and fully enclosed cavity shells;
- exact right cylinders, cones, spheres, and regular ring tori, with closed-form
  mass properties;
- normal extrusions of profiles bounded by exact lines and circular arcs,
  including multiple holes;
- exact inward and outward shelling of manifold analytical BREP bodies, including
  planar, cylindrical, and toroidal fillet/protrusion combinations, with optional
  removed-face openings;
- full and partial profile revolutions, retaining primitive supports where
  possible and exact revolved surfaces for other meridians;
- exact sweeps along line, circle, and regular planar NURBS rails, with a
  continuous tangent frame through inflections and closed rail seams;
- smooth rational section lofts through line, circle, and NURBS profiles, with
  exact subdivision and degree elevation for section correspondence, retaining
  planar supports and straight boundaries when their control nets permit it;
- local fillet/chamfer edge networks sharing propagation, contact trimming,
  endpoint handling, topology rewriting, history, and validation; chamfers use
  planes, cones, or exact ruled contact-rail surfaces instead of Boolean cutters;
- neutral plane drafts of planar, pull-aligned cylindrical/conical faces, and
  ruled spline faces with sections parallel to the neutral plane; spline drafts
  use an affine normal-offset law on the neutral section's pull extrusion;
- plane splits retaining either or both sides, including disconnected solids;
- face replacement by independently parameterized supports, rebuilding shared
  boundary curves and adjacent trims while retaining the other solids and face
  adjacency (periodic seam replacement and coincident adjacent faces are unsupported);
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
use oriented trims to determine material sides within one periodic chart.
Periodic intersection copies depend on domain coverage rather than surface types.
Nearest-edge classification uses the normals of both incident faces. Sewing
separates disconnected vertex fans at point contacts and reparameterizes conical
generators and circular meridians algebraically through surface poles. These operations retain exact
supports and curves. Edge blends likewise retain exact supports when trimming
across earlier fillets. Periodic
seam continuity and display bands use a consistent parameter chart, with retained
seams selecting the intended sweep interval at and beyond a half turn.

Offset, move, replace, and blend reconstruction share support continuation and a corner
projection solver. Sweep continuation extends the profile and rail while retaining
the original frame. Surface constraints locate regular intersections; tangent
contact rails and one periodic seam anchor supply the missing constraints at
singular intersections. Contact rails delimit a blend's material sector even
when a miter boundary extends beyond it. Boundary construction preserves exact
lines and circles through a single surface API. Mixed concave/convex blend
networks form concave setbacks first, then consume their smooth continuations
with the convex network, without searching construction-order permutations.
Asymmetric chamfers carry their reference faces through both stages using history.

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
Lineage is keyed by both persistent name and source revision, so an older modified
handle and a handle to the current face remain distinct. Operation history is
authoritative: transforms report motion directly, transaction publication completes only
implicit retained/deleted records, and owned results consume those records without
reclassifying geometry. Identity operations share their immutable geometry and
physical certificates.

Reconstruction carries face sources through BREP patches, sewing, and merging.
Feature-supplied correspondences take precedence; coincident Boolean arrangements
attribute their trimmed regions before publication. Edge correspondence uses those
face sources and overlapping retained curve intervals. Exact retention checks apply
only to established source/output pairs. They are conservative about representation
changes and never turn a tolerance-sized edit into a permanently unchanged revision.
Semantic creation roles identify extrusion caps and the profile segment behind
each swept face without requiring caller-supplied kernel names.
