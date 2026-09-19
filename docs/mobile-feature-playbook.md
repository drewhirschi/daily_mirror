# Playbook: shipping a mobile plus server feature with proof

This is the process used for household onboarding and guided face
enrollment (see `docs/mobile-onboarding-plan.md`). Reuse it for any change
that spans the Rust server, the shared API client, and the Expo app.

## 1. Write the contract first

Put one plan document under `docs/` before any code. It must contain the
data model changes, the domain functions, the HTTP routes with exact request
and response shapes, and the verification list. Agents working in parallel
code against that document, not against each other's branches. Commit it
early: agents in isolated worktrees cannot see uncommitted files.

## 2. Partition work by file ownership

Split into agents whose edits do not overlap:

| Agent                                                  | Owns                                                                                                  | Runs in              |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- | -------------------- |
| Server domain and routes                               | `server/src/*.rs`, `server/app/api/**`, `packages/api/src/schema.d.ts` (regenerated only), `justfile` | the feature worktree |
| Isolated server change (for example queue concurrency) | one function and its tests                                                                            | its own worktree     |
| Mobile                                                 | `mobile/`, `packages/api/src/index.ts`                                                                | its own worktree     |
| Tooling (emulator, scripts)                            | `scripts/`, `docs/`                                                                                   | the feature worktree |

Rules that saved time:

- Tell each agent exactly which files it must not touch.
- Isolated worktrees branch from `main`, so merge `main` into the feature
  branch before merging agent branches.
- The mobile agent cannot wait for the regenerated OpenAPI schema. It writes
  temporary hand-written types in `packages/api/src/onboarding-types.ts`
  style files; the merger replaces them with `components["schemas"][...]`
  aliases afterwards and runs `npm run typecheck`.
- Regenerate the client with `just client` after any route change and commit
  `packages/api/src/schema.d.ts`; `just check` fails on a stale schema.

## 3. Merge and gate

```bash
git merge main
git merge <agent-branch>
just check
```

`just check` covers Rust format, clippy and tests for device, processor, and
server, the client generation diff gate, TypeScript, `npm test`, and the iOS
bundle export.

## 4. Prove it on the Android emulator

`docs/android-emulator.md` and `scripts/android-emulator.sh` cover booting
the emulator, building the dev client, starting Metro, and recording. For a
server-backed walkthrough on this machine:

1. `server/.env.local` with local storage, `PORT=3000`,
   `DAILY_MIRROR_ALLOW_SIGNUP=1`, a processor token, and no Turso or R2
   variables. Start with `cd server && cargo dev`.
2. `processor/.env.local` pointing at `http://127.0.0.1:3000` with the same
   token. Models live in `processor/models/` (ignored; copy from another
   checkout if missing). `just process` drains the queue.
3. `adb reverse tcp:3000 tcp:3000` so the emulator reaches the server, and
   enter `http://127.0.0.1:3000` under Server settings in the app.
4. Camera input: set `hw.camera.front = virtualscene` in the AVD config and
   launch the emulator with a poster image so the virtual scene shows a real
   face. `data/faces/` has legacy face photos already committed.
5. Drive the UI with `adb shell input tap` and `uiautomator dump`, record
   with `adb shell screenrecord` in three-minute segments, and stitch with
   `ffmpeg -f concat`.

Record what the video shows with timestamps in the final report, and fix
bugs found during the walkthrough as separate commits.

## 5. Prove the hosted pipeline

`scripts/test-hosted-processing.py` runs the packaged Vercel function
executables locally against a temporary sqlite database and never touches
production. Plain `vercel build` does not produce a compatible package; use
the prepared-cache Docker workflow in `scripts/native/README.md`
(`scripts/native/build.sh` with `DAILY_MIRROR_NATIVE_CACHE` and
`DAILY_MIRROR_ZIG_DIR` set). The private models and libraries under
`server/resources/vision` are ignored by Git and must be copied from a
checkout that has them. A warm cache builds all three bundles in under a
minute.

Two lessons from the first run: any new route that calls a function behind
`#[cfg(feature = "image-processing")]` needs a `bundle.toml` assigning it to
the `images` bundle, or the default bundle fails to compile; and every local
libsql connection must use WAL mode with a busy timeout, or parallel workers
fail with instant lock errors.

## 6. Preview deployment

`just deploy-preview` builds locally and deploys an unaliased preview. Note
that preview deployments currently share the production database and R2
bucket, and hosted-processing variables exist only for Production. Before
testing signup or enrollment on a preview, give the Preview environment its
own `DAILY_MIRROR_DATABASE_URL`, `DAILY_MIRROR_DATABASE_AUTH_TOKEN`, and
`DAILY_MIRROR_R2_PREFIX`, plus `DAILY_MIRROR_ALLOW_SIGNUP=1`,
`DAILY_MIRROR_HOSTED_PROCESSING=1`, `DAILY_MIRROR_HOSTED_CONCURRENCY=4`,
`DAILY_MIRROR_WORKER_URL`, `DAILY_MIRROR_PROCESSOR_TOKEN`, `MEDIAPIPE_LIB`,
and `CRON_SECRET`. Point the emulator app at the preview URL and repeat the
walkthrough.

## 7. One pull request

Squash nothing; keep the agent commits. The PR description lists what was
verified, what was not, and the follow-ups from the plan document.
