# Shapex extrusion parity handoff

## Completed in this checkpoint

- Shapex is the default ProtoSolid kernel and loads through the external WASM bridge.
- Sketch/profile selection no longer calls the removed `geom()` API or creates invalid line arrays.
- Lines, circles, mixed line/arc regions, split-circle regions, multiple overlapping circles, and profiles with holes reach the Shapex extrusion path.
- Exact adjacent curved extrusions sew across their shared face using Euler topology operations.
- A smaller extrusion attached to a larger planar face now uses exact face-patch sewing: the attached body's base loop becomes an inner loop of the host face, preserving the analytic cylinder.
- Face workplanes are converted to rigid orthonormal frames before Shapex extrusion, so scaled BREP parameter axes cannot distort circles or extrusion directions.
- Exact face-patch sewing now reparameterizes the surviving planar face and its line/circle p-curves after transferring the attached loop. This covers side faces whose original sweep axes and trim ranges are scaled or reversed.
- The physical certifier recognizes a right prism with attached cylindrical bosses and combines exact component mass properties with the parallel-axis theorem.
- Curved through-cuts and constant-section genus-one Boolean results reconstruct as Euler-built profile extrusions.
- Curved Booleans never fall back to faceting. Parallel cylinders, mixed line/circle/arc profile arrangements, coaxial prism layers, and a cylinder contained in a linear prism have exact constructions; every other curved case returns an explicit unsupported-analytic-construction error.
- General coaxial line/circle/arc prism Join, Cut, and Intersect operations split their exact curves analytically, assemble exact profile wires, extrude exact axial layers, and sew those layers with Euler operators.
- Blind cylindrical pockets, including the normal UI direction from a face into a body, retain exact plane/cylinder geometry and exact mass properties. Tangential solid contact has explicit solid-set semantics: Cut is unchanged and Intersect is empty.
- Positively separated analytic bodies are detected using closed-form circle bounds and positive-weight NURBS control hulls, allowing disjoint spline-based extrusion compounds without an approximate Boolean.
- Spline sketch segments cross the bridge as their clamped NURBS degree, control points, positive weights, knot vector, and exact trim range.
- Exact NURBS prisms are physically certified by integrating their rational boundary in parameter space. The integration is split at exact knot spans and is scale-aware; no tessellated geometry participates.
- Wire self-intersection, profile-boundary intersection, and hole containment no longer use sampled polylines. Line and circle pairs are intersected analytically. NURBS control-hull/monotonicity proofs certify the supported spline cases and ambiguous cases fail closed.
- Shapex's frontend adapter no longer exposes the legacy sampled revolution, loft, shell, fillet, or split paths. Those features remain outside the current extrusion-only scope until they have analytical implementations.
- Display tessellation keeps each exact face's analytical surface normals. It does not infer smoothness from topology history, so planar creases stay sharp while cylinders and NURBS faces shade smoothly.

## Verification at this checkpoint

- All 41 Shapex kernel tests pass, including curved difference/intersection, adjacent curved union, general mixed line/circle arrangement, exact partial-height circular boss union, an inward blind cylindrical pocket, an exact NURBS prism, and a concentric circular-hole prism.
- The Shapex WASM bridge was rebuilt successfully with `npm run build-shapex`.
- Browser checks passed for rectangle extrusion, annular extrusion, split-circle half extrusion, overlapping-circle join/intersection, a circular through-cut, a smooth exact NURBS spline extrusion, two disjoint selected profiles, reverse-side extrusion, and a blind circular pocket.
- A circular boss was sketched on a block side face, previewed, committed, and visually inspected in the browser. It remains an analytic cylinder with a smooth tessellation.
- Shapex and OCC multi-circle extrusion previews were compared at the same view and tessellation settings and are visually equivalent. The blind Shapex pocket has clean planar shading and a smooth cylindrical wall.
- The Shapex WASM check, JavaScript syntax checks, release bridge build, and frontend tests pass.

## Remaining before declaring full extrusion parity

1. Extend the conservative NURBS proof machinery beyond monotone spline segments so arbitrary simple spline profiles, spline holes, and intersecting spline/curve profile Booleans can be certified without sampling. Disjoint NURBS extrusions and standalone NURBS prisms are already exact.
2. Extend exact physical certification for blind pockets whose boundaries contain general arcs or NURBS rather than a full cylinder.
3. Add browser fixtures for the remaining intersecting NURBS cases, then perform the last OCC visual comparison for those fixtures. OCC is the baseline and should not be changed.

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
