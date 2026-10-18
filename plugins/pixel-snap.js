// Pads content-sized elements to whole device pixels, as Chrome misaligns
// backdrop-filtered layers when an element's edges land at different subpixel offsets

const snap = (el) => {
  el.style.paddingRight = ''
  const ratio = window.devicePixelRatio
  const width = el.getBoundingClientRect().width * ratio
  const extra = (Math.ceil(width - 0.01) - width) / ratio
  if (extra > 0) el.style.paddingRight = `calc(${getComputedStyle(el).paddingRight} + ${extra}px)`
}

const observer = new ResizeObserver(entries => entries.forEach(entry => snap(entry.target)))

export default defineNuxtPlugin((nuxtApp) => {
  nuxtApp.vueApp.directive('pixel-snap', {
    mounted: (el) => observer.observe(el),
    beforeUnmount: (el) => observer.unobserve(el),
  })
})
