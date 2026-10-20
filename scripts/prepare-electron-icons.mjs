import { mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { Resvg } from '@resvg/resvg-js'

const root = fileURLToPath(new URL('../', import.meta.url))
const output = path.resolve(root, process.argv[2] || 'electron/build')
const svg = await readFile(path.join(root, 'electron/build/icon-source.svg'), 'utf8')
const canvas = 1024

// Collect the coordinates of all horizontal and vertical path edges.
function edges(svg) {
  const xs = new Set()
  const ys = new Set()
  for (const [, d] of svg.matchAll(/ d="([^"]+)"/g)) {
    let start, last
    for (const [, command, args] of d.matchAll(/([MLQZ])([^MLQZ]*)/g)) {
      const numbers = args.trim().split(/[\s,]+/).filter(Boolean).map(Number)
      const point = command === 'Z' ? start : numbers.slice(-2)
      if (command !== 'M' && (point[0] === last[0]) !== (point[1] === last[1])) {
        if (point[0] === last[0]) xs.add(point[0])
        else ys.add(point[1])
      }
      if (command === 'M') start = point
      last = point
    }
  }
  return [[...xs].sort((a, b) => a - b), [...ys].sort((a, b) => a - b)]
}

const spans = keys => keys.slice(1).map((k, i) => k - keys[i])

// Move edges onto whole pixels of a size x size render of the square frame
// [x, y, side], keeping a blank pixel around the glyph, stroke widths as close
// as possible to their true size and equal strokes equal across both axes.
// Returns the coordinate mapping for each axis.
function fit(size, [x, y, side]) {
  const unit = side / size
  const axes = [[xs, x], [ys, y]]
  const original = [...spans(xs), ...spans(ys)]
  let best
  for (let scale = 0.95; scale <= 1.05; scale += 0.005) {
    const [optionsX, optionsY] = axes.map(([keys, origin]) => {
      const center = origin + side / 2
      const options = []
      for (let shift = -0.5; shift < 0.5; shift += 1 / 16) {
        const pixels = keys.map(k => Math.round(((k - center) * scale + center - origin) / unit + shift))
        if (pixels[0] < 1 || pixels.at(-1) > size - 1) continue
        if (pixels.some((p, i) => i && p - pixels[i - 1] < 1)) continue
        let cost = 0
        for (let i = 0; i < keys.length; i++) {
          if (i) cost += (pixels[i] - pixels[i - 1] - (keys[i] - keys[i - 1]) / unit) ** 2
          cost += 0.25 * (pixels[i] - (keys[i] - origin) / unit) ** 2
        }
        // Break ties towards bolder strokes, which read better at small sizes
        cost -= 0.01 * (pixels.at(-1) - pixels[0])
        options.push({ cost, pixels })
      }
      return options
    })
    for (const optionX of optionsX) {
      for (const optionY of optionsY) {
        const fitted = [...spans(optionX.pixels), ...spans(optionY.pixels)]
        let cost = optionX.cost + optionY.cost
        for (let i = 0; i < original.length; i++) {
          for (let j = i + 1; j < original.length; j++) {
            if (Math.abs(original[i] - original[j]) < 1 && fitted[i] !== fitted[j]) cost += 10
          }
        }
        if (!best || cost < best.cost) best = { cost, pixels: [optionX.pixels, optionY.pixels] }
      }
    }
  }
  return axes.map(([keys, origin], axis) => {
    const targets = best.pixels[axis].map(p => origin + p * unit)
    return c => {
      if (c <= keys[0]) return c + targets[0] - keys[0]
      const i = keys.findIndex(k => c <= k)
      if (i < 0) return c + targets.at(-1) - keys.at(-1)
      return targets[i - 1] + (c - keys[i - 1]) * (targets[i] - targets[i - 1]) / (keys[i] - keys[i - 1])
    }
  })
}

// Grid-fit the artwork to the pixels of each render; like font hinting,
// only the renders change, never the master.
function hint(size, frame) {
  const [x, y, side] = frame
  const [mapX, mapY] = fit(size, frame)
  const round = v => +v.toFixed(2)
  return svg
    .replace(/\s*<!--[\s\S]*?-->/g, '')
    .replace(/viewBox="[^"]*"/, `width="${size}" height="${size}" viewBox="${x} ${y} ${side} ${side}"`)
    .replace(/ d="([^"]+)"/g, (_, d) => ` d="${d.replace(/(-?[\d.]+)[\s,]+(-?[\d.]+)/g,
      (_, x, y) => `${round(mapX(+x))} ${round(mapY(+y))}`)}"`)
    .replace(/ (x[12])="(-?[\d.]+)"/g, (_, attr, v) => ` ${attr}="${round(mapX(+v))}"`)
    .replace(/ (y[12])="(-?[\d.]+)"/g, (_, attr, v) => ` ${attr}="${round(mapY(+v))}"`)
}

function render(size, frame) {
  return new Resvg(hint(size, frame)).render().asPng()
}

const [xs, ys] = edges(svg)

// Windows and Linux icons fill their square like the artwork does,
// while macOS expects padding around the glyph.
const filled = [0, 0, canvas]
const padded = [-canvas / 8, -canvas / 8, canvas * 1.25]

const sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256, 512, 1024]
const renders = frame => new Map(sizes.map(size => [size, render(size, frame)]))
const mac = renders(padded)
const pngs = renders(filled)

function ico(pngs, sizes) {
  const directory = Buffer.alloc(6 + 16 * sizes.length)
  directory.writeUInt16LE(1, 2)
  directory.writeUInt16LE(sizes.length, 4)
  let offset = directory.length
  for (const [index, size] of sizes.entries()) {
    const entry = 6 + index * 16
    const png = pngs.get(size)
    directory[entry] = size % 256
    directory[entry + 1] = size % 256
    directory.writeUInt16LE(1, entry + 4)
    directory.writeUInt16LE(32, entry + 6)
    directory.writeUInt32LE(png.length, entry + 8)
    directory.writeUInt32LE(offset, entry + 12)
    offset += png.length
  }
  return Buffer.concat([directory, ...sizes.map(size => pngs.get(size))])
}

function icns(pngs, entries) {
  const chunks = entries.map(([type, size]) => {
    const header = Buffer.alloc(8)
    header.write(type, 0, 'ascii')
    header.writeUInt32BE(8 + pngs.get(size).length, 4)
    return Buffer.concat([header, pngs.get(size)])
  })
  const header = Buffer.alloc(8)
  header.write('icns', 0, 'ascii')
  header.writeUInt32BE(8 + chunks.reduce((sum, chunk) => sum + chunk.length, 0), 4)
  return Buffer.concat([header, ...chunks])
}

await rm(path.join(output, 'icons'), { recursive: true, force: true })
await mkdir(path.join(output, 'icons'), { recursive: true })
for (const size of [16, 24, 32, 48, 64, 128, 256, 512, 1024]) {
  await writeFile(path.join(output, 'icons', `${size}x${size}.png`), pngs.get(size))
}
await writeFile(path.join(output, 'icon.png'), pngs.get(1024))
await writeFile(path.join(output, 'icon.ico'), ico(pngs, [16, 20, 24, 32, 40, 48, 64, 128, 256]))
await writeFile(path.join(output, 'icon.icns'), icns(mac, [
  ['icp4', 16], ['ic11', 32], ['icp5', 32], ['ic12', 64], ['icp6', 64],
  ['ic07', 128], ['ic08', 256], ['ic13', 256], ['ic09', 512], ['ic14', 512], ['ic10', 1024],
]))
await writeFile(path.join(root, 'public/favicon.ico'), ico(pngs, [16, 24, 32, 48]))
console.log(`Generated Electron PNG, ICO, ICNS, and PNG sizes in ${output}`)
