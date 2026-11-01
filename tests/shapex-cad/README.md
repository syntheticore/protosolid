# Shapex saved-document regressions

```sh
node tests/shapex-cad/check.mjs          # passing cases in this directory
node tests/shapex-cad/check.mjs failing  # robustness cases that do not build yet
```

The checker rebuilds Shapex WASM from the current Rust source and loads each
`.cad` like the app does. A document passes when every feature builds without
an error or warning and its references resolve. A final fillet or chamfer must
also select real edges joining two faces. Cases have no checks of their own.

## Passing

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
| `021-shelled-sector-rim-blends-section.cad` | Rounded sector, open shell, 0.3 mm rim fillet and 0.2 mm rim chamfer |
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
| `047-spline-prism-curved-edge-fillet.cad` | Prism with two spline sides, 1 mm fillet on all four curved cap edges; at the obtuse spline/line corners the cap contacts end beyond the spline ends |
| `048-loft-section-extrusion.cad` | Circular loft section projected onto an axial sketch plane, then extruded 1 mm and joined along its tangent meridians |
| `049-revolved-sector-end-fillet-closed-shell-1mm.cad` | 020, then a closed 1 mm inward shell |
| `050-revolved-sector-end-fillet-closed-shell-2mm.cad` | 020, then a closed 2 mm inward shell |
| `051-revolved-sector-end-fillet-open-shell-1mm.cad` | 020, then a 1 mm inward shell open at the unfilleted radial end |
| `052-revolved-sector-end-fillet-open-shell-2mm.cad` | 020, then a 2 mm inward shell open at the unfilleted radial end |
| `053-revolved-sector-end-rim-fillet-open-shell-1mm.cad` | 020 with the fillet extended to the four circular rims, then a 1 mm inward shell open at the radial end |
| `054-revolved-sector-end-rim-fillet-open-shell-2mm.cad` | 020 with the fillet extended to the four circular rims, then a 2 mm inward shell open at the radial end |
| `055-shelled-sector-rim-blends-side-splits.cad` | 021, then split at both inner planar side walls through the open rim |

## Failing

Robustness cases for existing features: only the last feature fails, so
opening a file shows it in red. Move a case up here once it passes. Each case
stands for one distinct failure; gaps in the numbering are removed duplicates.

Base bodies, each about 20 mm in size: box 16 x 12 x 10, cylinder r6 x 12,
tube r7/r4 x 12, slot 20 x 8 x 8, frustum and cone revolved r7 to r3 or an apex
over 12, revolved sphere r7 (from a half disc, so with poles), torus R7/r3,
capsule r4 x 16, vase (revolved spline), sector (270 degrees of a 5 x 8
rectangle), spline prism (one spline side), circle to square loft, rounded box
(all edges filleted 2 mm), spline pipe (r2 circle swept along a spline), and
bracket (box with a boss, a bore and a spherical pocket). Boolean tools are
second bodies sketched on a tilted plane 3 mm above XY (oblique), on XY like
the target (coplanar), or on XZ (crossing). Shells have no openings.

Known wrong result that builds, so it has no case: joining the sphere with the
cylinder or tube of 073 instead of cutting yields less than the sphere's volume.

| File | Operation |
| --- | --- |
| `056-revolved-sphere-axis-parallel-split.cad` | Revolved sphere, split parallel to its axis, 4.37 mm off center |
| `058-tube-bore-tangent-split.cad` | Tube, split through the wall by an axial plane tangent to the bore |
| `059-vase-oblique-axis-split.cad` | Vase, oblique split through the axis origin |
| `060-rounded-box-oblique-split.cad` | Rounded box, oblique split |
| `061-spline-pipe-oblique-split.cad` | Spline pipe, oblique split |
| `062-bracket-oblique-split.cad` | Bracket, oblique split |
| `064-box-tube-crossing-join.cad` | Box joined with a crossing tube |
| `065-box-capsule-coplanar-cut.cad` | Box cut by a capsule lying in its bottom plane |
| `066-cylinder-capsule-oblique-join.cad` | Cylinder joined with an oblique capsule |
| `067-cylinder-loft-coplanar-intersect.cad` | Cylinder intersected with an offset loft on the same plane |
| `068-cylinder-rounded-box-oblique-cut.cad` | Cylinder cut by an oblique rounded box |
| `069-tube-sphere-coplanar-cut.cad` | Tube cut by a revolved sphere centered in its bottom plane |
| `070-tube-torus-oblique-intersect.cad` | Tube intersected with an oblique torus |
| `071-slot-cylinder-coplanar-cut.cad` | Slot cut by an offset cylinder on the same plane |
| `072-revolved-sphere-box-coplanar-join.cad` | Revolved sphere joined with a box on its equatorial plane |
| `073-revolved-sphere-cylinder-coplanar-cut.cad` | Revolved sphere cut by a cylinder on its equatorial plane |
| `074-revolved-sphere-frustum-crossing-join.cad` | Revolved sphere joined with a crossing frustum |
| `075-revolved-sphere-capsule-oblique-intersect.cad` | Revolved sphere intersected with an oblique capsule |
| `076-cone-tube-crossing-intersect.cad` | Cone intersected with a crossing tube |
| `077-torus-cone-crossing-cut.cad` | Torus cut by a crossing cone |
| `078-torus-rounded-box-crossing-join.cad` | Torus joined with a crossing rounded box |
| `079-capsule-sphere-coplanar-cut.cad` | Capsule cut by an offset revolved sphere centered in its axis plane |
| `080-capsule-torus-crossing-join.cad` | Capsule joined with a crossing torus |
| `081-vase-tube-crossing-cut.cad` | Vase cut by a crossing tube |
| `082-sector-torus-coplanar-join.cad` | Sector joined with an offset torus revolved in the same plane |
| `083-sector-sector-crossing-cut.cad` | Sector cut by a crossing copy of itself |
| `085-spline-prism-spline-prism-oblique-cut.cad` | Spline prism cut by an oblique copy of itself |
| `086-circle-square-loft-capsule-oblique-cut.cad` | Loft cut by an oblique capsule |
| `088-rounded-box-sector-coplanar-join.cad` | Rounded box joined with an offset sector revolved in its bottom plane |
| `089-spline-pipe-tube-coplanar-cut.cad` | Spline pipe cut by an offset tube on the same plane |
| `090-bracket-sphere-coplanar-cut.cad` | Bracket cut by an offset revolved sphere centered in its bottom plane |
| `091-bracket-cone-oblique-cut.cad` | Bracket cut by an oblique cone |
| `093-revolved-sphere-outward-shell.cad` | Revolved sphere, 1 mm outward shell |
| `095-torus-outward-shell-past-axis.cad` | Torus, 8 mm outward shell reaching past the axis |
| `096-tube-outward-shell-filling-bore.cad` | Tube, 8 mm outward shell closing the bore |
| `097-vase-outward-shell.cad` | Vase, 1 mm outward shell |
| `098-sector-outward-shell.cad` | Sector, 3 mm outward shell |
| `099-spline-prism-outward-shell.cad` | Spline prism, 8 mm outward shell |
| `100-circle-square-loft-outward-shell.cad` | Loft, 0.3 mm outward shell |
| `101-rounded-box-inward-shell-past-radius.cad` | Rounded box, 4.5 mm inward shell, deeper than the fillet radius |
| `102-spline-pipe-outward-shell.cad` | Spline pipe, 8 mm outward shell |
| `103-bracket-thin-inward-shell.cad` | Bracket, 0.1 mm inward shell |
| `105-top-filleted-box-outward-shell.cad` | Box with 1 mm top perimeter fillets, 0.4 mm outward shell |
| `106-box-top-g2-fillet.cad` | Box, 3 mm G2 top perimeter fillet (G1 builds) |
| `108-box-top-g2-fillet-split.cad` | Box with 1 mm G2 top perimeter fillets, oblique split through a blend (G1 splits) |
