import * as THREE from 'three'

import { saveFile } from '../utils.js'
import { makeID } from './id.js'
import {
  drawingModel, viewGeometry, applyBreaks, breakPoint, unbreakPoint, clipToCircle, hatch,
} from './drawing-geometry.js'
import {
  Ink, THIN, CHAIN, resolveEntity, textWidth, dimensionGeometry, drawDimension, drawBaseline, drawOrdinate,
  drawCenterline, drawCenterMark, drawNote, drawHoleCallout, drawBalloon, drawFinish, drawDatum, drawFrame, drawWeld,
} from './drawing-annotations.js'


// Landscape ISO sheets in mm
export const sheetSizes = {
  A4: [297, 210],
  A3: [420, 297],
  A2: [594, 420],
  A1: [841, 594],
  A0: [1189, 841],
}

export const drawingScales = ['10:1', '5:1', '2:1', '1:1', '1:2', '1:5', '1:10', '1:20', '1:50', '1:100', '1:200', '1:500', '1:1000']

export function createDrawing() {
  return {
    id: makeID(),
    title: 'Drawing',
    sheet: 'A3',
    scale: '1:1',
    projection: 'first',
    hiddenLines: true,
    format: 'PDF',
    path: null,
    autoSave: false,
    info: { partNumber: '', material: '', drawnBy: '', revision: '' },
    views: [],
    annotations: [],
    initialized: false,
    // Bumped on every edit, as views and annotations are edited raw
    revision: 0,
  }
}

const vector = (x, y, z) => new THREE.Vector3(x, y, z)

// Views look along dir, with u pointing right and v up on paper. Y is up.
export const orientations = {
  front: { label: 'Front', dir: vector(0, 0, -1), u: vector(1, 0, 0), v: vector(0, 1, 0) },
  back: { label: 'Back', dir: vector(0, 0, 1), u: vector(-1, 0, 0), v: vector(0, 1, 0) },
  top: { label: 'Top', dir: vector(0, -1, 0), u: vector(1, 0, 0), v: vector(0, 0, -1) },
  bottom: { label: 'Bottom', dir: vector(0, 1, 0), u: vector(1, 0, 0), v: vector(0, 0, 1) },
  left: { label: 'Left', dir: vector(1, 0, 0), u: vector(0, 0, 1), v: vector(0, 1, 0) },
  right: { label: 'Right', dir: vector(-1, 0, 0), u: vector(0, 0, -1), v: vector(0, 1, 0) },
  isometric: {
    label: 'Isometric',
    dir: vector(-1, -1, -1).normalize(),
    u: vector(1, 0, -1).normalize(),
    v: vector(-1, 2, -1).normalize(),
    hiddenLines: false,
  },
}

export const storeFrame = frame => ({ dir: frame.dir.toArray(), u: frame.u.toArray(), v: frame.v.toArray() })
const loadFrame = frame => ({ dir: vector(...frame.dir), u: vector(...frame.u), v: vector(...frame.v) })

// The view seen from the side p points to on paper (y up), turned over
// the parent's edge by the drawing's projection method
export function projectedFrame(parent, [px, py], projection) {
  const { u, v, dir } = parent
  const side = u.clone().multiplyScalar(-py).addScaledVector(v, px)
  const toward = u.clone().multiplyScalar(px).addScaledVector(v, py)
  const sign = projection == 'third' ? 1 : -1
  return {
    dir: toward.multiplyScalar(-sign),
    u: dir.clone().multiplyScalar(sign * px).addScaledVector(side, -py),
    v: dir.clone().multiplyScalar(sign * py).addScaledVector(side, px),
  }
}

export function scaleFactor(scale) {
  const [paper, model] = scale.split(':').map(Number)
  return paper / model
}

export function nextLetter(used) {
  for(let i = 0; ; i++) {
    const letter = String.fromCharCode(65 + i % 26) + (i >= 26 ? Math.floor(i / 26) : '')
    if(!used.includes(letter)) return letter
  }
}

export const isAligned = view => ['projected', 'section', 'auxiliary'].includes(view.type)

export function createDrawingContext(component, tree) {
  return { component, tree, model: drawingModel(component), entities: new Map() }
}


// Views placed on the sheet

const MARGIN = 10
const TITLE_WIDTH = 140
const TITLE_HEIGHT = 36
export const BREAK_GAP = 6
const VISIBLE = { width: 0.5 }
const HIDDEN = { width: 0.25, dash: [3, 1] }
const LABEL = 5

function resolveViews(context, drawing) {
  const views = new Map()
  const resolve = view => {
    if(views.has(view.id)) return views.get(view.id)
    const parent = view.parent && resolve(drawing.views.find(other => other.id == view.parent))
    const resolved = resolveView(context, drawing, view, parent)
    views.set(view.id, resolved)
    return resolved
  }
  drawing.views.forEach(resolve)
  return views
}

function resolveView(context, drawing, view, parent) {
  const frame = loadFrame(view.frame)
  const scale = view.scale || (parent && isAligned(view) ? parent.scale : drawing.scale)
  const factor = scaleFactor(scale)
  const origin = isAligned(view)
    ? [parent.origin[0] + view.direction[0] * view.distance, parent.origin[1] - view.direction[1] * view.distance]
    : [view.x, view.y]
  const breaks = view.breaks || []
  const gap = BREAK_GAP / factor
  const fromBroken = ([u, v]) => [origin[0] + factor * u, origin[1] - factor * v]
  const toSheet = point => fromBroken(breakPoint(point, breaks, gap))
  const section = view.section ? vector(...view.section) : view.type == 'detail' ? parent.section : null
  const resolved = {
    view, parent, frame, scale, factor, origin, section,
    toSheet,
    fromSheet: ([x, y]) => unbreakPoint([(x - origin[0]) / factor, (origin[1] - y) / factor], breaks, gap),
    project: point => toSheet([point.dot(frame.u), point.dot(frame.v)]),
  }

  const geometry = viewGeometry(context.model, frame, section)
  const process = points => {
    let runs = breaks.length ? applyBreaks(points, breaks, gap) : [points]
    if(view.detail) runs = runs.flatMap(run => clipToCircle(run, view.detail.center, view.detail.radius))
    return runs.map(run => run.map(fromBroken))
  }
  const showHidden = drawing.hiddenLines && view.hiddenLines !== false
  resolved.lines = geometry.lines
    .filter(line => showHidden || !line.hidden)
    .flatMap(line => process(line.points).map(points => ({ points, hidden: line.hidden, source: line.source })))
  resolved.cut = geometry.cut.flatMap(process)
  resolved.hatch = resolved.cut.length ? hatch(resolved.cut, 2.5) : []

  const box = new THREE.Box2()
  ;[...resolved.lines.map(line => line.points), ...resolved.cut].flat()
    .forEach(point => box.expandByPoint(new THREE.Vector2(...point)))
  if(box.isEmpty()) box.setFromCenterAndSize(new THREE.Vector2(...origin), new THREE.Vector2(10, 10))
  resolved.box = box
  return resolved
}

// Front, top and side views with an isometric view, fitted to the sheet
export function layoutInitialViews(context, drawing) {
  const [width, height] = sheetSizes[drawing.sheet]
  const area = { x: MARGIN, y: MARGIN, width: width - 2 * MARGIN, height: height - 2 * MARGIN - TITLE_HEIGHT }
  const third = drawing.projection == 'third'
  const front = orientations.front
  const top = projectedFrame(front, third ? [0, 1] : [0, -1], drawing.projection)
  const side = projectedFrame(front, [1, 0], drawing.projection)
  const cells = third
    ? [[front, 0, 1], [top, 0, 0], [side, 1, 1], [orientations.isometric, 1, 0]]
    : [[front, 0, 0], [top, 0, 1], [side, 1, 0], [orientations.isometric, 1, 1]]
  const placed = cells.map(([frame, column, row]) => ({ frame, column, row, bounds: projectedBounds(context.model.positions, frame) }))
  const columns = [0, 1].map(column => Math.max(0, ...placed.filter(item => item.column == column).map(item => item.bounds.width)))
  const rows = [0, 1].map(row => Math.max(0, ...placed.filter(item => item.row == row).map(item => item.bounds.height)))
  const gap = 0.12 * Math.min(area.width, area.height)
  const fit = 0.85 * Math.min((area.width - gap) / (columns[0] + columns[1]), (area.height - gap) / (rows[0] + rows[1]))
  drawing.scale = Number.isFinite(fit) ? drawingScales.find(scale => scaleFactor(scale) <= fit) || drawingScales.at(-1) : '1:1'
  const factor = scaleFactor(drawing.scale)

  const left = area.x + (area.width - factor * (columns[0] + columns[1]) - gap) / 2
  const topEdge = area.y + (area.height - factor * (rows[0] + rows[1]) - gap) / 2
  const cellX = [left, left + factor * columns[0] + gap]
  const cellY = [topEdge, topEdge + factor * rows[0] + gap]
  const origins = placed.map(({ column, row, bounds }) => [
    cellX[column] + factor * (columns[column] / 2 - bounds.centerU),
    cellY[row] + factor * (rows[row] / 2 + bounds.centerV),
  ])
  const base = (frame, [x, y]) => ({
    id: makeID(), type: 'base', frame: storeFrame(frame), x, y, breaks: [], hiddenLines: frame.hiddenLines,
  })
  const frontView = base(front, origins[0])
  const projected = (frame, direction, origin) => ({
    id: makeID(), type: 'projected', parent: frontView.id, frame: storeFrame(frame), direction, breaks: [],
    distance: (origin[0] - origins[0][0]) * direction[0] - (origin[1] - origins[0][1]) * direction[1],
  })
  drawing.views = [
    frontView,
    projected(top, third ? [0, 1] : [0, -1], origins[1]),
    projected(side, [1, 0], origins[2]),
    base(orientations.isometric, origins[3]),
  ]
  drawing.initialized = true
}

export function projectedBounds(positions, frame) {
  let minU = Infinity, maxU = -Infinity, minV = Infinity, maxV = -Infinity
  const point = new THREE.Vector3()
  for(let i = 0; i < positions.length; i += 3) {
    point.fromArray(positions, i)
    const u = point.dot(frame.u)
    const v = point.dot(frame.v)
    minU = Math.min(minU, u)
    maxU = Math.max(maxU, u)
    minV = Math.min(minV, v)
    maxV = Math.max(maxV, v)
  }
  if(!positions.length) return { width: 0, height: 0, centerU: 0, centerV: 0 }
  return { width: maxU - minU, height: maxV - minV, centerU: (minU + maxU) / 2, centerV: (minV + maxV) / 2 }
}


// The sheet with its views and annotations, in mm with y pointing down

export function drawingSheet(context, drawing) {
  const [width, height] = sheetSizes[drawing.sheet]
  const sheet = { width, height, paths: [], texts: [], annotations: [] }
  const views = resolveViews(context, drawing)
  sheet.views = views

  views.forEach(view => {
    sheet.paths.push(
      ...view.hatch.map(points => ({ width: 0.18, points })),
      ...view.lines.filter(line => line.hidden).map(line => ({ ...HIDDEN, points: line.points })),
      ...view.lines.filter(line => !line.hidden).map(line => ({ ...VISIBLE, points: line.points })),
      ...view.cut.map(points => ({ ...VISIBLE, points })),
    )
    const ink = new Ink()
    decorateView(ink, view)
    sheet.paths.push(...ink.paths)
    sheet.texts.push(...ink.texts)
  })

  drawing.annotations.forEach(annotation => {
    const view = views.get(annotation.view)
    const ink = view && annotationInk(context, view, annotation)
    if(!ink) return
    sheet.paths.push(...ink.paths)
    sheet.texts.push(...ink.texts)
    sheet.annotations.push({ annotation, box: ink.box, fields: ink.fields })
  })

  addFrame(sheet, drawing, context.component)
  return sheet
}

// Labels, cutting planes, detail circles and break lines
function decorateView(ink, view) {
  const { view: data, parent, box } = view
  const below = [(box.min.x + box.max.x) / 2, box.max.y + 8]
  if(data.type == 'section') {
    ink.text(below, `SECTION ${data.label}-${data.label}`, { anchor: 'middle', size: LABEL })
    cuttingPlane(ink, parent, view)
  }
  if(data.type == 'detail') {
    ink.text(below, `DETAIL ${data.label} (${view.scale})`, { anchor: 'middle', size: LABEL })
    const center = view.toSheet(data.detail.center)
    ink.circle(center, data.detail.radius * view.factor)
    const parentCenter = parent.toSheet(data.detail.center)
    const radius = data.detail.radius * parent.factor
    ink.circle(parentCenter, radius)
    ink.text([parentCenter[0] + radius * 0.75 + 1, parentCenter[1] - radius * 0.75 - 1], data.label, { size: LABEL })
  }
  if(data.type == 'auxiliary') {
    ink.text(below, `VIEW ${data.label}`, { anchor: 'middle', size: LABEL })
    const marker = parent.toSheet(data.marker)
    const look = sheetDirection(parent, view.frame.dir)
    const tail = [marker[0] - look[0] * 14, marker[1] - look[1] * 14]
    const head = [marker[0] - look[0] * 3, marker[1] - look[1] * 3]
    ink.line([tail, head])
    ink.arrow(head, look)
    ink.text([tail[0] - look[0] * 2, tail[1] - look[1] * 2 + 1.5], data.label, { anchor: 'middle', size: LABEL })
  }
  for(const { brk, positions } of breakSeams(view)) {
    positions.forEach(at => {
      if(brk.axis == 'u') {
        const middle = (box.min.y + box.max.y) / 2
        ink.line([[at, box.min.y - 2], [at, middle - 2], [at + 1.5, middle - 1], [at - 1.5, middle + 1], [at, middle + 2], [at, box.max.y + 2]])
      } else {
        const middle = (box.min.x + box.max.x) / 2
        ink.line([[box.min.x - 2, at], [middle - 2, at], [middle - 1, at + 1.5], [middle + 1, at - 1.5], [middle + 2, at], [box.max.x + 2, at]])
      }
    })
  }
}

// Paper positions of the two seam lines of each break along its axis. Breaks
// below a seam move it, so it goes through the same mapping as the geometry.
export function breakSeams(view) {
  return (view.view.breaks || []).map(brk => {
    const k = brk.axis == 'u' ? 0 : 1
    const at = view.toSheet(k ? [0, brk.from] : [brk.from, 0])[k]
    return { brk, positions: [at, k ? at - BREAK_GAP : at + BREAK_GAP] }
  })
}

// Direction on the sheet of a model direction in a view
function sheetDirection(view, direction) {
  const x = direction.dot(view.frame.u)
  const y = -direction.dot(view.frame.v)
  const length = Math.hypot(x, y) || 1
  return [x / length, y / length]
}

function cuttingPlane(ink, parent, view) {
  const look = sheetDirection(parent, view.frame.dir)
  const along = [-look[1], look[0]]
  const center = parent.project(view.section)
  const box = parent.box
  const reach = [box.min, box.max, new THREE.Vector2(box.min.x, box.max.y), new THREE.Vector2(box.max.x, box.min.y)]
    .map(corner => (corner.x - center[0]) * along[0] + (corner.y - center[1]) * along[1])
  const point = distance => [center[0] + along[0] * distance, center[1] + along[1] * distance]
  const ends = [Math.min(...reach) - 4, Math.max(...reach) + 4]
  ink.line(ends.map(point), CHAIN)
  ends.forEach((end, i) => {
    const outward = i ? 1 : -1
    const tip = point(end)
    ink.line([point(end - outward * 6), tip], { width: 0.7 })
    const head = [tip[0] + look[0] * 6, tip[1] + look[1] * 6]
    ink.line([tip, head])
    ink.arrow(head, look)
    ink.text([head[0] + along[0] * outward * 2.5, head[1] + along[1] * outward * 2.5 + 1.8], view.view.label, { anchor: 'middle', size: LABEL })
  })
}

// Primitives of an annotation, or null when its model references are lost.
// Previews pass the picked entities directly.
export function annotationInk(context, view, annotation) {
  const entities = annotation.entities || (annotation.items || []).map(item => resolveEntity(context, item))
  if(entities.some(entity => !entity)) return null
  const ink = new Ink()
  const position = annotation.position && view.toSheet(annotation.position)
  const target = annotation.target && view.toSheet(annotation.target)
  switch(annotation.type) {
    case 'dimension': drawDimension(ink, view, dimensionGeometry(view, entities, annotation.kind), annotation, position); break
    case 'baseline': drawBaseline(ink, view, entities, annotation, position); break
    case 'ordinate': drawOrdinate(ink, view, entities, annotation, position); break
    case 'centerline': drawCenterline(ink, view, entities); break
    case 'centermark': drawCenterMark(ink, view, entities[0]); break
    case 'note': drawNote(ink, annotation, position, target); break
    case 'hole': drawHoleCallout(ink, view, entities[0], context.model, position); break
    case 'balloon': drawBalloon(ink, balloonNumber(context, annotation.componentId), position, target); break
    case 'finish': drawFinish(ink, annotation, target); break
    case 'datum': drawDatum(ink, annotation, position, target); break
    case 'gdt': drawFrame(ink, annotation, position, target); break
    case 'weld': drawWeld(ink, annotation, position, target); break
  }
  return ink
}

// Item numbers follow the order of components in the tree
function balloonNumber(context, componentId) {
  const sources = [...new Set(context.model.occurrences.map(occurrence => occurrence.sourceId()))]
  const occurrence = context.model.occurrences.find(occurrence => occurrence.id == componentId)
  return occurrence ? sources.indexOf(occurrence.sourceId()) + 1 : '?'
}

function addFrame(sheet, drawing, component) {
  const rectangle = (x, y, width, height) => [[x, y], [x + width, y], [x + width, y + height], [x, y + height], [x, y]]
  const frame = { x: MARGIN, y: MARGIN, width: sheet.width - 2 * MARGIN, height: sheet.height - 2 * MARGIN }
  sheet.paths.push({ width: 0.7, points: rectangle(frame.x, frame.y, frame.width, frame.height) })

  const x = frame.x + frame.width - TITLE_WIDTH
  const y = frame.y + frame.height - TITLE_HEIGHT
  const row = TITLE_HEIGHT / 3
  sheet.paths.push(
    { ...THIN, points: rectangle(x, y, TITLE_WIDTH, TITLE_HEIGHT) },
    { ...THIN, points: [[x, y + row], [x + TITLE_WIDTH, y + row]] },
    { ...THIN, points: [[x, y + 2 * row], [x + TITLE_WIDTH, y + 2 * row]] },
  )
  sheet.texts.push({ x: x + 3, y: y + 8.5, size: 6, text: component.creator.title })
  const info = drawing.info
  const material = info.material || component.getMaterial()?.title || ''
  const rows = [
    [['Part No.', info.partNumber], ['Material', material], ['Revision', info.revision]],
    [['Drawn by', info.drawnBy], ['Scale', drawing.scale], ['Sheet', drawing.sheet], ['Projection', drawing.projection == 'first' ? 'First angle' : 'Third angle']],
  ]
  rows.forEach((fields, r) => {
    const top = y + (r + 1) * row
    const cell = TITLE_WIDTH / fields.length
    fields.forEach(([label, value], i) => {
      const left = x + i * cell
      if(i) sheet.paths.push({ ...THIN, points: [[left, top], [left, top + row]] })
      sheet.texts.push(
        { x: left + 2, y: top + 3.5, size: 2.2, text: label },
        { x: left + 2, y: top + 9.5, size: 3.5, text: value || '' },
      )
    })
  })
}


// SVG
export function drawingSvg(sheet) {
  const paths = sheet.paths.map(path => {
    const d = `M${path.points.map(([x, y]) => `${number(x)} ${number(y)}`).join('L')}`
    const dash = path.dash ? ` stroke-dasharray="${path.dash.join(' ')}"` : ''
    return `<path d="${d}" stroke-width="${path.width}"${dash}${path.fill ? ' fill="#000"' : ''}/>`
  })
  const texts = sheet.texts.filter(text => text.text).map(text => {
    const anchor = text.anchor && text.anchor != 'start' ? ` text-anchor="${text.anchor}"` : ''
    const rotate = text.angle ? ` transform="rotate(${number(-text.angle)} ${number(text.x)} ${number(text.y)})"` : ''
    return `<text x="${number(text.x)}" y="${number(text.y)}" font-size="${text.size}"${anchor}${rotate}>${escapeXml(text.text)}</text>`
  })
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${sheet.width}mm" height="${sheet.height}mm" viewBox="0 0 ${sheet.width} ${sheet.height}">
<rect width="${sheet.width}" height="${sheet.height}" fill="#fff"/>
<g fill="none" stroke="#000" stroke-linecap="round" stroke-linejoin="round">
${paths.join('\n')}
</g>
<g font-family="Helvetica, Arial, sans-serif" fill="#000">
${texts.join('\n')}
</g>
</svg>
`
}

function escapeXml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
}


// PDF, a single vector page with Helvetica text
function drawingPdf(sheet) {
  const mm = 72 / 25.4
  // Draw in mm with y pointing down, like the SVG
  const content = [`${number(mm)} 0 0 ${number(-mm)} 0 ${number(sheet.height * mm)} cm`, '1 J 1 j']
  sheet.paths.forEach(path => {
    content.push(`${path.width} w [${(path.dash || []).join(' ')}] 0 d`)
    const outline = path.points.map(([x, y], index) => `${number(x)} ${number(y)} ${index ? 'l' : 'm'}`).join(' ')
    content.push(outline + (path.fill ? ' b' : ' S'))
  })
  // Text matrices flip glyphs back upright and turn them by their angle
  sheet.texts.filter(text => text.text).forEach(text => {
    const radians = (text.angle || 0) * Math.PI / 180
    const cos = Math.cos(radians)
    const sin = Math.sin(radians)
    const shift = { start: 0, middle: 0.5, end: 1 }[text.anchor || 'start'] * textWidth(text.text, text.size)
    const x = text.x - cos * shift
    const y = text.y + sin * shift
    content.push(`BT /F1 ${text.size} Tf ${number(cos)} ${number(-sin)} ${number(-sin)} ${number(-cos)} ${number(x)} ${number(y)} Tm (${pdfString(text.text)}) Tj ET`)
  })
  const stream = content.join('\n')
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${number(sheet.width * mm)} ${number(sheet.height * mm)}] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>`,
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>',
    `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`,
  ]
  let pdf = '%PDF-1.4\n'
  const offsets = objects.map((object, index) => {
    const offset = pdf.length
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`
    return offset
  })
  const xref = pdf.length
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`
  pdf += offsets.map(offset => `${String(offset).padStart(10, '0')} 00000 n \n`).join('')
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`
  // Every character is a single Latin-1 byte, so string offsets are byte offsets
  return Uint8Array.from(pdf, char => char.charCodeAt(0))
}

function pdfString(value) {
  return String(value)
    .replace(/[^\x20-\xff]/g, '?')
    .replace(/[\\()]/g, '\\$&')
}

function number(value) {
  return +value.toFixed(3)
}


export async function exportDrawing(component, drawing, tree) {
  const context = createDrawingContext(component, tree)
  if(!drawing.initialized) layoutInitialViews(context, drawing)
  const sheet = drawingSheet(context, drawing)
  const data = drawing.format == 'SVG' ? drawingSvg(sheet) : drawingPdf(sheet)
  return await saveFile(data, drawing.format.toLowerCase(), drawing.path, component.creator.title)
}
