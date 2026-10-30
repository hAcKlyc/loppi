# Phase 1 and adjudication

Both required fresh-context reviewers completed read-only against the64-file frozen snapshot. Neither edited files or spawned reviewers.

Requirements: C01–C10/C12–C15/C17–C18 and migration/owner guarantees PASS within the contract; C11 PARTIAL, C16 UNVERIFIED; clean release/install is intentionally post-gate. Adversarial: one C07 blocker, no further proven blocker after inspecting all production paths. CI native playback output availability is a non-blocking risk requiring actual runner evidence, not an invented physical-platform promise.

## Accepted blockers

- R1 / C11 (requirements, main.rs:1499 in reviewed snapshot): malformed/truncated pre-RPC media errors fall through string classification to worker_failed and retire a healthy worker. Confirmed from media.rs decode errors through voice_input_16k before AnalysisClient.request. Repair retirement at actual RPC boundary, preserving worker retirement on protocol/timeout/resource cancellation. Recheck real success→corrupt/truncated input→success with same PID and existing real RPC error/parent cleanup checks. Prior low-volume-rejection evidence remains valid for that narrower case.
- A1 / C07 (adversarial, main.rs:2070): helper failure→join→slot removed/backoff→sleep leaves an open source_failure gap unsplit, because only surviving finished slots call record_sleep_gap. Confirmed; requirement reviewer direct Store test did not establish the supervisor ordering. Apply native sleep boundary at existing Store gap owner independent of slot, retaining per-session late-journal correction. Recheck no-slot sequence, power messages, gap/retry/journal regressions. Prior ordinary display-off and independent-source recovery evidence remains valid.

## Required evidence gap

- E1 / C16: prior samples cover idle after work and actual quiet dual capture, not active speech/backlog. Close with bounded isolated real-model speech ingestion and voice backlog Core/worker counters, exact build/PIDs and progress. No production cache, persistence weakening, long-duration claim or private/paid network operation is required.

## Limits kept

Physical macOS13/15/Intel and complete Sleep/DarkWake are unavailable; public fault injection/local available hardware are explicitly distinguished. Short resource samples do not prove absence of all leaks or energy problems. Focused follow-ups verify these reported findings and their repairs only; no second full review is planned. Final architecture reviews the complete repaired diff.

## Repair verification

- A1: Store now applies native sleep boundaries before per-slot handling and reuses open tail-session ownership. New no-slot/backoff/idempotence/recovery test plus four existing sleep/retry/journal/cleanup tests pass; separate failed-source gap closure test passes. The original adversarial reviewer completed focused follow-up: CLOSED, no remaining blocker. Native power reducer remains unchanged and previously injected-message evidence applies.
- R1: model retirement moved to actual AnalysisClient.request error boundary; preparation errors are input availability/quality. Real isolated build0.1.13/protocol23/epoch1793386668: corrupt Opus header and truncated file both reject as audio_unavailable with healthy PID42995; exact SIGKILL yields real worker_failed and existing retry; replacement PID43089 completes recovery and another success. No private text/cloud calls. Evidence voice-corrupt-input-lifecycle.json.
- E1: active18-observation voice backlog (3.894s, CorePID39478/worker39591, Core write1961984 bytes, worker0) and public speech dual-source six-block/six-job ingestion (uninterrupted9.30s; delayed close/crash12.84s) record exact build/PIDs and Core/worker counters. Output/time/retention equality holds. No long-duration or physical speech-capture claim.
- Full offline gate rerun after the production repairs:21 checks PASS; Core375 passed/20 ignored, remaining components/protocol/frontend/Python/native/identities/Core/diagnostics/test-owner PASS. No new broad review requested.

## Final review closure

Original requirements reviewer focused follow-up: R1/C11 and E1/C16 CLOSED, PASS, no broader review. Original adversarial reviewer: A1/C07 CLOSED. Fresh final architecture reviewer inspected the complete repaired production diff and returned PASS: all added transient/persistent mechanisms remain at approved owners, no second business store/scheduler, no production architecture blockers. Its sole editorial obligation (ARCHITECTURE.md old single-AudioContext sentence) was corrected to PlaybackAudio/NativeAudio facade and one native renderer session; two-buffer/cleanup/epoch wording remains. Editorial comparison and git diff --check suffice; no production retest required.

Additional actual GUI observation: hide the main window immediately after play; all seven chunks continue to be requested at approximately five-second intervals, native output ends, and reopening from Finder shows31-second end position and Play again. It does not rely on the removed unconditional status broadcasts.
