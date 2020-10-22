import Vue from 'vue';
import './main-common.js';
import App from './components/app.vue';

Vue.config.productionTip = false;
Vue.config.devtools = false;

new Vue({
  render: function (h) { return h(App) }
}).$mount('#app')

document.body.setAttribute('data-platform', window.electronPlatform || 'browser')
if(window.electronPlatform == 'darwin' && window.electronPlatformVersion < "18.0.0") {
  document.body.setAttribute('data-darwin-old', true)
}
