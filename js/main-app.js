import Vue from 'vue'
import './icons.js'
import App from './components/app.vue'

Vue.config.productionTip = false
Vue.config.devtools = false

new Vue({
  render: function (h) { return h(App) }
}).$mount('#app')

document.body.setAttribute(
  'data-platform',
  window.electron && window.electron.platform || 'browser'
)

if(
  window.electron &&
  window.electron.platform == 'darwin' &&
  window.electron.platformVersion < "18.0.0"
) {
  document.body.setAttribute('data-darwin-old', true)
}
