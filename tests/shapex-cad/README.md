# Shapex saved-document regressions

This directory contains saved `.cad` regressions.
All files exercise real modeling workflows and saved references.
Every feature in a passing regression must complete without an error or partial
reference loss.

| File | Operation |
| --- | --- |
| `002-eccentric-loft-shell.cad` | Eccentric circular loft, 1 mm inward shell with the upper cap open |
| `003-box-perimeter-chamfer.cad` | Box, 2 mm chamfer around the top perimeter |
| `005-cross-drilled-cylinder-chamfer.cad` | Two transverse bore cuts, 1 mm chamfer on both curved intersections |
| `006-l-block-perimeter-chamfer.cad` | L block, 1 mm top perimeter chamfer through a concave corner |
| `007-box-bore-perimeter-chamfer.cad` | Box with a bore, outer top perimeter and bore rim chamfered together |
| `008-box-asymmetric-perimeter-chamfer.cad` | Box, top perimeter chamfer with 1 and 2 mm setbacks |
| `009-box-angle-perimeter-chamfer.cad` | Box, top perimeter chamfer at 1 mm and 30 degrees |
| `011-chamfered-cylinder-shell.cad` | Chamfered cylinder, 1 mm inward shell with the top open |
| `013-eccentric-loft-rim-fillet.cad` | Eccentric circular loft, 1 mm upper rim fillet |
| `014-eccentric-loft-rim-chamfer.cad` | Eccentric circular loft, 1 mm upper rim chamfer |
| `015-eccentric-loft-side-offset.cad` | Eccentric circular loft, curved side offset outward by 1 mm |
| `016-three-section-loft-shell.cad` | Three circular sections, inward shell with the upper cap open |
| `017-three-section-loft-fillet.cad` | Three circular sections, 1 mm upper rim fillet |
| `018-filleted-box-tilted-cap-move.cad` | Box with top perimeter fillets, upper cap rotated 5 degrees about X |
| `019-revolved-sector-chamfer.cad` | 270 degree rectangular sector, 1 mm radial end perimeter chamfer |
| `020-revolved-sector-end-fillet.cad` | 270 degree rectangular sector, 2 mm radial end perimeter fillet |
| `021-shelled-sector-rim-blends-section.cad` | Rounded sector, open shell, 0.3 mm rim fillet and 0.2 mm rim chamfer; sketch section views |
| `022-cross-bore-move.cad` | Transverse cylindrical bore translated 1 mm along X |
| `023-eccentric-loft-draft.cad` | Eccentric circular loft, 3 degree draft relative to the lower cap |
| `024-tapered-square-loft-fillet.cad` | Tapered square loft, 0.5 mm fillet on all twelve edges |
| `025-tapered-square-loft-chamfer.cad` | Tapered square loft, 1 mm upper perimeter chamfer |
| `026-spline-prism-draft.cad` | Spline-sided prism, 3 degree draft on all four sides |
| `027-spline-sector-end-fillet.cad` | 270 degree spline-sided sector, 0.5 mm radial end perimeter fillet |
| `028-l-block-all-edge-fillet.cad` | L block, 1 mm fillet on all eighteen edges |
| `029-tapered-square-loft-side-offset.cad` | Tapered square loft, one side offset outward by 0.5 mm |
| `030-intersecting-bores-rim-fillet.cad` | Box with overlapping bores, 0.5 mm outer top perimeter and bore rim fillet |
| `032-hemisphere-revolution.cad` | Hemisphere revolution through a singular pole |
| `033-torus-up-to-face-pocket.cad` | Circular pocket terminated on a torus face |
| `034-spline-pipe-end-tilt.cad` | Spline pipe with a tilted end face |
| `035-spline-square-sweep-end-fillet.cad` | Square spline sweep with an end perimeter fillet |
| `036-spline-square-sweep-side-offset.cad` | Square spline sweep with an offset side |
| `037-spline-square-sweep-open-end-shell.cad` | Square spline sweep shelled with one end open |
| `038-box-orthogonal-equal-bores.cad` | Box with equal perpendicular bores |
| `039-bulging-square-loft-rim-fillet.cad` | Bulging square loft with an upper rim fillet |
| `040-bulging-square-loft-side-offset.cad` | Bulging square loft with an offset side |
| `041-l-block-asymmetric-all-edge-chamfer.cad` | L block with asymmetric chamfers on all edges |
| `042-reverse-revolution-between-spline-faces.cad` | Reverse circular revolution between two spline faces across a trim gap |
| `044-joined-revolution-upper-rim-fillet.cad` | Joined revolution between spline faces with a 1 mm upper rim fillet |
| `045-cut-rounded-shell-rim-fillets.cad` | Cut rounded shell with lower corner fillets and 1 mm fillets on both complete upper rims; hidden circle seams must not subdivide the selection or result |
| `046-chamfered-box-staged-fillets.cad` | Box with a 7 mm top chamfer, then successive fillets: 8 mm vertical corners and 4.5 mm ridges ending on four-face chamfer corners, a top edge, a chamfer base and a corner fillet's bottom rim |

From the repository root:

```sh
node tests/shapex-cad/check.mjs
```

The checker rebuilds Shapex WASM from the current Rust source, loads every
document in this directory, and exits **1** if any feature
fails or loses a reference. It verifies
that blend selections are semantic edges joining two faces and checks the
loft face counts. It continues after a failed document to report the whole suite.
It does not build or launch the application.

`runtime.mjs` uses the production serializer, timeline, feature classes, reference
resolution, and viewport face tessellation. Shapex supplies sketch geometry and
solid modeling; PlaneGCS solves sketch constraints.

The fixtures use persistent topology selections and producer manifests.
