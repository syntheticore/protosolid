import { mkdir, readdir, rm } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const root = fileURLToPath(new URL('..', import.meta.url))
const outputDirectory = path.join(root, '.output')
const electronAppDirectory = path.join(root, 'electron', 'app')

await rm(outputDirectory, { recursive: true, force: true })
await mkdir(electronAppDirectory, { recursive: true })

for(const entry of await readdir(electronAppDirectory)) {
  await rm(path.join(electronAppDirectory, entry), { recursive: true, force: true })
}
