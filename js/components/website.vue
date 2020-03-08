<template lang="pug">
  #app
    header
      .content
        .logo
          fa-icon(icon="vial")
          span Alchemy <em>Beta</em>
        nav
          a(href="#") Features
          a(href="#") Downloads
          a(href="#") Documentation
    main
      
      section.hero
        .content
          div.text
            hgroup
              h1 Mechanical CAD rethought
            p Alchemy is a cross-platform, open-source, mechanical design software with a new twist on parametric modeling
          figure
            img(src="img/screenshot-1.png")
      
      section.benefits
        .content
          .benefit
            h2 Direct Parametrics
            p.
              Lorem ipsum dolor sit amet, consectetur adipisicing elit, sed do eiusmod
              tempor incididunt ut labore et dolore magna aliqua.

            h3 Constant Productivity
            h3 Poses
            h3 Configurations
            h3 Pretended History
          .benefit
            h2 Workflow
            p.
              Lorem ipsum dolor sit amet, consectetur adipisicing elit, sed do eiusmod
              tempor incididunt ut labore et dolore magna aliqua.

            h3 Modeless Workspace
            h3 Visual Tools
            h3 Short mouse travel
            h3 Selection Sets
            h3 Visibility Sets
            h3 Saved Views
          .benefit
            h2 Performance
            p.
              Lorem ipsum dolor sit amet, consectetur adipisicing elit, sed do eiusmod
              tempor incididunt ut labore et dolore magna aliqua.

            h3 Based on shapex kernel
            h3 No need to re-solve everything
            h3 Parallel

      section.parametrics
        .content
          h2 Direct Parametrics
          p Direct Parametrics rock

          h3 Constant Productivity
          p.
            With typical history-based, parametric CAD software, your productivity ceases with the rising complexity of your projects. Finding the right place in history to place new features becomes an ever increasing effort and updating your model - once a really fast process - now takes forever. And that's only if it doesn't fail, resulting in a time consuming debugging process.
            <br>
            Direct modeling applications try to solve these problems, but don't address the loss of parametrism.
            <br>
            Alchemy offers a new approach that combines the best of both worlds. By applying dimensions and constraints directly to geometry, instead of the sketches that created it, parametric updates can occur without the need for recalculating the whole model, resulting in a much more interactive workflow. 
          h3 Poses
          h3 Configurations
          h3 Pretended History
      
      section.workflow
        .content
          h2 Workflow
          p.
            Lorem ipsum dolor sit amet, consectetur adipisicing elit, sed do eiusmod
            tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam,
            quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo
            consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse
            cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non
            proident, sunt in culpa qui officia deserunt mollit anim id est laborum.

          h3 Modeless Workspace
          h3 Visual Tools
          h3 Short mouse travel
          h3 Selection Sets
          h3 Visibility Sets
          h3 Saved Views
      
      section.performance
        .content
          h2 Performance
          p.
            Lorem ipsum dolor sit amet, consectetur adipisicing elit, sed do eiusmod
            tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam,
            quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo
            consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse
            cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non
            proident, sunt in culpa qui officia deserunt mollit anim id est laborum.

          h3 Based on shapex kernel
          h3 No need to re-solve everything
          h3 Parallel

      footer
</template>


<style lang="stylus" scoped>
  $main = #2f0940

  #app
    width: 100%
    overflow: hidden
    font-family: 'Baloo Paaji'
    background: $main

  main
    width: 100%

  header
    font-size: 13px
    padding: 18px 40px
    box-shadow: 0 0 4px rgba(black, 0.4)
    position: absolute
    width: 100%
    z-index: 3
    .content
      width: 100%
      max-width: 1150px
      display: flex
      margin: 0 auto
      align-items: center
    .logo
      font-size: 28px
      // font-family: 'Baloo Paaji'
      svg
        font-size: 26px
        margin-right: 6px
      em
        color: #7c07ff
        background: linear-gradient(#ff38f6, #1f00ea)
        -webkit-background-clip: text
        -webkit-text-fill-color: transparent
    nav
      margin-left: auto
      a
        color: white
        font-size: 16px
        text-decoration: none
        margin-left: 44px
        font-family: 'Teko'
        font-size: 22px
        transition: all 0.2s
        &:hover
          color: $main

  section
    color: white
    h2
      font-family: 'Teko'
      font-size: 42px
      margin-bottom: 12px
      text-align: center
    h3
      font-family: 'Teko'
      font-size: 22px
      margin-bottom: 12px
    p
      margin-bottom: 12px
      font-family: 'Quicksand'
      font-size: 18px
      line-height: 1.4
      

  .hero
    padding-top: 120px
    padding-bottom: 290px
    margin-bottom: -290px
    background: url('img/hero.png')
    background-repeat: no-repeat
    background-position: top
    // background-position: center
    // background-size: auto 100%
    background-size: 100% 100%
    .content
      display: flex
      align-items: center
      // justify-content: center
    .text
      margin-right: 30px
      width: 520px
      flex: 0 1 auto
    h1
      font-family: 'Knewave'
      // font-weight: bold
      font-size: 52px
      line-height: 1.2
      margin-top: 12px
      color: rgb(50, 22, 91)
      // color: #7c07ff
    p
      margin-top: 16px
      font-family: 'Quicksand'
      font-weight: 600
      font-size: 20px
      color: gray
      line-height: 1.3
      color: #7b51af
    figure
      flex: 0 1 auto
      margin-right: -32px
    img
      max-width: 100%
      // margin-top: 32px

  .benefits .content
    display: flex

  .benefit
    flex-grow: 1
    flex-basis: 0
    background: rgba(white, 0.3)
    margin: 24px
    padding: 28px
    border-radius: 5px
    &:first-child
      margin-left: 0
    &:last-child
      margin-right: 0

  footer
    height: 400px
    // background: darkgray

  .wrapper
    font-family: 'Oxanium'
    font-family: 'Teko'
    font-family: 'Poiret One'
    font-family: 'Rock Salt'
    font-family: 'Kaushan Script'
    font-family: 'Russo One'
    font-family: 'Viga'
    font-family: 'Days One'
    font-family: 'Bungee'
    font-family: 'Knewave'
    font-family: 'Krona One'
    font-family: 'Sulphur Point'
    font-family: 'Baloo Paaji'
    font-family: 'Blinker'
</style>


<script>
  export default {
    name: 'app',

    components: {

    },

    created() {

    },

    mounted() {

    },

    methods: {

    },
    
    data() {
      return {}
    }
  }
</script>
