# Review Contract

- Mode: repair. Reviewers read only; do not edit or spawn reviewers.
- Requirement: specs/prd/devplan_261102_people-vocabulary.md; human asks for top People/Vocabulary tabs in existing People page, default People, autonomous product-fitting interactions.
- Baseline: eeef7110aa6d43051975b0da02f8f51eaacdc0a9. PeopleView.tsx and scripts/test_people_settings.html were clean at task start; use git diff for them. Other scoped files are newly created.
- In scope: src/PeopleView.tsx, src/VocabularyView.tsx, src/vocabulary.css, scripts/test_vocabulary.html, scripts/test_people_settings.html, this devplan and specs/research/evidence/2026-11-02-vocabulary/* evidence.
- Out of scope: all other pre-existing dirty/untracked files. In particular App.tsx, WorkspaceViews.tsx, core, protocol, updater, root docs/skills. Do not review their unrelated diffs.
- Guarantees: default People; keyboard-operable tabs; leaving person detail disposes existing playback; distinguish loading/error/empty; editable canonical name/type/aliases/enabled; preserve custom CLI kind and entity id when editing; persist only through existing vocabulary Action/Core owner; no duplicate submissions; nested Context errors shown, draft retained; stale reads cannot replace mutation result; failure does not claim success; entry deletion confirmation; existing People behavior retained.
- Non-goals: imports, automatic terms, historical rebuild, changed ASR/LLM logic, cloud accuracy proof, install/release, stronger protocol concurrency semantics or new persistence/retry machinery.
- Owned defects: introduced/materially exposed by this scoped change or explicit guarantees above. Existing adjacent defects are observations only.
- Allowed mechanisms: no new owner/store/protocol/background queue/recovery/fallback. Local component snapshot, form state and request epoch reuse existing patterns.
- Required lenses: requirements, adversarial, architecture; phase 1 requirements/adversarial fixed snapshot, final architecture after adjudication.
- Evidence: static/typecheck/lint/build passed; 114 Node tests passed; WebKit 28 new UI interactions + 21 existing People checks passed. Long content and editor screenshots at 1120×760 / 760×540 with no horizontal overflow and modal within viewport. Browser fixture mocks Core IPC using actual response envelope; real native bridge and Core persistence implementation unchanged, no new OS behavior.
- Invalidation: production component change rerun static/build + affected browser cases; CSS rerun assets/visual; test-only selector change rerun that fixture. Do not demand unrelated recording/credentials/signing tests.
