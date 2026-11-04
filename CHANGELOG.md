# Changelog

Every release, newest first. The heading is the version its `v*` tag publishes.

## 0.1.6 — 2026-11-03

- feat(chat): rebuild the agents screen as a bots list and message threads
- feat(chat): draw room attachments in a bubble, through the core rather than the server origin
- feat(agent): add Meta Model API (Muse Spark) as a provider
- feat(chat): add Meta's Muse Code as a room harness when its CLI is installed
- feat(agent): run one headless Muse Code turn through muse exec --json, with items and no invented usage
- feat(agent): give the room Concord and AgentCAD MCP servers alongside obscura
- feat(browser): give each agent a named Space, batched actions and shadow/iframe snapshots
- feat(ui): add themed context menus across every surface
- feat(ui): put a themed turn-target picker beside the model selector
- feat(cloud): queue, list, inspect, cancel and retry agent runs
- fix(agent): read the Vertex project from the window and send the model it shows
- docs(cloud): record sponsored-run economics and the queue design

## 0.1.4 — 2026-10-27

- ci(release): refuse to publish until CI passes for the tag
- fix(release): hold the gate until every CI run on the commit finishes
- feat(bundle): brand the MSI and NSIS installers
- docs(platforms): say where the installer artwork comes from

## 0.1.3 — 2026-10-26

- feat(release): cut a release in one command
- fix(release): roll back a failed version bump
