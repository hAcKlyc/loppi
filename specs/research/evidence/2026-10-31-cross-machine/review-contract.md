# Cross review contract — hear 0.1.13

Mode: repair. Phase 1 is a fixed read-only snapshot. No reviewer edits or spawns other reviewers. Main agent owns adjudication, fixes, tests, commit and release.

Requirements: specs/prd/devplan_261031_cross_machine_reliability.md and specs/research/2026-10-31-consolidated-issues.md. Read both completely. The latest human instruction explicitly authorizes all C01–C18 repairs, deep architecture work using “架构正确、简单、可依赖”, autonomous Computer Use acceptance, version bump, signed release and local upgrade. Other-machine issues and local full-day findings have been integrated. Test/runtime evidence is linked in those documents; private evidence is local build/issue-intake/2026-10-31 (do not export user content).

Target: complete current working tree diff against 3651aa14ece9206fdfa499eb90086ad57129ad6f, branch codex/cross-machine-reliability-0.1.13, before commit. Root /Users/ethan/.codex/worktrees/hear-update-0-1-12/hear. Final candidate version0.1.13/protocol23. No in-scope writer runs during Phase1. Release/install and final artifact identity are necessarily after this gate, not missing pre-commit functional evidence.

Scope: all files listed below. They are owned by this batch. Out of scope: original /Users/ethan/Projects/hear branch and its108 unrelated untracked files, old worktrees, private data/media/logs. Do not edit the worktree. build/ ignored fixtures are evidence, not product source.

Required guarantees: Core/Store is sole business state owner; media/transcripts/manual corrections/paid remote IDs/usage history retained; additive migration idempotent and historical content preserved; user pause, account block and resource deferral separate; bounded memory/thermal/backlog/retry/response; no fake audio/timestamps; parent-bound explicitly offline tests; formal Login lifecycle unchanged; native playback two-buffer cleanup/cancellation and self-capture exclusion maintained; sources independently recover with honest gaps; correct same-day request invalidation; four components same source/epoch/version at release.

Non-goals: no model/quality threshold change, paid historical rerun, private upload, Keychain ACL change, blanket process kills, no unbounded cache/queue/retry or durability weakening, no automatic GitHub issue closure/messages. Account recharge is external. Physical macOS15/13/Intel and full Sleep/DarkWake hardware are unavailable here: required evidence may use public native fault injection plus available local hardware, but MUST explicitly distinguish limits. Do not raise this into a false all-platform physical-test promise.

In-scope defects: introduced/materially exposed by these changes, plus C01–C18 explicitly owned requirements. Unchanged adjacent baseline bugs are observations unless materially exposed. A missing required evidence gap should name the smallest meaningful check rather than demand new product machinery. Do not elevate optional extra examples into blockers.

Approved architecture changes: existing reads::Pool/Snapshot + indexed failure subset and bounded optimize; App in-flight coalescing/invalidation; Store-derived capacity recoverability; existing Governor/VoiceMatcher/AnalysisClient with bounded nonCritical progress and healthy worker preservation; cloud_queues.blocked_reason and cloud_tasks.retry_failures at current owner, explicit recovery and finite budgets; bounded DeepSeek SSE under existing cloud adapter; public IOKit power fact and existing capture supervisor/gap owner; public14.2Tap exclusion list with progress-confirmed GUI generation; uniform GUI AVAudioPlayer renderer replacing WebAudio under existing ContinuousPlayback, at most two native buffers/context and epoch, Tauri command+completion event (no business state in renderer); legacySCK restarts existing source on GUI identity change; typed numeric diagnostics through existing LogWorker; explicit test-parent kqueue and offline boundaries; protocol23/version0.1.13. No second business store/scheduler/ownership is approved.

Required lenses: Phase1 fresh requirements and adversarial in parallel; main adjudicates repairs and evidence; Phase3 fresh architecture on complete final repaired diff. Reviewers read their matching skill. Architecture also reads specs/DESIGN.md affected playback/entry sections and owner docs. Findings must cite verified file:line, concrete reachable behavior, exact violated requirement, applicability of prior checks, and smallest repair/recheck. Final architecture must enumerate material added mechanisms and verdict PASS/FAIL/ESCALATE. Do not edit or spawn.

Evidence: full python3 scripts/verify.py passed21 checks (Core374 +20 ignored, other Rust components/protocol, React/Node/Python/native, identity/Core/diagnostics/test-owner); frozen356001block semantic comparison and missing-statistics startup; true model samePID success/rejection/success; CUA historical dates/queue/people/sample/31sec playback/seek/capacityresume/pause; actual dual capture/displayoff13samples/forced mic helper recovery8.37s; native power and capture faults; explicit external limits. Smoke cancellation class and real large-response write disconnect are separate checks. Short resource samples do not prove long-term no leak. No cloud keys or real provider calls were used.

Invalidation: production behavior changes invalidate only affected unit/integration/CUA evidence; fixture/docs-only changes need appropriate static/result checks. Build identity changes at clean commit require rebuilt release and final installed verification. Additional findings beyond this fixed contract do not automatically trigger another full review round.

## In-scope files

- `Cargo.lock`
- `Cargo.toml`
- `crates/hear-core/build.rs`
- `crates/hear-core/native/audio_exclusions.h`
- `crates/hear-core/native/resources.m`
- `crates/hear-core/native/system_audio.m`
- `crates/hear-core/native/system_audio_tap.m`
- `crates/hear-core/src/capture.rs`
- `crates/hear-core/src/cloud.rs`
- `crates/hear-core/src/events.rs`
- `crates/hear-core/src/events/asr.rs`
- `crates/hear-core/src/events/gui_read_tests.rs`
- `crates/hear-core/src/knowledge/enrollment/tests.rs`
- `crates/hear-core/src/main.rs`
- `crates/hear-core/src/microphone.rs`
- `crates/hear-core/src/record.rs`
- `crates/hear-core/src/resources.rs`
- `crates/hear-core/src/source_process.rs`
- `crates/hear-core/src/system_audio.rs`
- `crates/hear-core/src/unilogs.rs`
- `crates/hear-protocol/src/diagnostic.rs`
- `crates/hear-protocol/src/lib.rs`
- `package-lock.json`
- `package.json`
- `scripts/measure_gui_reads.mjs`
- `scripts/smoke_agent_context.py`
- `scripts/smoke_core.py`
- `scripts/smoke_diagnostics.py`
- `scripts/smoke_model_preparation.py`
- `scripts/smoke_test_owner.py`
- `scripts/test_capture_status.mjs`
- `scripts/test_cloud_task_status.mjs`
- `scripts/test_native_audio.mjs`
- `scripts/test_native_playback.m`
- `scripts/test_native_power.m`
- `scripts/test_playback.mjs`
- `scripts/test_recording_schedule_ipc.py`
- `scripts/test_timeline_refresh.mjs`
- `scripts/test_upgrade_lifecycle.py`
- `scripts/verify.py`
- `scripts/verify_ingestion_quality.py`
- `specs/ARCHITECTURE.md`
- `specs/prd/devplan_261031_cross_machine_reliability.md`
- `specs/research/2026-10-31-consolidated-issues.md`
- `specs/research/evidence/2026-10-31-cross-machine/review-contract.md`
- `specs/tech_docs/diagnostics-and-verification.md`
- `src-tauri/Cargo.toml`
- `src-tauri/build.rs`
- `src-tauri/native/HearCore-Info.plist`
- `src-tauri/native/playback.m`
- `src-tauri/native/privacy.m`
- `src-tauri/src/entry.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/playback_output.rs`
- `src-tauri/tauri.conf.json`
- `src/App.tsx`
- `src/DetailDrawer.tsx`
- `src/TimelineView.tsx`
- `src/WorkspaceViews.tsx`
- `src/api.ts`
- `src/capture-status.ts`
- `src/cloud-task-status.ts`
- `src/native-audio.ts`
- `src/playback.ts`
