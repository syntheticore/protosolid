import Serialize from './serialize.js'

export class KernelMismatchError extends Error {}

export function parseDocument(data, context) {
  const dump = JSON.parse(data)
  const kernel = window.kernel.modeling
  if(dump.kernel && dump.kernel != kernel.id) {
    const title = { occ: 'OpenCascade', shapex: 'Shapex' }[dump.kernel] || dump.kernel
    throw new KernelMismatchError(`This document was saved using ${title}. Switch the modeling kernel to ${title} in Preferences and restart ProtoSolid to open it.`)
  }
  return Serialize.deserialize(dump, context)
}
