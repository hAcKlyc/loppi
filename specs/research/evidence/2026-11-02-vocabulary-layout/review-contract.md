# Review contract

Mode repair; source specs/prd/devplan_261102_vocabulary-layout.md. User says page is cluttered: remove explanations, settings together above, vocabulary list below. Main-page copy/layout only; preserve existing form hints, errors/confirmation, CRUD/settings/requests/tab behavior.

Base 711079f2d67c315d1631ff454faae4177a39a7a9. Scope ONLY src/VocabularyView.tsx, src/vocabulary.css, scripts/test_vocabulary.html, new devplan above and evidence/2026-11-02-vocabulary-layout. Other shared changes excluded. Review ONLY this diff (previous feature already reviewed), not the whole existing vocabulary implementation.

Guarantees: three labeled shared switches together above list, no introductory or explanatory paragraphs/examples on main page; concise empty state/aliases; add action belongs to list. Feedback, accessible control names, loading/error behavior, existing mutations and editor behavior retained. Layout fits 1120×760/760×540. No new state/store/owner/protocol/retry/mechanism or backend change allowed. Owned defects: introduced/materially exposed layout/copy/control behavior only. Non-goals recording/models/cloud accuracy/install/release.

Required lenses requirements/adversarial then final architecture, fresh context read-only. Do not edit or spawn. Verification: typecheck/lint/build pass; design assets8 pass; vocabulary existing30+2 layout assertions=32 pass in WebKit/system Chrome. Pointer-create preview, settings above list, three switches, no overflow and no explanatory paragraphs measured at both widths. Screenshot evidence in this folder. Final minor CSS grouping is being rechecked in same browser runner. JSX change invalidates relevant static/build/browser; CSS invalidates assets/visual; no other subsystem retest required.
