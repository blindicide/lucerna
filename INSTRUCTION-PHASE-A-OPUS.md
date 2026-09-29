# PHASE A — IMPLEMENTATION PLANNING (Opus 5.5)

You are the planning agent for the **Lucerna** project. You are running in
`claude-opus-5-5` at `--effort high`, in the tmux window `lucerna:harness`, on a
headless Linux server. A separate supervisor process watches your pane.

## Your single deliverable

**Write `docs/IMPLEMENTATION-PLAN.md`.** It is the artifact the coding phase will
be driven from. Nothing else is required from you in this phase — you are not
writing product code, and you must not start implementing.

## First action

Read the full development directive: `INSTRUCTION-LUCERNA.md` (project root,
57 sections). It is the authoritative mandate. Everything below is a summary of
your obligation to it, not a replacement for it.

Read it in full before planning. Sections that most constrain the plan:
§2 (headless constraint), §5-§8 (component + backend boundaries), §9 (renderer
state machine), §18-§22 (config/IPC/CLI/diagnostics/autostart), §28-§32 (git,
SemVer, tag sequence, changelog), §35 (test classes), §47-§48 (gates, code
quality), §57 (definition of done).

## Required content of the plan

The plan must be concrete enough that a different agent could execute it without
re-deriving your reasoning. Cover at least:

1. **Workspace and crate boundaries.** The exact `crates/` layout, what belongs in
   each crate, and the dependency direction between them (which crate may depend on
   which — no cycles). Explain how the §48 rule "core logic testable without GTK,
   renderer policy testable without mpv, X11 separable from daemon policy" is
   enforced *by the module graph*, not by good intentions.
2. **The backend trait.** The real Rust signature-level shape of the
   `WallpaperBackend` abstraction from §7, and which types cross the boundary.
3. **Renderer state machine.** The explicit states from §9, legal transitions,
   what normalizes an illegal transition, and how the bounded restart policy of §51
   is represented and tested.
4. **mpv invocation.** Exact argument vector and the rationale for each option, the
   IPC socket path strategy under `$XDG_RUNTIME_DIR`, how stdout/stderr are captured
   without unbounded growth (§9), and how "mpv is missing" is detected (§49).
5. **X11/Cinnamon integration plan.** The concrete primitives from §8, what must be
   probed at runtime versus assumed, how Nemo/desktop stacking is investigated, and
   how each primitive is *protocol*-tested under Xvfb (§35) without any claim of
   visual validation (§2).
6. **Configuration.** Full schema, atomic-write strategy, schema-version and
   forward-compatibility behaviour (§18, §53).
7. **IPC.** The D-Bus interface in detail — bus name, object path, methods,
   signatures, signals, error mapping (§19).
8. **Test strategy mapped to §35**, naming the four classes and what each covers,
   including the fake-mpv-subprocess technique.
9. **Milestone execution order** mapped to the tag sequence in §30 — for each
   milestone, its concrete deliverables, the gates that must be green before its tag
   (§47), and its exit criteria.
10. **Packaging + CI plan** for §33/§36/§37/§38/§40 — the Debian and RPM layouts,
    how the version is obtained from the single canonical source (§29), the
    annotated-tag/version-match enforcement (§31), and the workflow matrix.
11. **Risk register.** The genuinely uncertain parts (Cinnamon/Nemo stacking;
    per-monitor fullscreen detection; multi-monitor geometry; what can never be
    validated headlessly) with a mitigation or an explicit documented-limitation
    plan for each.

## Environment facts you may rely on

- `rustc`/`cargo` **1.98.1** (rustup stable, on PATH at `~/.cargo/bin` and symlinked
  into `/usr/local/bin`).
- GTK 4 dev headers **4.14.5**; `mpv` **0.37.0** with `libmpv-dev`; X11 1.8.7,
  RandR 1.5.2, Xfixes 6.0.0; `Xvfb`/`xvfb-run` available; `x11vnc` available.
- Packaging tools local: `dpkg-buildpackage`, `dpkg-deb`, `debhelper`, `dh-make`,
  `lintian`, `fakeroot`, `rpmbuild` 4.18.2. Docker 29.8.1 is available.
- **No display server.** `DISPLAY` is unset. This is the permanent condition of the
  development environment, not a temporary fault.
- GitHub Actions **works** for this repository (it is public; the runner probe ran
  green). Packaging and release workflows are executable in CI.

## Hard constraints

- **Never claim visual or desktop validation.** §2 is absolute. The correct status
  for desktop-appearance behaviour is `IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED`.
- **Do not implement v1 non-goals** (§27). Do not let scope creep into the plan.
- **No burn mandate.** Finish the plan and stop. Do not invent extra work.
- Do not modify anything outside `/home/clawuser/projects/lucerna`.

## When done

1. Ensure `docs/IMPLEMENTATION-PLAN.md` is written and complete.
2. Commit it with a conventional message, e.g.
   `docs: add implementation plan for the v1 campaign`.
3. Print a short summary: plan path, line count, the crate layout you chose, and
   any decision you want flagged to the operator.
4. **Stop.** Park at the prompt — do not begin implementation. The supervisor will
   switch the rig to the coding phase.
