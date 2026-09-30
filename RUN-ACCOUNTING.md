# RUN ACCOUNTING — Lucerna campaign

Rig: Claude Code v2.1.283, Claude Pro account (5h + weekly windows).
Server: theta-gryphonis (EPYC), 4 cores, 7 GB RAM. **Headless — no display, ever.**

Notation: token sums are raw JSONL sums from
`~/.claude/projects/-home-clawuser-projects-lucerna/*.jsonl`
(`input + cache_read + cache_creation + output`). Where `/usage` was readable, the
session/weekly percentages and reset time are recorded verbatim. Honest gaps are
recorded as gaps — a missing baseline is never interpolated.

---

## Phase A — Implementation planning (claude-opus-5-5, --effort high)

| Item | Value |
| --- | --- |
| Session start | 2026-09-30 00:46 CEST |
| Session end | 2026-09-30 01:05 CEST |
| Resume handle | `claude --resume 48009747-4bd4-4a3e-8623-d1e0c7d98a78` |
| Deliverable | `docs/IMPLEMENTATION-PLAN.md` (1,954 lines) |
| Commit | `b73abda docs: add implementation plan for the v1 campaign` |
| Tokens (in / cache_read / cache_create / out) | 20 / 1,076,841 / 175,650 / 121,388 (total 1,373,899) |
| `/usage` session % | 15% used (resets 5:30am Europe/Amsterdam) |
| `/usage` weekly % | 2% used (resets Oct 4, 9pm Europe/Amsterdam) |
| Cost | $4.10 (API 19m 29s, wall 21m 12s) |

Notes:
- Plan produced: 1,954 lines, covering 16 architectural decisions (D1–D16), 8-crate workspace boundary layout enforcing §48, mpv flags rationale, state machine, X11 integration protocol test strategy under Xvfb, packaging matrix, and explicit disclaimer that visual validation is not performed headlessly.
- Committed cleanly to `main` at `b73abda`.
- Session clean-exited with `/exit`, resume handle recorded.
- Phase B launched at 2026-09-30 01:09 CEST with Sonnet 5.5 (`--effort high`).

---

## Phase B — Implementation (claude-sonnet-5-5, --effort high)

| Item | Value |
| --- | --- |
| Session start | 2026-09-30 01:09 CEST |
| Session end | 2026-09-30 07:02 CEST (turn completed; parked idle) |
| Resume handle(s) | `claude --resume 0a6c5736-183f-496a-838a-f3ea63e272af` |
| Final commit / tag | `eea6446` / `v1.0.0-rc.1` (workflow fix merged at `3acface`) |
| Tokens (in / cache_read / cache_create / out) | 688 / 166,176,060 / 1,968,252 / 764,943 (total 168,909,943 unique) |
| `/usage` session % | 21% used (resets 10:29am Europe/Amsterdam) |
| `/usage` weekly % | 17% used (resets Oct 4, 8:59pm Europe/Amsterdam) |
| Cost | $49.25 (API 1h 31m 37s, wall 7h 35m 14s) |

Notes:
- Full milestone progression v0.0.1 through v1.0.0-rc.1 executed autonomously.
- Token counts: unique message sum above; raw assistant event stream totals 1,328 input / 309,156,856 cache read / 3,891,759 cache create / 1,616,140 output (314,666,083 total). Claude Code `/usage` reports 3.3k in / 168.1m cache read / 2.0m cache write / 773.9k out ($49.25).
- All 11 annotated tags created, pushed, and verified.
- Pre-release `v1.0.0-rc.1` published on GitHub with 4 assets, verified by `sha256sum -c`.
- Desktop acceptance tests LUC-T01 through LUC-T20 frozen in `docs/MANUAL-ACCEPTANCE.md` with status `NOT RUN — REQUIRES REAL DESKTOP`. Desktop appearance status: `IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED`.

### Milestone log

| Tag | SHA | Gates (fmt/clippy/test/build) | Packaging | CI | Notes |
| --- | --- | --- | --- | --- | --- |
| v0.0.1 | 8e6862f | PASS (fmt, clippy -D warnings, 6 tests, release build, headless smoke tests) | n/a | PASS (#36644636839) | Workspace bootstrap (8 crates), architecture boundary test, headless safe |
| v0.1.0 | 4434647 | PASS (fmt, clippy, unit/integration tests, mpv supervision) | n/a | PASS (#36646823076) | Renderer core, mpv process supervision & IPC socket, restart policy |
| v0.2.0 | 6652f06 | PASS (fmt, clippy, Xvfb protocol tests) | n/a | PASS (#36648650162) | Cinnamon/X11 backend, protocol tests under Xvfb, unmanaged bottom surface |
| v0.3.0 | 9e2bc3f | PASS (fmt, clippy, daemon integration, cli e2e) | n/a | PASS (#36652644543) | Daemon (lucernad) and CLI (lucernactl), D-Bus IPC, single instance lock |
| v0.4.0 | be7adf4 | PASS (fmt, clippy, GTK UI structure tests) | n/a | PASS (#36654603327) | GTK 4 control application (lucerna), headless-safe exit 1, settings presenter |
| v0.5.0 | ec3d3f9 | PASS (fmt, clippy, multi-monitor tests) | n/a | PASS (#36656747986) | Multi-monitor EDID identity, per-display wallpaper, occlusion & screen lock policy |
| v0.6.0 | fb9a87f | PASS (fmt, clippy, lifecycle recovery tests) | n/a | PASS (#36657813596) | Desktop lifecycle, autostart synchronization, crash recovery, bounded logs |
| v0.7.0 | 1862428 | PASS (fmt, clippy, packaging smoke tests) | deb+rpm (PASS) | PASS (#36666208616, pkgs #36665848190) | Native packaging definitions, man pages, container build & install smoke tests |
| v0.8.0 | 1f79de3 | PASS (fmt, clippy, full test matrix) | deb+rpm (PASS) | PASS (#36667188842, pkgs #36667189090, rel #36667189051) | Automated distribution pipeline (release.yml 12-step verification) |
| v0.9.0 | d52b568 | PASS (fmt, clippy, doctor diagnostics) | deb+rpm (PASS) | PASS (#36668440115, pkgs #36668440462, rel #36668440356) | Release candidate baseline, doctor diagnostics, docs freeze & acceptance test list |
| v1.0.0-rc.1 | eea6446 | PASS (fmt, clippy, full workspace tests, release build) | deb+rpm+tgz+sums (PASS) | PASS (#36669703759; see deviation for release) | Desktop acceptance candidate. GitHub pre-release with 4 verified assets |

### Deviations & Incidents
- **v1.0.0-rc.1 Release Workflow Failure & Manual Dispatch Fix:**
  On pushing the tag `v1.0.0-rc.1` (commit `eea6446`), the release workflow run `36669703879` passed jobs 1-4 (tag/version check), job 9 (source archive), jobs 5-6 (full test suite, clippy, headless smoke, cargo-deny, shellcheck), and jobs 7-8 (native container package builds and install smoke tests in fresh Ubuntu 24.04 and Fedora 44 containers), but **FAILED at job 10-12** (step 6: `11-12. Create or update the release and attach the artifacts`) due to asset naming in `gh release upload` where GitHub sanitizes `~` to `.` for uploaded Debian package names (`lucerna_1.0.0~rc1_amd64.deb` vs `lucerna_1.0.0.rc1_amd64.deb`).
  The harness developed a workflow fix on branch `ci/release-existing-tag` (commit `bc27f72`: *"ci: allow releasing an existing tag by hand and name assets as GitHub serves them"*), merged it to `main` (`3acface`), and re-ran the release workflow via manual `workflow_dispatch` run `36671007802` targeting `v1.0.0-rc.1`. Run `36671007802` completed successfully (6m 29s), publishing pre-release `v1.0.0-rc.1` with all four expected assets (`lucerna-1.0.0-0.rc1.x86_64.rpm`, `lucerna-1.0.0-rc.1.tar.gz`, `lucerna_1.0.0.rc1_amd64.deb`, `SHA256SUMS`). Checksum verification (`sha256sum -c SHA256SUMS`) verified cleanly on fresh download.
- **Supervisor Idle Discriminator Failure:**
  The supervisor monitoring loop failed to report milestones between v0.1.0 and completion because it checked `grep -q '❯ Try "'` to detect idle prompts. The Claude Code composer hint line is dynamic/rotating (observed values include `Save the session notes to memory`, `Research this topic and write me a brief`, `Try "write a test..."`), causing the supervisor to misclassify a parked/completed harness as working. SUPERVISOR-BRIEF.md has been corrected: absence of `esc to interrupt` in the status bar is the authoritative idle discriminator.

---

## Pre-flight (outer layer, before arming)

Recorded 2026-09-30:

- Claude Code v2.1.283 present; `claude auth status --text` → `Login method: Claude Pro account`.
- Model probes: `claude-opus-5-5` → `OPUS55_OK`; `claude-sonnet-5-5` → `SONNET55_OK`. Both `is_error: false`.
- Default model is `claude-sonnet-4-6`, so every launch names its model explicitly.
- Toolchain installed: rustc/cargo **1.98.1** (rustup stable; system cargo was 1.75.0),
  GTK4 dev **4.14.5**, mpv **0.37.0** + libmpv-dev, x11 1.8.7 / xrandr 1.5.2 / xfixes 6.0.0,
  Xvfb + xvfb-run, rpmbuild **4.18.2**, dpkg-buildpackage/debhelper/dh-make/lintian/fakeroot,
  Docker 29.8.1.
- `DISPLAY` unset; no graphical session available on this server.
- GitHub Actions: account billing blocks **private** repos
  (`blindicide/polmon` 2026-09-28 → `steps=0`, "job was not started because recent account
  payments have failed or your spending limit needs to be increased"). **Public** repos run
  free: probe run `36641333199` → `completed success`, job steps=5, no billing annotation.
  Repo `blindicide/lucerna` is public → CI/packaging/release workflows are executable.
- CI runner reference (`ubuntu24` image from the probe): rustc/cargo 1.98.1 **preinstalled**,
  GTK4 dev headers **absent** → `ci.yml` must install `libgtk-4-dev` (and mpv) itself.

### Supervisor rig (changed at operator request)

- First supervisor launch inherited the session default (`deepseek-flash` via
  `ds2.net.a.blindicide.ru`) and **stalled immediately**: `⚠ Auxiliary title generation
  failed: HTTP 503: No upstream keys are currently available`, parking at `msg=interrupt`.
  The harness (Claude Code) was never affected — it does not use that provider.
- Operator directive: move the supervisor to **AGY-B / `gemini-3.8-flash-high`**
  (`custom_providers.AGY-B`, base_url `https://agy.net.a.blindicide.ru/v1`).
- Verified the endpoint directly before repointing: `POST /v1/chat/completions`
  with `gemini-3.8-flash-high` → `AGY_OK`.
- Relaunched via
  `tmux respawn-window -k -t lucerna:supervisor 'hermes --cli --provider AGY-B -m gemini-3.8-flash-high'`.
  Provider key was placed in the **tmux session environment** (`tmux setenv`, value expanded by
  the shell so the secret never appears on a command line or in logs), because the key is not
  exported from `~/.bashrc`.
- Session `20260930_005314_8874af`. Confirmed live: brief read, phase briefs read, harness pane
  captured, first `sleep 240` in progress at ~45.3K/256K pinned context.

