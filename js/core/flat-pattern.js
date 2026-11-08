import { flatPattern } from './sheet-metal.js'
import { shapex as sx, threeVector } from './shapex.js'
import { normalFromMatrix } from './utils.js'
import { saveFile } from '../utils.js'

export const flatPatternDefaults = {
  // Flat patterns open like export configs, which this tells them apart from
  kind: 'flat-pattern',
  title: 'Flatten',
  format: 'DXF',
  kFactor: 0.44,
  unit: 'mm',
  bendLines: true,
  path: null,
  autoSave: false,
}

const units = {
  mm: { scale: 1, code: 4 },
  in: { scale: 1 / 25.4, code: 1 },
}

const deg = radians => radians * 180 / Math.PI

// Outline of the pattern's top faces, without the seams between them
function outlineEdges(body) {
  const faces = body.faces().filter(face => {
    const plane = face.getPlane()
    return plane && normalFromMatrix(plane).z > 1 - 1e-6 && Math.abs(face.center().z) < 1e-5
  })
  const edges = faces.flatMap(face => face.boundaryEdges())
  const counts = new Map()
  edges.forEach(edge => counts.set(edge.kernelId, (counts.get(edge.kernelId) || 0) + 1))
  return edges.filter(edge => counts.get(edge.kernelId) == 1)
}

// DXF entities of an edge, as lists of group codes and values
function edgeEntities(edge, layer) {
  const curve = edge.geom().curve()
  const points = edge.tesselate()
  const line = (a, b) => ['LINE', 8, layer, 10, a.x, 20, a.y, 30, 0, 11, b.x, 21, b.y, 31, 0]
  if(curve instanceof sx.Line3 || points.length < 3) return [line(points[0], points.at(-1))]
  if(curve instanceof sx.Circle3) {
    const center = threeVector(curve.center())
    const radius = curve.radius()
    const circle = [8, layer, 10, center.x, 20, center.y, 30, 0, 40, radius]
    if(points[0].distanceTo(points.at(-1)) < 1e-9) return [['CIRCLE', ...circle]]
    // Arcs run counterclockwise
    const [start, middle, end] = [points[0], points[Math.floor(points.length / 2)], points.at(-1)]
    const ccw = middle.clone().sub(start).cross(end.clone().sub(middle)).z > 0
    const angle = point => deg(Math.atan2(point.y - center.y, point.x - center.x))
    return [['ARC', ...circle, 50, angle(ccw ? start : end), 51, angle(ccw ? end : start)]]
  }
  return points.slice(1).map((point, i) => line(points[i], point))
}

// Minimal DXF of the pattern's outline, with bend lines on their own layer
function flatPatternDxf(pattern, config) {
  const unit = units[config.unit] || units.mm
  const entities = [
    ...outlineEdges(pattern.body).flatMap(edge => edgeEntities(edge, 'OUTLINE')),
    ...(config.bendLines ? pattern.bendLines.map(([a, b]) =>
      ['LINE', 8, 'BEND', 62, 1, 10, a.x, 20, a.y, 30, 0, 11, b.x, 21, b.y, 31, 0]) : []),
  ]
  // Coordinates use the 10s to 50s group codes
  const value = (code, item) => typeof item == 'number' && code >= 10 && code < 50 ? (item * unit.scale).toFixed(6) :
    typeof item == 'number' && code >= 50 && code < 60 ? item.toFixed(6) : item
  const records = [
    [0, 'SECTION'], [2, 'HEADER'], [9, '$INSUNITS'], [70, unit.code], [0, 'ENDSEC'],
    [0, 'SECTION'], [2, 'ENTITIES'],
    ...entities.flatMap(([type, ...pairs]) => [[0, type],
      ...pairs.flatMap((item, i) => i % 2 ? [] : [[item, value(item, pairs[i + 1])]])]),
    [0, 'ENDSEC'], [0, 'EOF'],
  ]
  return records.map(([code, item]) => `${code}\n${item}`).join('\n') + '\n'
}

export async function exportFlatPattern(component, config) {
  const pattern = flatPattern(component.compound, config.kFactor)
  const title = component.creator.title
  if(config.format == 'STEP') {
    const bodies = pattern.body.geom()
    const step = sx.StepAssembly.toStep({
      products: [{ name: title, bodies, colors: [], occurrences: [] }],
      roots: [0],
      unit: config.unit || 'mm',
    })
    return await saveFile(step, 'step', config.path, title)
  }
  return await saveFile(flatPatternDxf(pattern, config), 'dxf', config.path, title)
}
