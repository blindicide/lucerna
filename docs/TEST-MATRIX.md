# Test matrix

Traceability from the directive to the tests that exercise it. **Automated tests verify logic,
process supervision, D-Bus and X11 *protocol* behaviour. They never verify appearance.** Anything
that needs a real desktop is `IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED` and is covered by a
`LUC-Txx` test in `docs/MANUAL-ACCEPTANCE.md`, all of which are `NOT RUN — REQUIRES REAL DESKTOP`.

Test locations: unit tests live next to the code (`crates/*/src`); integration suites are in
`crates/lucerna-testkit/tests/`.

## Automated

| Requirement (directive) | Test(s) |
| --- | --- |
| Mission: version from one source (§29) | `lucerna-core version::tests`; `architecture::every_crate_inherits_workspace_metadata`; `tests/scripts/version_test.sh`; `headless` suites assert `--version` equals the workspace version |
| Crate boundaries / core testable without GTK (§48) | `architecture` suite |
| `--help`/`--version` work with no display (§24, §34) | `crates/{lucerna-ui,lucerna-daemon,lucerna-cli}/tests/headless.rs`; CI `build-release` smoke |
| `lucerna` without a display prints a message, no panic (§24) | `lucerna-ui headless::launch_without_display_explains_and_exits_1` |
| Renderer states, invalid transitions normalised or rejected (§9) | `renderer::machine::tests::transition_table` (every state × event cell), plus targeted tests |
| Bounded restart, no storm (§51) | `renderer::restart::tests`; `renderer_supervision::restart_storm_is_bounded` (exactly 4 spawns) |
| mpv launched without a shell; hostile file names (§9, §26) | `mpv::args::tests`; `renderer_supervision::a_file_named_like_a_shell_command_is_just_a_filename`; `library::tests::a_shell_metacharacter_file_name_is_just_a_name` |
| mpv argument vector, audio off by default, `--no-config` (§9, §16) | `mpv::args::tests::default_vector_is_exactly_the_documented_one`, `…audio_is_muted_by_default…`, `…every_combination_only_emits_known_options` |
| mpv option compatibility (§9) | `real_mpv::options_supported`; `mpv_discovery::*`; the fake mpv rejects undeclared options |
| mpv detection, missing mpv message (§9, §49) | `mpv::discovery::tests`; `renderer_supervision::missing_mpv_…`; `daemon_integration::a_missing_mpv_is_reported_with_the_actionable_message` |
| Crash / hang / IPC failure / unsupported file detection (§9) | `renderer_supervision::{renderer_recovers_after_two_crashes, unsupported_media_fails_once_…, hung_and_socketless_renderers_…, losing_the_control_socket_…}` |
| Stop escalation quit → TERM → KILL | `renderer_supervision::stop_escalates_from_quit_to_term_to_kill` |
| mpv output captured without unbounded growth (§9) | `bounded_log::tests`; `renderer_supervision::stderr_flood_stays_within_the_log_bound_…` |
| Stale socket cleanup (§26, §35) | `runtime::tests`; `renderer_supervision::stale_socket_files_are_replaced_at_start`; `daemon_integration::stale_sockets_and_registry_…` |
| Stale-process recovery spares unrelated mpv (§52) | `renderer_supervision::stale_process_recovery_kills_ours_and_spares_unrelated_renderers`; `registry::tests` |
| Real playback of mp4, webm, mkv, gif (§10) | `real_mpv::plays_{mp4,webm,mkv,gif}`; corrupt file: `corrupt_file_is_media_unsupported_and_is_not_retried` |
| Private runtime directory, no `/tmp` fallback (§26) | `runtime::tests` |
| Atomic config writes, no zero-byte file (§18) | `fsutil::tests` (fault injection after temp write and after rename, symlink target) |
| Configuration parsing, lenient, schema version (§18, §53) | `config::tests_file` (parse, unknown keys, bad values, missing/newer/older schema, corruption, unreadable) |
| Schema migration with backup (§53) | `config::migrate::tests`; `config::tests_file::an_older_schema_is_backed_up_migrated_…` |
| Unknown keys and comments preserved (§53) | `config::tests_file::a_save_preserves_unknown_keys_…`; `daemon_integration::unknown_config_keys_survive_a_daemon_save` |
| Library: references only, never deletes media (§10, §11) | `library::tests`; `daemon_integration::adding_and_removing_wallpapers_never_touches_the_media` |
| Missing file handled without a crash (§11) | `daemon_integration::a_missing_wallpaper_file_never_spawns_and_never_shows_a_surface` |
| Monitor identity from EDID, stable, collisions (§12) | `identity::tests` (synthetic, spec-conformant EDID blocks — no physical monitors are available on the server) |
| RandR enumeration incl. virtual monitors (§12) | `x11_protocol::{enumerate_single_screen, enumerate_virtual_monitors}` |
| Backend abstraction, daemon without an X server (§7, §48) | `fake_backend` suite; the whole `daemon_integration` suite runs on `FakeBackend` |
| Surface properties, click-through routing, restack, cleanup (§8, §14) — **protocol only** | `x11_protocol::{surface_properties, input_passthrough_protocol, lower_and_restack, destroy_and_shutdown, …}` |
| Nemo/Cinnamon probes (§8) — **protocol only** | `x11_protocol::nemo_simulation_detected`; `facts::tests` |
| Fullscreen detection, maximised ignored (§15) — **protocol only** | `x11_protocol::fullscreen_detection`; `geometry::tests`; `policy::tests` |
| Hotplug events debounced (§12) | `x11_protocol::hotplug_event` |
| Connection loss and reconnect (§35) | `x11_protocol::connection_lost_and_reconnect`; `process_lifecycle::the_display_going_away_shuts_the_daemon_down_cleanly` |
| mpv embeds in a surface (§9) — **protocol only** | `x11_protocol::mpv_embeds_into_surface` |
| Pause policy (§15) | `policy::tests` (all combinations) |
| Per-display assignment and scaling (§13) | `multi_monitor::each_display_can_have_its_own_wallpaper_and_scaling`; `plan::tests`; `daemon_integration::bad_requests_…` |
| Absent display preserved and restored (§12) | `multi_monitor::an_absent_display_keeps_its_assignment_and_gets_it_back` |
| Identity independent of enumeration order (§12) | `multi_monitor::display_identities_survive_a_different_enumeration_order`; `identity::tests` |
| Resize on geometry change | `multi_monitor::a_geometry_change_resizes_the_surface_without_restarting` |
| Fullscreen pauses only the covered monitor (§15) | `multi_monitor::{fullscreen_pauses_only_the_occluded_monitor, the_fullscreen_setting_can_be_turned_off_…}`; `geometry::tests` |
| Lock pause via three sources (§15) | `multi_monitor::{the_cinnamon_screensaver_…, the_freedesktop_screensaver_…, logind_locked_hint_…, without_any_lock_service_…, a_session_that_starts_locked_…, pause_reasons_combine_…}` |
| Renderer crash recovery, bounded restarts, user-visible failure (§51) | `lifecycle::{a_renderer_that_crashes_twice_…, a_crash_storm_stops_at_the_limit_…, an_unplayable_file_fails_once_…}`; `renderer_supervision::restart_storm_is_bounded` |
| Missing media resumes by itself; entry kept (§11) | `lifecycle::{a_missing_wallpaper_comes_back_by_itself_…, a_file_that_vanishes_is_noticed_…}` |
| mpv missing then installed (§49) | `lifecycle::mpv_installed_after_the_daemon_started_is_picked_up_by_reload`; `cli_e2e::a_missing_mpv_is_explained_by_status` |
| Corrupt configuration surfaced in CLI/doctor (§30 v0.6.0) | `cli_e2e::a_corrupt_configuration_is_explained_by_status_and_doctor` |
| Autostart not re-created when disabled (§22) | `lifecycle::{autostart_is_created_on_first_run_only_…, an_autostart_entry_disabled_from_the_desktop_settings_…}` |
| Login race: wait for the window manager (§22) | `process_lifecycle::{the_daemon_waits_for_the_window_manager_…, without_a_window_manager_the_wait_is_bounded}` |
| Logout (SIGHUP), kill -9, orphan recovery (§52) | `process_lifecycle::{sighup_ends_the_session_cleanly, when_the_daemon_is_killed_…, a_renderer_that_survives_a_killed_daemon_…, an_unrelated_mpv_is_never_touched_…}` |
| Bounded, concise daemon log (§25) | `process_lifecycle::the_daemon_keeps_a_concise_bounded_log_…`; `bounded_log::tests` |
| No restack fight (§14) | `multi_monitor::a_restack_fight_with_the_window_manager_is_rate_limited` |
| Reconciliation (hotplug, reassignment, missing file) | `plan::tests` |
| Session classification, Wayland detection (§50) | `session::tests`; `daemon_integration::a_wayland_session_is_reported_clearly_and_starts_nothing`; `process_lifecycle::a_wayland_session_is_explained_not_crashed_on` |
| D-Bus API, DTO round trips, errors and exit codes (§19, §20) | `lucerna-ipc` unit tests; `daemon_integration`; `cli_e2e` |
| Single instance (§5) | `daemon_integration::{a_second_daemon_on_the_same_bus_exits_cleanly, a_second_daemon_with_the_same_runtime_directory_is_stopped_by_the_lock}`; `process_lifecycle::duplicate_daemon_exits_cleanly_and_the_first_is_unaffected`; `lock::tests` |
| CLI commands, JSON, exit codes (§20) | `lucerna-cli args::tests`; `cli_e2e` |
| `doctor` and redaction (§21) | `doctor::tests`; `lucerna-cli doctor::tests`; `cli_e2e::doctor_*` |
| Autostart really switches (§22) | `autostart::tests`; `daemon_integration::autostart_is_a_real_switch` |
| Settings validated as a whole (§17, §23) | `daemon_integration::{settings_round_trip_validate_and_persist, invalid_settings_are_rejected_as_a_whole}` |
| Clean shutdown on SIGTERM / Quit / logout (§52) | `process_lifecycle::{sigterm_cleans_up_everything, quit_cleans_up_everything, the_display_going_away_…}` |
| Config reload (§35) | `daemon_integration::reload_applies_an_external_edit` |
| Corrupt / newer config handling (§30 v0.6.0) | `daemon_integration::{a_corrupt_config_is_moved_aside_…, a_newer_config_schema_is_read_only_but_still_renders}` |
| GUI structure: pages, controls, wiring to the daemon (§23) — **structure only, not appearance** | `crates/lucerna-ui/tests/ui_structure.rs` (real GTK window on Xvfb + real daemon on a private bus) |
| GUI view models and strings (§23, §48) | `lucerna-ui presenter::{wallpapers,displays,settings,banner,about}::tests` |
| GUI never owns renderers or X11 windows (§5, §48) | `architecture::dependency_edges_follow_the_allow_table` (ui may not depend on mpv/x11) |
| Requests during shutdown cannot hang it | `daemon_integration::calls_that_arrive_during_shutdown_…` |
| Packages install, dependencies resolve, binaries/desktop file/icon exist, `--version`/`--help` headless, removal keeps config (§35) | `scripts/smoke-test-package.sh` in fresh `ubuntu:24.04` / `fedora:44` containers (`packages.yml`: `deb-smoke`, `rpm-smoke`) |
| Package helper scripts | `tests/scripts/packaging_test.sh`, `tests/scripts/version_test.sh` |
| Release pipeline (§40) | added in v0.8.0 |

## Desktop-only (not automatable on the server)

Each of these is `IMPLEMENTED — MANUAL DESKTOP VALIDATION REQUIRED`, result `NOT RUN — REQUIRES REAL DESKTOP`.

| Behaviour | Manual test |
| --- | --- |
| Installation and menu entry | LUC-T01 |
| GUI appears, responsive, no terminal; layout, fonts and theming look right (**not validated on the server**) | LUC-T02 |
| Add wallpaper in the GUI | LUC-T03 |
| Animation appears as the desktop background | LUC-T04 |
| Icons visible, clickable, draggable | LUC-T05 |
| Desktop context menu | LUC-T06 |
| Stacking behind windows, panels not covered | LUC-T07 |
| Not in Alt+Tab or taskbar | LUC-T08 |
| Fullscreen pause and resume | LUC-T09 |
| Silent by default | LUC-T10 |
| Fit / Fill / Stretch / Center look as documented | LUC-T11 |
| Two monitors, separate assignments | LUC-T12 |
| Monitor disconnect and reconnect | LUC-T13 |
| Restore after log out/in | LUC-T14 |
| Wallpaper survives closing the GUI | LUC-T15 |
| CLI state matches what is observable | LUC-T16 |
| Missing file reported, daemon survives | LUC-T17 |
| mpv failure visible and recoverable | LUC-T18 |
| Autostart disabled really disables | LUC-T19 |
| Idle CPU / GPU / RAM sanity | LUC-T20 |
