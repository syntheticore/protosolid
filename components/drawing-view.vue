<template lang="pug">

  .drawing-view(
    ref="root"
    @wheel.prevent="wheel"
    @pointerdown="pointerDown"
    @pointermove="pointerMove"
    @pointerup="pointerUp"
    @pointercancel="pointerUp"
    @pointerleave="pointer = null; updateOverlay()"
    @dblclick="doubleClick"
    @contextmenu.prevent
    :style="{ cursor }"
  )
    .sheet(
      ref="sheet"
      :style="{ transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale})` }"
    )
      .paper(v-html="svg")

      svg.overlay(ref="overlay" :viewBox="`0 0 ${size[0]} ${size[1]}`" xmlns="http://www.w3.org/2000/svg")
        rect.box(
          v-for="(box, i) in overlay.boxes" :key="'b' + i"
          :x="box.x" :y="box.y" :width="box.width" :height="box.height"
        )
        rect.band(
          v-for="(band, i) in overlay.bands" :key="'r' + i"
          :x="band.x" :y="band.y" :width="band.width" :height="band.height"
        )
        path.highlight(v-for="(d, i) in overlay.highlights" :key="'h' + i" :d="d")
        circle.highlight(v-for="(point, i) in overlay.points" :key="'p' + i" :cx="point[0]" :cy="point[1]" r="0.9")
        path.preview(
          v-for="(path, i) in overlay.paths" :key="'l' + i"
          :d="path.d" :stroke-width="path.width" :stroke-dasharray="path.dash"
          :class="{ filled: path.fill }"
        )
        text.preview(
          v-for="(text, i) in overlay.texts" :key="'t' + i"
          :x="text.x" :y="text.y" :font-size="text.size" :text-anchor="text.anchor"
          :transform="text.angle ? `rotate(${-text.angle} ${text.x} ${text.y})` : null"
        ) {{ text.text }}

        //- Values of the annotation being edited, typed or picked where they are drawn
        foreignObject.field(
          v-for="field in editorFields" :key="field.key"
          :x="field.x" :y="field.y" :width="field.width" :height="field.height"
          :transform="field.angle ? `rotate(${-field.angle} ${field.x} ${field.y})` : null"
        )
          button.choice(
            v-if="field.choices"
            :data-key="field.key"
            :title="field.label"
            @pointerdown.stop
            @click="pickChoice(field)"
            @keydown.stop="fieldKey($event, field)"
          )
          component(
            v-else
            :is="field.multiline ? 'textarea' : 'input'"
            :data-key="field.key"
            :value="field.value"
            :placeholder="field.placeholder"
            :style="{ fontSize: field.size + 'px', lineHeight: field.size * 1.4 + 'px', padding: `0 ${field.pad}px`, textAlign: field.align }"
            spellcheck="false"
            @pointerdown.stop
            @dblclick.stop
            @input="setField(field, $event.target.value)"
            @keydown.stop="fieldKey($event, field)"
          )

    //- Choices open from the clicked point, or the field they are for
    .menu-anchor(ref="menuAnchor" :style="{ left: (menu?.x || 0) + 'px', top: (menu?.y || 0) + 'px' }")

    PopupMenu(
      v-if="menu"
      :anchor="menu.anchor || $refs.menuAnchor"
      :options="menu.options"
      :value="menu.value"
      @choose="menu.resolve($event)"
      @close="menu.resolve(null)"
    )

</template>


<style lang="stylus" scoped>

  // Covers the viewport while a drawing is open, below toolbox and side bars
  .drawing-view
    position: absolute
    inset: 0
    padding: 110px 300px 90px
    // Like the viewport it covers, in both themes
    background: radial-gradient(farthest-corner at 50% 150%, var(--viewport-bg-inner), var(--viewport-bg-outer))
    box-sizing: border-box
    overflow: hidden

  .sheet
    position: relative
    width: 100%
    height: 100%
    transform-origin: 0 0

  .paper
    width: 100%
    height: 100%

    :deep(svg)
      width: 100%
      height: 100%
      // Lighter on the light theme's bright background, where it still lifts the paper
      filter: unquote('drop-shadow(0 2px 8px rgb(0 0 0 / calc(0.1 + 0.5 * var(--shadow-strength))))')

    // Paper tint on screen only, exports stay white
    :deep(svg > rect:first-child)
      fill: #fbf6e2

  .overlay
    position: absolute
    inset: 0
    width: 100%
    height: 100%
    pointer-events: none
    fill: none
    stroke-linecap: round
    stroke-linejoin: round

    .highlight
      stroke: $blue
      stroke-width: 0.9
      opacity: 0.8

    circle.highlight
      fill: $blue

    .preview
      stroke: $blue

      &.filled
        fill: $blue

    text.preview
      stroke: none
      fill: $blue
      font-family: Helvetica, Arial, sans-serif

    .box
      stroke: $blue
      stroke-width: 0.3
      stroke-dasharray: 1.5 1

    .band
      fill: $blue
      fill-opacity: 0.15

    .field
      pointer-events: auto
      overflow: visible

      input
      textarea
      button
        display: block
        width: 100%
        height: 100%
        margin: 0
        border: none
        border-radius: 0.6px
        box-sizing: border-box
        outline: none

      // Covers the drawn value with paper, like typing on the sheet
      input
      textarea
        background: #fbf6e2
        color: $blue
        font-family: Helvetica, Arial, sans-serif
        box-shadow: 0 0 0 0.3px rgba($blue, 0.5)
        resize: none
        overflow: hidden
        white-space: pre

        &::placeholder
          color: rgba($blue, 0.45)

        &:focus
          box-shadow: 0 0 0 0.4px $blue

      button
        padding: 0
        background: rgba($blue, 0.08)
        box-shadow: 0 0 0 0.3px rgba($blue, 0.5)
        cursor: pointer

        &:hover
        &:focus
          background: rgba($blue, 0.2)
          box-shadow: 0 0 0 0.4px $blue

  .menu-anchor
    position: absolute
    width: 0
    height: 0

</style>


<script>
  import { markRaw, toRaw } from 'vue'
  import { makeID } from '../js/core/id.js'
  import {
    createDrawingContext, drawingSheet, drawingSvg, annotationInk, layoutInitialViews, orientations, storeFrame,
    projectedFrame, projectedBounds, scaleFactor, drawingScales, nextLetter, isAligned, BREAK_GAP, breakSeams,
  } from '../js/core/drawing.js'
  import { entity, dimensionKind, orientationAt, fieldRect, textWidth } from '../js/core/drawing-annotations.js'

  // Matches the zoom speed of the viewport's orbit controls
  const ZOOM_SPEED = 2.6

  // Views and annotations in the document selection, named for the selection bar
  const itemTitles = {
    base: 'Base View', projected: 'Projected View', section: 'Section View', detail: 'Detail View', auxiliary: 'Auxiliary View',
    dimension: 'Dimension', baseline: 'Baseline Dimension', ordinate: 'Ordinate Dimension', centerline: 'Centerline',
    centermark: 'Center Mark', note: 'Note', hole: 'Hole Callout', balloon: 'Balloon', finish: 'Surface Finish',
    datum: 'Datum', gdt: 'Geometric Tolerance', weld: 'Weld Symbol', break: 'Break',
  }

  class DrawingItem {
    constructor(kind, data) {
      this.kind = kind
      this.id = data.id
      this.data = data
    }
    typename() {
      return itemTitles[this.data.type || this.kind]
    }
  }

  // Annotations without these values are dropped when editing ends
  const requiredFields = { note: 'text', finish: 'text', datum: 'letter' }

  const resizeCursor = brk => brk.axis == 'u' ? 'ew-resize' : 'ns-resize'

  const add = (a, b) => [a[0] + b[0], a[1] + b[1]]
  const sub = (a, b) => [a[0] - b[0], a[1] - b[1]]
  const scale = (a, s) => [a[0] * s, a[1] * s]
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1]
  const normalize = a => scale(a, 1 / (Math.hypot(a[0], a[1]) || 1))
  const pathData = points => 'M' + points.map(([x, y]) => `${x} ${y}`).join('L')

  function nearestOnSegment(point, a, b) {
    const d = sub(b, a)
    const t = Math.min(1, Math.max(0, dot(sub(point, a), d) / (dot(d, d) || 1)))
    return add(a, scale(d, t))
  }

  export default {
    name: 'DrawingView',

    inject: ['bus'],

    props: {
      document: Object,
    },

    data() {
      return {
        svg: '',
        size: [420, 297],
        scale: 1,
        offset: { x: 0, y: 0 },
        gesture: null,
        pointer: null,
        step: 0,
        menu: null,
        editing: null,
        editorFields: [],
        cursor: 'default',
        overlay: { boxes: [], bands: [], highlights: [], points: [], paths: [], texts: [] },
      }
    },

    computed: {
      component() {
        return this.document.top().findChild(this.document.activeDrawingComponentId)
      },

      tool() {
        return this.document.activeDrawingTool || 'select'
      },

      // View or annotation selected with the move tool
      selected() {
        const item = this.document.selection.items[0]
        return item instanceof DrawingItem ? item : null
      },

      // Geometry changes arrive with regeneration
      settings() {
        const drawing = this.document.activeDrawing
        return JSON.stringify([drawing.id, this.document.activeDrawingComponentId, this.component.creator.title, drawing.revision])
      },
    },

    watch: {
      settings() {
        this.render()
      },

      tool() {
        if(this.editing) this.commitEdit()
        this.reset()
        this.updateOverlay()
      },
    },

    created() {
      this.task = { view: null, picks: [] }
    },

    mounted() {
      this.document.on('regenerated', this.rebuild)
      this.bus.on('escape', this.escape)
      this.bus.on('keydown', this.keyDown)
      this.rebuild()
    },

    beforeUnmount() {
      this.document.off('regenerated', this.rebuild)
      this.bus.off('escape', this.escape)
      this.bus.off('keydown', this.keyDown)
      this.menu?.resolve(null)
      if(this.editing) this.commitEdit()
      this.document.selection.clear()
    },

    methods: {
      drawing() {
        return toRaw(this.document.activeDrawing)
      },

      rebuild() {
        this.context = markRaw(createDrawingContext(toRaw(this.component), toRaw(this.document.top())))
        const drawing = this.drawing()
        drawing.info ||= { partNumber: '', material: '', drawnBy: '', revision: '' }
        drawing.views ||= []
        drawing.annotations ||= []
        // Through the proxy, so the treelet shows the fitted scale
        if(!drawing.initialized) layoutInitialViews(this.context, this.document.activeDrawing)
        this.render()
      },

      render() {
        this.sheet = markRaw(drawingSheet(this.context, this.drawing()))
        this.size = [this.sheet.width, this.sheet.height]
        this.svg = drawingSvg(this.sheet)
        this.candidatePoints = null
        this.layoutEditor()
        this.updateOverlay()
      },

      changed() {
        this.document.activeDrawing.revision++
        this.document.markChanged()
      },

      reset() {
        this.task = { view: null, picks: [] }
        this.step = 0
        this.document.selection.clear()
      },

      // Picked model items and views show in the selection bar
      select(pick) {
        // Silhouettes have no model edge
        const item = pick.kind == 'view' ? new DrawingItem('view', pick.view.view) : pick.item || pick.source?.edge
        if(item) this.document.selection.add(item)
      },

      next() {
        this.step++
      },


      // Navigation, like the viewport: wheel and middle drag zoom about the
      // view's center, right drag or modified left drag pan

      pointerDown(event) {
        this.menu?.resolve(null)
        const pan = event.button == 2 || event.button == 0 && (event.shiftKey || event.ctrlKey || event.metaKey)
        if(pan || event.button == 1) {
          event.preventDefault()
          this.$el.setPointerCapture(event.pointerId)
          this.gesture = { type: pan ? 'pan' : 'zoom', x: event.clientX, y: event.clientY }
          this.updateOverlay()
          return
        }
        if(event.button != 0) return
        // Clicking off the fields finishes editing
        if(this.editing) return this.commitEdit()
        this.client = [event.clientX, event.clientY]
        const point = this.sheetPoint(event)
        if(this.tool == 'select') this.startMove(point, event)
        else this.click(point)
      },

      pointerMove(event) {
        if(this.gesture?.type == 'pan' || this.gesture?.type == 'zoom') {
          const dx = event.clientX - this.gesture.x
          const dy = event.clientY - this.gesture.y
          this.gesture.x = event.clientX
          this.gesture.y = event.clientY
          if(this.gesture.type == 'pan') this.offset = { x: this.offset.x + dx, y: this.offset.y + dy }
          else this.zoom(dy)
          return
        }
        this.pointer = this.sheetPoint(event)
        if(this.gesture?.type == 'move') this.move(this.pointer)
        this.updateOverlay()
      },

      pointerUp() {
        if(this.gesture?.type == 'move' && this.gesture.moved) this.changed()
        this.gesture = null
        this.updateOverlay()
      },

      wheel(event) {
        const unit = [1, 16, 100][event.deltaMode]
        this.zoom(event.deltaY * unit * (event.ctrlKey ? 10 : 1))
      },

      // Positive deltas zoom out
      zoom(delta) {
        if(!delta) return
        const step = 0.95 ** (ZOOM_SPEED * Math.abs(delta) * 0.01)
        const scale = Math.min(Math.max(delta > 0 ? this.scale * step : this.scale / step, 0.2), 50)
        const factor = scale / this.scale
        const center = { x: this.$refs.sheet.clientWidth / 2, y: this.$refs.sheet.clientHeight / 2 }
        this.offset = {
          x: center.x - (center.x - this.offset.x) * factor,
          y: center.y - (center.y - this.offset.y) * factor,
        }
        this.scale = scale
      },

      doubleClick(event) {
        const annotation = this.annotationAt(this.sheetPoint(event))
        if(annotation && this.tool == 'select') this.startEdit(annotation)
        else if(!annotation && !this.viewAt(this.sheetPoint(event))) {
          this.scale = 1
          this.offset = { x: 0, y: 0 }
        }
      },


      // Picking

      sheetPoint(event) {
        const point = new DOMPoint(event.clientX, event.clientY).matrixTransform(this.$refs.overlay.getScreenCTM().inverse())
        return [point.x, point.y]
      },

      tolerance() {
        return 6 / this.$refs.overlay.getScreenCTM().a
      },

      views() {
        return [...this.sheet.views.values()]
      },

      viewAt(point) {
        const margin = 3
        return this.views()
          .filter(view => point[0] > view.box.min.x - margin && point[0] < view.box.max.x + margin &&
            point[1] > view.box.min.y - margin && point[1] < view.box.max.y + margin)
          .sort((a, b) => area(a.box) - area(b.box))[0]
      },

      annotationAt(point) {
        const margin = 1
        return this.sheet.annotations
          .filter(({ box }) => point[0] > box.min.x - margin && point[0] < box.max.x + margin &&
            point[1] > box.min.y - margin && point[1] < box.max.y + margin)
          .sort((a, b) => area(a.box) - area(b.box))[0]?.annotation
      },

      // Nearest line of a view, optionally only model edges of given kinds
      lineAt(point, kinds) {
        let best = null
        const views = this.task.view ? [this.task.view] : this.views()
        for(const view of views) for(const line of view.lines) {
          if(kinds && !line.source) continue
          for(let i = 1; i < line.points.length; i++) {
            const nearest = nearestOnSegment(point, line.points[i - 1], line.points[i])
            const distance = Math.hypot(...sub(point, nearest))
            if(distance < (best?.distance ?? this.tolerance())) {
              best = { kind: 'line', view, line, source: line.source, sheet: nearest, distance }
            }
          }
        }
        if(!best || !kinds) return best
        best.entity = entity(best.source.edge, best.source.occurrence, this.context.component)
        return kinds.includes(best.entity.kind) ? best : null
      },

      // Vertices and circle centers of the model edges shown in views
      pointAt(point) {
        this.candidatePoints ||= this.views().flatMap(view => {
          const edges = new Set(view.lines.filter(line => line.source).map(line => line.source))
          return [...edges].flatMap(({ edge, occurrence }) => {
            const ids = edge.vertexIds()
            const vertices = edge.solid.vertices().filter(vertex => ids.includes(vertex.kernelId))
              .map(vertex => ({ item: vertex, occurrence, center: false }))
            const curve = entity(edge, occurrence, this.context.component)
            if(curve.kind == 'circle') vertices.push({ item: edge, occurrence, center: true })
            return vertices.map(candidate => {
              const resolved = entity(candidate.item, occurrence, this.context.component, candidate.center)
              return { kind: 'point', view, ...candidate, entity: resolved, sheet: view.project(resolved.point) }
            })
          })
        })
        const views = this.task.view && [this.task.view]
        return this.candidatePoints
          .filter(candidate => !views || views.includes(candidate.view))
          .map(candidate => ({ ...candidate, distance: Math.hypot(...sub(point, candidate.sheet)) }))
          .filter(candidate => candidate.distance < this.tolerance() * 1.5)
          .sort((a, b) => a.distance - b.distance)[0]
      },

      // What the active tool can pick under the pointer
      candidate(point) {
        const { tool, step } = this
        const views = ['projected', 'section', 'detail', 'break']
        if(views.includes(tool) && step == 0) return this.viewAt(point) && { kind: 'view', view: this.viewAt(point) }
        if(tool == 'select') {
          const annotation = this.annotationAt(point)
          if(annotation) return { kind: 'annotation', annotation }
          const seam = this.breakAt(point)
          if(seam) return seam
          const view = this.viewAt(point)
          return view && { kind: 'view', view }
        }
        if(tool == 'tolerance') {
          const annotation = this.annotationAt(point)
          return annotation?.type == 'dimension' && { kind: 'annotation', annotation }
        }
        if(tool == 'auxiliary' && step == 0) return this.lineAt(point, ['line'])
        if(['section', 'detail'].includes(tool) && step == 1) return this.pointAt(point)
        if(tool == 'dimension' && step < 2) return this.pointAt(point) || this.lineAt(point, ['line', 'circle'])
        if(tool == 'angle' && step < 2) return this.lineAt(point, ['line'])
        if(['baseline', 'ordinate'].includes(tool)) return this.pointAt(point)
        if(tool == 'centerline') return this.pointAt(point) || this.lineAt(point, ['line'])
        if(['centermark', 'hole'].includes(tool) && step == 0) return this.lineAt(point, ['circle'])
        if(tool == 'balloon' && step == 0) return this.lineAt(point, ['line', 'circle', 'curve'])
        if(['note', 'finish', 'datum', 'gdt', 'weld'].includes(tool) && step == 0) return this.lineAt(point)
        return null
      },

      stored(pick) {
        const item = pick.item || pick.source.edge
        const ref = item.typename() == 'Vertex' ? item.vertexReference() : item.edgeReference()
        return { ref, center: !!pick.center }
      },


      // Tools

      async click(point) {
        const task = this.task
        const hit = this.candidate(point)
        const tool = this.tool

        if(tool == 'base') {
          const choice = await this.choose(Object.entries(orientations).map(([value, { label }]) => ({ value, label })))
          if(choice) this.addBaseView(orientations[choice], point)
          return
        }

        if(['projected', 'section', 'detail'].includes(tool) && this.step == 0) {
          if(!hit) return
          task.view = hit.view
          this.select(hit)
          return this.next()
        }

        if(tool == 'projected') return this.addAlignedView(this.projectedPlacement(point))

        if(tool == 'auxiliary') {
          if(this.step == 0) {
            if(!hit) return
            task.view = hit.view
            this.select(hit)
            task.edge = hit
            return this.next()
          }
          return this.addAlignedView(this.auxiliaryPlacement(point))
        }

        if(tool == 'section') {
          const snapped = hit?.sheet || point
          if(this.step == 1) task.start = snapped
          if(this.step == 2) task.end = this.snapLine(task.start, point)
          if(this.step < 3) return this.next()
          return this.addAlignedView(this.sectionPlacement(point))
        }

        if(tool == 'detail') {
          if(this.step == 1) task.center = hit?.sheet || point
          if(this.step == 2) task.radius = Math.max(Math.hypot(...sub(point, task.center)), 2)
          if(this.step < 3) return this.next()
          return this.addDetailView(point)
        }

        if(tool == 'break') {
          if(this.step == 1) return this.addBreak(point)
          if(!hit) return
          task.view = hit.view
          task.start = point
          return this.next()
        }

        if(['dimension', 'angle'].includes(tool)) {
          if(hit && this.step < 2) {
            task.view = hit.view
            this.select(hit)
            task.picks.push(hit)
            this.step = task.picks.length
            if(tool == 'angle' || task.picks.length < 2) return
            if(!dimensionKind(task.picks.map(pick => pick.entity))) return this.reset()
            return
          }
          if(this.preview(point)) this.addAnnotation(this.preview(point))
          return
        }

        if(['baseline', 'ordinate'].includes(tool)) {
          if(hit) {
            task.view = hit.view
            this.select(hit)
            task.picks.push(hit)
            this.step = 1
          } else if(task.picks.length > 1) {
            this.addAnnotation(this.preview(point))
          }
          return
        }

        if(tool == 'centerline') {
          if(!hit) return
          task.view = hit.view
          this.select(hit)
          task.picks.push(hit)
          if(task.picks.length < 2) return this.next()
          return this.addAnnotation({ type: 'centerline', entities: task.picks.map(pick => pick.entity) })
        }

        if(tool == 'centermark') {
          if(!hit) return
          task.view = hit.view
          this.select(hit)
          task.picks.push(hit)
          return this.addAnnotation({ type: 'centermark', entities: [hit.entity] })
        }

        if(['hole', 'balloon', 'datum', 'gdt', 'weld'].includes(tool)) {
          if(this.step == 0) {
            if(!hit) return
            task.view = hit.view
            this.select(hit)
            task.target = hit
            if(tool == 'hole') task.picks.push(hit)
            return this.next()
          }
          const annotation = this.addAnnotation(this.preview(point))
          // The symbol is picked first, then values are typed into the frame
          if(tool == 'gdt' || tool == 'weld') this.startEdit(annotation, { isNew: true })
          return
        }

        if(tool == 'note') {
          if(this.step == 0 && hit) {
            task.view = hit.view
            this.select(hit)
            task.target = hit
            return this.next()
          }
          task.view ||= this.viewAt(point)
          if(!task.view) return
          return this.startEdit(this.addAnnotation({ ...this.preview(point), text: '' }), { isNew: true })
        }

        if(tool == 'finish') {
          if(!hit) return
          task.view = hit.view
          this.select(hit)
          task.target = hit
          return this.startEdit(this.addAnnotation({ type: 'finish', text: 'Ra 3.2', target: hit.view.fromSheet(hit.sheet) }), { isNew: true })
        }

        if(tool == 'tolerance' && hit) return this.startEdit(hit.annotation, { focus: 'tolerance' })
      },

      // Annotation under construction at the pointer
      preview(point = this.pointer) {
        const { task, tool } = this
        if(!point || !task.view) return null
        const view = task.view
        const position = view.fromSheet(point)
        const target = task.target && view.fromSheet(task.target.sheet)
        const entities = task.picks.map(pick => pick.entity)
        switch(tool) {
          case 'dimension':
          case 'angle': {
            const kind = tool == 'angle' ? task.picks.length == 2 && 'angle' : entities.length && dimensionKind(entities)
            if(!kind || (tool == 'dimension' && entities.length == 1 && entities[0].kind == 'point')) return null
            return { type: 'dimension', entities, kind, orientation: this.orientation(view, entities, point), position, tolerance: '' }
          }
          case 'baseline':
          case 'ordinate': {
            if(entities.length < 2) return null
            const [a, b] = entities.slice(0, 2).map(entity => view.project(entity.point))
            const orientation = Math.abs(b[0] - a[0]) >= Math.abs(b[1] - a[1]) ? 'h' : 'v'
            return { type: tool, entities, orientation, position }
          }
          case 'note': return { type: 'note', text: 'Note', position, target }
          case 'hole': return this.step ? { type: 'hole', entities, position } : null
          case 'balloon': return this.step ? { type: 'balloon', componentId: task.target.source.occurrence.id, position, target } : null
          case 'datum': return this.step ? { type: 'datum', letter: this.nextDatum(), position, target } : null
          case 'gdt': return this.step ? { type: 'gdt', characteristic: 'position', tolerance: '0.1', datums: 'A', position, target } : null
          case 'weld': return this.step ? { type: 'weld', weldType: 'fillet', size: 'a3', position, target } : null
        }
        return null
      },

      orientation(view, entities, point) {
        const anchors = entities.length == 1 ? [entities[0].a, entities[0].b] : entities.map(entity => entity.point)
        if(anchors.some(anchor => !anchor)) return 'aligned'
        const [a, b] = anchors.map(anchor => view.project(anchor))
        return orientationAt(a, b, point)
      },

      nextDatum() {
        return nextLetter(this.drawing().annotations.filter(annotation => annotation.type == 'datum').map(annotation => annotation.letter))
      },

      addAnnotation(annotation = this.preview()) {
        const { entities, ...rest } = annotation
        const stored = {
          ...rest,
          id: makeID(),
          view: this.task.view.view.id,
          items: this.task.picks.map(this.stored),
        }
        this.drawing().annotations.push(stored)
        this.reset()
        this.changed()
        return stored
      },

      addBaseView(frame, point) {
        const factor = scaleFactor(this.drawing().scale)
        const bounds = projectedBounds(this.context.model.positions, frame)
        this.drawing().views.push({
          id: makeID(), type: 'base', frame: storeFrame(frame), breaks: [], hiddenLines: frame.hiddenLines,
          x: point[0] - factor * bounds.centerU, y: point[1] + factor * bounds.centerV,
        })
        this.changed()
      },

      addAlignedView(placement) {
        if(!placement) return
        const { view, frame, direction, distance, ...extra } = placement
        this.drawing().views.push({
          id: makeID(), parent: view.view.id, frame: storeFrame(frame), direction, distance, breaks: [], ...extra,
        })
        this.reset()
        this.changed()
      },

      addDetailView(point) {
        const { view, center, radius } = this.task
        const target = 2 * view.factor
        const scale = [...drawingScales].reverse().find(scale => scaleFactor(scale) >= target - 1e-9) || drawingScales[0]
        const modelCenter = view.fromSheet(center)
        const factor = scaleFactor(scale)
        this.drawing().views.push({
          id: makeID(), type: 'detail', parent: view.view.id, frame: view.view.frame, scale, breaks: [],
          label: this.nextViewLabel(),
          detail: { center: modelCenter, radius: radius / view.factor },
          x: point[0] - factor * modelCenter[0], y: point[1] + factor * modelCenter[1],
        })
        this.reset()
        this.changed()
      },

      // Removes the band between the first click and this one, joined with the
      // breaks whose seams it would touch. Bands narrower than the seam gap
      // would widen the view instead.
      addBreak(point) {
        const { view, start } = this.task
        const band = this.breakBand(view, start, point)
        const gap = BREAK_GAP / view.factor
        let [from, to] = band.model
        if(to - from > gap) {
          const breaks = view.view.breaks || []
          const overlapping = breaks.filter(other => other.axis == band.axis && other.from <= to + gap && other.to >= from - gap)
          overlapping.forEach(other => {
            from = Math.min(from, other.from)
            to = Math.max(to, other.to)
          })
          view.view.breaks = breaks.filter(other => !overlapping.includes(other)).concat({ id: makeID(), axis: band.axis, from, to })
          this.changed()
        }
        this.reset()
      },

      // Breaks cut across the longer side of a view, unless dragged the other way
      breakAxis(view, start, point) {
        const delta = sub(point, start).map(Math.abs)
        if(Math.max(...delta) > 2) return delta[0] >= delta[1] ? 'u' : 'v'
        const { min, max } = view.box
        return max.x - min.x >= max.y - min.y ? 'u' : 'v'
      },

      // Band between two points across a view, on paper and in the model
      breakBand(view, start, point) {
        const axis = this.breakAxis(view, start, point)
        const k = axis == 'u' ? 0 : 1
        const { min, max } = view.box
        const clamp = value => Math.min(Math.max(value, [min.x, min.y][k]), [max.x, max.y][k])
        const [from, to] = [clamp(start[k]), clamp(point[k])].sort((a, b) => a - b)
        const model = [from, to].map(value => view.fromSheet(k ? [start[0], value] : [value, start[1]])[k]).sort((a, b) => a - b)
        return { axis, from, to, model }
      },

      // Line across a view at a position along an axis, on paper
      crossLine(view, axis, position) {
        const { min, max } = view.box
        return axis == 'u'
          ? [[position, min.y - 2], [position, max.y + 2]]
          : [[min.x - 2, position], [max.x + 2, position]]
      },

      // The two seam lines of each placed break of a view, on paper
      seams(view) {
        return breakSeams(view).map(({ brk, positions }) => (
          { kind: 'break', view, brk, lines: positions.map(at => this.crossLine(view, brk.axis, at)) }
        ))
      },

      breakAt(point) {
        const tolerance = this.tolerance()
        for(const view of this.views()) for(const seam of this.seams(view)) {
          const near = seam.lines.some(([a, b]) => Math.hypot(...sub(point, nearestOnSegment(point, a, b))) < tolerance)
          if(!near) continue
          seam.brk.id ||= makeID()
          return seam
        }
      },

      nextViewLabel() {
        return nextLetter(this.drawing().views.map(view => view.label).filter(Boolean))
      },

      // Views placed along a direction p (y up) from their parent stay aligned with it
      alignedPlacement(view, frame, p, point) {
        const bounds = projectedBounds(this.context.model.positions, frame)
        const along = [p[0], -p[1]]
        const center = [view.factor * bounds.centerU, -view.factor * bounds.centerV]
        const distance = dot(sub(sub(point, view.origin), center), along)
        const origin = add(view.origin, scale(along, distance))
        const middle = add(origin, center)
        const half = [view.factor * bounds.width / 2, view.factor * bounds.height / 2]
        return { view, frame, direction: p, distance, box: { x: middle[0] - half[0], y: middle[1] - half[1], width: 2 * half[0], height: 2 * half[1] } }
      },

      projectedPlacement(point) {
        const view = this.task.view
        const center = [(view.box.min.x + view.box.max.x) / 2, (view.box.min.y + view.box.max.y) / 2]
        const delta = sub(point, center)
        const p = Math.abs(delta[0]) >= Math.abs(delta[1]) ? [Math.sign(delta[0]) || 1, 0] : [0, -Math.sign(delta[1]) || 1]
        const frame = projectedFrame(view.frame, p, this.drawing().projection)
        return { ...this.alignedPlacement(view, frame, p, point), type: 'projected' }
      },

      auxiliaryPlacement(point) {
        const { view, edge } = this.task
        const a = view.project(edge.entity.a)
        const b = view.project(edge.entity.b)
        let normal = normalize([-(b[1] - a[1]), b[0] - a[0]])
        const middle = scale(add(a, b), 0.5)
        if(dot(sub(point, middle), normal) < 0) normal = scale(normal, -1)
        const p = [normal[0], -normal[1]]
        // Auxiliary views always look at the picked edge from where they are placed
        const frame = projectedFrame(view.frame, p, 'third')
        const marker = view.fromSheet(middle)
        return { ...this.alignedPlacement(view, frame, p, point), type: 'auxiliary', label: this.nextViewLabel(), marker }
      },

      sectionPlacement(point) {
        const { view, start, end } = this.task
        let normal = normalize([-(end[1] - start[1]), end[0] - start[0]])
        if(dot(sub(point, start), normal) < 0) normal = scale(normal, -1)
        const p = [normal[0], -normal[1]]
        const frame = projectedFrame(view.frame, p, this.drawing().projection)
        const [u, v] = view.fromSheet(start)
        const section = view.frame.u.clone().multiplyScalar(u).addScaledVector(view.frame.v, v).toArray()
        return { ...this.alignedPlacement(view, frame, p, point), type: 'section', label: this.nextViewLabel(), section, hiddenLines: false }
      },

      // Cutting lines snap to horizontal and vertical
      snapLine(start, point) {
        const [dx, dy] = sub(point, start).map(Math.abs)
        const angle = Math.atan2(Math.min(dx, dy), Math.max(dx, dy)) * 180 / Math.PI
        if(angle > 4) return point
        return dx >= dy ? [point[0], start[1]] : [start[0], point[1]]
      },


      // Moving and editing

      startMove(point, event) {
        const annotation = this.annotationAt(point)
        const seam = !annotation && this.breakAt(point)
        if(annotation) {
          this.document.selection.only(new DrawingItem('annotation', annotation))
          if(annotation.position) this.gesture = { type: 'move', annotation, start: point, original: [...annotation.position] }
        } else if(seam) {
          this.document.selection.only(new DrawingItem('break', seam.brk))
          // Breaks slide within the part, so both ends stay visible, and
          // stop a seam gap short of their neighbors
          const { brk } = seam
          const bounds = projectedBounds(this.context.model.positions, seam.view.frame)
          const [center, size] = brk.axis == 'u' ? [bounds.centerU, bounds.width] : [bounds.centerV, bounds.height]
          const gap = BREAK_GAP / seam.view.factor
          const others = seam.view.view.breaks.filter(other => other !== brk && other.axis == brk.axis)
          const low = Math.max(center - size * 0.4, ...others.filter(other => other.to <= brk.from).map(other => other.to + gap))
          const high = Math.min(center + size * 0.4, ...others.filter(other => other.from >= brk.to).map(other => other.from - gap))
          const limits = [Math.min(0, low - brk.from), Math.max(0, high - brk.to)]
          this.gesture = { type: 'move', seam, limits, start: point, original: [seam.brk.from, seam.brk.to] }
        } else {
          const view = this.viewAt(point)
          if(view) this.document.selection.only(new DrawingItem('view', view.view))
          else this.document.selection.clear()
          if(view) this.gesture = { type: 'move', view, start: point, original: [view.view.x, view.view.y, view.view.distance] }
        }
        if(this.gesture) this.$el.setPointerCapture(event.pointerId)
        this.updateOverlay()
      },

      move(point) {
        const { annotation, view, seam, limits, start, original } = this.gesture
        const delta = sub(point, start)
        this.gesture.moved = true
        if(seam) {
          // Slides the removed band along the part, the view keeps its size
          const shift = Math.min(Math.max(seam.brk.axis == 'u' ? delta[0] / seam.view.factor : -delta[1] / seam.view.factor, limits[0]), limits[1])
          seam.brk.from = original[0] + shift
          seam.brk.to = original[1] + shift
        } else if(annotation) {
          const factor = this.sheet.views.get(annotation.view).factor
          annotation.position = [original[0] + delta[0] / factor, original[1] - delta[1] / factor]
        } else if(isAligned(view.view)) {
          view.view.distance = original[2] + dot(delta, [view.view.direction[0], -view.view.direction[1]])
        } else {
          view.view.x = original[0] + delta[0]
          view.view.y = original[1] + delta[1]
        }
        this.render()
      },


      // Editing values in place. Fields follow the annotation as it redraws
      // with every keystroke. New symbols ask for their kind first.

      startEdit(annotation, { focus, isNew } = {}) {
        // Raw, to find the annotation on the sheet
        this.editing = markRaw({ annotation, isNew, original: { ...annotation } })
        this.document.selection.only(new DrawingItem('annotation', annotation))
        this.render()
        if(!this.editorFields.length) {
          this.editing = null
          return
        }
        const choice = isNew && this.editorFields.find(field => field.choices)
        const key = focus || this.editorFields.find(field => !field.choices).key
        this.$nextTick(() => choice ? this.pickChoice(choice) : this.focusField(key, true))
      },

      layoutEditor() {
        if(!this.editing) return
        const { annotation } = this.editing
        const fields = this.sheet.annotations.find(entry => entry.annotation === annotation)?.fields || []
        this.editorFields = fields.map(field => {
          const value = field.choices ? annotation[field.key] : String(annotation[field.key] ?? '')
          const text = field.choices ? '' : value.split('\n').reduce((a, b) => a.length >= b.length ? a : b) || field.placeholder || ''
          const width = Math.max(field.width, textWidth(text, field.size) + field.size * 0.2)
          return {
            ...field,
            ...fieldRect(field, width),
            value,
            align: { start: 'left', middle: 'center', end: 'right' }[field.anchor],
            label: field.choices?.find(choice => choice.value == value)?.label,
          }
        })
      },

      fieldElement(key) {
        return this.$el.querySelector(`.field [data-key="${key}"]`)
      },

      focusField(key, select) {
        const element = this.fieldElement(key)
        element.focus()
        if(select && element.select) element.select()
      },

      setField(field, value) {
        this.editing.annotation[field.key] = value
        this.render()
      },

      // Picked kinds redraw the symbol, then typing continues in the next field
      async pickChoice(field) {
        if(this.menu) return this.menu.resolve(null)
        const value = await this.choose(field.choices, this.fieldElement(field.key), field.value)
        if(!this.editing) return
        if(value) this.setField(field, value)
        const next = this.editorFields[this.editorFields.findIndex(other => other.key == field.key) + 1]
        this.$nextTick(() => this.focusField(next && value ? next.key : field.key, true))
      },

      fieldKey(event, field) {
        const { key, shiftKey } = event
        if(key == 'Escape') return this.cancelEdit()
        if(key == 'Tab') {
          event.preventDefault()
          const index = this.editorFields.findIndex(other => other.key == field.key)
          const count = this.editorFields.length
          return this.focusField(this.editorFields[(index + (shiftKey ? -1 : 1) + count) % count].key, true)
        }
        if(field.choices) {
          if(['Enter', ' ', 'ArrowDown'].includes(key)) {
            event.preventDefault()
            this.pickChoice(field)
          }
          return
        }
        // Notes take more lines with shift
        if(key == 'Enter' && !(shiftKey && field.multiline)) {
          event.preventDefault()
          this.commitEdit()
        }
      },

      commitEdit() {
        const { annotation } = this.editing
        this.editing = null
        this.editorFields = []
        this.menu?.resolve(null)
        if(annotation.tolerance) annotation.tolerance = annotation.tolerance.trim()
        const required = requiredFields[annotation.type]
        if(required && !String(annotation[required]).trim()) this.removeAnnotation(annotation)
        this.changed()
      },

      // New annotations are dropped, edited ones restored
      cancelEdit() {
        const { annotation, isNew, original } = this.editing
        this.editing = null
        this.editorFields = []
        this.menu?.resolve(null)
        if(isNew) this.removeAnnotation(annotation)
        else Object.assign(annotation, original)
        this.changed()
      },

      removeAnnotation(annotation) {
        const drawing = this.drawing()
        drawing.annotations = drawing.annotations.filter(other => other !== annotation)
        this.document.selection.clear()
      },

      remove() {
        const drawing = this.drawing()
        const { kind, id } = this.selected
        if(kind == 'annotation') {
          drawing.annotations = drawing.annotations.filter(annotation => annotation.id != id)
        } else if(kind == 'break') {
          drawing.views.forEach(view => view.breaks = (view.breaks || []).filter(brk => brk.id != id))
        } else {
          const removed = new Set([id])
          let grown = true
          while(grown) {
            grown = false
            drawing.views.forEach(view => {
              if(view.parent && removed.has(view.parent) && !removed.has(view.id)) {
                removed.add(view.id)
                grown = true
              }
            })
          }
          drawing.views = drawing.views.filter(view => !removed.has(view.id))
          drawing.annotations = drawing.annotations.filter(annotation => !removed.has(annotation.view))
        }
        this.document.selection.clear()
        this.changed()
      },

      keyDown(key) {
        if(document.activeElement?.closest('input, textarea')) return
        if((key == 'Delete' || key == 'Backspace') && this.tool == 'select' && this.selected) this.remove()
      },

      escape() {
        if(this.menu) this.menu.resolve(null)
        else if(this.editing) this.cancelEdit()
        else if(this.step || this.task.picks.length) this.reset()
        else if(this.document.selection.items.length) this.document.selection.clear()
        else this.document.activeDrawingTool = null
        this.updateOverlay()
      },

      // Options in a small menu below an element, or at the last click
      choose(options, anchor, value) {
        const bounds = this.$el.getBoundingClientRect()
        const at = anchor ? [bounds.left, bounds.top] : this.client
        return new Promise(resolve => {
          this.menu = {
            anchor,
            value,
            x: at[0] - bounds.left,
            y: at[1] - bounds.top,
            options,
            resolve: value => {
              this.menu = null
              resolve(value)
            },
          }
        })
      },


      // Highlights and previews on top of the sheet

      updateOverlay() {
        const overlay = { boxes: [], bands: [], highlights: [], points: [], paths: [], texts: [] }
        const seamLines = brk => this.views().flatMap(this.seams).find(seam => seam.brk === brk)?.lines.map(pathData) || []
        const boxOf = box => ({ x: box.min.x - 1.5, y: box.min.y - 1.5, width: box.max.x - box.min.x + 3, height: box.max.y - box.min.y + 3 })
        if(this.selected) {
          const { kind, id } = this.selected
          if(kind == 'break') overlay.highlights.push(...seamLines(toRaw(this.selected.data)))
          const box = kind == 'break' ? null : kind == 'view'
            ? this.sheet.views.get(id)?.box
            : this.sheet.annotations.find(({ annotation }) => annotation.id == id)?.box
          if(box) overlay.boxes.push(boxOf(box))
        }
        const point = this.pointer
        const hit = point && !this.gesture && !this.menu && this.candidate(point)
        this.cursor = this.cursorFor(hit)
        if(point && !this.gesture && !this.menu) {
          if(hit?.kind == 'view') overlay.boxes.push(boxOf(hit.view.box))
          if(hit?.kind == 'annotation') overlay.boxes.push(boxOf(this.sheet.annotations.find(({ annotation }) => annotation === hit.annotation).box))
          if(hit?.kind == 'point') overlay.points.push(hit.sheet)
          if(hit?.kind == 'break') overlay.highlights.push(...hit.lines.map(pathData))
          if(hit?.kind == 'line') {
            const lines = hit.source ? hit.view.lines.filter(line => line.source === hit.source) : [hit.line]
            overlay.highlights.push(...lines.map(line => pathData(line.points)))
          }
          this.task.picks.forEach(pick => {
            if(pick.kind == 'point') overlay.points.push(pick.sheet)
            else overlay.highlights.push(...pick.view.lines.filter(line => line.source === pick.source).map(line => pathData(line.points)))
          })
          this.previewOverlay(overlay, point)
        }
        this.overlay = overlay
      },

      // Crosshairs where lines, circles and model geometry are picked,
      // a text cursor where notes go, the arrow where things are placed
      cursorFor(hit) {
        const { tool, step, gesture } = this
        if(gesture) return { pan: 'grabbing', zoom: 'ns-resize', move: gesture.seam ? resizeCursor(gesture.seam.brk) : 'move' }[gesture.type]
        if(!this.pointer || this.menu) return 'default'
        if(tool == 'select') return hit?.kind == 'break' ? resizeCursor(hit.brk) : hit ? 'move' : 'default'
        if(['projected', 'section', 'detail'].includes(tool) && step == 0) return hit ? 'pointer' : 'default'
        if(tool == 'tolerance') return hit ? 'pointer' : 'default'
        if(tool == 'note') return step == 0 && hit ? 'crosshair' : 'text'
        const picking = {
          section: step < 3, detail: step < 3, break: true, auxiliary: step == 0,
          dimension: step < 2, angle: step < 2, baseline: true, ordinate: true, centerline: true, centermark: true,
          finish: true, hole: step == 0, balloon: step == 0, datum: step == 0, gdt: step == 0, weld: step == 0,
        }[tool]
        return picking ? 'crosshair' : 'default'
      },

      // A cut line at the pointer, then the band that will be removed
      breakPreview(overlay, point) {
        const { task, step } = this
        const view = step ? task.view : this.viewAt(point)
        if(!view) return
        const band = this.breakBand(view, task.start || point, point)
        const line = at => overlay.paths.push({ d: pathData(this.crossLine(view, band.axis, at)), width: 0.35, dash: '2 1' })
        line(band.from)
        if(!step) return
        line(band.to)
        const vertical = band.axis == 'v'
        const [a, b] = this.crossLine(view, band.axis, band.from)
        overlay.bands.push(vertical
          ? { x: a[0], y: band.from, width: b[0] - a[0], height: band.to - band.from }
          : { x: band.from, y: a[1], width: band.to - band.from, height: b[1] - a[1] })
      },

      previewOverlay(overlay, point) {
        const { task, tool, step } = this
        const ghost = box => overlay.boxes.push(box)
        if(tool == 'projected' && step == 1) ghost(this.projectedPlacement(point).box)
        if(tool == 'auxiliary' && step == 1) ghost(this.auxiliaryPlacement(point).box)
        if(tool == 'section') {
          if(step == 2) overlay.paths.push({ d: pathData([task.start, this.snapLine(task.start, point)]), width: 0.5 })
          if(step == 3) {
            overlay.paths.push({ d: pathData([task.start, task.end]), width: 0.5 })
            ghost(this.sectionPlacement(point).box)
          }
        }
        if(tool == 'detail') {
          const circle = (center, radius) => overlay.paths.push({
            d: `M${center[0] - radius} ${center[1]}a${radius} ${radius} 0 1 0 ${2 * radius} 0a${radius} ${radius} 0 1 0 ${-2 * radius} 0`,
            width: 0.35,
          })
          if(step == 2) circle(task.center, Math.hypot(...sub(point, task.center)))
          if(step == 3) {
            circle(task.center, task.radius)
            circle(point, task.radius * 2)
          }
        }
        if(tool == 'break') this.breakPreview(overlay, point)
        const annotation = this.preview(point)
        const ink = annotation && annotationInk(this.context, task.view, annotation)
        if(!ink) return
        overlay.paths.push(...ink.paths.map(path => ({ d: pathData(path.points), width: path.width, dash: path.dash?.join(' '), fill: path.fill })))
        overlay.texts.push(...ink.texts)
      },
    },
  }

  function area(box) {
    return (box.max.x - box.min.x) * (box.max.y - box.min.y)
  }
</script>
