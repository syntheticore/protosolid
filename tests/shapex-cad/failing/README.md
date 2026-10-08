# Modeling failures awaiting review

These nine `.cad` documents were tested against WASM rebuilt from the current
Rust source. Open a file with Shapex selected: the marker is at the end, setup
sketches are hidden, and the last feature fails while the preceding body remains
visible. The documents store actual feature settings and topology references;
they contain no saved errors or artificial failure flags.

| Case | Successful setup and attempted final operation | Observed failure |
| --- | --- | --- |
| [022](022-cross-bore-move.cad) | Radius 10 mm cylinder, height 20 mm, transverse radius 3 mm bore at z=10; translate the bore face 1 mm along X | Offset faces do not meet at a common vertex |
| [023](023-eccentric-loft-draft.cad) | Circular loft from radius 10 to radius 6, height 20 mm and upper center displaced 4 mm; draft its curved side by 3 degrees relative to the lower cap | Draft of this face support is not implemented |
| [024](024-tapered-square-loft-fillet.cad) | Loft from a 24 mm square to a 16 mm square, height 20 mm and upper center displaced 2 mm; fillet all twelve edges at radius 0.5 mm | Surface intersection correction does not converge |
| [025](025-tapered-square-loft-chamfer.cad) | Same tapered square loft; equal 1 mm chamfer around the four upper rim edges | Curve intersection correction does not converge |
| [026](026-spline-prism-draft.cad) | Extrude a 30 mm square with one side replaced by a shallow cubic spline, height 20 mm; draft all four side faces by 3 degrees relative to the bottom | Draft of this face support is not implemented |
| [027](027-spline-sector-end-fillet.cad) | Revolve a radial 8 x 8 mm profile through 270 degrees, with its outer side replaced by a spline bulging outward 1.5 mm; radius 0.5 mm fillet on one radial end perimeter | Curve intersection correction does not converge |
| [028](028-l-block-all-edge-fillet.cad) | Extruded L profile, height 20 mm; radius 1 mm fillet on all eighteen edges, including the concave vertical corner | Mixed convex/concave three-edge blends require a setback corner patch |
| [029](029-tapered-square-loft-side-offset.cad) | Same tapered square loft; offset one side outward by 0.5 mm | Offset faces do not meet at a common vertex |
| [030](030-intersecting-bores-rim-fillet.cad) | 30 x 30 x 20 mm box with two overlapping radius 5 mm through bores, centers 6 mm apart; radius 0.5 mm fillet on the outer top perimeter and the merged bore rim | Generated planar wire self-intersects |

Cases 024, 025, and 029 reuse one shape to compare three distinct operations.
Cases 023 and 026 exercise draft on a loft surface and an extruded spline
surface respectively. These are review candidates, not approved requirements.

The earlier passing documents remain in the parent directory. From the
repository root, run:

```sh
node tests/shapex-cad/check.mjs --verify-failures
```

The checker rebuilds Shapex before loading the actual saved files. It requires
the earlier regressions to pass and each new file to have a complete, successful
setup, intact references, and exactly one final Shapex modeling error. Blend
edges must be semantic edges joining two faces; shell openings bordering
fillets are excluded. The regular checker command without `--verify-failures`
reports the failures and exits 1.
