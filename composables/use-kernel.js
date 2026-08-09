import preferences from '~/js/preferences.js'
import { loadShapexBackend } from '~/js/kernels/shapex.js'
import { OccBackend } from '~/js/kernels/occ.js'

export async function useKernel() {
  // PlaneGCS and the existing 2D sketch implementation still use the small OCC
  // compatibility surface. All 3D modeling goes through `modeling`.
  const compatibility = await useOC()
  const modeling = preferences.modelingKernel == 'shapex'
    ? await loadShapexBackend()
    : new OccBackend(compatibility.oc)

  return Object.assign(compatibility, {
    id: modeling.id,
    title: modeling.title,
    modeling,
  })
}
