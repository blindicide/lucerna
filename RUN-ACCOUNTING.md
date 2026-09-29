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
| Session start | (pending) |
| Session end | (pending) |
| Resume handle | (pending) |
| Deliverable | `docs/IMPLEMENTATION-PLAN.md` |
| Commit | (pending) |
| Tokens (in / cache_read / cache_create / out) | (pending) |
| `/usage` session % | (pending) |
| `/usage` weekly % | (pending) |
| Cost | (pending) |

Notes:

---

## Phase B — Implementation (claude-sonnet-5-5, --effort high)

| Item | Value |
| --- | --- |
| Session start | (pending) |
| Resume handle(s) | (pending) |
| Final commit / tag | (pending) |
| Tokens (in / cache_read / cache_create / out) | (pending) |
| `/usage` session % | (pending) |
| `/usage` weekly % | (pending) |
| Cost | (pending) |

### Milestone log

| Tag | SHA | Gates (fmt/clippy/test/build) | Packaging | CI | Notes |
| --- | --- | --- | --- | --- | --- |
| v0.0.1 | | | n/a | | |
| v0.1.0 | | | n/a | | |
| v0.2.0 | | | n/a | | |
| v0.3.0 | | | n/a | | |
| v0.4.0 | | | n/a | | |
| v0.5.0 | | | n/a | | |
| v0.6.0 | | | n/a | | |
| v0.7.0 | | | deb+rpm | | |
| v0.8.0 | | | deb+rpm | | |
| v0.9.0 | | | deb+rpm | | |
| v1.0.0-rc.1 | | | deb+rpm+tgz+sums | | |

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
