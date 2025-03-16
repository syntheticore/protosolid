export let lastId = BigInt(0)

export function makeID() {
  return 'id-' + (++lastId)
}

export function setLastId(id) {
  lastId = BigInt(id)
}
