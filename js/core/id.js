export let lastId = BigInt(0)

export function makeID() {
  return 'id-' + (++lastId)
}
