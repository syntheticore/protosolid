import * as THREE from 'three'

import { makeID } from '../core/id.js'
import { exportConfigured } from '../core/export.js'
import { exportFlatPattern, flatPatternDefaults } from '../core/flat-pattern.js'
import {
  createDrawing, createDrawingContext, layoutInitialViews, drawingSheet, drawingSvg, exportDrawing,
  sheetSizes, drawingScales, orientations, storeFrame, projectedFrame, nextLetter,
} from '../core/drawing.js'
import { resolveEntity, dimensionKind, orientationAt, gdtCharacteristics, weldTypes } from '../core/drawing-annotations.js'
import {
  CamSetup, addOperation, generateToolpaths, exportGcode, operationTypes, toolShapes, cutDirections,
  orientations as camOrientations, origins as camOrigins, postProcessors,
} from '../core/cam.js'
import { Edge, Vertex } from '../core/geom3d.js'
import { ToolError, itemId, resolveItem, findComponent, describeStored } from './describe.js'
import { editable, closeEditing, editableComponent, findStored, option, number, bool, string, assign } from './common.js'

// Files for other applications: model exports, flat patterns, 2D drawings and G-code

const positive = min => (value, key) => number(value, key, { min })

const formats = {
  step: 'STEP', stp: 'STEP', stl: 'STL', '3mf': '3MF', dxf: 'DXF', pdf: 'PDF', svg: 'SVG', nc: 'NC', gcode: 'NC',
}

// Copies keep the stored setups and drawings unchanged by the export path
const withPath = (item, values) => Object.assign(Object.create(Object.getPrototypeOf(item)), item, values)

async function svgImage(svg, width) {
  const url = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }))
  try {
    const image = new Image()
    await new Promise((resolve, reject) => {
      image.onload = resolve
      image.onerror = () => reject(new ToolError('The drawing could not be rendered'))
      image.src = url
    })
    const [, , sheetWidth, sheetHeight] = svg.match(/viewBox="([^"]+)"/)[1].split(' ').map(Number)
    const canvas = document.createElement('canvas')
    canvas.width = width
    canvas.height = Math.round(width * sheetHeight / sheetWidth)
    canvas.getContext('2d').drawImage(image, 0, 0, canvas.width, canvas.height)
    return canvas.toDataURL('image/png').split(',')[1]
  } finally {
    URL.revokeObjectURL(url)
  }
}

const frameOf = view => ({
  dir: new THREE.Vector3(...view.frame.dir), u: new THREE.Vector3(...view.frame.u), v: new THREE.Vector3(...view.frame.v),
})

function orientationName(frame) {
  return Object.keys(orientations).find(name => new THREE.Vector3(...frame.dir).distanceTo(orientations[name].dir) < 1e-6 &&
    new THREE.Vector3(...frame.u).distanceTo(orientations[name].u) < 1e-6)
}

function drawingContext(doc, component) {
  return createDrawingContext(component, doc.top())
}

function describeDrawing(doc, component, drawing) {
  const sheet = drawingSheet(drawingContext(doc, component), drawing)
  const box = box => [box.min.x, box.min.y, box.max.x, box.max.y].map(value => Number(value.toFixed(1)))
  return {
    ...describeStored('drawing', drawing),
    component: itemId(component),
    projection: drawing.projection,
    hiddenLines: drawing.hiddenLines,
    info: drawing.info,
    sheetSize: sheetSizes[drawing.sheet],
    views: [...sheet.views.values()].map(view => ({
      id: view.view.id,
      type: view.view.type,
      ...(orientationName(view.view.frame) && { orientation: orientationName(view.view.frame) }),
      ...(view.view.parent && { parent: view.view.parent }),
      ...(view.view.label && { label: view.view.label }),
      scale: view.scale,
      box: box(view.box),
    })),
    annotations: drawing.annotations.map(annotation => {
      const placed = sheet.annotations.find(entry => entry.annotation === annotation)
      return {
        id: annotation.id,
        type: annotation.type,
        view: annotation.view,
        ...(annotation.kind && { kind: annotation.kind }),
        ...(annotation.text && { text: annotation.text }),
        ...(placed ? { box: box(placed.box) } : { error: 'Its geometry was lost' }),
      }
    }),
  }
}

const viewOrientations = Object.keys(orientations)
const paperDirections = { right: [1, 0], left: [-1, 0], up: [0, 1], down: [0, -1] }

function addView(drawing, views, spec) {
  if(spec.projectFrom) {
    const parent = views.get(spec.projectFrom)
    if(!parent) throw new ToolError(`No view ${spec.projectFrom}`)
    const direction = paperDirections[option(spec.direction, paperDirections, 'direction')]
    return {
      id: makeID(),
      type: 'projected',
      parent: parent.view.id,
      frame: storeFrame(projectedFrame(frameOf(parent.view), direction, drawing.projection)),
      direction,
      distance: number(spec.distance ?? 80, 'distance', { min: 1 }),
      breaks: [],
    }
  }
  const frame = orientations[option(spec.orientation, viewOrientations, 'orientation')]
  const [x, y] = spec.position || []
  return {
    id: makeID(),
    type: 'base',
    frame: storeFrame(frame),
    x: number(x, 'position x'),
    y: number(y, 'position y'),
    breaks: [],
    hiddenLines: spec.hiddenLines ?? frame.hiddenLines,
    ...(spec.scale && { scale: option(spec.scale, drawingScales, 'scale') }),
  }
}

const annotationTypes = ['dimension', 'baseline', 'ordinate', 'hole', 'centermark', 'centerline', 'note', 'balloon', 'datum', 'gdt', 'weld', 'finish']
// These point a leader at their item instead of keeping a reference to it
const leaderTypes = ['note', 'balloon', 'datum', 'gdt', 'weld', 'finish']

// Annotations reference model edges and vertices, placed in view coordinates
function addAnnotation(doc, component, drawing, context, views, spec) {
  const type = option(spec.type, annotationTypes, 'type')
  const view = views.get(spec.view)
  if(!view) throw new ToolError(`No view ${spec.view}, see the views of the drawing`)
  // Chains measure between points, so circles count by their centers
  const center = !!spec.center || type == 'baseline' || type == 'ordinate'
  const items = (spec.items || []).map(id => {
    const item = resolveItem(doc, id)
    if(!(item instanceof Edge || item instanceof Vertex)) throw new ToolError(`${id} must be an edge or vertex`)
    return { ref: item instanceof Vertex ? item.vertexReference() : item.edgeReference(), center }
  })
  const entities = items.map(stored => resolveEntity(context, stored))
  if(entities.some(entity => !entity)) throw new ToolError('An item is not part of the drawn component')
  const sheetPoint = (value, label) => {
    if(!Array.isArray(value) || value.length != 2 || !value.every(Number.isFinite)) throw new ToolError(`${label} must be [x, y] on the sheet`)
    return view.fromSheet(value)
  }
  const anchors = entities.flatMap(entity => entity.point ? [entity.point] : entity.a ? [entity.a, entity.b] : entity.center ? [entity.center] : [])
    .map(anchor => view.project(anchor))
  const middle = anchors.reduce((sum, [x, y]) => [sum[0] + x / anchors.length, sum[1] + y / anchors.length], [0, 0])
  const defaultPosition = () => {
    const center = view.box.getCenter(new THREE.Vector2())
    const away = new THREE.Vector2(middle[0] - center.x, middle[1] - center.y)
    if(away.lengthSq() < 1e-6) away.set(0, -1)
    away.normalize().multiplyScalar(12)
    return [middle[0] + away.x, middle[1] + away.y]
  }
  const position = spec.position ? sheetPoint(spec.position, 'position') : view.fromSheet(defaultPosition())
  const single = (kind, message) => {
    if(entities.length != 1 || (kind && entities[0].kind != kind)) throw new ToolError(message)
    return entities[0]
  }
  // Leaders end on the item, at the rim of circles
  const target = () => {
    if(spec.target) return sheetPoint(spec.target, 'target')
    const entity = single(null, `${type} annotations take one item or a target`)
    if(entity.kind == 'curve') return view.fromSheet(view.project(entity.points[entity.points.length >> 1]))
    if(entity.kind != 'circle') return view.fromSheet(middle)
    const toward = new THREE.Vector2(...view.toSheet(position)).sub(new THREE.Vector2(...middle))
    toward.normalize().multiplyScalar(entity.radius * view.factor)
    return view.fromSheet([middle[0] + toward.x, middle[1] + toward.y])
  }
  const base = { id: makeID(), view: view.view.id, items: leaderTypes.includes(type) ? [] : items }
  const values = choices => choices.map(choice => choice.value)
  switch(type) {
    case 'dimension': {
      const kind = dimensionKind(entities)
      if(!kind || (entities.length == 1 && entities[0].kind == 'point')) throw new ToolError('These items cannot be dimensioned. Use one straight or circular edge, or two edges or vertices.')
      const orientations = { horizontal: 'h', vertical: 'v', aligned: 'aligned' }
      const sheetPosition = view.toSheet(position)
      const orientation = spec.orientation ? orientations[option(spec.orientation, orientations, 'orientation')] :
        anchors.length == 2 && kind == 'linear' ? orientationAt(anchors[0], anchors[1], sheetPosition) : 'aligned'
      return { ...base, type, kind, orientation, position, tolerance: spec.tolerance || '' }
    }
    case 'baseline':
    case 'ordinate': {
      if(entities.length < 2 || entities.some(entity => entity.kind != 'point')) throw new ToolError(`${type} dimensions take two or more vertices or circular edges, the first being the origin`)
      const [a, b] = anchors
      const orientation = spec.orientation ? { horizontal: 'h', vertical: 'v' }[option(spec.orientation, ['horizontal', 'vertical'], 'orientation')] :
        Math.abs(b[0] - a[0]) >= Math.abs(b[1] - a[1]) ? 'h' : 'v'
      return { ...base, type, orientation, position }
    }
    case 'hole':
      single('circle', 'Hole callouts take one circular edge')
      return { ...base, type, position }
    case 'centermark':
      single('circle', 'Center marks take one circular edge')
      return { ...base, type }
    case 'centerline':
      if(entities.length != 2) throw new ToolError('Center lines take two items')
      return { ...base, type }
    case 'note':
      return { ...base, type, text: string(spec.text ?? 'Note', 'text'), position, ...((spec.target || entities.length) && { target: target() }) }
    case 'balloon':
      return { ...base, type, componentId: single(null, 'Balloons take one item of the part they number').occurrence.id, position, target: target() }
    case 'datum': {
      const used = drawing.annotations.filter(annotation => annotation.type == 'datum').map(annotation => annotation.letter)
      return { ...base, type, letter: spec.letter ? string(spec.letter, 'letter') : nextLetter(used), position, target: target() }
    }
    case 'gdt':
      return {
        ...base, type, position, target: target(),
        characteristic: option(spec.characteristic || 'position', values(gdtCharacteristics), 'characteristic'),
        tolerance: string(spec.tolerance ?? '0.1', 'tolerance'),
        datums: string(spec.datums ?? '', 'datums'),
      }
    case 'weld':
      return { ...base, type, position, target: target(), weldType: option(spec.weldType || 'fillet', values(weldTypes), 'weldType'), size: string(spec.size ?? 'a3', 'size') }
    case 'finish':
      return { ...base, type, text: string(spec.text ?? 'Ra 3.2', 'text'), target: target() }
  }
}

const drawingSettings = {
  title: string,
  sheet: value => option(value, sheetSizes, 'sheet'),
  scale: value => option(value, drawingScales, 'scale'),
  projection: value => option(value, ['first', 'third'], 'projection'),
  hiddenLines: bool,
  partNumber: string,
  material: string,
  drawnBy: string,
  revision: string,
}

const camSettings = {
  title: string,
  up: value => option(value, camOrientations, 'up'),
  origin: value => option(value, camOrigins, 'origin'),
  stockOffset: positive(0),
  clampHeight: positive(0),
  quality: (value, key) => number(value, key, { min: 1, max: 5, integer: true }),
  postProcessor: value => option(value, postProcessors, 'postProcessor'),
}

const operationSettings = {
  shape: value => option(value, toolShapes, 'shape'),
  toolDiameter: positive(0.1),
  direction: value => option(value, cutDirections, 'direction'),
  stepover: positive(0.05),
  stepdown: positive(0.05),
  peck: positive(0.1),
  leave: positive(0),
  feed: positive(1),
  plungeFeed: positive(1),
  spindleSpeed: positive(1),
}

function applyOperation(operation, values) {
  const allowed = Object.fromEntries(Object.keys(operationTypes[operation.type].defaults).map(key => [key, operationSettings[key]]))
  assign(operation, values, allowed, `${operation.type} operations`)
}

function describeCam(setup, component, result) {
  return {
    ...describeStored('cam', setup),
    component: itemId(component),
    ...Object.fromEntries(Object.keys(camSettings).map(key => [key, setup[key]])),
    operations: setup.operations.map(operation => ({
      ...operation,
      ...(result && { moves: result.operations.find(entry => entry.operation === operation)?.moves.length || 0 }),
    })),
    ...(result ? {
      minutes: Number(result.minutes.toFixed(1)),
      stock: [result.grid.width, result.grid.depth, result.grid.height].map(value => Number(value.toFixed(2))),
      holes: result.holes.map(hole => ({ diameter: hole.diameter, through: hole.through })),
    } : { warning: 'The component has no bodies to machine' }),
  }
}

export const outputTools = [
  {
    name: 'export_file',
    title: 'Export file',
    description: 'Writes a component to a file for other applications: STEP, STL or 3MF models, DXF or STEP flat patterns of sheet metal, ' +
      'PDF or SVG drawings and G-code of CAM setups. The format follows from the file extension unless given. ' +
      'Without a path the user is asked for a location. Model options: maxDistance (mm) and maxAngle (deg) for mesh detail, ' +
      'structure (assembly, flat, single), unit (mm, in), includeHidden, colors. Flat pattern options: kFactor, unit, bendLines.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'Defaults to the active component' },
        path: { type: 'string', description: 'Absolute path of the file to write' },
        format: { type: 'string', enum: ['STEP', 'STL', '3MF', 'DXF', 'PDF', 'SVG', 'NC'] },
        flatPattern: { type: 'boolean', description: 'Exports the flat pattern of sheet metal, implied by DXF' },
        drawing: { type: 'string', description: 'Drawing to export as PDF or SVG, like drawing:id-3' },
        cam: { type: 'string', description: 'CAM setup to export as G-code, like cam:id-8' },
        options: { type: 'object', additionalProperties: true },
      },
    },
    run: async ({ component: id, path, format, flatPattern, drawing: drawingId, cam: camId, options = {} }, { doc }) => {
      await closeEditing(doc)
      format ||= path && formats[path.split('.').pop().toLowerCase()]
      let file
      if(drawingId) {
        const { component, item: drawing } = findStored(doc, drawingId, 'drawing')
        file = await exportDrawing(component, withPath(drawing, { path: path || null, format: option(format || drawing.format, ['PDF', 'SVG'], 'format') }), doc.top())
      } else if(camId) {
        const { component, item: setup } = findStored(doc, camId, 'cam')
        file = await exportGcode(withPath(setup, { path: path || null }), component)
      } else {
        const component = findComponent(doc, id)
        if(flatPattern || format == 'DXF') {
          const config = { ...flatPatternDefaults, path: path || null, format: option(format || 'DXF', ['DXF', 'STEP'], 'format') }
          assign(config, options, { kFactor: (value, key) => number(value, key, { min: 0, max: 1 }), unit: value => option(value, ['mm', 'in'], 'unit'), bendLines: bool }, 'Flat patterns')
          try {
            file = await exportFlatPattern(component, config)
          } catch(error) {
            throw new ToolError(`The flat pattern could not be created: ${error.message || error}`)
          }
        } else {
          const config = { format: option(format || 'STEP', ['STEP', 'STL', '3MF'], 'format'), path: path || null }
          assign(config, options, {
            maxDistance: positive(1e-4), maxAngle: positive(0.1),
            structure: value => option(value, ['assembly', 'flat', 'single'], 'structure'),
            unit: value => option(value, ['mm', 'in'], 'unit'),
            includeHidden: bool, colors: bool,
          }, 'Model exports')
          try {
            file = await exportConfigured(component, config)
          } catch(error) {
            if(error == 'canceled') throw error
            throw new ToolError(error.message)
          }
        }
      }
      return { file }
    },
  },
  {
    name: 'edit_drawing',
    title: 'Edit drawing',
    description: 'Creates, changes or removes a 2D drawing of a component, and describes its views and annotations. ' +
      'New drawings start with front, top, side and isometric views fitted to the sheet. Sheet positions are in mm from the top left corner, y pointing down. ' +
      'Views look along the model: front from +Z, top from +Y, left from +X. Projected views unfold from a parent view in a paper direction. ' +
      'Annotations reference model edges and vertices: dimensions measure one straight or circular edge or two items, ' +
      'baseline and ordinate dimensions measure from the first of several vertices or hole centers, hole callouts and center marks take a circular edge, ' +
      'center lines two items. Notes, balloons, datums, GD&T frames, weld and surface finish symbols point a leader at one item or a target. ' +
      'Use capture_drawing to look at it. ' +
      'Actions apply in the order settings, removeViews, addViews, removeAnnotations, addAnnotations.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'For new drawings, defaults to the active component' },
        drawing: { type: 'string', description: 'Existing drawing, like drawing:id-3. Without it a new drawing is created.' },
        settings: {
          type: 'object',
          description: `Any of title, sheet (${Object.keys(sheetSizes).join(', ')}), scale (${drawingScales.join(', ')}), projection (first, third angle), ` +
            'hiddenLines, and the title block fields partNumber, material, drawnBy, revision',
          additionalProperties: true,
        },
        addViews: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              orientation: { type: 'string', enum: viewOrientations },
              position: { type: 'array', items: { type: 'number' }, description: 'Sheet position of the model origin' },
              scale: { type: 'string', description: 'Own scale, defaults to the drawing scale' },
              hiddenLines: { type: 'boolean' },
              projectFrom: { type: 'string', description: 'Parent view id for a projected view, instead of orientation and position' },
              direction: { type: 'string', enum: Object.keys(paperDirections) },
              distance: { type: 'number', description: 'Sheet distance from the parent view, defaults to 80' },
            },
          },
        },
        removeViews: { type: 'array', items: { type: 'string' } },
        addAnnotations: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              type: { type: 'string', enum: annotationTypes },
              view: { type: 'string' },
              items: { type: 'array', items: { type: 'string' }, description: 'Edge and vertex ids' },
              center: { type: 'boolean', description: 'Measures to the centers of circular edges' },
              orientation: { type: 'string', enum: ['horizontal', 'vertical', 'aligned'] },
              position: { type: 'array', items: { type: 'number' }, description: 'Sheet position of the dimension line or symbol, defaults to 12 mm outward of the items' },
              target: { type: 'array', items: { type: 'number' }, description: 'Sheet point the leader ends at, instead of an item' },
              text: { type: 'string', description: 'Of notes and surface finish symbols' },
              tolerance: { type: 'string', description: 'Like "±0.1" for dimensions, the tolerance value of GD&T frames' },
              letter: { type: 'string', description: 'Datum letter, defaults to the next free one' },
              characteristic: { type: 'string', enum: gdtCharacteristics.map(choice => choice.value) },
              datums: { type: 'string', description: 'GD&T datum references, like "A B"' },
              weldType: { type: 'string', enum: weldTypes.map(choice => choice.value) },
              size: { type: 'string', description: 'Weld size, like "a3"' },
            },
            required: ['type', 'view'],
          },
        },
        removeAnnotations: { type: 'array', items: { type: 'string' } },
        show: { type: 'boolean', description: 'Opens the drawing in the app' },
        remove: { type: 'boolean' },
      },
    },
    run: async ({ component: componentId, drawing: id, settings, addViews, removeViews, addAnnotations, removeAnnotations, show, remove }, { doc }) => {
      editable(doc)
      await closeEditing(doc)
      let component, drawing
      if(id) {
        ({ component, item: drawing } = findStored(doc, id, 'drawing'))
        component = editableComponent(doc, itemId(component))
      } else {
        component = editableComponent(doc, componentId)
        drawing = createDrawing()
      }
      if(remove) {
        if(doc.activeDrawing === drawing) doc.activateDrawing(null)
        component.creator.drawings.splice(component.creator.drawings.indexOf(drawing), 1)
        doc.markChanged()
        return { removed: id }
      }
      const context = drawingContext(doc, component)
      if(!drawing.initialized) layoutInitialViews(context, drawing)
      const { partNumber, material, drawnBy, revision, ...general } = settings || {}
      const info = Object.fromEntries(Object.entries({ partNumber, material, drawnBy, revision }).filter(([, value]) => value !== undefined))
      assign(drawing, general, drawingSettings, 'Drawings')
      assign(drawing.info, info, drawingSettings, 'Drawings')
      removeViews?.forEach(viewId => {
        if(!drawing.views.some(view => view.id == viewId)) throw new ToolError(`No view ${viewId}`)
        // Projected views and annotations go with their view
        const removed = new Set([viewId])
        drawing.views.forEach(view => { if(removed.has(view.parent)) removed.add(view.id) })
        drawing.views = drawing.views.filter(view => !removed.has(view.id))
        drawing.annotations = drawing.annotations.filter(annotation => !removed.has(annotation.view))
      })
      const resolvedViews = () => drawingSheet(context, drawing).views
      addViews?.forEach(spec => drawing.views.push(addView(drawing, resolvedViews(), spec)))
      removeAnnotations?.forEach(annotationId => {
        if(!drawing.annotations.some(annotation => annotation.id == annotationId)) throw new ToolError(`No annotation ${annotationId}`)
        drawing.annotations = drawing.annotations.filter(annotation => annotation.id != annotationId)
      })
      if(addAnnotations?.length) {
        const views = resolvedViews()
        addAnnotations.forEach(spec => drawing.annotations.push(addAnnotation(doc, component, drawing, context, views, spec)))
      }
      drawing.revision++
      if(!id) component.creator.drawings.push(drawing)
      doc.markChanged()
      if(show) {
        doc.activateComponent(component)
        doc.activateDrawing(drawing, component)
      }
      return describeDrawing(doc, component, drawing)
    },
  },
  {
    name: 'capture_drawing',
    title: 'Capture drawing',
    readOnly: true,
    description: 'Image of a drawing sheet as it would be exported.',
    inputSchema: {
      type: 'object',
      properties: {
        drawing: { type: 'string' },
        width: { type: 'integer', description: 'Image width in pixels, defaults to 1600' },
      },
      required: ['drawing'],
    },
    run: async ({ drawing: id, width = 1600 }, { doc }) => {
      const { component, item: drawing } = findStored(doc, id, 'drawing')
      const svg = drawingSvg(drawingSheet(drawingContext(doc, component), drawing))
      return { image: await svgImage(svg, width), mimeType: 'image/png' }
    },
  },
  {
    name: 'edit_cam',
    title: 'Edit CAM setup',
    description: 'Creates, changes or removes a 3-axis milling setup of a component, generates its toolpaths and reports machining time, stock and holes. ' +
      'The stock surrounds the part by stockOffset, with clampHeight extra below for the vise. "up" is the side of the part facing the spindle. ' +
      `Operations run in the order ${Object.keys(operationTypes).join(', ')}. Their settings: ${Object.keys(operationSettings).join(', ')} ` +
      '(mm, mm/min and rpm), where each type uses the ones of its defaults. Export G-code with export_file.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'For new setups, defaults to the active component' },
        cam: { type: 'string', description: 'Existing setup, like cam:id-8. Without it a new setup is created.' },
        settings: {
          type: 'object',
          description: `Any of title, up (${Object.keys(camOrientations).join(', ')}), origin (${Object.keys(camOrigins).join(', ')}), ` +
            `stockOffset, clampHeight, quality (1 to 5), postProcessor (${Object.keys(postProcessors).join(', ')})`,
          additionalProperties: true,
        },
        addOperations: {
          type: 'array',
          items: {
            type: 'object',
            properties: { type: { type: 'string', enum: Object.keys(operationTypes) } },
            required: ['type'],
            additionalProperties: true,
          },
        },
        editOperations: {
          type: 'array',
          items: { type: 'object', properties: { id: { type: 'string' } }, required: ['id'], additionalProperties: true },
        },
        removeOperations: { type: 'array', items: { type: 'string' } },
        show: { type: 'boolean', description: 'Opens the setup in the app with its toolpaths' },
        remove: { type: 'boolean' },
      },
    },
    run: async ({ component: componentId, cam: id, settings, addOperations, editOperations, removeOperations, show, remove }, { doc }) => {
      editable(doc)
      await closeEditing(doc)
      let component, setup
      if(id) {
        ({ component, item: setup } = findStored(doc, id, 'cam'))
        component = editableComponent(doc, itemId(component))
      } else {
        component = editableComponent(doc, componentId)
        setup = new CamSetup()
      }
      if(remove) {
        doc.activateInteractivePanel(null)
        component.creator.camSetups.splice(component.creator.camSetups.indexOf(setup), 1)
        doc.markChanged()
        return { removed: id }
      }
      assign(setup, settings, camSettings, 'CAM setups')
      removeOperations?.forEach(operationId => {
        const index = setup.operations.findIndex(operation => operation.id == operationId)
        if(index < 0) throw new ToolError(`No operation ${operationId}`)
        setup.operations.splice(index, 1)
      })
      addOperations?.forEach(({ type, ...values }) => {
        option(type, operationTypes, 'type')
        const before = new Set(setup.operations)
        // Drills fit the smallest hole, as with the toolpaths of an open setup
        if(type == 'drill') setup.result = generateToolpaths(setup, component)
        addOperation(setup, type)
        setup.result = null
        applyOperation(setup.operations.find(operation => !before.has(operation)), values)
      })
      editOperations?.forEach(({ id: operationId, ...values }) => {
        const operation = setup.operations.find(operation => operation.id == operationId)
        if(!operation) throw new ToolError(`No operation ${operationId}`)
        applyOperation(operation, values)
      })
      if(!id) component.creator.camSetups.push(setup)
      doc.markChanged()
      const result = generateToolpaths(setup, component)
      if(show) {
        doc.activateComponent(component)
        doc.activateInteractivePanel(`cam:${component.id}:${setup.id}`)
      }
      return describeCam(setup, component, result)
    },
  },
]
