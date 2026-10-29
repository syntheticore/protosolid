import Serialize from './serialize.js'
import { Repository, isRepositoryDump } from './repository.js'
import { migrateDocument } from './document-migration.js'

// Reads a .cad file, which is either a plain document dump or a versioned
// repository whose checked out commit holds the document dump.
export function readDocumentFile(data) {
  const dump = JSON.parse(data)
  if(!isRepositoryDump(dump)) return { repository: null, content: migrateDocument(dump) }
  const repository = new Repository(dump)
  return { repository, content: repository.read() }
}

// Plain document data of the checked out version, for consumers that don't need history
export function checkedOutData(data) {
  const { repository, content } = readDocumentFile(data)
  return repository ? JSON.stringify(content) : data
}

export function parseDocument(data, context) {
  return Serialize.deserialize(readDocumentFile(data).content, context)
}

// Indented JSON for debug files. Arrays of plain values, like IDs, vectors
// and incidence paths, stay on one line. So do topology descriptions, which
// mix plain values with such arrays.
export function formatDocument(value, indent = '') {
  const plain = value => value === null || typeof value != 'object'
  const flat = value => Array.isArray(value) && value.every(plain)
  const inline = value => plain(value) || flat(value) ||
    (Array.isArray(value) && value.some(plain) && value.every(entry => plain(entry) || flat(entry)))
  if(inline(value)) return JSON.stringify(value)
  const inner = indent + '  '
  const entries = Array.isArray(value) ? value.map(entry => formatDocument(entry, inner)) :
    Object.entries(value).filter(([, entry]) => entry !== undefined)
      .map(([key, entry]) => `${JSON.stringify(key)}: ${formatDocument(entry, inner)}`)
  if(!entries.length) return Array.isArray(value) ? '[]' : '{}'
  const [open, close] = Array.isArray(value) ? ['[', ']'] : ['{', '}']
  return `${open}\n${entries.map(entry => inner + entry).join(',\n')}\n${indent}${close}`
}

// Compact documents are stored gzipped
export async function gzip(text) {
  const stream = new Blob([text]).stream().pipeThrough(new CompressionStream('gzip'))
  return new Uint8Array(await new Response(stream).arrayBuffer())
}

// Text of a loaded file, which may be gzipped
export function decodeFile(bytes) {
  const stream = new Blob([bytes]).stream()
  const gzipped = bytes[0] == 0x1f && bytes[1] == 0x8b
  return new Response(gzipped ? stream.pipeThrough(new DecompressionStream('gzip')) : stream).text()
}
