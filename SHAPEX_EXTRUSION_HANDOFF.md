# Shapex extrusion parity handoff

## Completed in this checkpoint

- Shapex is the default ProtoSolid kernel and loads through the external WASM bridge.
- Sketch/profile selection no longer calls the removed `geom()` API or creates invalid line arrays.
- Lines, circles, mixed line/arc regions, split-circle regions, multiple overlapping circles, and profiles with holes reach the Shapex extrusion path.
- Exact adjacent curved extrusions sew across their shared face using Euler topology operations.
- A smaller extrusion attached to a larger planar face now uses exact face-patch sewing: the attached body's base loop becomes an inner loop of the host face, preserving the analytic cylinder.
- The physical certifier recognizes a right prism with attached cylindrical bosses and combines exact component mass properties with the parallel-axis theorem.
- Curved through-cuts and constant-section genus-one Boolean results reconstruct as Euler-built profile extrusions.
- Faceted curved Boolean results no longer run the second planar-face merge that produced self-intersecting faces.
- Coplanar Boolean reconstruction handles holes and opposite coplanar fragments more robustly.

## Verification at this checkpoint

- Focused curved-kernel tests pass, including curved difference/intersection, adjacent curved union, and exact partial-height circular boss union.
- The Shapex WASM bridge was rebuilt successfully with `npm run build-shapex`.
- Browser checks passed for rectangle extrusion, annular extrusion, split-circle half extrusion, overlapping-circle join, and a circular through-cut.
- The browser was reloaded after the final WASM build, which cleared the unsaved fixture before the exact boss workflow could be repeated in the UI. The exact boss path is covered by the Rust regression test but should be the first browser check next time.

## Remaining before declaring full extrusion parity

1. Repeat the short circular boss `Join` workflow in the browser using the rebuilt bridge and visually compare it with OCC.
2. Exercise `Intersect` and non-coplanar/partially overlapping curved `Join` in the browser.
3. Check multiple disjoint selected profiles and compound output.
4. Check supported spline/NURBS sketch profiles, both extrusion sides, and negative/two-sided distances.
5. Run the complete final verification suite and a visual Shapex/OCC comparison. Do not change OCC behavior.

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
