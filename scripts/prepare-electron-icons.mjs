import { existsSync, readFileSync } from 'node:fs'
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { Resvg } from '@resvg/resvg-js'

const root = fileURLToPath(new URL('../', import.meta.url))
const output = path.resolve(root, process.argv[2] || 'electron/build')
const svg = await readFile(path.join(root, 'electron/build/icon-source.svg'), 'utf8')
const canvas = 1024

// Windows and Linux icons fill their square like the artwork does,
// while macOS expects padding around the glyph.
const filled = [0, 0, canvas]
const padded = [-canvas / 8, -canvas / 8, canvas * 1.25]

// Small sizes use variants of the artwork laid out on their own pixel grid
// (vertical edges and the frame on whole pixels); larger ones scale the master.
function render(size, frame) {
  const hinted = path.join(root, 'electron/build/hinted', `${frame === padded ? 'mac-' : ''}${size}.svg`)
  if (existsSync(hinted)) return new Resvg(readFileSync(hinted, 'utf8')).render().asPng()
  const [x, y, side] = frame
  const sized = svg.replace(/viewBox="[^"]*"/, `width="${size}" height="${size}" viewBox="${x} ${y} ${side} ${side}"`)
  return new Resvg(sized).render().asPng()
}

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
