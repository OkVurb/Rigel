# Rigel — prototype

The Phase 0 interface for Rigel. React 19 + TypeScript + Vite.

```bash
npm install
npm run dev
```

Layout: `src/App.tsx` owns the shell and view state; `components/` holds the panes; `data.ts` is the
seeded catalogue; `github.ts`, `hub.ts` and `db.ts` are the real fetch, marketplace and persistence
layers; `targets.ts` and `deps.ts` are the measured harness matrix and the dependency catalogue.

`vite.config.ts` contains a dev-only `/__hub` proxy — SkillsMP and SkillHub send no CORS headers, so the
marketplace tabs only work under `npm run dev`. See the root [README](../README.md) for the full picture.
