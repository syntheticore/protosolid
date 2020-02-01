import Vue from 'vue';
import App from './components/app.vue';

Vue.config.productionTip = false;
Vue.config.devtools = false;

new Vue({
  render: function (h) { return h(App) }
}).$mount('#app');
