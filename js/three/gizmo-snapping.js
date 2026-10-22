import { Vector3 } from 'three'

export function translationSnapStep(camera, position, viewportHeight) {
  const depth = -new Vector3().copy(position).applyMatrix4(camera.matrixWorldInverse).z
  const visibleHeight = camera.isOrthographicCamera ?
    (camera.top - camera.bottom) / camera.zoom :
    2 * depth * Math.tan(camera.getEffectiveFOV() * Math.PI / 360)

  // Keep steps at least eight CSS pixels apart, using the same clean
  // increments at every scale: ... 1, 0.5, 0.25, 0.1, 0.05 ...
  const target = visibleHeight * 8 / viewportHeight
  const magnitude = 10 ** Math.floor(Math.log10(target))
  const step = [1, 2.5, 5, 10].find(value => value >= target / magnitude)
  return Number((step * magnitude).toPrecision(15))
}

export function snapTranslation(value, step) {
  return Number((Math.round(value / step) * step).toPrecision(15))
}
