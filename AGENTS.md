## Behaviour

Do not build or run the application unrequested.
Only use browser debugging when you actually need it to debug a problem, not for routine inspection.
After changing files, reload the page fully, as HMR can be a bit broken at times.
Do not write tests unasked if you don't need them for debugging yourself.
Avoid defensive programming practices.

OpenAI ChatGPT/Codex only (not Claude): Use only `apply_patch` for file edits so changes are visible and undoable in the Codex UI. Do not edit files with `Set-Content`, shell redirection, scripts, or other file-writing commands.
