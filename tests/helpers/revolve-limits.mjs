import '../browser-env.mjs'
import nodeTest from 'node:test'
import { readFile } from 'node:fs/promises'
import * as THREE from 'three'
import { initializeShapex } from '../../js/core/shapex.js'
import { Profile, Wire } from '../../js/core/geom3d.js'
import { Line, Circle, Spline } from '../../js/core/geom2d.js'
import { rotationFromNormal } from '../../js/core/utils.js'
import { withTopologyNaming, topologyDescriptions } from '../../js/core/topological-naming.js'

let namingFeature
export const test = (name, evaluate) => nodeTest(name, () => {
  namingFeature = { id: name }
  return withTopologyNaming(namingFeature, evaluate)
})
const document9 = JSON.parse(await readFile(new URL('../fixtures/revolve-limits-document-9.json', import.meta.url)))
await initializeShapex({ wasm: await readFile(new URL('../../js/shapexJS/pkg/shapex_js_wasm_bg.wasm', import.meta.url)) })

// This saved fixture is replayed from its sketch generators. Tests select
// their authored construction roles, rather than the former display names.
export function construction(item, owner, role, source) {
  return item.topology.anchors.some(anchor => {
    const [kind, actualRole, sources, feature] = JSON.parse(topologyDescriptions(namingFeature).get(anchor))
    return kind == 'face' && feature == owner && (!role || actualRole == role) && (!source || sources.includes(source))
  })
}
export function fixtureFace(body, name) {
  const [, owner, role, source] = name.match(/^\/extrude-(.*?)\/(swept|top|bottom)(?:\/(.*))?$/)
  return body.faces().find(face => construction(face, owner,
    role == 'swept' ? 'profile-side' : role == 'top' ? 'profile-end' : 'profile-start', source))
}
export function revolutionEdge(edge) {
  return edge.connectedFaces().some(face => construction(face, 'id-67306'))
}

export function revolveLimitsFixture() {
  const features = document9
  const sketches = new Map()
  let body
  const vector = value => new THREE.Vector3(...value.elements)
  function profile(sketch, ringIds) {
    const segments = ringIds.map(id => sketch.elements.find(element => element.id == id)).map(element => {
      const segment = element.$class == 'Circle' ? new Circle(vector(element._center), element.radius)
        : element.$class == 'Spline' ? new Spline(element.points.map(vector)) : new Line(...element.points.map(vector))
      segment.id = element.id
      return segment
    })
    if(segments.length > 1) {
      if(!segments[1].endpoints().some(p => p.distanceTo(segments[0].endpoints().at(-1)) < 1e-7)) segments[0].flip()
      for(let i = 1; i < segments.length; i++) {
        if(segments[i].endpoints()[0].distanceTo(segments[i - 1].endpoints().at(-1)) > 1e-7) segments[i].flip()
      }
    }
    return new Profile({ workplane: sketch.plane }, [new Wire(segments)])
  }
  for(const feature of features) {
    if(feature.type == 'CreateSketchFeature') {
      const ref = feature.values.plane.item
      const plane = ref.helperId ? rotationFromNormal(ref.helperId.endsWith('XZ') ? new THREE.Vector3(0, 1, 0) : new THREE.Vector3(0, 0, 1)) :
        fixtureFace(body, ref.topoId).getPlane()
      sketches.set(feature.sketchId, { elements: feature.elements, plane })
    } else {
      let tool
      for(const ref of feature.values.profiles) {
        const p = profile(sketches.get(ref.sketchId), ref.rings[0])
        const both = feature.values.side == 'both'
        const extrusion = p.extrude('c', feature.values.distance * (both ? 2 : 1), feature.id, both ? -feature.values.distance : 0)
        tool = tool ? tool.boolean(extrusion, 'join') : extrusion
      }
      body = body ? body.boolean(tool, feature.values.operation) : tool
    }
  }
  const sketch = sketches.get('id-6')
  const source = profile(sketch, ['id-7'])
  const line = sketch.elements.find(element => element.id == 'id-37')
  const start = vector(line.points[0]).applyMatrix4(sketch.plane)
  const end = vector(line.points[1]).applyMatrix4(sketch.plane)
  const axis = rotationFromNormal(end.sub(start).normalize()).setPosition(start)
  return { body, source, axis }
}
