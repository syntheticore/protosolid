// ProtoSolid's own icons for actions Font Awesome has no glyph for.
//
// Icons are registered into the Font Awesome library under the `fas` prefix,
// so they are used by name exactly like FA icons: Icon(icon="extrude").
//
// Each icon is drawn on a 512x512 grid from filled primitives. FA renders a
// single path with the nonzero fill rule, so every primitive is emitted with
// clockwise winding and overlapping shapes simply union. Only `hole`, `ring`
// and hollow handles produce cut-outs, and only where nothing else overlaps.
//
// To match FA solid, icons fill the whole grid, positive strokes are around
// 56-64 units wide, negative space between shapes is 32 units (GAP) and
// filled shapes get rounded corners.

const S = 56 // Default stroke width
const THIN = 40 // Secondary strokes, like dashes and construction lines
const GAP = 32 // Negative space between shapes

const fmt = n => String(Math.round(n * 10) / 10)

const pathOf = points => 'M' + points.map(([x, y]) => fmt(x) + ' ' + fmt(y)).join('L') + 'Z'

function area(points) {
  let sum = 0
  points.forEach(([x1, y1], i) => {
    const [x2, y2] = points[(i + 1) % points.length]
    sum += x1 * y2 - x2 * y1
  })
  return sum / 2
}

const clockwise = points => area(points) < 0 ? points.slice().reverse() : points

const polar = (cx, cy, rx, ry, deg) => {
  const a = deg * Math.PI / 180
  return [cx + rx * Math.cos(a), cy + ry * Math.sin(a)]
}

const sample = (fn, n) => Array.from({ length: n + 1 }, (_, i) => fn(i / n))

const bezierPoint = (p0, p1, p2, p3) => t => {
  const u = 1 - t
  return [0, 1].map(k =>
    u * u * u * p0[k] + 3 * u * u * t * p1[k] + 3 * u * t * t * p2[k] + t * t * t * p3[k]
  )
}

// Filled polygon
const poly = (...points) => pathOf(clockwise(points))

// Cut-out polygon, must lie inside a single filled shape
const hole = (...points) => pathOf(clockwise(points).reverse())

function circle(cx, cy, r, ccw = false) {
  const s = ccw ? 0 : 1
  return `M${fmt(cx - r)} ${fmt(cy)}A${fmt(r)} ${fmt(r)} 0 1 ${s} ${fmt(cx + r)} ${fmt(cy)}` +
    `A${fmt(r)} ${fmt(r)} 0 1 ${s} ${fmt(cx - r)} ${fmt(cy)}Z`
}

const ring = (cx, cy, r, w = S) => circle(cx, cy, r + w / 2) + circle(cx, cy, r - w / 2, true)

const ellipse = (cx, cy, rx, ry) => poly(...sample(t => polar(cx, cy, rx, ry, t * 360), 64).slice(1))

// Straight stroke with round caps
function seg(x1, y1, x2, y2, w = S) {
  const r = w / 2
  const len = Math.hypot(x2 - x1, y2 - y1)
  const nx = (y2 - y1) / len * r
  const ny = -(x2 - x1) / len * r
  const a = `A${fmt(r)} ${fmt(r)} 0 0 1 `
  return `M${fmt(x1 + nx)} ${fmt(y1 + ny)}L${fmt(x2 + nx)} ${fmt(y2 + ny)}` +
    a + `${fmt(x2 - nx)} ${fmt(y2 - ny)}L${fmt(x1 - nx)} ${fmt(y1 - ny)}` +
    a + `${fmt(x1 + nx)} ${fmt(y1 + ny)}Z`
}

// Straight stroke with flat caps
function bar(x1, y1, x2, y2, w = S) {
  const len = Math.hypot(x2 - x1, y2 - y1)
  const nx = (y2 - y1) / len * w / 2
  const ny = -(x2 - x1) / len * w / 2
  return poly([x1 + nx, y1 + ny], [x2 + nx, y2 + ny], [x2 - nx, y2 - ny], [x1 - nx, y1 - ny])
}

// Polyline stroke with round joins
const lines = (points, w = S) => points.slice(1).map((p, i) => seg(...points[i], ...p, w)).join('')

const closed = (points, w = S) => lines([...points, points[0]], w)

// Dashed stroke with round caps, spanning exactly from start to end
function dashed(x1, y1, x2, y2, count, w = THIN, gap = GAP) {
  const len = Math.hypot(x2 - x1, y2 - y1)
  const dash = (len - (count - 1) * gap) / count
  const at = d => [x1 + (x2 - x1) * d / len, y1 + (y2 - y1) * d / len]
  return Array.from({ length: count }, (_, i) => {
    const start = i * (dash + gap) + w / 2
    return seg(...at(start), ...at(start + dash - w), w)
  }).join('')
}

// Stroke along a parametric curve fn(t), t in [0, 1], with round caps
function curve(fn, w = S, n = 48) {
  const r = w / 2
  const points = sample(fn, n)
  const normals = points.map((_, i) => {
    const [ax, ay] = points[Math.max(i - 1, 0)]
    const [bx, by] = points[Math.min(i + 1, n)]
    const len = Math.hypot(bx - ax, by - ay)
    return [-(by - ay) / len, (bx - ax) / len]
  })
  const offset = (i, s) => [points[i][0] + normals[i][0] * r * s, points[i][1] + normals[i][1] * r * s]
  const cap = (i, s) => sample(t => {
    const a = t * Math.PI
    const [nx, ny] = normals[i]
    const tx = -ny * s
    const ty = nx * s
    return [
      points[i][0] + r * (nx * s * Math.cos(a) + tx * Math.sin(a)),
      points[i][1] + r * (ny * s * Math.cos(a) + ty * Math.sin(a)),
    ]
  }, 8).slice(1, -1)
  const outline = [
    ...points.map((_, i) => offset(i, 1)),
    ...cap(n, 1),
    ...points.map((_, i) => offset(n - i, -1)),
    ...cap(0, -1),
  ]
  return poly(...outline)
}

const arc = (cx, cy, r, a0, a1, w = S) => curve(t => polar(cx, cy, r, r, a0 + (a1 - a0) * t), w)

const ellipseArc = (cx, cy, rx, ry, a0, a1, w = S) => curve(t => polar(cx, cy, rx, ry, a0 + (a1 - a0) * t), w)

// Bezier stroke, optionally trimmed to the parameter range [t0, t1]
const bezier = (p0, p1, p2, p3, w = S, t0 = 0, t1 = 1) => {
  const fn = bezierPoint(p0, p1, p2, p3)
  return curve(t => fn(t0 + (t1 - t0) * t), w)
}

// Convex polygon shrunk inwards by distance d
function inset(points, d) {
  const pts = clockwise(points)
  const edges = pts.map((p, i) => {
    const q = pts[(i + 1) % pts.length]
    const dir = [q[0] - p[0], q[1] - p[1]]
    const len = Math.hypot(...dir)
    return [[p[0] - dir[1] / len * d, p[1] + dir[0] / len * d], dir]
  })
  const cross = (a, b) => a[0] * b[1] - a[1] * b[0]
  return edges.map(([q, s], i) => {
    const [p, r] = edges[(i - 1 + edges.length) % edges.length]
    const t = cross([q[0] - p[0], q[1] - p[1]], s) / cross(r, s)
    return [p[0] + t * r[0], p[1] + t * r[1]]
  })
}

// Convex polygon shrunk inwards by distance d, with corners rounded to radius r
function roundPoly(points, d, r) {
  const core = inset(points, d + r)
  return poly(...core) + closed(core, 2 * r)
}

// Filled shapes with FA style rounded corners
const shape = (...points) => roundPoly(points, 0, 24)

const rect = (x, y, w, h, r = 24) => roundPoly([[x, y], [x + w, y], [x + w, y + h], [x, y + h]], 0, r)

// Hollow square handle, as used for sketch points. Strokes ending at a
// handle must stop `HANDLE_REACH` short of its center to keep the hole open.
const HANDLE_REACH = 88
function handle(x, y, size = 144, hollow = 48) {
  const h = hollow / 2
  return rect(x - size / 2, y - size / 2, size, size) + hole([x - h, y - h], [x + h, y - h], [x + h, y + h], [x - h, y + h])
}

// Stroke between two points, trimmed by the given lengths at each end
function link(x1, y1, x2, y2, w = S, trim1 = HANDLE_REACH, trim2 = HANDLE_REACH) {
  const len = Math.hypot(x2 - x1, y2 - y1)
  const dx = (x2 - x1) / len
  const dy = (y2 - y1) / len
  return seg(x1 + dx * trim1, y1 + dy * trim1, x2 - dx * trim2, y2 - dy * trim2, w)
}

// Rounded triangular arrow head with its tip at (x, y), pointing in direction deg
function arrow(x, y, deg, length = 160, width = 224) {
  const [dx, dy] = polar(0, 0, 1, 1, deg)
  const bx = x - dx * length
  const by = y - dy * length
  return roundPoly([[x, y], [bx - dy * width / 2, by + dx * width / 2], [bx + dy * width / 2, by - dx * width / 2]], 0, 16)
}

// Arrow head continuing an elliptic arc that ends at angle deg
function arcArrow(cx, cy, rx, ry, deg, length, width) {
  const [x, y] = polar(cx, cy, rx, ry, deg)
  const a = deg * Math.PI / 180
  const dir = Math.atan2(ry * Math.cos(a), -rx * Math.sin(a)) * 180 / Math.PI
  const [dx, dy] = polar(0, 0, 1, 1, dir)
  return arrow(x + dx * length * 0.7, y + dy * length * 0.7, dir, length, width)
}

// Disc A minus disc B, both centered on the same horizontal line, with B to the right
function crescent(ax, y, ra, bx, rb) {
  const d = bx - ax
  const x = (ra * ra - rb * rb + d * d) / (2 * d)
  const h = Math.sqrt(ra * ra - x * x)
  const top = `${fmt(ax + x)} ${fmt(y - h)}`
  const bottom = `${fmt(ax + x)} ${fmt(y + h)}`
  return `M${top}A${fmt(rb)} ${fmt(rb)} 0 0 0 ${bottom}A${fmt(ra)} ${fmt(ra)} 0 1 1 ${top}Z`
}

// Intersection of two equal discs centered on the same horizontal line
function lens(ax, y, bx, r) {
  const h = Math.sqrt(r * r - ((bx - ax) / 2) ** 2)
  const x = (ax + bx) / 2
  const top = `${fmt(x)} ${fmt(y - h)}`
  const bottom = `${fmt(x)} ${fmt(y + h)}`
  return `M${top}A${fmt(r)} ${fmt(r)} 0 0 1 ${bottom}A${fmt(r)} ${fmt(r)} 0 0 1 ${top}Z`
}

// Cube in oblique projection with front face size a and depth offset k
function cube(ox, oy, a, k) {
  const ftl = [ox, oy + k], ftr = [ox + a, oy + k], fbr = [ox + a, oy + k + a], fbl = [ox, oy + k + a]
  const btl = [ox + k, oy], btr = [ox + a + k, oy], bbr = [ox + a + k, oy + a], bbl = [ox + k, oy + a]
  return {
    faces: [[ftl, ftr, fbr, fbl], [ftl, btl, btr, ftr], [ftr, btr, bbr, fbr]],
    visible: [[ftl, ftr], [ftr, fbr], [fbr, fbl], [fbl, ftl], [btl, btr], [btr, bbr], [ftl, btl], [ftr, btr], [fbr, bbr]],
    hidden: [[btl, bbl], [bbl, bbr], [fbl, bbl]],
  }
}

const cubeFaces = (c, d, r = 24) => c.faces.map(face => roundPoly(face, d, r)).join('')

const cubeEdges = (edges, w) => edges.map(([p, q]) => seg(...p, ...q, w)).join('')

const cubeDashes = (edges, w) => edges.map(([p, q]) => dashed(...p, ...q, 2, w)).join('')

// Display mode cubes fill the whole grid, like FA's own cube. Solid
// cubes are inset by half a gap, stroked ones by half a stroke.
const solidCube = cube(0, 0, 336, 176)
const strokedCube = cube(28, 28, 280, 176)

// Ghost of the sharp corner at (x, y) replaced by a fillet or chamfer
const ghostCorner = (x, y, arm) => lines([[x, y + arm], [x, y], [x + arm, y]], THIN)

// Wall profile with its top left corner rounded, for fillet options
const FILLET_CENTER = 412
const filletProfile = seg(28, 484, 28, FILLET_CENTER) +
  arc(FILLET_CENTER, FILLET_CENTER, FILLET_CENTER - 28, 180, 270) +
  seg(FILLET_CENTER, 28, 484, 28)

// Corner brackets around the grid, framing the content of zoom icons
const viewFrame = (arm = 112) => [
  [[28, 28 + arm], [28, 28], [28 + arm, 28]],
  [[484 - arm, 28], [484, 28], [484, 28 + arm]],
  [[484, 484 - arm], [484, 484], [484 - arm, 484]],
  [[28 + arm, 484], [28, 484], [28, 484 - arm]],
].map(points => lines(points)).join('')

// Diagonal hatching inside the square [min, max], for section views
const hatch = (min, max, offsets, w) => offsets.map(c => {
  const x0 = Math.max(min, c - max)
  const x1 = Math.min(max, c - min)
  return seg(x0, c - x0, x1, c - x1, w)
}).join('')

// Filled region between a bezier curve shifted down by top and by bottom
function slab(points, top, bottom) {
  const fn = bezierPoint(...points)
  const edge = sample(fn, 32)
  return poly(...edge.map(([x, y]) => [x, y + top]), ...edge.reverse().map(([x, y]) => [x, y + bottom]))
}

const thickenSurface =[[28, 104], [170, 40], [342, 168], [484, 104]]

const planeOutline = [[156, 60], [484, 60], [356, 452], [28, 452]]

// Block with its top left edge filleted or chamfered. The treated edge is
// drawn as a separate face, set into the block by a gap and two slits
// along x = R and y = R, keeping the block's silhouette intact.
function edgeTreatment(round) {
  const R = 320 // Size of the treated corner
  const w = 72 // Width of the edge face
  const s = GAP / 2
  const block = rect(R + s, 0, 512 - R - s, 512) + rect(0, R + s, 512, 512 - R - s)
  if(round) {
    const core = R - w - GAP
    // Arc of radius r around (R, R) between the two slits
    const arcPoints = r => {
      const a = Math.asin(s / r) * 180 / Math.PI
      return sample(t => polar(R, R, r, r, 180 + a + t * (90 - 2 * a)), 24)
    }
    return block +
      poly([R + s, R - core], [R + s, R + s], [R - core, R + s], ...sample(t => polar(R, R, core, core, 180 + t * 90), 24)) +
      poly(...arcPoints(R), ...arcPoints(R - w).reverse())
  }
  // Points on the line x + y = d between the two slits
  const ends = d => [[R - s, d - R + s], [d - R + s, R - s]]
  const core = R + (w + GAP) * Math.SQRT2
  return block +
    poly([R + s, core - R - s], [R + s, R + s], [core - R - s, R + s]) +
    poly(...ends(R), ...ends(R + w * Math.SQRT2).reverse())
}

// Curvature comb: spikes normal to the curve, scaled by its curvature
function curvatureComb() {
  const base = bezierPoint([0, 160], [200, 160], [380, 200], [440, 480])
  const h = 0.001
  const at = t => {
    const [x0, y0] = base(t - h), [x1, y1] = base(t), [x2, y2] = base(t + h)
    const v = [(x2 - x0) / (2 * h), (y2 - y0) / (2 * h)]
    const a = [(x2 - 2 * x1 + x0) / (h * h), (y2 - 2 * y1 + y0) / (h * h)]
    const speed = Math.hypot(...v)
    const k = (v[0] * a[1] - v[1] * a[0]) / speed ** 3
    return { point: [x1, y1], normal: [-v[1] / speed, v[0] / speed], k }
  }
  const range = t => 0.06 + t * 0.88
  const kMax = Math.max(...sample(t => Math.abs(at(range(t)).k), 64))
  const point = t => at(range(t)).point
  const tip = t => {
    const { point, normal, k } = at(range(t))
    const len = 48 + 300 * (Math.abs(k) / kMax) ** 2
    return [point[0] - normal[0] * len * Math.sign(k), point[1] - normal[1] * len * Math.sign(k)]
  }
  // Scale the comb uniformly to fit the grid, leaving room for stroke widths
  const all = [...sample(point, 64), ...sample(tip, 64)]
  const xs = all.map(p => p[0]), ys = all.map(p => p[1])
  const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)]
  const scale = 440 / Math.max(x1 - x0, y1 - y0)
  const fit = ([x, y]) => [256 + (x - (x0 + x1) / 2) * scale, 256 + (y - (y0 + y1) / 2) * scale]
  return curve(t => fit(point(t)), 64) +
    sample(t => t, 4).map(t => seg(...fit(point(t)), ...fit(tip(t)), 32)).join('') +
    curve(t => fit(tip(t)), THIN)
}

function coil() {
  const turns = 3
  const fn = u => {
    const t = u * turns * 2 * Math.PI
    return [256 + 228 * Math.sin(t), 416 - u * 320 + 52 * Math.cos(t)]
  }
  // Split into half turns, so no single stroke crosses itself
  return Array.from({ length: turns * 2 }, (_, i) =>
    curve(t => fn((i + t) / (turns * 2)), S, 24)
  ).join('')
}

// Sheet ending in an upright flange at x, next to the dashed original edge
const flangeSection = x => lines([[36, 440], [x, 440], [x, 48]], 80) + dashed(256, 0, 256, 512, 4, THIN)

// Sheet folding back over itself, its bend starting at x
const hemSection = x => seg(36, 416, x, 416, 64) + arc(x, 352, 64, 90, -90, 64) + seg(x, 288, x - 160, 288, 64) +
  dashed(256, 0, 256, 512, 4, THIN)

// Sheet bending upwards, its bend starting at x, next to the dashed bend line
const bendSection = x => seg(36, 440, x, 440, 64) + arc(x, 312, 128, 90, 0, 64) + seg(x + 128, 312, x + 128, 48, 64) +
  dashed(256, 0, 256, 512, 4, THIN)

const definitions = {
  // Solid features
  'extrude': [
    rect(0, 400, 512, 112),
    lines([[28, 340], [28, 28], [484, 28], [484, 340]]),
    seg(256, 228, 256, 324, 88),
    arrow(256, 88, -90, 152, 240),
  ],
  'revolve': [
    dashed(256, 0, 256, 512, 4),
    rect(316, 0, 196, 208),
    ellipseArc(256, 376, 228, 96, -48, 225),
    arcArrow(256, 376, 228, 96, 225, 136, 176),
  ],
  'loft': [
    ellipse(256, 432, 256, 80),
    ellipse(256, 72, 152, 72),
    seg(36, 404, 124, 96),
    seg(476, 404, 388, 96),
  ],
  'sweep': [
    bezier([116, 400], [116, 196], [400, 332], [400, 128]),
    ellipse(116, 440, 116, 72),
    ellipseArc(400, 92, 92, 44, 0, 360, THIN),
  ],
  'fillet': [edgeTreatment(true)],
  'chamfer': [edgeTreatment(false)],
  'shell': [
    rect(0, 64, 128, 448),
    rect(384, 64, 128, 448),
    rect(0, 384, 512, 128),
    dashed(160, 84, 352, 84, 3),
  ],
  'draft': [
    shape([0, 0], [304, 0], [400, 512], [0, 512]),
    dashed(472, 512, 472, 0, 4),
  ],
  'split': [
    shape([0, 0], [440, 0], [0, 440]),
    shape([512, 72], [512, 512], [72, 512]),
    dashed(28, 484, 484, 28, 5),
  ],
  'offset-faces': [
    rect(0, 384, 512, 128),
    dashed(0, 40, 512, 40, 5),
    seg(128, 232, 128, 320, 72),
    arrow(128, 92, -90, 144, 208),
    seg(384, 232, 384, 320, 72),
    arrow(384, 92, -90, 144, 208),
  ],
  'dissolve': [
    rect(0, 48, 224, 416),
    rect(256, 48, 96, 96),
    rect(256, 208, 96, 96),
    rect(256, 368, 96, 96),
    rect(384, 128, 64, 64, 16),
    rect(384, 320, 64, 64, 16),
    rect(480, 240, 32, 32, 8),
  ],
  'thicken': [
    bezier(...thickenSurface, THIN),
    slab(thickenSurface, 224, 368),
    arrow(256, 296, 90, 140, 192),
  ],
  'coil': [coil()],
  'rib': [
    rect(0, 0, 128, 512),
    rect(0, 384, 512, 128),
    shape([160, 96], [160, 352], [416, 352]),
  ],
  // Sheet metal features
  'sheet-face': [
    shape([144, 88], [512, 88], [368, 320], [0, 320]),
    rect(0, 352, 368, 72, 16),
  ],
  'sheet-contour': [lines([[36, 144], [208, 144], [304, 368], [476, 368]], 64)],
  'sheet-flange': [
    rect(0, 408, 256, 80, 16),
    arc(320, 320, 128, 90, 0, 64),
    seg(448, 320, 448, 40, 64),
  ],
  'sheet-hem': [
    seg(32, 176, 352, 176, 64),
    arc(352, 272, 96, -90, 90, 64),
    seg(352, 368, 160, 368, 64),
  ],
  'sheet-bend': [
    rect(0, 96, 208, 320),
    rect(304, 96, 208, 320),
    dashed(256, 48, 256, 464, 4),
  ],
  'sheet-unfold': [
    rect(0, 408, 512, 80, 16),
    dashed(152, 24, 152, 344, 3, THIN),
    bezier([216, 48], [376, 64], [464, 160], [464, 256], THIN),
    arrow(464, 352, 90, 112, 160),
  ],
  // Flange positions, as cross sections with the original sheet edge dashed
  'flange-material-inside': [flangeSection(216)],
  'flange-material-outside': [flangeSection(296)],
  'flange-bend-outside': [flangeSection(400)],
  // Corners of two flanges, seen from above
  'corner-open': [
    rect(0, 28, 380, 84),
    rect(428, 160, 84, 352),
  ],
  'hem-material-inside': [hemSection(160)],
  'hem-bend-outside': [hemSection(288)],
  // Bend positions relative to the sketched line
  'bend-centerline': [bendSection(192)],
  'bend-start': [bendSection(256)],
  'bend-end': [bendSection(128)],
  'sheet-refold': [
    rect(0, 408, 304, 80, 16),
    dashed(360, 448, 512, 448, 3, THIN),
    bezier([464, 352], [464, 240], [400, 152], [280, 144], THIN),
    arrow(168, 144, 180, 112, 160),
  ],
  // Unfolded box with dashed bend lines
  'flat-pattern': [
    closed([[176, 24], [336, 24], [336, 176], [488, 176], [488, 336], [336, 336], [336, 488], [176, 488], [176, 336], [24, 336], [24, 176], [176, 176]], 48),
    dashed(208, 176, 304, 176, 2, THIN),
    dashed(208, 336, 304, 336, 2, THIN),
    dashed(176, 208, 176, 304, 2, THIN),
    dashed(336, 208, 336, 304, 2, THIN),
  ],
  'corner-miter': [
    shape([0, 28], [484, 28], [400, 112], [0, 112]),
    shape([507, 51], [507, 512], [423, 512], [423, 135]),
  ],
  'mirror': [
    shape([204, 0], [204, 512], [0, 512]),
    closed([[332, 76], [332, 484], [484, 484]], 48),
    dashed(256, 0, 256, 512, 5),
  ],
  'align': [
    seg(28, 484, 484, 484),
    rect(32, 200, 192, 224),
    rect(288, 24, 192, 400),
  ],
  'pattern-radial': [
    ...Array.from({ length: 8 }, (_, i) => circle(...polar(256, 256, 200, 200, i * 45), 56)),
    circle(256, 256, 64),
  ],

  // Boolean operations
  'boolean-union': [
    crescent(168, 256, 168, 344, 168 + GAP),
    circle(344, 256, 168),
  ],
  'boolean-subtract': [
    crescent(168, 256, 168, 344, 168 + GAP),
    ring(344, 256, 144, 48),
  ],
  'boolean-intersect': [
    lens(168, 256, 344, 168),
    arc(168, 256, 144, 96, 264, 48),
    arc(344, 256, 144, -84, 84, 48),
  ],
  'body-new': [
    circle(160, 256, 160),
    seg(384, 256, 480, 256, 64),
    seg(432, 208, 432, 304, 64),
  ],

  // Construction
  'construction-plane': [
    closed(planeOutline, 48),
    roundPoly(planeOutline, 24 + GAP, 16),
  ],
  'construction-axis': [
    dashed(20, 492, 340, 172, 3, S),
    arrow(500, 12, -45, 176, 224),
  ],
  'construction-point': [
    circle(256, 288, 112),
    link(256, 288, 256, 28, S, 112 + GAP + S / 2, 0),
    link(256, 288, 44, 452, S, 112 + GAP + S / 2, 0),
    link(256, 288, 468, 452, S, 112 + GAP + S / 2, 0),
  ],
  'kinematic-joint': [
    ring(256, 256, 104, 64),
    circle(256, 256, 40),
    bar(...polar(256, 256, 112, 112, 135), 56, 456, 112),
    circle(56, 456, 56),
    bar(...polar(256, 256, 112, 112, -27.5), 456, 152, 112),
    circle(456, 152, 56),
  ],
  'section-view': [
    closed([[28, 28], [484, 28], [484, 484], [28, 484]]),
    hatch(108, 404, [286, 399, 512, 625, 738], THIN),
  ],
  'curvature': [curvatureComb()],

  // Sketch tools
  'sketch-line': [
    link(72, 440, 440, 72, 64),
    handle(72, 440),
    handle(440, 72),
  ],
  'sketch-circle': [
    ring(256, 256, 224, 64),
    circle(256, 256, 64),
  ],
  'sketch-arc': [
    arc(256, 277, 217, 170, 370, 64),
    handle(72, 392),
    handle(440, 392),
    circle(256, 277, 48),
  ],
  'sketch-rectangle': [
    lines([[72, 304], [72, 120], [352, 120]], 64),
    lines([[440, 208], [440, 392], [160, 392]], 64),
    handle(72, 392),
    handle(440, 120),
  ],
  'sketch-rectangle-center': [
    closed([[28, 76], [484, 76], [484, 436], [28, 436]]),
    ...[[28, 76], [484, 76], [484, 436], [28, 436]].map(([x, y]) => link(x, y, 256, 256, THIN, 130, 100)),
    circle(256, 256, 48),
  ],
  'sketch-slot': [
    seg(184, 104, 328, 104),
    seg(184, 408, 328, 408),
    arc(180, 256, 152, 90, 270),
    arc(332, 256, 152, -90, 90),
    circle(180, 256, 44),
    circle(332, 256, 44),
  ],
  'sketch-polygon': [
    closed(Array.from({ length: 6 }, (_, i) => polar(256, 256, 228, 228, -90 + i * 60))),
    circle(256, 256, 64),
  ],
  'sketch-spline': [
    bezier([72, 440], [152, 64], [360, 448], [440, 72], 64, 0.09, 0.91),
    handle(72, 440),
    handle(440, 72),
  ],
  'sketch-point': [
    seg(256, 28, 256, 92),
    seg(256, 420, 256, 484),
    seg(28, 256, 92, 256),
    seg(420, 256, 484, 256),
    circle(256, 256, 104),
  ],
  'sketch-trim': [
    seg(32, 320, 480, 320, 64),
    seg(256, 320, 256, 480, 64),
    dashed(256, 0, 256, 256, 3, 48),
  ],
  'sketch-fillet': [
    seg(28, 484, 28, 288),
    arc(288, 288, 260, 180, 270),
    seg(288, 28, 484, 28),
    ghostCorner(28, 28, 40),
  ],
  'sketch-break': [
    seg(32, 256, 72, 256, 64),
    seg(440, 256, 480, 256, 64),
    handle(168, 256),
    handle(344, 256),
  ],
  'sketch-extend': [
    seg(32, 256, 248, 256, 64),
    arrow(424, 256, 0, 96, 176),
    seg(484, 28, 484, 484),
  ],
  'sketch-intersect': [
    link(28, 484, 256, 256, S, 0),
    link(256, 256, 484, 28, S, HANDLE_REACH, 0),
    link(28, 160, 256, 256, S, 0),
    link(256, 256, 484, 352, S, HANDLE_REACH, 0),
    handle(256, 256),
  ],
  'sketch-project': [
    rect(128, 0, 256, 144),
    dashed(160, 176, 160, 368, 2),
    dashed(352, 176, 352, 368, 2),
    seg(28, 484, 484, 484),
    rect(128, 400, 256, 112),
  ],
  'sketch-offset': [
    arc(256, 512, 456, 240, 300),
    arc(256, 512, 232, 240, 300),
    arrow(256, 116, -90, 104, 176),
  ],
  'sketch-mirror': [
    link(72, 440, 136, 72, 64),
    handle(72, 440),
    handle(136, 72),
    link(440, 440, 376, 72, 64),
    handle(440, 440),
    handle(376, 72),
    dashed(256, 0, 256, 512, 5),
  ],
  'dimension': [
    seg(24, 28, 24, 484, 48),
    seg(488, 28, 488, 484, 48),
    seg(152, 256, 360, 256, 56),
    arrow(80, 256, 180, 128, 192),
    arrow(432, 256, 0, 128, 192),
  ],

  // Sketch constraints
  'constraint-horizontal': [
    seg(88, 256, 424, 256, 72),
    circle(88, 256, 88),
    circle(424, 256, 88),
  ],
  'constraint-vertical': [
    seg(256, 88, 256, 424, 72),
    circle(256, 88, 88),
    circle(256, 424, 88),
  ],
  'constraint-coincident': [
    link(36, 476, 256, 152, 64, 0, 120 + GAP + 32),
    link(256, 152, 476, 448, 64, 120 + GAP + 32, 0),
    circle(256, 152, 120),
  ],
  'constraint-touch': [
    arc(256, 560, 300, 220, 320, 64),
    circle(256, 260, 120),
  ],
  'constraint-midpoint': [
    seg(28, 256, 484, 256),
    seg(28, 144, 28, 368),
    seg(484, 144, 484, 368),
    shape([256, 120], [392, 256], [256, 392], [120, 256]),
  ],
  'constraint-intersection': [
    seg(28, 484, 484, 28),
    seg(28, 160, 484, 352),
    ring(256, 256, 104),
  ],
  'constraint-perpendicular': [
    seg(44, 468, 468, 468, 88),
    seg(256, 44, 256, 468, 88),
  ],
  'constraint-parallel': [
    seg(100, 476, 268, 36, 72),
    seg(244, 476, 412, 36, 72),
  ],
  'constraint-concentric': [
    ring(256, 256, 228),
    ring(256, 256, 112),
    circle(256, 256, 40),
  ],
  'constraint-tangent': [
    ring(256, 304, 168, 80),
    seg(40, 136, 472, 136, 80),
  ],
  'constraint-symmetric': [
    circle(88, 256, 88),
    circle(424, 256, 88),
    dashed(256, 0, 256, 512, 4, 72),
  ],

  // Display modes
  'display-shaded': [
    cubeFaces(solidCube, 16),
  ],
  'display-shaded-edges': [
    cubeEdges(strokedCube.visible, 56),
    cubeFaces(strokedCube, 56, 12),
  ],
  'display-hidden-lines': [
    cubeEdges(strokedCube.visible, 56),
    cubeDashes(strokedCube.hidden, 48),
  ],
  'display-wireframe': [
    cubeEdges([...strokedCube.visible, ...strokedCube.hidden], 56),
  ],

  // Zoom
  'zoom-fit-all': [
    viewFrame(),
    rect(104, 104, 128, 128),
    rect(280, 104, 128, 128),
    rect(104, 280, 128, 128),
    rect(280, 280, 128, 128),
  ],
  'zoom-fit-active': [
    viewFrame(),
    circle(256, 256, 108),
  ],
  'zoom-fit-selection': [
    viewFrame(),
    // Centroid on the icon's center
    arrow(124, 124, -135, 280, 240),
  ],

  // Feature options
  'fillet-constant-radius': [
    filletProfile,
    circle(FILLET_CENTER, FILLET_CENTER, 48),
    seg(FILLET_CENTER, FILLET_CENTER, ...polar(FILLET_CENTER, FILLET_CENTER, 176, 176, 225), 56),
    arrow(...polar(FILLET_CENTER, FILLET_CENTER, 324, 324, 225), 225, 160, 208),
  ],
  'fillet-constant-width': [
    filletProfile,
    seg(168, 344, 344, 168, 56),
    arrow(88, 424, 135, 120, 176),
    arrow(424, 88, -45, 120, 176),
  ],
  'extent-to-face': [
    rect(0, 0, 512, 96),
    seg(256, 288, 256, 464, 96),
    arrow(256, 128, -90, 160, 288),
  ],
}

export const customIcons = Object.entries(definitions).map(([iconName, parts]) => ({
  prefix: 'fas',
  iconName,
  icon: [512, 512, [], '', parts.join('')],
}))
