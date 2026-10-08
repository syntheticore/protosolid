# Shapex saved-document regressions

The seventeen `.cad` files in this directory are passing regressions. Nine
cases awaiting review are in [failing/](failing/README.md), numbered 022–030.
All files exercise real modeling workflows and saved references.
Every feature in a passing regression must complete without an error or partial
reference loss. Circular
lofts must publish one curved side surface and two planar caps.

| File | Operation |
| --- | --- |
| `002-eccentric-loft-shell.cad` | Eccentric circular loft, 1 mm inward shell with the upper cap open |
| `003-box-perimeter-chamfer.cad` | Box, 2 mm chamfer around the top perimeter |
| `005-cross-drilled-cylinder-chamfer.cad` | Transverse cylinder bore, 1 mm chamfer on both curved intersections |
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

From the repository root:

```sh
node tests/shapex-cad/check.mjs
```

The checker rebuilds Shapex WASM from the current Rust source, loads both the
passing regressions and the failing catalogue, and exits **1** if any feature
fails or loses a reference. It verifies
that blend selections are semantic edges joining two faces and checks the
circular loft face counts. It does not build or launch the application.
Case 019 checks that all chamfer boundary edges are visible. Case 020 applies
1 and 2 mm inward shells, with and without a planar opening, and verifies volume
and face tessellation. It also adds four circular rims to the upstream fillet
and checks that the shell keeps the original opening reference after regeneration.
Case 021 sections both ways at every large planar side face after each modeling
feature, verifies conservation of volume, and tessellates the resulting faces.

`runtime.mjs` uses the production serializer, timeline, feature classes, reference
resolution, repair, and viewport face tessellation. OpenCascade supplies the
existing 2D sketch utilities and PlaneGCS solver; all 3D operations use Shapex.

The loft selections in cases 013, 014, 015, and 017 refer to the single continuous
side surface and its rim, replacing the former four patch selections. Operation
dimensions and selected geometric regions are unchanged. Case numbering is
preserved from the original failure catalogue.

To verify the failure catalogue while also requiring the existing regressions
to pass:

```sh
node tests/shapex-cad/check.mjs --verify-failures
```

This exits **0** only when each document in `failing/` has exactly one final
Shapex modeling failure, a successful setup, and intact saved references. A newly
passing case fails this verification so it can be reviewed and moved into the
passing regressions. Shell openings bordering fillets and blend seam selections
are excluded. The catalogue contains 17 regression documents and nine documents
with final modeling failures. Case 020's additional follow-up shell check
currently fails, although its saved feature sequence builds.
