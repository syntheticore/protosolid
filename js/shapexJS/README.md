# ShapexJS

ShapexJS is a self-contained JavaScript package for the Shapex kernel. Its
private Rust transport lives in `rust/` and depends directly on the `shapex`
crate. It has no application dependencies.

The public modeling API follows Shapex's Rust object model, argument order,
return shapes, ownership semantics, and error boundary. Names use ordinary
JavaScript camel case.

## Rust and JavaScript

Rust:

```rust
let tolerance = ModelTolerance::default();
let outer = Wire2::new([
    TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(20.0, 0.0))?,
    TrimmedCurve2::line(Point2::new(20.0, 0.0), Point2::new(20.0, 10.0))?,
    TrimmedCurve2::line(Point2::new(20.0, 10.0), Point2::new(0.0, 10.0))?,
    TrimmedCurve2::line(Point2::new(0.0, 10.0), Point2::new(0.0, 0.0))?,
], tolerance)?;
let profile = Profile::new(outer, [], tolerance)?;
let result = profile.extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0))?;
println!("{}", result.volume());
```

JavaScript:

```js
import initShapex from './js/shapexJS/index.js'

const sx = await initShapex()
const tolerance = sx.ModelTolerance.default()
const outer = sx.Wire2.new([
  sx.TrimmedCurve2.line(sx.Point2.new(0, 0), sx.Point2.new(20, 0)),
  sx.TrimmedCurve2.line(sx.Point2.new(20, 0), sx.Point2.new(20, 10)),
  sx.TrimmedCurve2.line(sx.Point2.new(20, 10), sx.Point2.new(0, 10)),
  sx.TrimmedCurve2.line(sx.Point2.new(0, 10), sx.Point2.new(0, 0)),
], tolerance)
const profile = sx.Profile.new(outer, [], tolerance)
const result = profile.extrude(sx.Plane3.xy(), sx.Vec3.new(0, 0, 5))
console.log(result.volume())
```

Values such as curves, wires, profiles, planes, and transforms are immutable
and reusable. Native topology snapshots (`Face`, `Edge`, `Vertex`, `Solid`, and
`Compound`) can be passed directly to subsequent operations from the same
ShapexJS instance.

## Curves and planar geometry

Parameters are native geometric parameters: line parameters, circle angles in
radians, or NURBS knot parameters. `curve.domain()` describes the basis;
`TrimmedCurve2` adds a finite oriented `ParameterRange`. Decreasing ranges reverse
traversal without mutating the basis. Derivatives are with respect to the basis
parameter, so an oriented tangent also accounts for the range direction.

```js
const curve = sx.TrimmedCurve2.arcThrough(
  sx.Point2.new(0, 0), sx.Point2.new(2, 2), sx.Point2.new(4, 0),
)
const nearest = curve.closestPoint(sx.Point2.new(3, 4))
const pieces = curve.splitAt([nearest.parameter])
const display = curve.tessellate(sx.TessellationOptions.new(0.01, 0.1, 24))
```

Basis `closestPoint` searches the natural domain; a trimmed curve includes its
endpoints and searches only its retained interval. Length is nonnegative in both
directions. Intersections distinguish crossing/tangent point contacts from
overlapping intervals, with parameters on both inputs. `selfIntersections`
reports distinct parameter pairs, excluding the ordinary closed-curve seam.

`offset(distance)` on a curve or path measures to the left of traversal. Lines
and circles retain analytic representations; general offsets retain the exact
parallel-curve evaluator. `approximateNurbs(tolerance)` is an explicit adaptive
cubic conversion returning `{ curve, estimatedError }`. Its error is a numerical
validation estimate, not a certificate for arbitrary procedural geometry.
Modeling does not implicitly substitute that approximation for an offset.
`Profile.approximateRational(tolerance)` retains line, circle, and NURBS sections
exactly and returns `{ profile, estimatedError }` for an explicit conversion of
other curves. It supports consumers such as rational section lofting. Exact
derived curves can be extruded, revolved, and swept directly.

`Path2` represents connected open or closed sequences. `path.offset(distance,
{ join: 'round' | 'miter' | 'bevel', miterLimit: 4 })` returns a path plus an input
segment index for each retained segment; generated joins have a `null` source.
`join` can also be an array with one style per corner in traversal order;
`miterLimit: Infinity` explicitly retains uncapped miters. Offsets reject collapsed
or reversed trims. A path can still cross itself: `validateSimple()` explicitly
requires no self-intersections, overlaps, or contacts beyond adjacent endpoints.
Closed paths also answer `signedArea()` (positive counter-clockwise) and
`classifyPoint(point)` from the exact curves without requiring simplicity, so a
boundary walk that retraces a bridge between loops still has an orientation.
`Wire2` remains a validated simple closed boundary. `Profile.offset(distance)`
operates on filled regions: positive distances expand, negative distances erode;
results can contain several profiles or be empty. `classifyPoint` on wires and
profiles returns `inside`, `outside`, or `boundary`, with holes respected.
Filled-region offsets use Euclidean distance. Miter and bevel joins are explicit
path construction choices, separate from that region operation.

`RigidTransform2` supports immutable planar rotations and translations. Transforming
a circular curve can change its canonical angular origin; the returned trim
describes the resulting basis. Splitting and reversal preserve source parameters.

Planes supply `pointAt`, `vectorAt`, and `coordinates` for their coordinate charts.
`liftCurve` embeds a trimmed 2D curve in space; `expressCurve` requires a coplanarity
certificate. `projectCurve` performs orthogonal projection, returning either
`{ kind: 'curve', curve }` or `{ kind: 'point', point }`. Noncircular images such as
ellipses retain exact evaluators. `edge.trimmedCurve()` exposes evaluable spatial
geometry and its actual trim, including nonanalytic edges, without tessellation.

Numerical queries use `ModelTolerance`. Failures throw `ShapexError`, whose
`diagnostics` retain native kinds, operation context, entity identities, and
accuracy information. Empty intersections and empty eroded regions are valid
results. These APIs do not depend on a browser, Three.js, Vue, a sketch solver,
or ProtoSolid document conventions.

## Build

From this directory:

```sh
npm run build
```

This runs `wasm-pack` against `rust/` and writes the private generated transport
to `pkg/`. Generated files and Rust build output are ignored. `initShapex()`
loads that colocated build by default; callers may also provide custom module
and WASM inputs.
