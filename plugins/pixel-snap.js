// Pads content-sized elements to whole device pixels, as Chrome misaligns
// backdrop-filtered layers when an element's edges land at different subpixel offsets.
// An optional selector moves the padding to a child that should stay flush with the right edge

const snap = (el) => {
  const target = el.pixelSnapTarget || el
  target.style.paddingRight = ''
  const ratio = window.devicePixelRatio
  const width = el.getBoundingClientRect().width * ratio
  const extra = (Math.ceil(width - 0.01) - width) / ratio
  if (extra > 0) target.style.paddingRight = `calc(${getComputedStyle(target).paddingRight} + ${extra}px)`
}

const observer = new ResizeObserver(entries => entries.forEach(entry => snap(entry.target)))

export default defineNuxtPlugin((nuxtApp) => {
  nuxtApp.vueApp.directive('pixel-snap', {
    mounted: (el, { value }) => {
      if (value) el.pixelSnapTarget = el.querySelector(value)
      observer.observe(el)
    },
    beforeUnmount: (el) => observer.unobserve(el),
  })
})
