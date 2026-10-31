# Windows native implementation and verification

Status: in progress. This is the Windows implementation notebook; the coordinating Mac session owns the unified delivery devplan and release decision.

## Contract

- Base: `1e36cf56c5c49b9cc4ba9d9305131e7e83782ca9`; branch: `codex/windows-native`; version: 0.1.18.
- Support Windows 11 x64 (build 22000 or later). Retain the existing Core as the only business/database owner and the current IPC commands, lifecycle, limits, media journals and model manifests.
- Implement authenticated local IPC, QPC capture coordinates, microphone and WASAPI process-tree exclusion, safe credentials, screenshots and independent offline OCR, playback, entry/tray, startup, resources, worker supervision and complete NSIS resources.
- Preserve macOS implementations and original PNG/ICNS bytes. No Linux, ARM64 claim, historical hear migration, paid API testing, user-data uploads, security weakening or certificate purchase.
- File ownership: this branch owns crates/**, src-tauri/**, Cargo manifests/lock, config/**, backend/build scripts and new Windows evidence/docs. The Mac coordinator owns src/**, frontend tests/design, README.md, specs/ARCHITECTURE.md and the unified devplan.
- Source candidate requires static/build/runtime verification and independent requirements/adversarial/architecture review before commit. The coordinator integrates frontend and supplies final clean source for release builds. No main/tag/release mutation here without its explicit final instruction.

## Required evidence

- [ ] Shared Rust platform implementation and Windows compile/tests.
- [ ] Actual dual-source capture, time mapping, local inference and own-playback exclusion.
- [ ] Restricted IPC identity, bounds/deadlines, owner/lifecycle/worker cancellation and failures.
- [ ] Test-only credential round trip with cleanup; screenshot/OCR and GUI entry/playback.
- [ ] Offline models, Unicode/spaced paths, missing/corrupt resources and four-component build identity.
- [ ] Actual NSIS install, launch, upgrade and uninstall preserving user data; runtime/DLL resources.
- [ ] Independent review, fixes, candidate commit and push.

## Audit and decisions

- Windows 11 Pro x64 build 26300; Node 24.13.0, Rust 1.96 MSVC, VS/SDK 26100, WebView2 and NSIS 3.11 are installed. No code-signing identity was found; Windows artifacts must say unsigned.
- Initial worktree clean, no existing Windows backend, no running loppi/hear processes. Microphone and render endpoints are present. Only disposable test data and synthetic/public audio are authorized.
- About 14.8 GiB disk free initially: disable dev debug symbols/incremental builds for this task and keep regenerated artifacts bounded.
- Native Windows Media OCR is not a supported dependency for an unpackaged NSIS application. Use an independent offline engine with pinned resources and licenses.

## Progress

- Created the isolated implementation branch and read the start-dev workflow. No implementation changes before this contract was recorded.
- Shared protocol compiles on Windows. Implemented logon-SID-restricted named pipes, actual peer user/session/PID checks, bounded framing and overlapped I/O cancellation; private-file DACL creation, Unicode commands, QPC clock and process lifetime watches are in place.
- Core/CLI/worker development binaries linked successfully with pinned sherpa 2.0.0 shared DLLs. Real CLI on-demand status and quit passed in `build/windows-audit-data` without touching user data. Actual microphone probe: 48 kHz stereo, 47,520 frames, zero drops; actual process-exclusion WASAPI probe: 48 kHz stereo, 48,000 frames (silent at probe time), zero drops. These are probes, not full continuous-recording acceptance.
- Models downloaded to `build/cache/windows/models` and verified against the unchanged models.lock. Tesseract 5.4.0 archive was extracted without running its installer, SHA256 pinned along with chi_sim/eng tessdata_fast weights in config/windows-assets.json.
- Windows GUI backend passes cargo check. Native tray (persisted `top`), floating entry, rendering, settings/clipboard/login, independent OCR child, WIC PNG encoding and power/resource bridges are implemented. GUI runtime and installer acceptance remain open.
- Current work: compile and adapt portable Rust tests, stage complete runtime resources, run real isolated GUI/capture/inference/playback/OCR and lifecycle acceptance, finish NSIS build and independent review. No candidate commit/push or release action yet.

## Integration candidate boundary

The coordinator explicitly requested an independently reviewed source candidate before final installer acceptance. A candidate commit is not release acceptance. Integrated frontend/Mac regressions, clean-source Windows packaging, installation/upgrade/uninstall sentinels and unlocked desktop capture remain mandatory. Do not move v0.1.18 or upload artifacts from this working tree.

Core defaults to `%LOCALAPPDATA%\app.loppi.core`; GUI route/preferences retain Tauri's application identifier directory; NSIS current-user install defaults to `%LOCALAPPDATA%\loppi`. These are separate roots, with no migration/fallback. Default uninstall must retain Core data and credentials. Tauri's optional application-data removal concerns GUI configuration only.

## Implemented platform boundaries

- `loppi-protocol::platform` owns OS file/process/time operations. Private files receive a protected user DACL at creation. LockFileEx uses a regular `.publication.lock` for journals; Windows cannot lock a directory handle. Files are flushed and MoveFileExW publishes with WRITE_THROUGH; Windows directory sync validates directory presence, not POSIX directory fsync. Database media keys always use `/`.
- Local-only named pipes are scoped by canonical data-root hash, user SID and session. A protected logon-SID DACL and actual process user/session/PID verification constrain both ends. First-instance creation prevents replacing a live namespace. Transport frames are capped at 48 MiB; Core retains its 1 MiB request, 32-client and 3-second request bounds. Overlapped cancellation waits before releasing buffers.
- CPAL microphone and process-loopback WASAPI use QPC. GetBuffer's QPCPosition is already 100 ns units, multiplied by 100 for ns. Process loopback uses explicit 48 kHz stereo float, not GetMixFormat. Exclusion targets the same-user/session/sibling-install GUI process tree, including WebView descendants; GUI replacement changes the existing exclusion generation.
- Background spawns hide consoles. Worker channels retain deadlines/cancellation and parent-process-handle watches. Process initialization clears inherited stdio flags, preventing background Core from holding CLI output open. Third-party OCR uses a kill-on-job-close job.
- Credential Manager implements the existing service/reference abstraction; Core still owns cloud configuration transactions. Tests create unique synthetic targets and remove them. No real user key or paid API was used.
- Windows resources expose memory load, process private bytes, battery/power and suspend/resume generation. Thermal data is unavailable in this bridge; zero is neutral input, not a sensor measurement.
- Persisted entry `top` is the system tray. Native outline/hit tests use 26px panel / 20px menu radii with actual DPI. The tray handles Explorer restart and routes Open/Quit through existing lifecycle. HKCU Run contains a quoted executable plus `--login`; disabled StartupApproved state is reported, and another installation's Run registration is retained.
- CPAL playback preserves context/epoch/buffer commands and `loppi-playback-ended`, with bounded PCM queues and actual output callbacks.

## OCR and screenshot facts

The shipped OCR is a PNG-only Tesseract 5.4.0 executable built from four checksum/size-pinned upstream archives in `config/windows-ocr-sources.json`: Tesseract, Leptonica 1.84.1, libpng 1.6.43 and zlib 1.3.1. Static MSVC/CRT linkage leaves only KERNEL32.dll imports. Archive/curl/TIFF/JPEG/training/GUI/OpenMP dependencies are disabled. Four upstream license texts and source provenance are bundled. Pinned chi_sim/eng weights remain in `config/windows-assets.json`. The previously inspected UB-Mannheim installer is not the shipped engine/dependency source.

Wire HWND IDs remain u32: LongToHandle preserves the significant 32 bits with x64 sign extension. Every excluded HWND is checked against the authenticated pipe peer PID. Hiding is asynchronous with a 500 ms completion bound per window. GDI/WIC encodes PNG; the OCR helper has a 15-second deadline and 128 KiB output limit.

Windows `privacy_status.system_audio` is independent of `screen_and_system_audio`: initially 0, then 1 only after the current GUI's actual successful probe or while Core reports system recording. An unavailable Core source clears the probe fact. This is an ephemeral native observation, not persistent configuration. The frontend must use it for Windows setup. Screenshot readiness uses the screen field. `request_screenshot_permission` returns 1 for accessible/unlocked Default desktop, otherwise 0, without a permission prompt.

Screenshot errors: `screenshot_desktop_unavailable` for locked/unknown/inaccessible desktop, `screenshot_window_owner` for invalid/foreign HWND, `screenshot_timeout` for hide timeout, otherwise `screenshot_failed`. A failed capture removes pending output and does not insert a successful moment/receipt. Mac behavior is preserved.

Actual console facts: process Session=1, active console=1, connection=Active, SessionFlags=0 (locked), window station=WinSta0, thread/input desktop=Default, foreground HWND=0. GDI and independent Pillow both returned black pixels. No unlocking or security bypass was attempted. Real desktop capture/HWND exclusion are **pending unlock**. Synthetic PNG OCR and WebView screenshots do not close that acceptance. Flag meanings follow [Microsoft documentation](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ns-wtsapi32-wtsinfoex_level1_w).

## Commands and observed evidence

Use PowerShell, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_INCREMENTAL=0`, and absolute `SHERPA_ONNX_LIB_DIR` pointing to the extracted pinned library. Use one `LOPPI_BUILD_EPOCH` for identity checks. Native `smoke_windows_*` commands are explicit opt-in, outside automatic test discovery.

| Command | Observed evidence |
| --- | --- |
| `cargo build --workspace --locked` | All four development components link on Windows. |
| `cargo test --workspace --locked` | Core 392 passed / 21 ignored; GUI 11; original protocol 14; CLI 2; worker 2. Subsequent platform tests: protocol 19 passed. Final affected results belong to candidate review. |
| `python scripts/smoke_windows_core.py` | Actual 10-second parallel mic/system capture, four blocks/two clocks; offline X-ASR recognizes local synthetic English speech. Audible system-media playback assertion added for rerun. |
| `node scripts/smoke_windows_gui.mjs` | Actual Tauri/WebView entry mode roundtrip; native four-second tone and ended event; system exclusion probe: 96,000 frames, peak 0, no drops. App-content image only. Integrated frontend still pending. |
| `python scripts/smoke_windows_lifecycle.py` | Real dual-source Start/Pause/stable block count/Resume/new blocks/Quit passed in Unicode+space data path. Synthetic configured+paused queue metadata and existing test owner prevent all credential/network calls. Seven CLI context operations return typed results; missing event returns a truthful error. Actual MCP stdio advertises seven tools. |
| `python scripts/smoke_windows_ocr.py` | Core OCR helper recognizes locally rendered English/Chinese PNG; one opt-in test passed. No desktop capture claim. |
| `node --test scripts/test_build_plan.mjs` | 11 passed, translated-Intel-on-Mac check skipped. Mac defaults remain configuration-driven. |
| `python scripts/verify_build_identity.py --directory build/windows-dev-runtime` | Four development identities compared with one epoch. Dirty builds are not release builds. |
| `python scripts/verify.py --suite all --report build/windows-verify.json` | Routes to Windows offline static/unit/build checks. Native acceptance remains explicit. |
| `./build_dev.ps1` / `./build_release.ps1` | Same build owner; release adds a clean-source gate. Complete NSIS acceptance is pending; first dirty build paused to prioritize source review. |

The builder verifies models/native archives, builds all four components with one epoch, bundles inference DLLs, app-local VC runtime, offline WebView2 installer, models, OCR, licenses and SHA inventory, checks compiled identities and writes unsigned metadata. Tested platform is one Windows 11 Pro x64 build 26300 Surface Laptop 3; no other-device/ARM64 claim. Final NSIS install/upgrade/uninstall, default-root retention, missing/corrupt models and unlocked desktop checks remain open. This is not release readiness.

## Repaired source checkpoint

Latest workspace run: Core 392 passed / 22 ignored; GUI 11, protocol 19, CLI 2, worker 2; full four-component development build passes. The curated JSON supersedes earlier incremental observations above. Phase 1 independent requirements/adversarial reviewers and focused repairs are documented in [windows-review.md](windows-review.md).

`smoke_windows_lifecycle.py --runtime "build/windows-unicode-install/<Unicode install>"` passed with bundled read-only models. This is actual Core persistent dual capture, VAD/X-ASR/token loading, coarse rows linked to system media/time ranges, and successful current-context CLI read of synthetic speech. It is separate from direct worker synthetic-WAV recognition and private source probes. Paused synthetic cloud metadata under the existing offline test parent blocks all paid/credential requests; production has no new readiness bypass. Normal prepare-install during resumed dual recording returns stopped=true, forced=false.

The current real WebView2 smoke passes independent locked-screen/system-audio privacy facts, native playback ended/exclusion, entry roundtrip and normal GUI/Core exit. [main-webview.png](windows-evidence/main-webview.png) contains only synthetic onboarding and proves the Windows host font/layout; it does not prove desktop capture. Actual captured system speech decodes to peak 0.612976 over 240,000 samples. Locked capture returns screenshot_desktop_unavailable with zero successful moments and zero images while WASAPI continues capturing.

Source-built OCR embeds `windows-ocr.manifest` with activeCodePage UTF8. Real Core-child OCR passes with image, engine and tessdata paths containing Chinese, emoji and spaces. All four application components also embed a shared UTF8/long-path manifest. System locale is unchanged. The bundled OCR's license/source hashes remain pinned.

Windows install-cli uses an installation launcher and bounded private target metadata instead of a hard link. It follows the original installation across replacement, preserving cold Core lookup and stdio; repeat install, MCP stdio and unrelated-file preservation pass. NSIS prepare hooks call normal GUI exit, existing Core quit/drain, then wait for this installation's remaining processes; failure aborts instead of killing recordings. Only a registered matching installation's existing CLI is invoked.

VC redistributables are package dependencies even when installed on this development machine. `config/windows-assets.json` pins all ten official installed VC CRT DLLs, their sizes and hashes, plus Microsoft's redistribution license document. `verify_windows_dependencies.py` checks regular and delay imports against an explicit Windows 11 inbox baseline without consulting System32. A fresh 19-image app/OCR/CRT/inference stage passes; withholding msvcp140_1.dll correctly fails. The first dirty optimized NSIS build completed (388,293,595 bytes) but predates these final repairs and is not installation or release evidence. Rebuild the complete package before acceptance.

Both PowerShell entrypoints use the same Node build owner; release rejects dirty source as intended. Initial frontend lint encounters inherited Windows CRLF formatting, while type checking passes; coordinator-owned frontend files were not normalized here. Mac source/runtime verification belongs to the coordinator after candidate push.
