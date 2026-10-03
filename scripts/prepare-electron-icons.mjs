import { copyFile, mkdir, readFile, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const output = path.resolve(root, process.argv[2] || 'electron/build')
const source = path.join(root, 'electron/build/icon-source.png')
const converted = path.join(output, '.converted')

// Resolve through Electron Builder so this works with nested npm dependencies too.
const require = createRequire(import.meta.url)
const builderRequire = createRequire(require.resolve('electron-builder/package.json'))
const { executeAppBuilder } = builderRequire('builder-util')

async function convert(format, input, destination) {
  const result = JSON.parse(await executeAppBuilder([
    'icon', '--format', format, '--input', input, '--out', destination,
  ]))
  if (result.error) throw new Error(result.error)
  return result.icons
}

await mkdir(path.join(output, 'icons'), { recursive: true })
await convert('icns', source, output)
const icons = await convert('set', path.join(output, 'icon.icns'), converted)
const pngs = new Map()
for (const { file, size } of icons) {
  pngs.set(size, await readFile(file))
  await copyFile(file, path.join(output, 'icons', `${size}x${size}.png`))
}
await writeFile(path.join(output, 'icon.png'), pngs.get(1024))

// Assemble a multi-resolution Windows ICO, using the downscaled PNG alpha.
const sizes = [16, 32, 48, 64, 128, 256]
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
await writeFile(path.join(output, 'icon.ico'), Buffer.concat([
  directory, ...sizes.map(size => pngs.get(size)),
]))
console.log(`Generated Electron PNG, ICO, ICNS, and PNG sizes in ${output}`)
