import Vue from 'vue'
import './icons.js'
import App from './components/website.vue'

Vue.config.productionTip = false
Vue.config.devtools = false

new Vue({
  render: function (h) { return h(App) }
}).$mount('#app')
