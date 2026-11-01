# Windows source candidate review

Contract: [windows-review-contract.md](windows-review-contract.md). Baseline `1e36cf56c5c49b9cc4ba9d9305131e7e83782ca9`; repair mode. Coordinator frontend patch is excluded from authored scope and commit. This is an integration checkpoint, not release acceptance.

## Phase 1 and root adjudication

Two fresh read-only reviewers inspected the frozen 74-file initial scope. Both independently reproduced the OCR failure. Requirements also inspected CLI installation and macOS test compatibility; adversarial inspected concurrency, security, capture, lifecycle and entry geometry.

| Finding | Classification and decision | Repair and affected evidence |
| --- | --- | --- |
| Non-ACP OCR paths lose emoji | Blocking defect: required Unicode resource/data paths fail in the actual native engine. | Embed activeCodePage UTF8 in source-built OCR process; do not alter system locale. Actual Core OCR child recognizes synthetic PNG with engine/tessdata/image in Unicode+space paths: `build/windows-ocr-test/report.json`. |
| CLI hard links lose original install location and repeat/upgrade behavior | Blocking defect: cold installed CLI cannot locate Core. | Windows CLI installation copies a delegating executable plus bounded private target metadata; validates ownership/hash/size and forwards to the actual installation. No business/config owner. Repeat install, cold Core lookup without override, MCP stdio and unrelated-file preservation pass: `build/windows-cli-install-test/report.json`. Process initialization clears inherited stdio before delegation. |
| macOS onboarding test lost PermissionsExt import | Blocking source defect. | Restore test-only Unix trait import. Windows unit build passes; actual Mac test is coordinated after this source checkpoint. |
| Offscreen saved floating anchor makes native region disappear | Blocking defect: HWND moves into work area but region does not. | Select anchor monitor and clamp anchor before local geometry; actual Win32 region checks pass for 15 work-area/mode/DPI cases: `build/windows-entry-geometry/report.json`. |
| Current GUI smoke and audible system-capture rerun incomplete | Required candidate evidence gaps, not production defects. | Correct Vite/CDP environment; actual current WebView2 GUI privacy/entry/playback/exclusion/normal shutdown passes. Actual system capture produces nonzero PCM (peak 0.612976): GUI and Core reports in `windows-evidence.json`. |

Additional root repairs within the same contract:

- Package PE closure found ONNX Runtime requires `msvcp140_1.dll`. Pin and bundle all ten official installed VC CRT files plus redistribution license. Inspect regular and delay imports; host System32 never satisfies a VC dependency. Fresh 19-image stage passes; temporarily removing that DLL produces a failing closure report.
- PowerShell native stderr warnings must not abort a successful build: wrappers preserve actual Node exit status.
- Installer preparation routes GUI through its normal exit callback, then existing Core quit/drain and sibling process handles. NSIS aborts on failure and only invokes an existing CLI registered to that installation. Real GUI and active dual-recording Core preparation exit normally, forced=false. Actual NSIS upgrade/uninstall remains a final-delivery check.
- All four Windows components embed the same UTF8/long-path process manifest. Actual read-only Unicode model VAD/X-ASR/token loading and persisted capture-to-CLI text pass; no prior Sherpa Unicode failure is claimed.

## Verification after repairs

`cargo test --workspace --locked`: Core 392 passed / 22 ignored, GUI 11, protocol 19, CLI 2, worker 2; no failures. Full workspace development build passed with one build epoch. Installed CLI follow-up passed. Synthetic Credential Manager target is removed after write/read/delete. Existing actual named-pipe identity, DACL, exclusivity, frame/deadline and lock tests pass.

Latest native evidence is curated in [windows-evidence.json](windows-evidence.json). The persisted Core recording/VAD/X-ASR/CLI chain is distinct from the private source probe and direct synthetic-WAV worker test. Cloud queues use disposable configured+paused metadata under the existing offline test owner; no paid request or user credential is used. The packaged source has no new test bypass.

## Final architecture gate

Requirements focused follow-up: PASS. All three source defects and both candidate evidence gaps closed. Reviewer independently verified the actual Unicode fixture, bounded launcher and cold/MCP tests, scoped Mac import, current GUI and audible system reports, and repaired build/test logs. Mac execution remains coordinated after push.

Adversarial focused follow-up: PASS. Reviewer independently reran the originally failing Unicode PNG against rebuilt engine SHA256 `538bf25b9a2e3e3415341fc145db34f5e869441a69e865d7e8fb36406a4677ed`: exit 0 with expected WINDOWS/12345. Independently reran the actual native geometry executable: 15 cases pass. Both findings closed.

Fresh architecture review inspected all 87 frozen files and accepted the necessary platform, IPC, capture, credential, OCR, GUI, launcher and installer mechanisms. No owner/protocol/store/retry complexity issue or escalation was found. It found a second Unix-only trait import removed from `events/understanding_replay_tests.rs`; root classified this as a blocking macOS test compile defect and restored module-level cfg(unix) OpenOptionsExt. No new abstraction or runtime behavior is introduced. Focused architecture follow-up is pending; actual Mac compilation belongs to coordinator integration after push. Coordinator frontend remains excluded.

Final architecture focused follow-up: **PASS**. All 87 refreshed hashes matched; the only blocker is closed and the complete prior architecture inspection remains applicable. Requirements independently verified this second source repair: PASS. All three required lenses are complete. Actual Mac compilation and remaining final-delivery checks are not represented as completed.

## Remaining final-delivery obligations

Mac integration/regression; actual NSIS install/overwrite/default uninstall with active recording and default-root SQLite/media/note sentinels retained; clean integrated source and four-component package identity; missing/corrupt resource acceptance; unlocked desktop capture/HWND exclusion and physical UI interaction. One real Windows 11 x64 device is covered. The desktop remains locked; no unlock/security bypass. Internal WebView2 screenshot proves host layout only. Windows signing identity is unavailable: unsigned.
