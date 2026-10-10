export function resolveSimulationFaces(tree, faceGroups, bind=true) {
  const references = faceGroups.flat()
  const faces = []
  for(const reference of references) {
    const resolved = bind ? reference : reference.clone()
    if(resolved.update(tree)) return { message: 'A picked face does not resolve uniquely here', warning: true }
    faces.push(resolved.getItem())
  }

  const first = references[0]
  if(references.some((reference, index) =>
    reference.componentId != first.componentId || faces[index].solid !== faces[0].solid
  )) return 'Pick faces on one solid'
}
