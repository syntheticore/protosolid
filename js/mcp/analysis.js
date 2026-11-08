import * as THREE from 'three'

import { Face, Edge, Vertex, Solid } from '../core/geom3d.js'
import { measureOptions, measureValue, acceptsMeasure } from '../core/measures.js'
import { ConstructionHelper } from '../core/helpers.js'
import { SectionView } from '../core/component.js'
import { StaticSimulation, safetyFactor, stressLimit } from '../core/static.js'
import { ThermalSimulation } from '../core/thermal.js'
import { FluidSimulation, fluidMaterials } from '../core/fluid.js'
import { solveSimulation } from '../core/simulation-runner.js'
import { worldTransform, relativeComponentTransform } from '../core/assembly.js'
import { shapex as sx, threeVector } from '../core/shapex.js'
import { rotationFromNormal } from '../core/utils.js'
import { ToolError, itemId, resolveItem, findComponent, sketchComponent, describeStored } from './describe.js'
import { editable, closeEditing, editableComponent, findStored, option, number, vector3, assign } from './common.js'

// Mass properties, measurements between items, simulations and section views

const round = value => Number(value.toPrecision(6))
const vector = v => [round(v.x), round(v.y), round(v.z)]

function massProperties(component, solids) {
  let mass = 0, volume = 0, area = 0
  const weighted = new THREE.Vector3()
  const centroid = new THREE.Vector3()
  const missing = new Set()
  solids.forEach(({ owner, solid }) => {
    const solidVolume = solid.volume()
    const center = solid.center().applyMatrix4(relativeComponentTransform(owner, component))
    const material = owner.getMaterial()
    if(material) {
      mass += solidVolume * material.density / 1000
      weighted.addScaledVector(center, solidVolume * material.density / 1000)
    } else {
      missing.add(itemId(owner))
    }
    volume += solidVolume
    area += solid.area()
    centroid.addScaledVector(center, solidVolume)
  })
  return {
    ...(!missing.size && solids.length && {
      mass: round(mass),
      centerOfMass: vector(weighted.divideScalar(mass)),
    }),
    ...(missing.size && { componentsWithoutMaterial: [...missing] }),
    volume: round(volume),
    area: round(area),
    ...(solids.length && { centroid: vector(centroid.divideScalar(volume)) }),
    bodies: solids.length,
  }
}

// Geometry of an item in world space: sample points, segments and triangles,
// plus the exact point, line or plane where it has one
function geometry(doc, id) {
  const tree = doc.top()
  const item = resolveItem(doc, id)
  const toWorld = componentId => worldTransform(tree.findChild(componentId))
  const result = { id, points: [], segments: [], triangles: [] }
  const addTriangles = (positions, transform) => {
    for(let i = 0; i < positions.length; i += 9) {
      const corners = [0, 3, 6].map(offset => new THREE.Vector3().fromArray(positions, i + offset).applyMatrix4(transform))
      result.triangles.push(corners)
    }
  }
  const addPolyline = points => {
    result.points.push(...points)
    points.slice(1).forEach((point, index) => result.segments.push([points[index], point]))
  }
  const frame = (matrix, kind) => {
    result.origin = new THREE.Vector3().setFromMatrixPosition(matrix)
    result[kind] = new THREE.Vector3().setFromMatrixColumn(matrix, 2).normalize()
  }

  if(item instanceof Vertex) {
    result.point = item.point.clone().applyMatrix4(toWorld(item.solid.compound.componentId))
    result.points.push(result.point)
  } else if(item instanceof ConstructionHelper) {
    const matrix = toWorld(item.componentId).multiply(item.transform)
    const type = item.typename()
    if(type == 'Point') {
      result.point = new THREE.Vector3().setFromMatrixPosition(matrix)
      result.points.push(result.point)
    } else {
      frame(matrix, type == 'Plane' ? 'normal' : 'direction')
      result.infinite = true
    }
  } else if(item instanceof Edge) {
    const transform = toWorld(item.solid.compound.componentId)
    addPolyline(item.tesselate().map(point => point.clone().applyMatrix4(transform)))
    result.center = item.center().applyMatrix4(transform)
    const curve = item.geom().curve()
    if(curve instanceof sx.Line3) frame(transform.clone().multiply(item.getAxis()), 'direction')
    if(curve instanceof sx.Circle3) {
      result.point = threeVector(curve.center()).applyMatrix4(transform)
      result.radius = curve.radius()
    }
  } else if(item instanceof Face) {
    const transform = toWorld(item.solid.compound.componentId)
    addTriangles(item.tesselate().positions, transform)
    result.points.push(...result.triangles.flat())
    result.center = item.center().applyMatrix4(transform)
    const plane = item.getPlane()
    const axis = item.getAxis()
    if(plane) frame(transform.clone().multiply(plane), 'normal')
    if(axis) {
      frame(transform.clone().multiply(axis), 'direction')
      result.radius = measureValue(item, 'radius')
    }
  } else if(item instanceof Solid) {
    const transform = toWorld(item.compound.componentId)
    item.faces().forEach(face => addTriangles(face.tesselate().positions, transform))
    result.points.push(...result.triangles.flat())
    result.center = item.center().applyMatrix4(transform)
  } else if(item.sketch) {
    const transform = toWorld(sketchComponent(tree, item.sketch.id).id).multiply(item.sketch.workplane)
    const samples = item.point ? [item.point] : item.tesselate?.() || item.handles()
    const points = samples.map(point => new THREE.Vector3(point.x ?? point[0], point.y ?? point[1], 0).applyMatrix4(transform))
    if(points.length == 1) result.point = points[0]
    addPolyline(points)
  } else {
    throw new ToolError(`${id} cannot be measured`)
  }
  result.center ||= result.point ||
    result.points.reduce((sum, point) => sum.add(point), new THREE.Vector3()).divideScalar(result.points.length || 1)
  return result
}

// Closest points of two segments, after Ericson's Real-Time Collision Detection
function segmentClosest(p1, q1, p2, q2) {
  const d1 = q1.clone().sub(p1), d2 = q2.clone().sub(p2), r = p1.clone().sub(p2)
  const a = d1.dot(d1), e = d2.dot(d2), f = d2.dot(r)
  let s = 0, t = 0
  if(a <= 1e-12 && e <= 1e-12) return [p1, p2]
  if(a <= 1e-12) {
    t = THREE.MathUtils.clamp(f / e, 0, 1)
  } else {
    const c = d1.dot(r)
    if(e <= 1e-12) {
      s = THREE.MathUtils.clamp(-c / a, 0, 1)
    } else {
      const b = d1.dot(d2), denominator = a * e - b * b
      s = denominator > 1e-12 ? THREE.MathUtils.clamp((b * f - c * e) / denominator, 0, 1) : 0
      t = (b * s + f) / e
      if(t < 0) { t = 0; s = THREE.MathUtils.clamp(-c / a, 0, 1) }
      else if(t > 1) { t = 1; s = THREE.MathUtils.clamp((b - c) / a, 0, 1) }
    }
  }
  return [p1.clone().addScaledVector(d1, s), p2.clone().addScaledVector(d2, t)]
}

function uniquePoints(points) {
  const seen = new Map()
  points.forEach(point => seen.set(point.toArray().map(value => value.toFixed(6)).join(), point))
  return [...seen.values()]
}

// Smallest distance of the sampled geometry, exact for points, straight edges and planar faces
function closestPoints(a, b) {
  const pointsA = uniquePoints(a.points), pointsB = uniquePoints(b.points)
  const work = pointsA.length * (b.segments.length + b.triangles.length + 1) +
    pointsB.length * (a.segments.length + a.triangles.length + 1) + a.segments.length * b.segments.length
  if(work > 5e7) throw new ToolError('These items are too detailed to measure against each other. Measure faces or edges instead of bodies.')
  let best = { distance: Infinity }
  const consider = (p, q) => {
    const distance = p.distanceTo(q)
    if(distance < best.distance) best = { distance, a: p.clone(), b: q.clone() }
  }
  const segment = new THREE.Line3(), triangle = new THREE.Triangle(), target = new THREE.Vector3()
  const toOther = (point, other, swap) => {
    const add = q => swap ? consider(q, point) : consider(point, q)
    if(!other.segments.length && !other.triangles.length) other.points.forEach(add)
    other.segments.forEach(([start, end]) => {
      segment.set(start, end).closestPointToPoint(point, true, target)
      add(target)
    })
    other.triangles.forEach(corners => {
      triangle.set(...corners).closestPointToPoint(point, target)
      add(target)
    })
  }
  pointsA.forEach(point => toOther(point, b, false))
  pointsB.forEach(point => toOther(point, a, true))
  a.segments.forEach(([p1, q1]) => b.segments.forEach(([p2, q2]) => consider(...segmentClosest(p1, q1, p2, q2))))
  return best
}

const angleBetween = (u, v) => THREE.MathUtils.radToDeg(Math.acos(THREE.MathUtils.clamp(Math.abs(u.dot(v)), 0, 1)))

function compare(a, b) {
  const result = {}
  if(!a.infinite && !b.infinite) {
    const closest = closestPoints(a, b)
    const delta = b.center.clone().sub(a.center)
    Object.assign(result, {
      distance: round(closest.distance),
      closestPoints: [vector(closest.a), vector(closest.b)],
      centerDistance: round(delta.length()),
      centerDelta: vector(delta),
    })
  } else if(a.infinite != b.infinite) {
    // Origin planes and axes extend without bounds, the other item's points are measured to them
    const [line, other] = a.infinite ? [a, b] : [b, a]
    const distanceTo = point => {
      const offset = point.clone().sub(line.origin)
      return line.normal ? offset.dot(line.normal) : offset.sub(line.direction.clone().multiplyScalar(offset.dot(line.direction))).length()
    }
    const distances = other.points.map(distanceTo)
    const [low, high] = [Math.min(...distances), Math.max(...distances)]
    result.distance = round(line.normal && low < 0 && high > 0 ? 0 : Math.min(...distances.map(Math.abs)))
    if(line.normal) result.signedDistanceRange = [round(low), round(high)]
  }
  const dirA = a.direction || a.normal, dirB = b.direction || b.normal
  if(dirA && dirB) {
    const angle = angleBetween(dirA, dirB)
    const lineAndPlane = !!a.direction != !!b.direction
    result.angle = round(lineAndPlane ? 90 - angle : angle)
    const parallel = lineAndPlane ? Math.abs(90 - angle) < 1e-4 : angle < 1e-4
    if(parallel) {
      const offset = b.origin.clone().sub(a.origin)
      const normal = a.normal || b.normal
      result.parallelDistance = round(normal ? Math.abs(offset.dot(normal)) : offset.sub(dirA.clone().multiplyScalar(offset.dot(dirA))).length())
    }
  }
  // Points against lines and planes, measured to their infinite extension
  const point = a.point && !b.point ? a.point : b.point && !a.point ? b.point : null
  const other = point == a.point ? b : a
  if(point && other.origin) {
    const offset = point.clone().sub(other.origin)
    result.perpendicularDistance = round(other.normal ? Math.abs(offset.dot(other.normal)) :
      offset.sub(other.direction.clone().multiplyScalar(offset.dot(other.direction))).length())
  }
  return result
}

const simulationTypes = {
  static: StaticSimulation,
  thermal: ThermalSimulation,
  fluid: FluidSimulation,
}

const faceSettings = {
  static: ['fixedFaces', 'loadFaces'],
  thermal: ['coldFaces', 'hotFaces'],
  fluid: ['inletFaces', 'outletFaces'],
}

const any = (value, key) => number(value, key)
const valueSettings = {
  static: {
    forceX: any, forceY: any, forceZ: any,
    quality: (value, key) => number(value, key, { min: 1, max: 5, integer: true }),
  },
  thermal: { coldTemperature: any, hotTemperature: any },
  fluid: {
    inletSpeed: (value, key) => number(value, key, { min: 1e-9 }),
    outletPressure: any,
    density: (value, key) => number(value, key, { min: 1e-9 }),
    viscosity: (value, key) => number(value, key, { min: 1e-12 }),
    resolution: (value, key) => number(value, key, { min: 8, max: 24, integer: true }),
  },
}

function applySimulationSettings(doc, simulation, settings) {
  const faces = faceSettings[simulation.mode]
  const values = {}
  Object.entries(settings || {}).forEach(([key, value]) => {
    if(faces.includes(key)) {
      simulation[key] = [value].flat().map(id => {
        const face = resolveItem(doc, id)
        if(!(face instanceof Face)) throw new ToolError(`${id} is no face`)
        return face.faceReference()
      })
    } else if(key == 'fluid' && simulation.mode == 'fluid') {
      simulation.setFluid(option(value, fluidMaterials, 'fluid'))
    } else if(key == 'title') {
      simulation.title = String(value)
    } else {
      assign(values, { [key]: value }, valueSettings[simulation.mode], `${simulation.mode} simulations`)
    }
  })
  Object.assign(simulation, values)
  if(simulation.mode == 'fluid' && (values.density || values.viscosity)) simulation.fluid = 'custom'
}

function simulationSettings(simulation, tree) {
  const keys = ['title', ...Object.keys(valueSettings[simulation.mode]), ...(simulation.mode == 'fluid' ? ['fluid'] : [])]
  return {
    ...Object.fromEntries(keys.map(key => [key, simulation[key]])),
    ...Object.fromEntries(faceSettings[simulation.mode].map(key => [key, simulation[key].map(reference => {
      const resolved = reference.clone()
      return resolved.update(tree) ? 'lost' : itemId(resolved.getItem())
    })])),
  }
}

function simulationResult(simulation, probes) {
  const result = simulation.result
  if(!result) return null
  const sample = position => {
    const point = new THREE.Vector3(...vector3(position, 'probe'))
    if(simulation.mode == 'thermal') {
      const temperature = simulation.temperatureAt(point)
      return { at: position, ...(temperature === undefined ? { outside: true } : { temperature: round(temperature) }) }
    }
    const values = (simulation.mode == 'fluid' || simulation.nodeAt(point) !== undefined) && simulation.sampleAt(point)
    if(!values) return { at: position, outside: true }
    if(simulation.mode == 'static') return { at: position, stress: round(values.stress), displacement: vector(values.displacement) }
    return { at: position, velocity: vector(values.velocity), speed: round(values.speed), pressure: round(values.pressure) }
  }
  const range = values => {
    let min = Infinity, max = -Infinity
    for(const value of values) { min = Math.min(min, value); max = Math.max(max, value) }
    return [round(min), round(max)]
  }
  const summary = {
    static: () => ({
      maxStress: round(result.maxStress),
      maxDisplacement: round(result.maxDisplacement),
      ...(safetyFactor(result) && { safetyFactor: round(safetyFactor(result)) }),
      ...(stressLimit(result, result.maxStress) && { exceeds: stressLimit(result, result.maxStress) == 'failure' ? 'ultimate strength' : 'yield strength' }),
      solverResidual: round(result.relativeResidual),
    }),
    thermal: () => ({ temperatureRange: range(result.temperatures) }),
    fluid: () => ({
      maxSpeed: round(result.maxSpeed),
      pressureRange: range(result.pressures),
      outletToInletFlow: round(result.outletFlow / result.inletFlow),
      converged: result.converged,
    }),
  }[simulation.mode]()
  return { ...summary, ...(probes?.length && { probes: probes.map(sample) }) }
}

const units = {
  static: 'Forces in N, stresses in MPa, displacements in mm.',
  thermal: 'Temperatures in °C.',
  fluid: 'Inlet speed and velocities in m/s, pressures in Pa gauge, density in kg/m³, viscosity in Pa·s.',
}

export const analysisTools = [
  {
    name: 'get_mass_properties',
    title: 'Get mass properties',
    readOnly: true,
    description: 'Mass in g, volume in mm³, surface area in mm² and the center of mass of a component with its children, or of a single body, ' +
      'in the coordinates of the component. Mass needs materials, the centroid assumes uniform density.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'Defaults to the active component' },
        body: { type: 'string', description: 'A single body instead' },
        includeHidden: { type: 'boolean', description: 'Include hidden components and bodies, defaults to true' },
      },
    },
    run: ({ component: id, body, includeHidden = true }, { doc }) => {
      if(body) {
        const solid = resolveItem(doc, body)
        if(!(solid instanceof Solid)) throw new ToolError(`${body} is no body`)
        const owner = doc.top().findChild(solid.compound.componentId)
        return { body, ...massProperties(owner, [{ owner, solid }]) }
      }
      const component = findComponent(doc, id)
      const visible = owner => includeHidden || (!owner.creator.hidden && (!owner.parent || visible(owner.parent)))
      const solids = component.getChildren().filter(visible).flatMap(owner => owner.compound.solids()
        .filter(solid => includeHidden || !owner.isItemHidden(solid.id))
        .map(solid => ({ owner, solid })))
      return { component: itemId(component), ...massProperties(component, solids) }
    },
  },
  {
    name: 'measure',
    title: 'Measure',
    readOnly: true,
    description: 'Measures one item, or the distance and angle between two. With two items it gives the smallest distance between them with the closest points, ' +
      'the distance and offset between their centers, the angle between lines, axes and plane normals, the distance of parallel lines and planes, ' +
      'and the distance of a point to a line or plane. Coordinates are in world space. Items are vertices, edges, faces, bodies, sketch curves and points, ' +
      'and origin planes, axes and points.',
    inputSchema: {
      type: 'object',
      properties: {
        items: { type: 'array', items: { type: 'string' }, minItems: 1, maxItems: 2 },
      },
      required: ['items'],
    },
    run: ({ items }, { doc }) => {
      if(items.length == 1) {
        const item = resolveItem(doc, items[0])
        const values = geometry(doc, items[0])
        const measures = acceptsMeasure(item) ? measureOptions(item).filter(({ key, group }) => !group && !['x', 'y', 'z'].includes(key)) : []
        return {
          item: items[0],
          ...Object.fromEntries(measures.map(({ key }) => [key, round(measureValue(item, key))])),
          ...(values.point && { point: vector(values.point) }),
          ...(values.origin && { origin: vector(values.origin) }),
          ...(values.direction && { direction: vector(values.direction) }),
          ...(values.normal && { normal: vector(values.normal) }),
          center: vector(values.center),
        }
      }
      const [a, b] = items.map(id => geometry(doc, id))
      return { items, ...compare(a, b) }
    },
  },
  {
    name: 'simulate',
    title: 'Simulate',
    description: 'Adds, changes, solves or removes a simulation of a body and returns its results. ' +
      'Static load simulations deform a body fixed at some faces under a force on others, and need a material with elastic properties (see set_material). ' +
      'Thermal simulations conduct heat between cold and hot faces at fixed temperatures. ' +
      'Fluid simulations push a fluid through a body that models the fluid volume, from inlet to outlet faces. ' +
      'All faces must belong to one body. Probes sample the result at points in the coordinates of the body\'s component. ' +
      `Settings: static ${Object.keys(valueSettings.static).join(', ')}, ${faceSettings.static.join(', ')}; ` +
      `thermal ${Object.keys(valueSettings.thermal).join(', ')}, ${faceSettings.thermal.join(', ')}; ` +
      `fluid fluid (${Object.keys(fluidMaterials).join(', ')}), ${Object.keys(valueSettings.fluid).join(', ')}, ${faceSettings.fluid.join(', ')}. ` +
      Object.values(units).join(' '),
    inputSchema: {
      type: 'object',
      properties: {
        simulation: { type: 'string', description: 'Existing simulation, like simulation:id-9' },
        type: { type: 'string', enum: Object.keys(simulationTypes), description: 'Adds a new simulation of this type' },
        component: { type: 'string', description: 'Component of a new simulation, defaults to the active component' },
        settings: {
          type: 'object',
          description: 'Values and face lists to change, face lists take face ids. "title" names the simulation.',
          additionalProperties: true,
        },
        probes: { type: 'array', items: { type: 'array', items: { type: 'number' } }, description: 'Points [x, y, z] to sample' },
        show: { type: 'boolean', description: 'Opens the simulation in the app, so capture_view shows its result' },
        remove: { type: 'boolean' },
      },
    },
    run: async ({ simulation: id, type, component: componentId, settings, probes, show, remove }, { doc }) => {
      editable(doc)
      await closeEditing(doc)
      let component, simulation
      if(id) {
        ({ component, item: simulation } = findStored(doc, id, 'simulation'))
        component = editableComponent(doc, itemId(component))
      } else {
        if(!type) throw new ToolError('Give a simulation id or the type of a new simulation')
        component = editableComponent(doc, componentId)
        simulation = new simulationTypes[option(type, simulationTypes, 'type')]()
      }
      if(remove) {
        if(doc.activeSimulation === simulation) doc.activateSimulation(null)
        component.creator.simulations.splice(component.creator.simulations.indexOf(simulation), 1)
        doc.markChanged()
        return { removed: id }
      }
      applySimulationSettings(doc, simulation, settings)
      if(!id) component.creator.simulations.push(simulation)
      doc.markChanged()
      const tree = doc.top()
      const warning = await solveSimulation(simulation, tree, () => {})
      const description = {
        ...describeStored('simulation', simulation),
        settings: simulationSettings(simulation, tree),
        ...(warning && { warning }),
        result: simulationResult(simulation, probes),
      }
      if(show) {
        doc.activateComponent(component)
        doc.activateSimulation(simulation)
      }
      return description
    },
  },
  {
    name: 'edit_section',
    title: 'Edit section view',
    description: 'Adds, changes or removes a section view, which cuts a component and its children open along a plane in the viewport. ' +
      'Without a section id a new one is added. Sections only change the display, not the model.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'For new sections, defaults to the active component' },
        section: { type: 'string', description: 'Existing section, like section:id-4' },
        point: { type: 'array', items: { type: 'number' }, description: 'A point on the cutting plane, in component coordinates' },
        normal: { type: 'array', items: { type: 'number' }, description: 'Normal of the cutting plane' },
        flip: { type: 'boolean', description: 'Removes the other side' },
        hidden: { type: 'boolean' },
        remove: { type: 'boolean' },
      },
    },
    run: async ({ component: componentId, section: id, point, normal, flip, hidden, remove }, { app, doc }) => {
      editable(doc)
      await closeEditing(doc)
      let component, section
      if(id) {
        ({ component, item: section } = findStored(doc, id, 'section'))
        component = editableComponent(doc, itemId(component))
      } else {
        component = editableComponent(doc, componentId)
        section = new SectionView()
      }
      if(remove) {
        doc.activateInteractivePanel(null)
        component.creator.sectionViews = component.creator.sectionViews.filter(other => other !== section)
      } else {
        const transform = section.getTransform()
        if(normal) {
          const direction = new THREE.Vector3(...vector3(normal, 'normal'))
          if(direction.lengthSq() < 1e-12) throw new ToolError('normal must not be zero')
          transform.copy(rotationFromNormal(direction.normalize()).setPosition(new THREE.Vector3().setFromMatrixPosition(transform)))
        }
        if(point) transform.setPosition(...vector3(point, 'point'))
        section.setTransform(transform, !!normal)
        if(flip !== undefined) section.side = !!flip
        if(hidden !== undefined) section.hidden = !!hidden
        if(!id) component.creator.sectionViews.push(section)
      }
      doc.markChanged()
      app.bus.emit('render-needed')
      return remove ? { removed: id } : { component: itemId(component), ...describeStored('section', section) }
    },
  },
]
