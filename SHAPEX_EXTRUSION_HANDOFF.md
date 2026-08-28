# Shapex extrusion parity handoff

## Completed in this checkpoint

- Shapex is the default ProtoSolid kernel and loads through the external WASM bridge.
- Sketch/profile selection no longer calls the removed `geom()` API or creates invalid line arrays.
- Lines, circles, mixed line/arc regions, split-circle regions, multiple overlapping circles, and profiles with holes reach the Shapex extrusion path.
- Exact adjacent curved extrusions sew across their shared face using Euler topology operations.
- A smaller extrusion attached to a larger planar face now uses exact face-patch sewing: the attached body's base loop becomes an inner loop of the host face, preserving the analytic cylinder.
- Face workplanes are converted to rigid orthonormal frames before Shapex extrusion, so scaled BREP parameter axes cannot distort circles or extrusion directions.
- Exact face-patch sewing now reparameterizes the surviving planar face and its line/circle p-curves after transferring the attached loop. This covers side faces whose original sweep axes and trim ranges are scaled or reversed.
- The physical certifier recognizes a right prism with attached analytic prismatic bosses and combines exact component mass properties with the parallel-axis theorem. Shared cap areas come from the exact profile wire, so line/arc/circle/NURBS bosses use the same path.
- Curved through-cuts and constant-section genus-one Boolean results reconstruct as Euler-built profile extrusions.
- Curved Booleans never fall back to faceting. Parallel cylinders, mixed line/circle/arc profile arrangements, coaxial prism layers, and a cylinder contained in a linear prism have exact constructions; every other curved case returns an explicit unsupported-analytic-construction error.
- General coaxial line/circle/arc prism Join, Cut, and Intersect operations split their exact curves analytically, assemble exact profile wires, extrude exact axial layers, and sew those layers with Euler operators.
- Blind cylindrical pockets, including the normal UI direction from a face into a body, retain exact plane/cylinder geometry and exact mass properties. Tangential solid contact has explicit solid-set semantics: Cut is unchanged and Intersect is empty.
- Positively separated analytic bodies are detected using closed-form circle bounds and positive-weight NURBS control hulls, allowing disjoint spline-based extrusion compounds without an approximate Boolean.
- Spline sketch segments cross the bridge as their clamped NURBS degree, control points, positive weights, knot vector, and exact trim range.
- Exact NURBS prisms are physically certified by integrating their rational boundary in parameter space. The integration is split at exact knot spans and is scale-aware; no tessellated geometry participates.
- Wire self-intersection, profile-boundary intersection, and hole containment no longer use sampled polylines. Line and circle pairs are intersected analytically. Rational Bézier extraction, Bernstein implicit root isolation, positive-weight control-hull subdivision, and analytic Newton refinement now cover NURBS/line, NURBS/circle, and NURBS/NURBS intersections. Ambiguous cases fail closed.
- Coaxial NURBS prisms participate in exact layered Join, Cut, and Intersect operations. Coplanar Euler face merges reparameterize NURBS p-curves by exact affine projection while preserving degree, weights, knots, and trim ranges.
- Blind NURBS pockets and attached NURBS bosses have exact prismatic mass properties. Their volumes, centroids, inertia, and exposed surface areas are computed from analytic profiles and certified prisms, never from display meshes.
- Single NURBS segments are certified simple by exact rational Bézier subdivision into projection-monotone pieces followed by exact pairwise self-intersection checks. This admits folded simple splines while rejecting self-intersections; a nonregular curve that cannot be certified at model tolerance still fails closed.
- Shapex's frontend adapter no longer exposes the legacy sampled revolution, loft, shell, fillet, or split paths. Those features remain outside the current extrusion-only scope until they have analytical implementations.
- Display tessellation keeps each exact face's analytical surface normals. It does not infer smoothness from topology history, so planar creases stay sharp while cylinders and NURBS faces shade smoothly.

## Verification at this checkpoint

- All 45 Shapex kernel tests pass, including exact NURBS/line, NURBS/circle, and NURBS/NURBS intersections; a folded simple NURBS boundary; exact NURBS prism Booleans; an inward blind NURBS pocket; and an attached NURBS boss.
- The Shapex WASM bridge was rebuilt successfully with `npm run build-shapex`.
- Browser checks passed for rectangle extrusion, annular extrusion, split-circle half extrusion, overlapping-circle join/intersection, a circular through-cut, a smooth exact NURBS spline extrusion, two disjoint selected profiles, reverse-side extrusion, a blind circular pocket, a blind NURBS pocket, and an attached NURBS boss.
- A circular boss was sketched on a block side face, previewed, committed, and visually inspected in the browser. It remains an analytic cylinder with a smooth tessellation.
- Shapex and OCC multi-circle, mixed line/NURBS, and blind NURBS pocket results were compared at the same view and tessellation settings and are visually equivalent. Shapex keeps planar seams sharp and shades its analytic NURBS walls smoothly.
- The Shapex WASM check, JavaScript syntax checks, release bridge build, and diff hygiene checks pass. The repository's Node test command currently stops before its tests because committed `js/preferences.js` accesses `window` at module import time; this is outside the current Shapex diff.

## Remaining before declaring full extrusion parity

1. Add a browser fixture in which an intersecting NURBS prism participates in each of Join, Cut, and Intersect. The same paths are kernel-tested analytically; this is remaining UI coverage rather than missing kernel construction.

## Useful commands

```sh
cargo test --manifest-path rust/lib/shapex/Cargo.toml
cargo check --manifest-path rust/lib/shapex-wasm/Cargo.toml
node --check js/core/geom3d.js js/kernels/shapex.js js/core/expression.js
npm test -- --runInBand
npm run build-shapex
git diff --check
```

The active scope remains extrusion parity only; revolutions, shelling/Offset, fillets, loft, and the broader kernel abstraction work should wait.
