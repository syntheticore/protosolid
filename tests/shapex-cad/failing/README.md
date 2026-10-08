# Modeling failures awaiting review: 032–041

These ten `.cad` files were tested after recompiling Shapex from the current
Rust source. Each setup succeeds and only its final feature fails. Open a file
with Shapex selected to inspect the red timeline feature and decide whether the
operation should be supported. The files contain actual feature values and saved
references, with no stored errors or forced failure flags. No kernel changes
were made.

| Case | Setup and final operation | Observed failure |
| --- | --- | --- |
| [032](032-hemisphere-revolution.cad) | Radius 10 mm quarter-disk profile in the XZ plane, bounded by the Z axis and a radial line; revolve 360 degrees around Z to form a hemisphere | Surface projection reaches a singular parameterization |
| [033](033-torus-up-to-face-pocket.cad) | Torus with major radius 12 mm and tube radius 3 mm; cut a radius 1 mm circular pocket from z=0 up to the upper curved torus surface | Cut produces a BREP edge with one incident face use |
| [034](034-spline-pipe-end-tilt.cad) | Radius 2 mm circular profile swept along a gentle S-shaped cubic spline; tilt the upper planar end 5 degrees about Y | Surface intersection correction does not converge |
| [035](035-spline-square-sweep-end-fillet.cad) | 4 x 4 mm square swept along the same spline; radius 0.3 mm fillet around all four upper rim edges | Surface intersection correction does not converge |
| [036](036-spline-square-sweep-side-offset.cad) | Same square sweep; offset all four side faces outward by 0.3 mm | Surface intersection correction does not converge |
| [037](037-spline-square-sweep-open-end-shell.cad) | Same unblended square sweep; inward 0.4 mm shell with only the outer upper cap removed | Surface intersection correction does not converge |
| [038](038-spline-sector-draft.cad) | 270 degree sector with a spline-shaped outer wall; draft the cylindrical inner and curved outer walls by 3 degrees relative to the lower horizontal face | Draft of this support is not implemented |
| [039](039-bulging-square-loft-rim-fillet.cad) | Three square sections at z=0, 10, 20 mm, widths 20, 24, 16 mm and X offsets 0, 2, 1 mm; fillet the four upper rim edges at radius 0.5 mm | NURBS surface parameters lie outside the natural domain |
| [040](040-bulging-square-loft-side-offset.cad) | Same three-section square loft; offset one curved side outward by 0.5 mm | Certified surface intersection exceeds its subdivision budget |
| [041](041-l-block-asymmetric-all-edge-chamfer.cad) | Extruded L block, height 20 mm; chamfer all eighteen edges with 0.5 and 0.75 mm setbacks | Mixed convex/concave corner requires a setback patch |

Case 032 keeps its sketch visible because the failed revolution is the first
solid-creating feature. An independent OpenCascade revolution of that exact
profile is valid and has the hemisphere volume 2094.3951023931954 mm³. The other
files hide their setup sketches and retain the successful body in the viewport.

Cases 034–037 share one rail: cubic control points (X,Z) = (0,0), (0,8),
(10,12), (10,20) mm, in the XZ plane. The square cases compare blending, face
offset, and shelling on that same geometry. Cases 039–040 compare blending and
face offset on a three-section loft rather than a sweep.

Case 037 is the only shell test. Its opening is the outermost upper cap at
z=20 mm, not a pocket floor. There are no fillets anywhere in its setup. The
rail ends perpendicular to the cap, and all four cap-to-wall boundaries meet
at right angles. Boundary samples on each neighboring face have an absolute
normal dot product of zero against the cap normal. These observations document
the fixture review. Shell exclusions are handled when choosing and reviewing
cases. Cases 001, 004, 010, 012, and 031 remain removed.

From the repository root:

```sh
node tests/shapex-cad/check.mjs
node tests/shapex-cad/failing/check.mjs --verify-failures
```

Both commands rebuild Shapex before initializing WASM. The first checks the
26 passing regressions. The second checks these saved files for complete setup,
intact references with no partial loss, and exactly one final Shapex modeling
error. Blend selections must be semantic edges joining two faces. Run the second command
without `--verify-failures` to report failures normally and exit 1 while they
remain. A newly passing file fails `--verify-failures` so it can be reviewed and
promoted to the passing catalogue.
