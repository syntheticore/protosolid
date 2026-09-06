# ProtoSolid geometry-kernel boundary

ProtoSolid selects one `modeling` backend during startup. Application geometry
objects call that backend for three-dimensional construction, Boolean operations,
rigid transforms, and tessellation. The setting is intentionally restart-bound:
one document evaluation never contains objects from two kernels.

- `occ.js` adapts the existing OpenCascade.js implementation.
- `shapex.js` adapts the public API exported by `js/shapexJS`.

`js/shapexJS` is a separate, self-contained JavaScript library. Its private
transport crate depends directly on Shapex, while its public object API
parallels Shapex's normal Rust API using JavaScript camel case. ProtoSolid uses
the same public API as any other ShapexJS consumer.

The private transport is generated with `npm run build-shapex` into
`js/shapexJS/pkg`. Generated JavaScript and WebAssembly are not source files and
are ignored by Git. The Shapex Rust crate itself contains no binding or
WebAssembly code.

PlaneGCS and the current exact 2D sketch implementation still use the existing
OpenCascade.js compatibility object. That object is not used for 3D modeling in
Shapex mode. Migrating the independent 2D curve utilities does not require a
change to this 3D backend contract.

Backends publish explicit capabilities. Shapex supports profile extrusion,
full and partial revolutions, rail sweeps, smooth section lofts with optional alignment,
Booleans across multiple solids, fillet, shell, chamfer, draft, plane splitting,
rigid transforms, properties, topology resolution, and entity tessellation.

Fillet and chamfer share a local edge-network rewrite, including closed rims,
concave edges, tangent continuations, and corner transitions. Chamfer retains
planes and cones for elementary supports and exact ruled patches otherwise;
it does not construct cutter solids or call Boolean operations.
Its Propagation radio bar switches between following tangent edges and using only
the selection. Connected tangent chamfers are constructed as one contour; saved
chamfers created before this option retain selection-only behavior.
Sweep accepts a complete line, arc, circle, or regular planar spline rail.
The section retains its initial placement and turns continuously with the rail;
straight and circular rails reuse extrusion and revolution, while spline rails
retain exact swept surfaces. Closed spline rails must have matching seam tangents.
Partial rail bounds and profiles with holes remain unsupported, as in the OCC path.
Draft supports planar faces and cylindrical or conical faces aligned with the pull
direction. Loft accepts line, circular, and NURBS sections, reconciles their curve
counts and degrees, and passes through every section; profiles with holes remain
unsupported, as in the OCC application path. Revolution profiles must lie on one
side of an axis in the sketch plane. Geometry outside the supported set fails
with an error shown in the feature UI; it never executes in the other kernel.

After changing the Rust implementation or transport API, regenerate the ignored
WASM package with `npm run build-shapex` before using those changes in the app.
