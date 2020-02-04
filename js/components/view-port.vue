<template lang="pug">
  .view-port
    canvas
</template>


<style lang="stylus" scoped>
  .view-port
    position: relative
    overflow: hidden
    border-top: 1px solid #152b41
  canvas
    background: radial-gradient(50% 150%, farthest-corner, #2f4553 * 0.7, #08111b * 1.4)
</style>


<script>
  import * as THREE from 'three'
  import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
  import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js'
  import { DragControls } from 'three/examples/jsm/controls/DragControls.js'
  import { HDRCubeTextureLoader } from 'three/examples/jsm/loaders/HDRCubeTextureLoader.js';

  var rendering = true;
  var renderer, controls, scene, camera, mesh;

  export default {
    name: 'ViewPort',

    mounted: function() {
      renderer = new THREE.WebGLRenderer({
        canvas: this.$el.querySelector('canvas'),
        antialias: window.devicePixelRatio <= 1.0,
        alpha: true
      });

      renderer.setPixelRatio(window.devicePixelRatio);
      renderer.outputEncoding = THREE.sRGBEncoding;
      renderer.physicallyCorrectLights = true;
      renderer.shadowMap.enabled = true;
      // renderer.shadowMap.type = THREE.VSMShadowMap;
      // renderer.toneMapping = THREE.ReinhardToneMapping;
      // renderer.toneMapping = THREE.LinearToneMapping;
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      // renderer.toneMappingExposure = 1.2;

      camera = new THREE.PerspectiveCamera(70, 1, 0.01, 10000);
      camera.position.x = 3;
      camera.position.y = 2;
      camera.position.z = 3;

      scene = new THREE.Scene();
      // scene.fog = new THREE.Fog(0xcce0ff, 0.1, 20);
      // scene.add(new THREE.AmbientLight(0x666666));
      var sun = new THREE.DirectionalLight(0xdfebff, 1);
      sun.position.set(0, 200, 0);
      sun.castShadow = true;
      // sun.shadow.mapSize.width = 1024;
      // sun.shadow.mapSize.height = 1024;
      scene.add(sun);

      var light = new THREE.HemisphereLight( 0xffffbb, 0x080820, 1 );
      scene.add( light );
   
      // geometry = new THREE.BoxGeometry( 0.2, 0.2, 0.2 );
      var geometry = new THREE.TorusKnotBufferGeometry(1, 0.4, 200, 35);
      var material = new THREE.MeshStandardMaterial({
        color: 'coral',
        roughness: 0,
        metalness: 0.1,
      });
   
      mesh = new THREE.Mesh(geometry, material);
      mesh.castShadow = true;
      mesh.receiveShadow = true;
      scene.add(mesh);

      var groundGeo = new THREE.PlaneBufferGeometry(10, 10);
      groundGeo.rotateX(- Math.PI / 2);
      var ground = new THREE.Mesh(groundGeo, new THREE.ShadowMaterial({opacity: 0.2}));
      ground.receiveShadow = true;
      ground.position.y = -1.85;
      scene.add(ground);

      var grid = new THREE.GridHelper(20, 20);
      grid.position.y = -1.8;
      grid.material.opacity = 0.1;
      grid.material.transparent = true;
      scene.add(grid);
   
      var pmremGenerator = new THREE.PMREMGenerator(renderer);
      pmremGenerator.compileCubemapShader();

      new HDRCubeTextureLoader()
      .setPath('textures/cubemap/')
      .setDataType(THREE.UnsignedByteType)
      .load(['px.hdr', 'nx.hdr', 'py.hdr', 'ny.hdr', 'pz.hdr', 'nz.hdr'], (texture) => {
        var envMap = pmremGenerator.fromCubemap(texture).texture;
        scene.environment = envMap;
        texture.dispose();
        pmremGenerator.dispose();
        this.render();
      });

      controls = new OrbitControls( camera, renderer.domElement );
      controls.enableDamping = true;
      controls.damping = 0.2;
      controls.panSpeed = 1.5;
      controls.keyPanSpeed = 12;
      controls.zoomSpeed = 0.5;
      controls.screenSpacePanning = true;
      controls.rotateSpeed = 1.6;
      // controls.autoRotate = true;
      controls.addEventListener('change', this.render.bind(this));

      var transformControl = new TransformControls(camera, renderer.domElement);
      transformControl.space = 'world';
      // transformControl.translationSnap = 0.5;
      // transformControl.rotationSnap = THREE.MathUtils.degToRad(10) ;
      // transformControl.setMode('rotate');
      transformControl.addEventListener('change', this.render);
      transformControl.addEventListener('dragging-changed', function(event) {
        controls.enabled = !event.value;
      });
      transformControl.addEventListener('objectChange', function(event) {});
      scene.add(transformControl);
      transformControl.attach(mesh);

      // var dragcontrols = new DragControls([mesh], camera, renderer.domElement);
      // dragcontrols.addEventListener('hoveron', function(event) {
      //   transformControl.attach(event.object);
      // });

      window.addEventListener('resize', this.onWindowResize.bind(this), false);
      this.onWindowResize();
      this.animate();
    },

    beforeDestroy: function() {
      rendering = false;
      window.removeEventListener('resize', this.onWindowResize, false);
    },

    methods: {
      animate: function() {
        if(!rendering) return;
        requestAnimationFrame(this.animate.bind(this));
        controls.update();
        // mesh.rotation.x += 0.01; mesh.rotation.y += 0.01;
        // render();
      },
      render: function() {
        renderer.render(scene, camera);
      },
      onWindowResize: function() {
        const canvas = this.$el.querySelector('canvas');
        if(!canvas) return;
        renderer.setSize(canvas.parentElement.offsetWidth, canvas.parentElement.offsetHeight);
        camera.aspect = canvas.parentElement.offsetWidth / canvas.parentElement.offsetHeight;
        camera.updateProjectionMatrix();
        this.render();
      },
    }
  }
</script>
