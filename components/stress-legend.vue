<template lang="pug">

  .stress-legend

    .bar(:style="{ background: gradient }")
      .marker(
        v-for="threshold in thresholds"
        :key="threshold.label"
        :style="{ bottom: threshold.position + '%' }"
      )

    .labels
      .sizer(v-for="label in labels" :key="label.text") {{ label.text }}
      span(
        v-for="label in labels"
        :key="label.text"
        :class="label.kind"
        :style="{ bottom: label.position + 'px' }"
      ) {{ label.text }}

</template>


<style lang="stylus" scoped>

  .stress-legend
    display: grid
    grid-template-columns: 14px auto
    column-gap: 6px
    height: 140px
    color: $bright2
    font-size: 11px
    text-shadow: 0 0 2px rgba(0,0,0, 0.8)
    pointer-events: none

  .labels
    position: relative

    span
      position: absolute
      left: 0
      transform: translateY(50%)
      white-space: nowrap

      &.threshold
        font-weight: bold
        color: white

  // Labels are placed absolutely, so invisible copies give the column its width
  .sizer
    visibility: hidden
    height: 0
    white-space: nowrap

  .bar
    position: relative
    border-radius: 2px
    box-shadow: 0 0 0 1px rgba(0,0,0, 0.5), 0 1px 3px rgba(0,0,0, 0.5)

  .marker
    position: absolute
    left: 0
    right: -5px
    height: 0
    border-top: 1px solid white

</style>


<script>
  import * as THREE from 'three'
  import { elasticStressColor, elasticStressRange, stressColors } from '../js/core/static.js'

  const scaleHeight = 140
  const labelSpacing = 13

  export default {
    name: 'StressLegend',

    props: {
      result: Object,
    },

    computed: {
      gradient() {
        const max = this.result.maxStress
        const elastic = elasticStressRange(this.result)
        const color = new THREE.Color()
        const stops = Array.from({ length: 13 }, (_, i) =>
          `#${elasticStressColor(i / 12, color).getHexString()} ${elastic * i / 12 / max * 100}%`)
        const bands = [
          { from: this.result.yieldStrength, to: this.result.ultimateStrength, color: stressColors.plastic },
          { from: this.result.ultimateStrength, to: Infinity, color: stressColors.failure },
        ].filter(band => band.from < Math.min(band.to, max))
        bands.forEach(band => stops.push(
          `#${band.color.getHexString()} ${band.from / max * 100}%`,
          `#${band.color.getHexString()} ${Math.min(band.to, max) / max * 100}%`,
        ))
        return `linear-gradient(to top, ${stops.join(', ')})`
      },

      // Strength limits reached within the scale; yield only matters below break
      thresholds() {
        const { yieldStrength, ultimateStrength, maxStress } = this.result
        return [
          yieldStrength < ultimateStrength && { stress: yieldStrength, label: 'Yield' },
          { stress: ultimateStrength, label: 'Break' },
        ].filter(threshold => threshold && threshold.stress <= maxStress).map(threshold => ({
          ...threshold,
          position: threshold.stress / maxStress * 100,
        }))
      },

      // Thresholds are pushed up away from each other, then down below the
      // peak value, which stays at the top of the bar
      labels() {
        const labels = this.thresholds.map(threshold => ({
          text: `${threshold.label} – ${this.format(threshold.stress)}`,
          kind: 'threshold',
          position: threshold.position / 100 * scaleHeight,
        }))
        for(let i = 1; i < labels.length; i++) {
          labels[i].position = Math.max(labels[i].position, labels[i - 1].position + labelSpacing)
        }
        labels.push({ text: `${this.format(this.result.maxStress)} MPa`, kind: 'value', position: scaleHeight })
        for(let i = labels.length - 2; i >= 0; i--) {
          labels[i].position = Math.min(labels[i].position, labels[i + 1].position - labelSpacing)
        }
        return labels
      },
    },

    methods: {
      format(value) {
        return String(Number(value.toPrecision(3)))
      },
    },
  }
</script>
