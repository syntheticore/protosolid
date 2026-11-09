import * as THREE from 'three'

const loads = new Map()
let fontList
let lastFont = { family: 'Arial', style: 'Regular' }

// Only Chromium based browsers, including WebView2 on Windows, grant access to system fonts
export const hasSystemFonts = () => 'queryLocalFonts' in window

export function listSystemFonts() {
  fontList ||= hasSystemFonts() ? window.queryLocalFonts().catch(() => []) : Promise.resolve([])
  return fontList
}

export const findFont = (fonts, family, style) =>
  fonts.find(font => font.family == family && font.style == style)

export function preferredFont(fonts) {
  return findFont(fonts, lastFont.family, lastFont.style) || fonts[0]
}

export function rememberFont(family, style) {
  lastFont = { family, style }
}

// Collections share tables at absolute offsets, so moving a member's table
// directory to the start of the file makes it readable as a single font.
function parseCollection(parse, buffer, postscriptName) {
  const view = new DataView(buffer)
  const source = new Uint8Array(buffer)
  const target = buffer.slice(0)
  for(let index = 0; index < view.getUint32(8); index++) {
    const offset = view.getUint32(12 + index * 4)
    new Uint8Array(target).set(source.subarray(offset, offset + 12 + view.getUint16(offset + 4) * 16))
    const font = parse(target)
    if(Object.values(font.names).some(names => names.postScriptName?.en == postscriptName)) return font
  }
  throw new Error(`${postscriptName} is missing from its font collection`)
}

// The system and the font name a style's words differently, as in Display Bold and Bold Display
const styleKey = style => style.split(' ').filter(word => word != 'Regular').sort().join(' ')

// All styles of a variable font share one file
function parseFont(parse, buffer, { postscriptName, style }) {
  const isCollection = new DataView(buffer).getUint32(0) == 0x74746366
  const font = isCollection ? parseCollection(parse, buffer, postscriptName) : parse(buffer)
  const instances = font.tables.fvar?.instances || []
  const instance = instances.findIndex(({ name }) => styleKey(name.en) == styleKey(style))
  if(instance >= 0) font.variation.set(instance)
  return font
}

async function readFont(data) {
  const [{ parse }, blob] = await Promise.all([import('opentype.js'), data.blob()])
  return parseFont(parse, await blob.arrayBuffer(), data)
}

export function loadSystemFont(data) {
  if(!loads.has(data.postscriptName)) loads.set(data.postscriptName, readFont(data))
  return loads.get(data.postscriptName)
}

function outlinePieces(commands) {
  const pieces = []
  let start, last
  const add = (...points) => {
    pieces.push([last, ...points])
    last = points.at(-1)
  }
  // Font paths have their y axis pointing down
  const point = (x, y) => new THREE.Vector3(x, -y, 0)
  commands.forEach(({ type, x, y, x1, y1, x2, y2 }) => {
    if(type == 'M') {
      start = last = point(x, y)
    } else if(type == 'Z') {
      if(!last.almost(start)) add(start)
    } else {
      const end = point(x, y)
      if(type == 'Q') add(point(x1, y1), end)
      else if(type == 'C') add(point(x1, y1), point(x2, y2), end)
      else if(!last.almost(end)) add(end)
    }
  })
  return pieces.map(piece => piece.flatMap(({ x, y }) => [x, y]))
}

// Outline for a text size of 1 with the baseline starting at the origin, as
// expected by SketchText. Glyphs are laid out one by one, as opentype.js cannot
// apply all of the substitutions fonts define when shaping whole strings.
export function textPieces(font, text) {
  const scale = 1 / font.unitsPerEm
  const glyphs = Array.from(text, char => font.charToGlyph(char))
  let x = 0
  return glyphs.flatMap((glyph, index) => {
    const pieces = outlinePieces(glyph.getPath(x, 0, 1, {}, font).commands)
    // Reading this after getPath yields the width of the active font variation
    x += glyph.advanceWidth * scale
    if(glyphs[index + 1]) x += font.getKerningValue(glyph, glyphs[index + 1]) * scale
    return pieces
  })
}
