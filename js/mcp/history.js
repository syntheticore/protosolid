import * as THREE from 'three'
import { toRaw } from 'vue'

import { PoseFeature } from '../core/features.js'
import { makeID } from '../core/id.js'
import { worldTransform, solveAssembly } from '../core/assembly.js'
import { MergeSession } from '../core/merge-session.js'
import { ToolError, itemId, findComponent, findFeature, describeTimeline } from './describe.js'
import { settle, editable, closeEditing, failingFeatures, vector3, number } from './common.js'

// Timeline order, assembly poses, saved views and version history

const round = value => Number(value.toFixed(4))

function placement(component) {
  const world = worldTransform(component)
  const position = new THREE.Vector3(), rotation = new THREE.Quaternion()
  world.decompose(position, rotation, new THREE.Vector3())
  const euler = new THREE.Euler().setFromQuaternion(rotation, 'XYZ')
  return {
    component: itemId(component),
    position: position.toArray().map(round),
    rotation: euler.toArray().slice(0, 3).map(angle => round(THREE.MathUtils.radToDeg(angle))),
  }
}

function repository(doc) {
  if(!doc.repository) throw new ToolError('The document has no version history. Saving it in the versioned format starts one, see the preferences.')
  return doc.repository
}

// Repository errors explain themselves
async function versionChange(action) {
  try {
    await action()
  } catch(error) {
    if(error instanceof ToolError) throw error
    throw new ToolError(error.message || String(error))
  }
}

export const historyTools = [
  {
    name: 'reorder_feature',
    title: 'Reorder feature',
    description: 'Moves a feature before or after another one in the timeline and regenerates the model. ' +
      'Features fail when they move before the features they depend on.',
    inputSchema: {
      type: 'object',
      properties: {
        feature: { type: 'string' },
        before: { type: 'string', description: 'Feature to move it in front of' },
        after: { type: 'string', description: 'Feature to move it behind' },
      },
      required: ['feature'],
    },
    run: async ({ feature: id, before, after }, { doc }) => {
      editable(doc)
      if(!before == !after) throw new ToolError('Give either before or after')
      const feature = toRaw(findFeature(doc, id))
      const other = toRaw(findFeature(doc, before || after))
      if(feature === other) throw new ToolError('A feature cannot move relative to itself')
      await closeEditing(doc)
      await doc.reorder(feature, other, !!after)
      await settle(doc)
      return { marker: doc.timeline.marker, timeline: describeTimeline(doc), failingFeatures: failingFeatures(doc) }
    },
  },
  {
    name: 'pose_component',
    title: 'Pose component',
    description: 'Moves a child component in world space like dragging it in the viewport, keeping its joints intact, ' +
      'and keeps the resulting pose as a pose feature in the timeline. Components fixed by joints do not move. ' +
      'Rotations turn about the axis through "point", which defaults to the component origin.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string' },
        translate: { type: 'array', items: { type: 'number' }, description: '[x, y, z] in mm' },
        rotate: {
          type: 'object',
          properties: {
            axis: { type: 'array', items: { type: 'number' } },
            angle: { type: 'number', description: 'Degrees' },
            point: { type: 'array', items: { type: 'number' } },
          },
          required: ['axis', 'angle'],
        },
      },
      required: ['component'],
    },
    run: async ({ component: id, translate, rotate }, { doc }) => {
      editable(doc)
      await closeEditing(doc)
      const component = findComponent(doc, id)
      if(!component.parent) throw new ToolError('The top component cannot be posed')
      const current = worldTransform(component)
      const motion = new THREE.Matrix4()
      if(rotate) {
        const axis = new THREE.Vector3(...vector3(rotate.axis, 'axis')).normalize()
        if(!Number.isFinite(axis.x)) throw new ToolError('axis must not be zero')
        const point = rotate.point ? new THREE.Vector3(...vector3(rotate.point, 'point')) : new THREE.Vector3().setFromMatrixPosition(current)
        motion.makeTranslation(point.x, point.y, point.z)
          .multiply(new THREE.Matrix4().makeRotationAxis(axis, THREE.MathUtils.degToRad(number(rotate.angle, 'angle'))))
          .multiply(new THREE.Matrix4().makeTranslation(-point.x, -point.y, -point.z))
      }
      if(translate) motion.premultiply(new THREE.Matrix4().makeTranslation(...vector3(translate, 'translate')))
      const desired = motion.multiply(current)
      const tree = doc.top()
      solveAssembly(tree, component, desired)
      if(!tree.hasPoseChange()) throw new ToolError('The component did not move, its joints hold it in place')
      // As keepPose, awaiting the regeneration
      const feature = new PoseFeature(doc)
      const moved = feature.transforms.map(entry => tree.findChild(entry.id))
      const result = moved.map(placement)
      tree.resetPose()
      await doc.addFeature(feature)
      await settle(doc)
      const origin = matrix => new THREE.Vector3().setFromMatrixPosition(matrix)
      const offset = origin(worldTransform(doc.top().findChild(component.id))).distanceTo(origin(desired))
      return {
        feature: itemId(feature),
        moved: result,
        ...(offset > 1e-3 && { warning: `Joints kept the component ${round(offset)} mm from the requested placement` }),
        failingFeatures: failingFeatures(doc),
      }
    },
  },
  {
    name: 'edit_views',
    title: 'Edit saved views',
    description: 'Saves the current camera, visibilities and section views as a named view, or renames or removes a saved view. ' +
      'capture_view shows saved views by title. Standard views cannot be changed.',
    inputSchema: {
      type: 'object',
      properties: {
        save: { type: 'string', description: 'Title of a new view' },
        position: { type: 'array', items: { type: 'number' }, description: 'Camera position of the new view instead of the current one' },
        target: { type: 'array', items: { type: 'number' }, description: 'Camera target of the new view' },
        marker: { type: 'boolean', description: 'The new view also restores the timeline marker' },
        activeComponent: { type: 'boolean', description: 'The new view also activates the active component' },
        rename: { type: 'object', properties: { view: { type: 'string' }, title: { type: 'string' } }, required: ['view', 'title'] },
        remove: { type: 'string', description: 'Title of a view to remove' },
      },
    },
    run: async ({ save, position, target, marker, activeComponent, rename, remove }, { doc }) => {
      editable(doc)
      const find = title => {
        const index = doc.views.findIndex(view => view.title == title)
        if(index < 0) throw new ToolError(`No view '${title}'. Views are ${doc.views.map(view => view.title).join(', ')}.`)
        if(doc.views[index].system) throw new ToolError('Standard views cannot be changed')
        return index
      }
      if(remove) doc.removeView(find(remove))
      if(rename) {
        doc.views[find(rename.view)].title = rename.title
        doc.markChanged()
      }
      if(save) {
        if(doc.views.some(view => view.title == save)) throw new ToolError(`A view '${save}' exists already`)
        const camera = doc.dirtyView || doc.currentCameraView
        const components = doc.top().getChildren()
        // Like saving a view in the view chooser
        const view = {
          id: makeID(),
          title: save,
          position: position ? new THREE.Vector3(...vector3(position, 'position')) : camera.position.clone(),
          target: target ? new THREE.Vector3(...vector3(target, 'target')) : camera.target.clone(),
          zoomToFit: false,
          rotationOnly: false,
          visibilities: Object.fromEntries(components.map(component => [component.id, {
            compHidden: component.creator.hidden,
            itemsHidden: JSON.parse(JSON.stringify(component.creator.itemsHidden)),
          }])),
          sections: Object.fromEntries(components.map(component => [component.id, component.creator.sectionViews.map(section => ({
            id: section.id,
            orientation: { ...section.orientation },
            position: { ...section.position },
            side: section.side ?? false,
            hidden: section.hidden ?? false,
          }))])),
          ...(activeComponent && { activeComponentId: doc.activeComponent.id }),
          ...(marker && { marker: doc.timeline.marker }),
        }
        doc.addView(view)
        await settle(doc)
      }
      return { views: doc.views.map(view => ({ title: view.title, ...(view.system && { standard: true }) })) }
    },
  },
  {
    name: 'get_versions',
    title: 'Get versions',
    readOnly: true,
    description: 'Version history of a versioned document: its branches and saved versions, newest first.',
    inputSchema: {
      type: 'object',
      properties: { limit: { type: 'integer', description: 'Number of versions, defaults to 30' } },
    },
    run: ({ limit = 30 }, { doc }) => {
      const repo = repository(doc)
      return {
        currentBranch: repo.currentBranch || null,
        head: repo.headId || null,
        unsavedChanges: doc.hasChanges,
        branches: repo.branches,
        versions: Object.entries(repo.commits)
          .sort(([, a], [, b]) => b.date.localeCompare(a.date))
          .slice(0, limit)
          .map(([id, commit]) => ({ id, message: commit.message, date: commit.date, parents: commit.parents })),
      }
    },
  },
  {
    name: 'edit_versions',
    title: 'Edit versions',
    description: 'Switches the document to another branch or version, or creates, renames, deletes or merges branches. ' +
      'Branch changes are written to the file right away. Switching with unsaved changes asks the user first. ' +
      'Merging opens a preview the user finishes in the app. Save new versions with save_document.',
    inputSchema: {
      type: 'object',
      properties: {
        checkout: { type: 'string', description: 'Branch name or version id' },
        createBranch: {
          type: 'object',
          properties: { name: { type: 'string' }, from: { type: 'string', description: 'Version id, defaults to the current one' } },
          required: ['name'],
        },
        renameBranch: { type: 'object', properties: { name: { type: 'string' }, newName: { type: 'string' } }, required: ['name', 'newName'] },
        deleteBranch: { type: 'string' },
        merge: { type: 'string', description: 'Branch to merge into the current branch' },
      },
    },
    run: async ({ checkout, createBranch, renameBranch, deleteBranch, merge }, { doc }) => {
      editable(doc)
      const repo = repository(doc)
      await closeEditing(doc)
      await versionChange(async () => {
        if(checkout) {
          if(!repo.branches[checkout] && !repo.commits[checkout]) throw new ToolError(`No branch or version '${checkout}'`)
          await doc.checkoutVersion(checkout)
          // Declining the discard prompt leaves the changes in place
          if(doc.hasChanges) throw new ToolError('The user kept the unsaved changes, so the version was not switched')
        }
        if(createBranch) {
          const from = createBranch.from || repo.headId
          if(!repo.commits[from]) throw new ToolError(`No version '${from}'`)
          await doc.createBranch(createBranch.name, from)
        }
        if(renameBranch) await doc.renameBranch(renameBranch.name, renameBranch.newName)
        if(deleteBranch) await doc.deleteBranch(deleteBranch)
        if(merge) {
          if(!repo.branches[merge]) throw new ToolError(`No branch '${merge}'`)
          // Checked here, the app would interrupt the user with an alert instead
          if(!repo.currentBranch) throw new ToolError('Switch to a branch before merging')
          if(doc.hasChanges) throw new ToolError('Save the changes with save_document before merging')
          if(new MergeSession(doc, merge).plan.type == 'up-to-date') throw new ToolError(`'${repo.currentBranch}' already contains all changes from '${merge}'`)
          await doc.mergeBranch(merge)
        }
      })
      await settle(doc)
      const current = repository(doc)
      return {
        currentBranch: current.currentBranch || null,
        head: current.headId || null,
        branches: current.branches,
        ...(doc.activeMerge && { merging: 'The merge preview is open. Ask the user to review and finish it in the app.' }),
      }
    },
  },
]
