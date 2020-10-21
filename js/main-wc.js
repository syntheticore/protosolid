import Vue from 'vue'
import wrap from '@vue/web-component-wrapper'

import './main-common.js'
import App from './components/app.vue'

Vue.config.productionTip = false
Vue.config.devtools = false

const wrappedElement = wrap(Vue, App)
window.customElements.define('alchemy-editor', wrappedElement)

document.body.setAttribute('data-platform', 'browser')
