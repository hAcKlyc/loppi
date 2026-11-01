# Review results

## Phase 1 requirements
Fresh reviewer vocabulary_requirements: PASS, no blocker or required evidence gap. Confirmed default tabs, keyboard reuse, disposal via unmount, CRUD, entity/custom-kind preservation, existing Core ownership/envelope, errors and stale-read handling, deletion, and People regression. All criteria mapped to code/runtime evidence.

## Phase 1 adversarial and adjudication
Fresh reviewer vocabulary_adversarial: one P2 blocker. An unconditional canonical-name guard blocked editing valid CLI-created same-name entries when one was disabled. Core allows this state with explicit IDs. Classified as introduced in-scope defect (existing entries editable guarantee), accepted.

Root repair: canonical-name guard now applies only to new entries, where null-ID Core upsert would otherwise overwrite an existing same-name entry. Existing edits continue to obey enabled-entry alias conflict validation. No protocol, persistent state, retry, owner or compatibility mechanism added.

Affected verification: typecheck/lint/build PASS; new fixture cases edit a disabled same-name entry and disable active entry. WebKit and system Google Chrome: vocabulary 30 checks each + existing People 21 checks each PASS. Existing new-entry overwrite checks, including a new disabled duplicate, remain passing. Keyboard/pointer tab activation and Escape tested in both engines. Node114 already passed; one-guard fix does not affect other modules.

Adversarial focused follow-up: RESOLVED. The !entry condition limits canonical rejection to creation; existing edits pass through the enabled-entry collision check and preserve IDs.

## Final architecture
Fresh reviewer vocabulary_architecture: PASS, no findings. Core remains sole writer; local snapshot/draft/request epochs use existing patterns. Shared controls/tokens and playback cleanup reused. No new persistent state, protocol, store, queue, retry, dependency, runtime branch or version/schema obligation.

## Final verification
- npm run typecheck / npm run lint / npm run build: PASS after repair. Vite reports the existing large-main-bundle advisory; build succeeds.
- node --test scripts/test_*.mjs: 114/114 PASS (before the isolated guard repair; unrelated test code unchanged).
- WebKit + system Google Chrome: vocabulary30 + People21 each PASS after repair; keyboard ArrowRight/Enter, pointer tabs, Escape and real input fills also PASS.
- Long-content screenshots at 1120×760 and 760×540: no main-content horizontal overflow; modal inside viewport and vertically scrollable.
- All samples are synthetic, IPC mocked; no real recordings, database, credentials or cloud calls. Native bridge and Core implementation unchanged; no OS integration or release claim.
- Advisory manual look: open People, select Vocabulary; add a term/alias and edit it; disable then delete with confirmation. No pending required user decision or mandatory acceptance item.
