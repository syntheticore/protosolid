import { access, cp, mkdir, readdir } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const root = fileURLToPath(new URL('..', import.meta.url))
const sourceDirectory = path.join(root, '.output', 'public')
const destinationDirectory = path.join(root, 'electron', 'app')

await access(sourceDirectory)
await mkdir(destinationDirectory, { recursive: true })

for(const entry of await readdir(sourceDirectory)) {
  await cp(
    path.join(sourceDirectory, entry),
    path.join(destinationDirectory, entry),
    { recursive: true, force: true },
  )
}
