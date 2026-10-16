---
name: working-on-suhina
description: How to work in the Suhina codebase productively - the working loop (claim, branch, command, test, gate, PR), the crate and layer map, the UI and egui/kittest patterns with their gotchas (labels, menus, modifiers, flaky repaints), the test fixtures, and every verification command. Use when about to write or review code in this repository, when a build or test misbehaves, when adding a command, device or view, or when asked how to do anything in Suhina.
---

Read [`AGENTS.md`](../../../AGENTS.md) first — its golden rules decide designs. This skill is the
practical distillation: what those rules mean while working, and what this codebase's code and
tests look like.

## The working loop

1. **Read the docs for the task**: [`docs/INDEX.md`](../../../docs/INDEX.md) maps every document
   to the words it is read for and the code it covers. `docs/architecture.md` has the layers and
   the threads.
2. **Claim the issue**: comment `/claim` with the plan and a `Touches:` line naming the paths
   (`Touches: crates/ui/src/detail, crates/session`). The bot answers with overlapping work;
   never start a second PR for the same thing.
3. **Branch** `<kind>/<short-description>` from latest `main` — `feature/`, `fix/`, `docs/`,
   `chore/`. From a fork, PRs use `--head <fork-owner>:<branch>` on `suhina-daw/suhina-daw` (the
   fork's copy of the branch must be pushed); a PR
   that builds on an unmerged one branches from that PR's head and says `Depends on #N`.
4. **Code**: user-visible behaviour is a **command** in the engine/session (id, params, tests);
   views turn gestures into `Action::new("command", json!({...}))` and never change the project
   themselves. UI-only selections are view state, not commands.
5. **Test**: every change comes with tests (see the patterns below).
6. **The gate** (all must pass):

   ```sh
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   python scripts/docs.py check
   python scripts/rename_suhina.py --check
   python scripts/parity/update_report.py --check-inputs
   ```

7. **PR**: the repository's template (What and why · Where the knowledge comes from · How to
   check it · How I tested it · Checklist). *How to check it* follows
   [`.claude/skills/how-to-check/SKILL.md`](../how-to-check/SKILL.md). The parity line: what
   moved in `scripts/parity/score.py`, or `Parity: no change` and why. The docs a change touches
   are updated in the same PR (`python scripts/docs.py impact` says what covers your code).

## The map

- **L0** `model` (the project as pure data) · `dsp` (effect/instrument building blocks) ·
  `device-api` (the Device trait) · `ipc` (audio blocks over shared memory) · `stretch`
  (time-stretching) · `device-sdk` (.sd effect graphs)
- **L1** `engine` (offline/real-time rendering, the launcher; no file I/O) · `device-registry` ·
  `als` (Live Set import) · `project-file` (`.suhina`: versioned JSON; `.locr` still opens) ·
  `plugins` (the engine's side of the host process)
- **L2** `session` (open projects, undo, the command registry, the library, the rack) · `media` ·
  `library` · `soundmap` · `separation`
- **L3** `ui` (egui; every change is a command) · `output` (the audio device) · `automation`
  (MCP) · `midi`
- **Devices** live in `devices/<name>` (`suhina-device-<name>`); apps in `apps/` (`suhina`,
  `suhina-cli`).
- A crate depends only on its own or a lower layer. Nothing below `ui` knows egui.

## UI code, the house style

- The views live in `crates/ui/src/<view>/` (`app/`, `arrangement/`, `browser/`, `detail/`,
  `interaction/` — menus, drop, keymap — `kit/` — theme, widgets, motion — `session/`), one
  module file beside each folder (`app.rs`, `browser.rs`, …).
- **Labels are the test and accessibility API.** Every interactive thing sets
  `widget_info(WidgetInfo::labeled(...))` with a label the tests `get_by_label`. Name them as a
  user would say them ("Launch Walk on Bass (scene 2)", "Device Utility"). Keep labels stable;
  tests click them.
- **Theme colours are tokens** (`palette!` in `kit/theme.rs`): never a raw `Color32` for a
  semantic colour — add a token (`selection`, "The selected device's outline") and give **every
  shipped theme** in `crates/ui/themes/` a value for it (the theme tests enforce that).
- `motion::mix/toward/pulse` for hovers and fills; `widgets::button_area` for clickable rects;
  the status bar takes hints through `Status::Hint` (the browser's `out.status`, the app's
  `set_status`).
- **Menus with fields** (a rename field, any text edit): open them with
  `egui::Popup::context_menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)`
  — egui's default closes on a click inside, which kills fields and their buttons.
- **TextEdits need explicit `.id(...)`** — without one the field's label never reaches the
  accessibility tree and tests cannot find it. Prefill drafts from egui temp data
  (`d.get_temp(id)` / `d.insert_temp`), clear after applying.

## egui/kittest gotchas

- `clicked()` fires on every click of a double-click, and `double_clicked()` on the second:
  guard the single-click path when both are handled.
- kittest's context menus open fine now, but their **TextEdits take no typed keys** — test the
  field's presence and the command's behaviour; leave the typing to a hand check.
- kittest's `drop_at` carries `Modifiers::NONE`, and its `key_down_modifiers` resets right
  after. To hold a modifier over a drop: push
  `egui::Event::ModifiersChanged(mods)` onto `harness.input_mut().events`, step, drop, run, then
  push `Modifiers::default()` and run.
- `harness.run()` runs to rest and fails its max steps when something repaints every frame (the
  sample map's layout, a playing monitor). Use `harness.run_steps(n)` / `step()` there.
- Drag tests: `hover_at` → `step` → `drag_at` → `step` → `hover_at(target)` → `step` →
  `drop_at(target)` → `run`. A double-click needs `double_click_at` (frames within egui's
  0.3 s window).
- Tests that need an audio device are `#[ignore]`d and run by hand. A software
  `suhina_engine::Renderer` attached through `session.attach_player(Player::new(renderer.controller(), None))`
  needs no device and hears everything.

## Test patterns

- `crates/ui/tests/window.rs`: the app's window. `demo_app()` (the demo project, browser
  hidden), `window_with_step(app, 1/60)` (double-click frames), `project_with_a_device()`.
- `crates/ui/tests/browser.rs`: the browser. `pack(&folder)` builds a library (drums, a loop, a
  MIDI file, a project), `wav()`, `midi()`, `row(name)` labels, a fresh library seeds the seven
  colour tags.
- `crates/ui/tests/browser_preview.rs`, `piano_roll_tools.rs`, `settings` tests: the same
  harness, one concern each.
- `crates/session/tests/launcher.rs`: commands against an offline renderer (`playing()` +
  `advance(&mut renderer, frames)`); the fixture `project()` has clips in slots, a deactivated
  clip and an empty no-stop slot; contents are shared between fixture clips.
- Engine tests (`crates/engine/src/chain.rs`, `launcher.rs`): `Renderer::stopped(&project, …)` +
  `renderer.handle(LaunchCommand::…)` + `renderer.process(&mut block)`; the monitor reads back
  playing/queued/elapsed per column.

## Formats an agent meets

- `.suhina` (and `.locr`): versioned JSON, the whole project; binary (plug-in state) base64;
  audio by relative path (`rebase_audio` on save-elsewhere). Spec: `docs/spec/suhina-format.md`.
- `.als`: Live Set import (`als` crate) — clean-room, spec first in `docs/spec/`.
- `.sd`: data-only device graphs (`device-sdk`).
- Missing samples on import are reported and relinkable, never silently dropped.
