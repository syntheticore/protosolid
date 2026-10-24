const branchColors = ['#ffbb40', '#ee3367', '#4b8ee3', '#15c115', '#a052f7', '#ea6a43']

export const laneColor = lane => branchColors[lane % branchColors.length]

// Assign every commit a lane, git log --graph style.
// Rows are commits newest first with their lane and the branches pointing at them,
// edges connect a child to its parent, traveling along the given lane.
export function layoutHistory(repository) {
  const tips = {}
  Object.entries(repository.branches).forEach(([name, id]) => (tips[id] ||= []).push(name) )

  const lanes = []
  const claimLane = id => {
    let lane = lanes.indexOf(null)
    if(lane < 0) lane = lanes.length
    lanes[lane] = id
    return lane
  }
  const edges = []
  const rows = repository.log().map((commit, index) => {
    let lane = lanes.indexOf(commit.id)
    if(lane < 0) lane = claimLane(commit.id)
    // Lines of other children waiting for this commit end here
    lanes.forEach((id, i) => { if(id == commit.id && i != lane) lanes[i] = null })
    lanes[lane] = null
    commit.parents.forEach((parent, i) => {
      // The first parent continues in this lane, merged parents join or open another one
      let travel = i == 0 ? lane : lanes.indexOf(parent)
      if(travel < 0) travel = claimLane(parent)
      lanes[travel] = parent
      edges.push({ child: commit.id, parent, lane: travel })
    })
    return { ...commit, index, lane, branches: tips[commit.id] || [] }
  })
  return { rows, edges }
}

// Colors of all branches, as shown in the history panel
export function branchColorMap(repository) {
  const colors = {}
  layoutHistory(repository).rows.forEach(row => row.branches.forEach(name => colors[name] = laneColor(row.lane) ))
  return colors
}
