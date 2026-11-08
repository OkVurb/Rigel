# Eight-state audit (roadmap 127)

Every control in the session, approval, picker and settings surfaces, audited against the eight
states: default, hover, focus, active, disabled, loading, error and success. A state that the
control cannot reach is `n/a`. A state the control should have but does not is `gap`, and every gap
names the owner who still has to close it — "no state is missing without an owner".

`ui/tests/states.spec.ts` reads this table and fails on a cell that is neither `yes`, `n/a` nor
`gap (owner: …)`, so a new control cannot be added without an audit row.

| Surface | Control | default | hover | focus | active | disabled | loading | error | success |
|---|---|---|---|---|---|---|---|---|---|
| session | composer textarea | yes | yes | yes | n/a | yes | n/a | gap (owner: ui) | n/a |
| session | Send | yes | yes | yes | yes | yes | yes | gap (owner: ui) | n/a |
| session | + session | yes | yes | yes | yes | yes | yes | gap (owner: ui) | n/a |
| session | rail item | yes | yes | yes | yes | n/a | n/a | n/a | n/a |
| session | auto-refine toggle | yes | yes | yes | yes | yes | n/a | n/a | yes |
| session | transcript turn | yes | n/a | n/a | n/a | n/a | yes | yes | yes |
| session | model picker trigger | yes | yes | yes | yes | yes | n/a | gap (owner: ui) | n/a |
| approval | Allow once | yes | yes | yes | yes | yes | yes | n/a | yes |
| approval | This session | yes | yes | yes | yes | yes | yes | n/a | yes |
| approval | This project | yes | yes | yes | yes | yes | yes | n/a | yes |
| approval | Always | yes | yes | yes | yes | yes | yes | n/a | yes |
| approval | Reject | yes | yes | yes | yes | yes | n/a | n/a | yes |
| approval | command row | yes | n/a | n/a | n/a | n/a | n/a | yes | n/a |
| approval | cannot_determine row | yes | n/a | n/a | n/a | n/a | n/a | yes | n/a |
| picker | search field | yes | yes | yes | n/a | n/a | n/a | gap (owner: ui) | n/a |
| picker | provider group | yes | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| picker | model row | yes | yes | yes | yes | yes | n/a | n/a | yes |
| picker | use-model button | yes | yes | yes | yes | yes | n/a | gap (owner: ui) | yes |
| settings | page option | yes | yes | yes | yes | n/a | n/a | n/a | n/a |
| settings | auto-refine checkbox | yes | yes | yes | yes | yes | n/a | n/a | yes |
| settings | route inspector inputs | yes | yes | yes | n/a | n/a | yes | yes | yes |
| settings | refresh stale button | yes | yes | yes | yes | yes | yes | yes | yes |

## Gaps and their owners

The four `gap` rows are the error state of controls whose failure currently surfaces only as a
notice elsewhere on the surface. `ui` owns closing them by giving each control its own inline error
state; the audit moves to `yes` in the same change that adds the state.
