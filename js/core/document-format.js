import Serialize from './serialize.js'
import { Repository, isRepositoryDump } from './repository.js'

// Reads a .cad file, which is either a plain document dump or a versioned
// repository whose checked out commit holds the document dump.
export function readDocumentFile(data) {
  const dump = JSON.parse(data)
  if(!isRepositoryDump(dump)) return { repository: null, content: dump }
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
