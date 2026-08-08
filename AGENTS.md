# ProtoSolid development notes

## Behaviour

Do not build or run the application unrequested.
Only user browser debugging when you actually need it to debug a problem, not for routine inspection.
Do not write tests unasked if you don't need them for debugging yourself.

## Browser debugging

- Use the `S` shortcut to activate the sketch tool and `C` to activate the circle tool when reproducing sketch-based workflows.
- After entering a numeric value such as `20mm`, press Enter to commit the field value before confirming the feature or tool.
- Before clicking an edge to select it, move the pointer around the edge until the edge is visibly highlighted; clicking too quickly may miss the intended edge.

## Default geometry fixture

Most browser-testing sequences start with one circular profile:

1. Press `S` and choose the top plane.
2. Press `C` to activate the circle tool.
3. Click once to set the circle center.
4. Click a second time to set a point on the circle and define its radius.
5. Confirm the sketch.
