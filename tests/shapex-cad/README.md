# Shapex modeling cases awaiting review

These three `.cad` files are the tests. Open them in ProtoSolid with the Shapex
backend selected. The marker is at the end, the setup sketches are hidden, and
only the final feature fails. The last successful body remains visible; select
the red feature to inspect or edit its settings. Move the marker back to inspect
earlier steps. No kernel or application implementation was changed.

These are reproducible modeling failures, not yet approved requirements. Decide
whether each operation should work before turning it into a kernel regression
test. The documents contain real feature values and saved references, with no
stored error or artificial failure flag.

| File | Working setup | Final operation | Observed error |
| --- | --- | --- | --- |
| `001-filleted-cylinder-shell.cad` | Radius 10 mm circle; extrusion 20 mm; top rim fillet radius 2 mm | Shell inward by 1 mm, opening the top face, rounded corners | `Shapex shell failed: offsets of adjacent shell faces do not intersect` |
| `002-eccentric-loft-shell.cad` | Radius 10 mm circle at z=0; second sketch on a plane at z=20 mm, radius 6 mm, center displaced 4 mm; untwisted loft | Shell inward by 1 mm, opening the upper cap, rounded corners | `Shapex shell failed: offset shell faces do not meet at a common vertex (5#0, 3 incident edges)` |
| `003-box-perimeter-chamfer.cad` | 30 x 30 mm square; extrusion 20 mm | Equal-distance chamfer of all four top perimeter edges, distance 2 mm, tangent propagation | `Shapex chamfer failed: selected blend edges do not define a supported local corner transition` |

## Repeat the checks

From the repository root, using the existing generated WASM packages:

```sh
node tests/shapex-cad/check.mjs
```

This evaluates the actual saved files and exits **1** while modeling failures
remain. It does not build or launch the app. These cases are deliberately outside
the default `npm test` suite while they await review.

```sh
node tests/shapex-cad/check.mjs --verify-failures
```

This exits **0** only when every feature is complete, every setup feature builds,
and the final feature alone reports a Shapex modeling error with intact saved
references. A newly passing case fails this verification so it can be reviewed
and removed from the failing library.

`runtime.mjs` uses the app's serializer, timeline, feature classes, reference
resolution, repair, and viewport face tessellation. It also loads the existing
OpenCascade 2D sketch utilities and the exact PlaneGCS solver implementation from
`composables/use-oc.js`; all 3D operations use the production Shapex backend.
Checks use the currently generated `js/shapexJS/pkg` WASM, the same package the app
loads, rather than recompiling Rust. Validation was headless, through the feature
evaluator, including save/load round trips.

For future batches, try varied operations, discard passing or incomplete cases,
exclude broken saved references and invalid seam selections, and retain only
documents whose setup builds after reloading the serialized `.cad`. Keep one
final failure per file so it can be judged independently.
