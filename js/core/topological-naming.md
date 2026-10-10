# Persistent topology references

`js/core/topological-naming.js` owns document identities and selection recipes.
`js/core/occ-topology.js` translates OCC history and incidence into that model.
The shared layer has no OCC types or geometric matching.

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
branch cannot retarget the reference to its sole remaining sibling. A multi-item
feature fails if any required selection fails, rather than quietly dropping it.
The shared API also supports an explicit `all-descendants` policy. A selection
made before a split can request all descendants; a single selection must resolve
uniquely. A selection made on one branch remains scoped to that branch under
either policy.

Construction and incidence cannot distinguish every possible symmetric result.
Picking such a cell raises an explicit error; an existing selection that becomes
ambiguous fails to resolve. There is no nearest-geometry or first-output choice.
Unnamed body branches receive persistent identities only when construction and
complete distinguishing incidence identify them uniquely. Rebuilds process saved
bindings even for a sole remaining body. Conflicting bindings stay unresolved,
and selections require the restored branch identity rather than bypassing it
through broad ancestry. This deliberately favors a failed reference over silently
retargeting a body when the available evidence cannot establish continuity.

Body IDs scope logical identities by component occurrence, preserving saved
visibility and feature scope. Split identifies both sides before applying Keep,
so its side identities are independent of Keep and of whether either side is
empty. Symmetric results without sufficient construction or incidence evidence
remain unselectable; transient display IDs are not a persistence mechanism.
Viewport object IDs and tessellation IDs are not persisted reference identities;
unresolved split families may have temporary distinct viewport IDs.

## Backend contract and file format

Another kernel adapter must expose faces, edges, bodies, named incidence, primitive
roles, logical operand roles, semantic branch roles where available, and exact
operation correspondence. Runtime kernel handles can supply that
correspondence, but the document manifest and selection binding remain shared.
The application uses `Compound.topologySelection`, `resolveTopology`,
`restoreTopologySelections`, and `namespaceTopology` to access the adapter.

The new format contains feature manifests and selection descriptors. It has no
reader for legacy `solidId` / `topoId` references and no migration during
regeneration. Existing files require a separate converter.

Run `npm test` for the shared naming, OCC, timeline, reference, assembly, and
simulation checks. Transition coverage includes branch disappearance and return,
initially empty split sides, conflicting unnamed branches, consumed and retained
boolean tools, independent copies, merges, and save/reload with a missing branch.
The OCC fixture requires the generated `assets/oc/oc.js` and
`assets/oc/oc.wasm`, which are ignored by Git. Build the kernel first with
`npm run build-oc` (`npm run build-oc-win` on Windows). The tests load that kernel
without building or launching the application.
