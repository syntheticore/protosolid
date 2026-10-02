# Shapex modeling cases awaiting review

These twenty `.cad` files are the tests. Open them in ProtoSolid with the Shapex
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
| `004-multiple-solids-filleted-shell.cad` | Two separate cylinders, both top rims filleted at radius 2 mm | open both top caps in one shell. | Shapex shell failed: offsets of adjacent shell faces do not intersect |
| `005-cross-drilled-cylinder-chamfer.cad` | Transversely bored cylinder | chamfer both curved bore intersections at distance 1 mm. | Shapex chamfer failed: blend corner misses its incident contact rail |
| `006-l-block-perimeter-chamfer.cad` | Extruded L profile with a concave corner | equal 1 mm chamfer around the top perimeter. | Shapex chamfer failed: selected blend edges do not define a supported local corner transition |
| `007-box-bore-perimeter-chamfer.cad` | Box with a radius 5 mm through bore | chamfer the outer top perimeter and bore rim together. | Shapex chamfer failed: selected blend edges do not define a supported local corner transition |
| `008-box-asymmetric-perimeter-chamfer.cad` | Box extrusion | all four top edges chamfered with setbacks 1 and 2 mm. | Shapex chamfer failed: selected blend edges do not define a supported local corner transition |
| `009-box-angle-perimeter-chamfer.cad` | Box extrusion | all four top edges chamfered at distance 1 mm and angle 30 degrees. | Shapex chamfer failed: selected blend edges do not define a supported local corner transition |
| `010-filleted-box-shell.cad` | Extruded box, four top edges filleted at radius 2 mm | inward shell with the top face removed. | Shapex shell failed: offsets of adjacent shell faces do not intersect |
| `011-chamfered-cylinder-shell.cad` | Cylinder with equal 2 mm top rim chamfer | shell inward by 1 mm with the top open. | Shapex shell failed: offset shell faces do not meet at a common vertex (6#0, 2 incident edges) |
| `012-filleted-cylinder-outward-shell.cad` | Cylinder with radius 2 mm top fillet | outward shell, opening its top. | Shapex shell failed: offset shell faces do not meet at a common vertex (6#0, 2 incident edges) |
| `013-eccentric-loft-rim-fillet.cad` | Eccentric circular loft | radius 1 mm fillet around its upper cap. | Shapex fillet failed: local surface-intersection correction did not converge |
| `014-eccentric-loft-rim-chamfer.cad` | Eccentric circular loft | equal 1 mm chamfer around its upper cap. | Shapex chamfer failed: local surface-intersection correction did not converge |
| `015-eccentric-loft-side-offset.cad` | Eccentric circular loft | offset its curved side outward by 1 mm. | Shapex face offset failed: offset shell faces do not meet at a common vertex (1#0, 3 incident edges) |
| `016-three-section-loft-shell.cad` | Three circular loft sections at z=0,10,20 with radii 8,11,6 and middle center offset 3 | inward shell with upper cap open. | Shapex shell failed: offset shell faces do not meet at a common vertex (4#0, 3 incident edges) |
| `017-three-section-loft-fillet.cad` | Three circular loft sections | radius 1 mm fillet around the upper cap. | Shapex fillet failed: local surface-intersection correction did not converge |
| `018-filleted-box-tilted-cap-move.cad` | Box with a radius 2 mm top perimeter fillet | rotate its planar upper cap 5 degrees about X. | Shapex face movement failed: offset shell faces do not meet at a common vertex (8#0, 4 incident edges) |
| `019-revolved-sector-chamfer.cad` | 270 degree rectangular sector | equal 1 mm chamfer on the perimeter of its radial end cap. | Shapex chamfer failed: selected blend edges do not define a supported local corner transition |
| `020-revolved-sector-end-fillet.cad` | 270 degree revolution of a radial 8 x 8 mm rectangle | radius 2 mm fillet around its radial end cap. | Topology validation rejects the fillet: edge endpoints and face p-curves disagree with their vertices. |

Case 004 selects the top cap of **each of two distinct solids** for one shell
operation; both preceding rim fillets build. This reproduces a failure also seen
on a single filleted cylinder, so it does not establish a separate multi-solid
bug. Plain multi-solid shell combinations (boxes, cylinders, mixed shapes,
opposite openings, a single extrusion of two profiles, and a split box) passed
and were discarded. Failed shell cases can therefore be reviewed without
assuming all multi-solid shell operations are broken.

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
