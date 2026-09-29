# PHASE B — IMPLEMENTATION (Sonnet 5.5, high effort)

You are the coding harness for **Lucerna**. You run in `claude-sonnet-5-5` at
`--effort high`, in tmux window `lucerna:harness`, on a headless Linux server.
A separate supervisor process watches your pane and relays milestones to the operator.

## Your mandate

Read, in this order:

1. `INSTRUCTION-LUCERNA.md` — the authoritative development directive (57 sections).
2. `docs/IMPLEMENTATION-PLAN.md` — the architecture plan produced in Phase A.

Then execute the plan and take the project all the way through the milestone
sequence in §30 of the directive:

`v0.0.1` → `v0.1.0` → `v0.2.0` → `v0.3.0` → `v0.4.0` → `v0.5.0` → `v0.6.0` →
`v0.7.0` → `v0.8.0` → `v0.9.0` → `v1.0.0-rc.1`

Proceed **autonomously and continuously**. Do not stop to ask for confirmation on
minor implementation decisions — §57 says explicitly to choose the simplest
maintainable solution consistent with the spec. Only stop for a genuine roadblock
(missing tooling, contradictory requirements, a decision that changes the product).

## Per-milestone ritual (this is the spine of the campaign)

For each milestone in §30:

1. Implement the milestone's required contents, on a short-lived branch
   (`feat/...`, `fix/...`) when the work is non-trivial; keep `main` buildable (§28).
2. Write/refresh the documentation the milestone requires.
3. Update `CHANGELOG.md` with a real section for the version (§32) — prose, not a
   commit-hash dump.
4. Run the §47 automated gates and make them genuinely pass:
   ```
   cargo fmt --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo build --workspace --release
   ```
5. From `v0.7.0` onward also: build the `.deb` and `.rpm`, inspect them
   (`dpkg-deb -I/-c`, `rpm -qpi/-qpl`), and run a headless install smoke test —
   `lucerna --version`, `lucerna --help`, `lucernad --version`, `lucernactl --version`
   must work with no display (§34, §47).
6. Merge to `main`, then create an **annotated** tag and push it (§31):
   `git tag -a vX.Y.Z -m "Lucerna vX.Y.Z" && git push origin main vX.Y.Z`
7. Confirm CI for that tag is green before moving on (§33/§36/§40).
8. Print a short milestone summary in the pane so the supervisor can verify it.

## Non-negotiable honesty rules

- **§2 dominates everything.** You are on a headless server. Never claim the
  wallpaper visually appears, that icons remain clickable, that stacking is
  correct, that layout/fonts/scaling look right, or that any UI element is
  aesthetically satisfactory. Xvfb is for **protocol-level tests only** and is
  never evidence of visual correctness.
- The only valid status for desktop-appearance behaviour is:
  `IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED`
- You must **write** `docs/MANUAL-ACCEPTANCE.md` containing the full LUC-T01…LUC-T20
  campaign from §45. You must **not** execute it and must **not** fill in results.
  Leave every result cell as `NOT RUN — REQUIRES REAL DESKTOP` (§46).
- Report real failures and real gaps. A gate that does not pass is reported as
  failing, with the error, not quietly marked done.
- Never fabricate output, test results, package contents, or CI status.

## Version discipline

- The root Cargo workspace metadata is the **single canonical version source** (§29).
  Every binary, the GTK About page, the RPM spec, and the Debian changelog/control
  must derive from it — never a second hardcoded copy.
- CI must **fail** a release when the tag version does not equal the workspace
  version (§31). Do not paper over a mismatch by rewriting package metadata.

## CI / packaging

- `.github/workflows/ci.yml` (§33): fmt, clippy with `-D warnings`, test, release
  build; triggers on `push` and `pull_request`.
- `.github/workflows/packages.yml` (§36): builds a real `.deb` in a
  Debian/Ubuntu-family environment and a real `.rpm` in a Fedora/RPM-family
  environment; **no cross-format repackaging**; both with smoke tests.
- `.github/workflows/release.yml` (§40): triggered by `v*` tags; performs all twelve
  numbered steps — including verifying the tag is **annotated**, verifying the
  tag/version match, running the full suite, building every artifact, SHA-256
  checksums, and attaching everything to the GitHub Release. It must fail rather
  than publish a partial release.
- A temporary probe workflow `.github/workflows/probe.yml` exists from repository
  bootstrap. It has served its purpose (it proved runners work for this public
  repo). **Delete it** when you create the real `ci.yml` in `v0.0.1`.

## Environment facts

- `rustc`/`cargo` **1.98.1** (rustup stable).
- GTK 4 **4.14.5**, `mpv` **0.37.0** (+ `libmpv-dev`), X11 1.8.7 / RandR 1.5.2 /
  Xfixes 6.0.0, `Xvfb` + `xvfb-run` present.
- Local packaging tools: `dpkg-buildpackage`, `debhelper`, `dh-make`, `lintian`,
  `fakeroot`, `rpmbuild`, Docker 29.8.1.
- **`DISPLAY` is unset.** There is no graphical session, ever. Wayland is likewise
  absent. All §34/§47 headless smoke tests must pass in exactly this condition.
- GitHub Actions runs are **free and working** for this public repository.

## Forbidden

- Any v1 non-goal from §27 (Workshop, HTML/WebGL/shader wallpapers, Chromium,
  Wayland/GNOME/Plasma/Hyprland/Sway backends, accounts, cloud sync, telemetry,
  auto-download, Windows/macOS).
- `unwrap()` / `expect()` / `panic!()` in user-triggerable production paths (§48).
- Shell invocation of wallpaper paths; any `sh -c "mpv $FILE"` pattern (§9, §26).
- Root/setuid components; world-writable control sockets (§26).
- Modifying anything outside `/home/clawuser/projects/lucerna`.
- Any destructive git operation: no force-push to `main`, never rewrite a published tag (§28).
- There is **no burn/improvement-wave mandate**. Finish the spec's milestones,
  verify them, and stop.

## Final action

When `v1.0.0-rc.1` exists with its artifacts (`.deb`, `.rpm`, source archive,
`SHA256SUMS`) produced through the release pipeline and all server-executable gates
are green, produce the §55 final agent report in the pane — including the explicit
statement that manual desktop validation was **NOT PERFORMED BY THE DEVELOPMENT AGENT
AND REQUIRES A REAL CINNAMON/X11 DESKTOP** — then park and let the supervisor verify
and tear down.
