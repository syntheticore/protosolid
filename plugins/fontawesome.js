import { library, config } from '@fortawesome/fontawesome-svg-core'
import { FontAwesomeIcon } from '@fortawesome/vue-fontawesome'
import { fas } from '@fortawesome/free-solid-svg-icons'
// import { far } from '@fortawesome/free-regular-svg-icons'
// import { fab } from '@fortawesome/free-brands-svg-icons'
import { customIcons } from '../js/icons.js'

config.autoAddCss = true
library.add(fas, ...customIcons)
// library.add(far)
// library.add(fab)

export default defineNuxtPlugin((nuxtApp) => {
  nuxtApp.vueApp.component('Icon', FontAwesomeIcon, {})
})
