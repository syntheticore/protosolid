# Persistent topology references

`js/core/topological-naming.js` owns document identities and selection recipes.
`js/core/topology.js` supplies shared incidence and reference resolution.
`js/core/occ-topology.js` and `js/core/shapex-topology.js` translate their kernel's
history into that model. The shared layer has no kernel types or geometric proximity matching.

## Construction and history

Each feature executes inside `withTopologyNaming`. Its serialized
`topologyNames` manifest binds construction descriptions to persistent IDs.
A description consists of the entity kind, semantic construction role, source
identities, and owning feature. Source sets are canonicalized. Native output
indices, feature positions in the timeline, and allocation order are absent.

Primitive faces use their generator's actual correspondence: a named sketch
curve produces a side; the generator identifies its start and end caps. Sweep
start and end edges have explicit roles too, allowing periodic revolution seams
to remain distinct even when they share the same face. Loft, pipe, and limited
extrusion/revolution generators supply their own correspondence.

Adapters normalize later operation history into records of this form:

```js
{ source, relation: 'modified' | 'generated' | 'contributed', outputs: [...] }
```

These records distinguish geometric contribution from logical continuity:

- `modified` continues the source cell, including exact retention.
- `generated` constructs a new cell from named inputs.
- `contributed` records geometric ancestry without continuing the source cell.

Runtime `ancestors` record provenance. `identity` and `anchors` describe the
logical cell, and `names` carry its continuity and selection aliases. Reference
resolution never treats `ancestors` as proof of identity. Merges retain the
continuity names of every logical parent. Generated cells receive new names.

The operation declares its body operands explicitly. A cut continues target
bodies; tool bodies contribute geometry but are consumed. Union and intersection
allow bodies from both operands to continue. Tool face and edge correspondence
remains available for the resulting boundaries. Direct kernel body history takes
precedence over face correspondence, since distinct bodies can share a face.
Operations without body history use exact face correspondence; body indices and
majority voting are not identities.

Split supplies the semantic roles `left` and `right` to the shared naming layer.
Each resulting body, face and edge is named by its input and side, even when the
other side is empty or the input lies entirely on one side. The branch keeps
upstream continuity aliases, but selections made on it require its own anchors.
Thus a selection of the right branch cannot bind to the left when the plane moves
outside the body. Moving the plane back restores the original identities.

Independent copies retain provenance but reset continuity names in a new
namespace, keyed by the source and logical pattern
cell or imported component. A grid cell `(u, v)` retains its identity when the
number of rows changes. Component occurrences scope references separately.
Projected sketch curves use the persistent projection ID.

## Selection and rebuilding

A face, edge, or body reference stores a component ID and a selection descriptor:

```js
{ version: 1, kind, identity, policy: 'single', context: [...] }
```

Picking a cell records a recipe in its producing feature's manifest and attaches
a persistent selection name to that cell. Recipes use logical anchors and named
incidence paths. Explicit branch roles are required anchors, not optional boundary
hints. For operations without semantic branch roles, a recipe also records the
full distinguishing boundary evidence; losing that evidence fails resolution.

A one-to-one modification without an explicit branch role preserves the original cell's producer, incidence
recipe, and shared selection annotations. Selecting it after a later modifier
therefore binds the producer's cached cell too. Removing that modifier cannot
delete the selection. Splits and merges establish separate cell origins and
annotation lists; they do not share new branch selections with their ancestors.

After regeneration, the producer restores those selection names from its
recipes. Subsequent operations propagate the names through history. Therefore
references survive independent reconstruction and save/reload, including feature
insertion before the producer or between the producer and the consumer.

Resolution returns `resolved`, `deleted`, or `ambiguous`. Losing a selected split
branch cannot retarget the reference to its sole remaining sibling. Multi-item
edge, face, and body inputs use their surviving selections with a warning when
some references fail. If none survive, the input still fails. Single-item inputs
and ordered profile inputs still require all their references to resolve.
The shared API also supports an explicit `all-descendants` policy. A selection
made before a split can request all descendants; a single selection must resolve
uniquely. A selection made on one branch remains scoped to that branch under
either policy.

Accepting a warned feature persists its surviving selections and repaired sketch
profiles, then regenerates it to clear the warning. An explicit pick also repairs
that input by discarding unresolved references while retaining valid selections.
Regeneration alone preserves failed references so they can recover if their
upstream topology returns. Cancel restores the original selection and its cached
editor bindings.

Construction and incidence cannot distinguish every possible symmetric result.
The picker accepts timeline geometry even when those identities are ambiguous.
An explicit pick can supplement its producer recipe with oriented side constraints
relative to named supporting faces. A through-bore's two rims, for example, lie
on opposite axial sides of its cylindrical wall's center. A constraint identifies
the supporting face by its construction anchors and records a frame axis and
side; it stores neither coordinates nor an output index. The producer reevaluates
the constraint on reconstruction, before later operations propagate the selection.
Each supporting face must resolve uniquely. Two cylindrical supports can also
define a transverse direction to distinguish periodic rim segments; that direction
rotates with the supports.
Missing or conflicting evidence fails resolution even when only one candidate
remains. Unsupported ambiguity produces an explicit selection error rather than
silently hiding valid geometry. Display-only section boundaries remain excluded.
There is no nearest-geometry or first-output choice.
Unnamed body branches receive persistent identities only when construction and
complete distinguishing incidence identify them uniquely. Rebuilds process saved
bindings even for a sole remaining body. Conflicting bindings stay unresolved,
and selections require the restored branch identity rather than bypassing it
through broad ancestry. This deliberately favors a failed reference over silently
retargeting a body when the available evidence cannot establish continuity.

Body IDs scope logical identities by component occurrence, preserving saved
visibility and feature scope. Split identifies both sides before applying Keep,
so its side identities are independent of Keep and of whether either side is
empty. Transient display IDs are not a persistence mechanism for symmetric results.
Viewport object IDs and tessellation IDs are not persisted reference identities;
unresolved split families may have temporary distinct viewport IDs.

## Backend contract and file format

Another kernel adapter must expose faces, edges, bodies, named incidence, primitive
roles, logical operand roles, semantic branch roles where available, and exact
operation correspondence. Runtime kernel handles can supply that
correspondence, but the document manifest and selection binding remain shared.
The application uses `Compound.topologySelection`, `resolveTopology`,
`restoreTopologySelections`, and `namespaceTopology` to access the adapter.

Shapex publishes input-profile correspondence with construction patches before welding,
and exposes face, edge and body history through its WASM bindings. Native IDs are
opaque correspondence tokens, never serialized document identities. Transformed
copies receive fresh native IDs with exact lineage so combining copies cannot
conflate their histories. Shell construction distinguishes the continuing source
skin from the generated offset skin and opening rims.

The kernel exposes typed construction roles independently of entity ancestry.
Trimming can change a face's construction role without replacing its actual
source history. The application maps input segment indices to sketch IDs and
maps native shell roles to document naming roles. Feature IDs, selection recipes,
timeline behavior and reference policies remain outside the kernel.

The new format contains feature manifests and selection descriptors. It has no
reader for legacy `solidId` / `topoId` references and no migration during
regeneration. Existing files require a separate converter.

Run `npm test` for the shared naming, OCC, Shapex, timeline, reference, assembly, and
simulation checks. Transition coverage includes branch disappearance and return,
initially empty split sides, conflicting unnamed branches, consumed and retained
boolean tools, independent copies, merges, and save/reload with a missing branch.
The OCC fixture requires the generated `assets/oc/oc.js` and
`assets/oc/oc.wasm`, which are ignored by Git. Build the kernel first with
`npm run build-oc` (`npm run build-oc-win` on Windows). The tests load that kernel
without building or launching the application.
The same reference contract runs against both adapters. Shapex tests also require
the generated WASM package from `npm run build-shapex`.
