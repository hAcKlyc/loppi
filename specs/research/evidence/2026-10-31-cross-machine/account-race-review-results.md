# Account-race review results

New material C08 correction against2b740c0, after the broad review gate. Same0.1.13 delivery; no independent product release. Old draft binary was interrupted before installation/distribution and marked obsolete.

Fresh requirements and adversarial reviewers both returned PASS, no blocking findings or required evidence gaps. They confirmed all four request sites hold the existing provider gate through account persistence, the common handler skips another402 mutation (including after ASR becomes submitted through explicit retry), task/queue transaction is atomic, terminal replay consumes no extra failure budget, and storage-persistence failure cannot apply stale account blocking or create paid automatic replay. No remoteID, usage, pause/configuration or historical-replay changes.

The new terminal replay/configuration/continue regression FAILED on the pre-repair source and PASSed after repair. Adversarial independently reran it and confirmed PASS. All21 offline checks pass after the repair, Core376 passed/20 ignored. No live provider, recharge or private export used. A separate full ASR interleaving runtime case is additional coverage: ordering is established at the actual four call sites and common-handler branch, without adding test-only product machinery.

Original baseline CI run41113351437 passed the full gate on its macos15 runner, including native playback; the corrected commit will run CI again. Physical source-capture platform and full Sleep/DarkWake limits remain unchanged. Final architecture on this narrow complete diff is required before its commit/rebuild.

Final fresh architecture verdict PASS: all4 error adapters hold the existing gate through Store/events persistence, common402 skip prevents late ASR submitted retry overwrite, and affected-row count is the account-block source of truth. Helper/fixed storage-error reuse existing Failure/transaction paths; no new state, owner, lock, schema or recovery service. No findings or extra evidence gap. Review gate complete; rebuild corrected clean commit before installation.
