# Candidate checks

- 85 Node tests PASS, including 12 build-plan/host cases, canonical assets/tokens and existing date/DST logic; 2 Python compiled-identity regressions PASS.
- lint, typecheck, frontend build and locked workspace cargo check PASS. Existing >500k JS advisory only.
- 11 browser suites PASS (new date navigation + existing adjacent interactions), 9 viewport boundaries, 18 visual scenes + reduced-motion PASS.
- Current-time geometry: verified iframe innerWidth1120/760, innerHeight760/540 ×00:00/01:17/12:00/23:59; line aligns first track, pure label remains above it and inside axis. Hover label takes priority and restores on leave.
- Real keyboard Return activates both day arrows; keyboard focus is visible and returns to date when Next becomes disabled at today. Month/year/leap-day boundaries separately exercised in real App fixture.
- Actual default ./build_dev.sh completed both thin DMGs. All4 binaries have exactly one intended slice; both reports share source/version/epoch; ARM native and Intel Rosetta identities, worker runtime and read-only mounted-DMG model smoke PASS. Reports/checksums in dev-builds.json.
- Dirty release entry refused before compilation. Final clean-source signed/notarized two release packages and ARM install/native date proof are post-commit mandatory gates; completion remains in-progress until recorded.
- Original ICON byte fingerprints and shared palette/wordmark remain covered by canonical-asset tests. Existing .17 native entry geometry/interaction proof remains applicable to unchanged entry code.

Phase1 repair: narrow evidence/assets were replaced using the existing explicit-size visual iframe; all clock evidence now records actual innerWidth/innerHeight and image dimensions are verified. Both real pointer calendar triggers close on second click, Escape restores the real trigger, and the narrow popover fits. Host detection now queries actual hardware, with translated Node regression passing; worker reports use the actual runtime architecture. Final repaired dev two-package build completed under Intel Node/Rosetta; both actual mounted-DMG checks passed, ARM native and Intel translated reports correct.
