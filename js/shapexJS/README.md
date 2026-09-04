# ShapexJS

ShapexJS is a self-contained JavaScript package for the Shapex kernel. Its
private Rust transport lives in `rust/` and depends directly on the `shapex`
crate. It does not import or depend on ProtoSolid's existing WASM adapter.

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

## Build

From this directory:

```sh
npm run build
```

This runs `wasm-pack` against `rust/` and writes the private generated transport
to `pkg/`. Generated files and Rust build output are ignored. `initShapex()`
loads that colocated build by default; callers may also provide custom module
and WASM inputs.
