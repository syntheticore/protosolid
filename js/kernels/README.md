# ProtoSolid geometry-kernel boundary

ProtoSolid selects one `modeling` backend during startup. Application geometry
objects call that backend for three-dimensional construction, Boolean operations,
rigid transforms, and tessellation. The setting is intentionally restart-bound:
one document evaluation never contains objects from two kernels.

- `occ.js` adapts the existing OpenCascade.js implementation.
- `shapex.js` adapts the high-level classes exported by the external
  `rust/lib/shapex-wasm` crate.

The bridge is generated with `npm run build-shapex` into
`public/kernels/shapex`. Generated JavaScript and WebAssembly are not source
files and are ignored by Git. The Shapex Rust crate itself contains no binding
or WebAssembly code.

PlaneGCS and the current exact 2D sketch implementation still use the existing
OpenCascade.js compatibility object. That object is not used for 3D modeling in
Shapex mode. Migrating the independent 2D curve utilities does not require a
change to this 3D backend contract.

Backends publish explicit capabilities. Shapex currently supports profile
extrusion with lines, arcs, circles and holes, certified single-solid Booleans,
rigid transforms, properties, topology resolution, and entity tessellation.
Features outside that certified set fail with a structured capability error;
they never silently execute in the other kernel.
