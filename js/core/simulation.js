export function resolveSimulationFaces(tree, faceGroups) {
  const references = faceGroups.flat()
  for(const reference of references) {
    const component = tree.findChild(reference.componentId)
    const solid = component?.compound.solids().find(item => item.id == reference.solidId)
    const face = solid?.faces().find(item => item.id == reference.topoId)
    if(!face) return { message: 'A picked face does not exist here', warning: true }
    reference.item = face
  }

  const first = references[0]
  if(references.some(reference =>
    reference.componentId != first.componentId || reference.solidId != first.solidId
  )) return 'Pick faces on one solid'
}
