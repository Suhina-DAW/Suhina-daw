"""Parity scoring: how much of a reference manual's documented surface Suhina has today.

    python scripts/parity/score.py [--dir target/parity]

Page counts come from scripts/parity/headings.py; the coverage estimates and the prose notes are
ours (see docs/research/parity-live-12.md, Sources and IP). Prints the roll-ups; the tables of the
report are rendered from the data here by update_report.py, and the figures by charts.py.
"""

import argparse
import json
import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# The measurement's provenance: bumped by hand when scripts/parity/run.py is run against a manual
# and the result is checked against the code. The report's front matter, its headline block and
# both figures carry it, so a stale measurement is visible rather than implied.
MEASURED_AT = "2026-10-13"
MEASURED_COMMIT = "eeb8899"

# Section counts from the last full run (headings.py over the manual). Used when target/parity/ is
# not there, so the report and the figures still render for someone without a copy of the manual -
# including the workflow that regenerates them on main.
SECTIONS = {"total": 344, "zero": 111, "coverage": 0.168}

# The manual the measurement was read from, and the command count the report quotes. Both are kept
# by hand: the manual is not in the repository, and docs/commands.md is regenerated on main, so a
# pull request cannot read the count of a command it just added from it.
MANUAL_CHAPTERS = 43  # chapters 1 to 43, the same chapters CHAPTERS lists
MANUAL_PDF_PAGES = 1039
COMMANDS = 140

# num, chapter title, pages of the manual body, coverage 0..1 (None = not a feature chapter),
# status, note. "coverage" is the share of what the chapter documents that Suhina has today.
CHAPTERS = [
    (1, "Welcome to Live", 1, None, "n/a", "Front matter."),
    (2, "First Steps (installation, Learn/Info view, Settings)", 11, 0.52, "partial",
     "Release and nightly packages for Windows, macOS and Linux; Help > About names the build "
     "(version, nightly date and commit, platform). Settings pages: General, Interface, Graphics, Audio (driver/device/rate/buffer/exclusive; "
     "a change or a device back after unplugging keeps playback and launched clips going), "
     "Shortcuts, Plug-ins. Missing Link, Tempo & MIDI, File & Folder, Library, Record/Warp/Launch, "
     "Learn view, Info view."),
    (3, "Live Concepts", 27, 0.50, "partial",
     "Control bar, browser, sets, arrangement/session, tracks, audio/MIDI clips, devices, mixer "
     "concepts exist. Routing, recording, racks, clip/device view and presets are missing or thin."),
    (4, "Working with the Browser", 60, 0.49, "partial",
     "Search, categories, folders watched on disk, pins, recent, colour tags, preview through the "
     "engine, sample map by sound similarity with kinds recognised from the sound and set by hand; samples and MIDI files dragged from the system's file "
     "manager land like a browser drag (exact placement waits for pointer positions from the "
     "windowing layer); installed plug-ins grouped by vendor, by type (instrument and effect groups "
     "from the formats' own categories) or in a flat list sortable by name, vendor, type, format "
     "and date, with pins and tags on them. Missing collections/tag editor depth, packs, Splice, "
     "Cloud, Push transfer, filter groups, browser history."),
    (5, "Managing Files and Sets", 43, 0.35, "partial",
     ".suhina save/open, .als import, templates, several projects in tabs, missing-file locate and "
     "relink. Missing project folders/collect, unused files, packing, .asd, decoding cache, set "
     "merging, file-reference view, export audio/video."),
    (6, "Arrangement View", 22, 0.47, "partial",
     "Layout, zoom, transport, ruler/play start, play from the selected clip, clip drag+move, "
     "overlap replace/stack, fades and fade shapes, grid snap, nested groups folded and unfolded, "
     "tracks dragged into and out of groups, markers (locators: add, move, rename, delete, jump; imported from Live "
     "Sets), the arrangement loop brace (draw, resize, move, on/off with Ctrl+L; playback wraps "
     "sample-accurately; imported from Live Sets). A clip's loop range is read and drawn but no "
     "command sets it. Missing previous/next marker, punch in/out, time-signature changes, split, "
     "consolidate, crop, crossfades, linked-track editing, the time commands."),
    (7, "Session View", 13, 0.60, "partial",
     "Slots, scenes, scene launch, stop buttons, track status, mixer strip, quantised launching; "
     "scenes inserted, duplicated, deleted and renamed from a menu, with a tempo and time signature "
     "each. A launched clip is selected and the detail area shows its follow-action controls. "
     "Missing capture-and-insert scene, select-on-launch options."),
    (8, "Clip View", 34, 0.15, "started",
     "Detail area shows clip properties and, for a MIDI clip in the Arrangement, a first piano roll "
     "(docs/spec/ui-clip-editor.md): keys, ruler, note grid, velocity lane, zoom and scroll, a toolbar of "
     "tool icons with the note clipboard, and the quantise controls in the clip column. No loop "
     "or marker editing, no clip panels, no sample editor, no groove/scale panels."),
    (9, "Audio Clips, Tempo, and Warping", 18, 0.35, "partial",
     "Real-time time-stretching along warp markers read from Live Sets, clip gain, transpose, "
     "six warp modes in the model, tempo detect for the browser. One stretch algorithm serves "
     "every mode except Re-pitch, so the mode names are not yet different algorithms, and rates "
     "outside 0.25x-4x fall back to Re-pitch. Missing tap tempo, tempo follower/leader, auto-warp "
     "of long samples, warp-marker editing, quantise audio, tempo automation."),
    (10, "Editing MIDI", 41, 0.33, "started",
     "A first piano roll draws, selects (click, rectangle, Ctrl+A), moves, resizes and deletes notes "
     "on the grid (Alt off-grid), by mouse or by arrow keys, with Select, Draw, Paint, Erase and Slice "
     "tools (keys 1 to 5, the wheel with the right button held steps through them) and "
     "its own grid value, pulled up to the full height, copies, cuts, pastes and duplicates them, "
     "splits them at a drawn cut line (note.split splits every note the line crosses), plays keys "
     "through the track's instrument, "
     "quantises them (Ctrl+U, and grid, amount and ends in the clip column) and edits velocity in a lane, "
     "through note.* commands with undo; a duplicated clip's notes are its own once edited. No "
     "transpose tools, fold, scales, chords, deactivated notes, probability or MPE lanes."),
    (11, "MIDI Tools", 37, 0.00, "absent", "No transformation or generative MIDI tools."),
    (12, "Editing MPE", 10, 0.00, "absent", "No MPE data, editing or MPE-aware devices."),
    (13, "Converting Audio to MIDI", 5, 0.05, "planned",
     "Spec written (docs/spec/audio-to-midi.md); slicing and the four detectors are roadmap."),
    (14, "Using Grooves", 6, 0.00, "absent", "No groove pool, extraction or commit."),
    (15, "Using Tuning Systems", 9, 0.00, "absent", "No .scl/.ascl tuning systems."),
    (16, "Launching Clips", 12, 0.52, "partial",
     "Launch quantisation (none, 8 bars..1/32, triplets), scenes (with their tempo and time "
     "signature), stop, loop, back to arrangement, warped launched clips, follow actions (A/B by "
     "chance, linked or timed, jump to a slot; by command and in the detail area), samples looping "
     "in slots with or without a known tempo, a slot clip's loop switched on or off (clip.setLoop). "
     "Missing launch modes, legato, velocity amount, clip offset/nudge, follow-action multiplier "
     "and scene follow actions."),
    (17, "Routing and I/O", 22, 0.10, "absent",
     "Only track output to main or a group (track.setOutput); track input is a model field read "
     "from .als. No monitoring, external audio/MIDI in-out, resampling, sidechain, multi-timbral."),
    (18, "Mixing", 18, 0.52, "partial",
     "Faders, pan, mute, solo (exclusive/Ctrl), groups as buses nested up to 16 deep (solo and mute "
     "through the levels), master, latency compensation, "
     "sends pre or post fader into return tracks that run their device chains (track.setSend, "
     "track.setSendMode, return.add/remove/rename, send knobs in the Session mixer), and level "
     "meters (peak, RMS, clip) per track, group, return and main, also over MCP; return tracks "
     "imported and drawn. Missing crossfader, cue, track delay, send fields in the Arrangement "
     "headers."),
    (19, "Recording New Clips", 13, 0.00, "absent",
     "No audio or MIDI recording, no arm, no overdub, no count-in, no capture MIDI."),
    (20, "Bounce to Audio", 6, 0.15, "partial",
     "Whole-project offline render to 32-bit float WAV (CLI/MCP), from an open project with its "
     "running plug-ins' state as heard; no per-track or group bounce, no paste."),
    (21, "Comping", 6, 0.00, "absent", "No take lanes, no comping."),
    (22, "Stem Separation", 8, 0.00, "planned",
     "Roadmap item (Demucs candidate, weights unaudited). The pipeline around a model (chunking, "
     "resampling) exists in crates/separation with a test backend; no model and nothing a user "
     "can run yet."),
    (23, "Working with Instruments and Effects (Device View)", 34, 0.30, "partial",
     "Device chains per track and bus, parameters through device.* commands and the detail area, "
     "DX7 voices, stock-preset ADR, latency compensation, plug-in state in the panel. Missing A/B, "
     "hot-swap, preset browser depth, defaults, device delay review."),
    (24, "Using Plug-Ins", 12, 0.64, "partial",
     "CLAP and VST3 hosted out of process, sandboxed, VST3 bundles on Windows, macOS and Linux, "
     "editors (Windows/X11), scans, latency "
     "compensation (followed when a plug-in changes its latency while it plays), saved state "
     "(a change in an editor counts as unsaved), VST2 read and matched to VST3, parameters set "
     "and automated from the engine. Missing AU, VST2 hosting, "
     "configure mode, VST preset banks, sidechain parameters."),
    (25, "Instrument, Drum and Effect Racks", 20, 0.03, "absent",
     "A track's device chain is a single serial chain. No parallel chains, macros, key/velocity/"
     "chain zones, drum pads or rack presets."),
    (26, "Automation and Editing Envelopes", 22, 0.45, "partial",
     "Envelopes for track volume, pan and mute, built-in device and plug-in parameters: breakpoints "
     "with curved segments and jumps, saved, played in playback and renders (64-frame steps, ramped "
     "mixer gains), a manual change turning a lane off until re-enabled. Lanes under each track in "
     "the Arrangement: a parameter chooser, points added, moved on the grid, selected and moved or "
     "deleted together, curves bent by dragging, re-enabled from the lane. Missing recording, "
     "drawing with a pencil, plug-in lanes in the view, send, master and tempo automation, "
     "locking, simplifying envelopes."),
    (27, "Clip Envelopes", 13, 0.00, "absent", "No clip envelopes."),
    (28, "Working with Video", 6, 0.00, "absent", "No video import or window."),
    (29, "Live Audio Effect Reference", 139, 0.15, "partial",
     "Suhina has 7 effects (utility, auto-pan, three-band DJ EQ, vinyl distortion, overdrive, "
     "compressor, limiter) against 42 documented audio effects; several are close equivalents in "
     "kind but far narrower in parameters and modes."),
    (30, "Live MIDI Effect Reference", 13, 0.00, "absent",
     "None of the 8 MIDI effects (arpeggiator, chord, scale, velocity...) exist."),
    (31, "Live Instrument Reference", 120, 0.08, "started",
     "One instrument: the FM synth (DX7-style, MSFA port) against 13 instruments; no sampler, "
     "drum sampler, wavetable, subtractive or physical modelling yet."),
    (32, "Max for Live", 6, 0.05, "planned",
     "ADR 0008 proposes Pure Data patch devices and keeps imported Max devices as placeholders; "
     "no patching runtime."),
    (33, "Max for Live Devices", 35, 0.00, "absent", "Not applicable until Max for Live exists."),
    (34, "MIDI and Key Remote Control", 9, 0.15, "partial",
     "Every command can be bound to a key (keymap.json, Settings > Shortcuts); no MIDI remote "
     "mapping, takeover modes or mapping browser."),
    (35, "Using Push 1", 40, 0.00, "absent", "No control-surface support."),
    (36, "Using Push 2", 59, 0.00, "absent", "No control-surface support."),
    (37, "Synchronizing with Link, Tempo Follower, and MIDI", 9, 0.00, "absent",
     "No Link, no Tempo Follower, no MIDI clock sync."),
    (38, "Computer Audio Resources and Strategies", 7, 0.40, "partial",
     "CPU-load meter, the program's RAM in the control bar, buffer-size choice, render-ahead "
     "engine, plug-in deadline reporting, real-time scheduling without setup (MMCSS Pro Audio, "
     "SCHED_FIFO or rtkit), an 80 s / 58-warped-clip set opens in 0.4 s. Missing track freeze, "
     "disk-load meter."),
    (39, "Audio Fact Sheet", 8, None, "n/a",
     "Not a feature list. Suhina renders 32-bit float and does not dither; no neutrality test suite."),
    (40, "MIDI Fact Sheet", 6, None, "n/a",
     "Not a feature list; MIDI timing and quantisation behaviour. Suhina has no MIDI timing tests yet."),
    (41, "Accessibility and Keyboard Navigation", 11, 0.05, "absent",
     "No screen-reader or keyboard-navigation story beyond the shortcut system."),
    (42, "Live Keyboard Shortcuts", 23, 0.30, "partial",
     "Keymap covers command binding, selection actions and window actions; many editing shortcuts "
     "have no command behind them yet."),
    (43, "Credits", 3, None, "n/a", "Front matter."),
]

AREAS = {
    "Views and editing (ch 6-10, 16)": [6, 7, 8, 9, 10, 16],
    "Browser, files, project (ch 4, 5)": [4, 5],
    "Devices (ch 23-25, 29-31)": [23, 24, 25, 29, 30, 31],
    "Mixing, routing, I/O (ch 17, 18, 38)": [17, 18, 38],
    "Automation, envelopes (ch 26, 27)": [26, 27],
    "Generative/transform (ch 11-15)": [11, 12, 13, 14, 15],
    "Recording, bounce, comp (ch 19-22)": [19, 20, 21, 22],
    "Outside scope (ch 28, 32-37, 41)": [28, 32, 33, 34, 35, 36, 37, 41],
    "Setup, shortcuts (ch 2, 3, 42)": [2, 3, 42],
}

# Chapters that describe products or hardware outside a clip-launching DAW's own scope.
# ADR 0016 › Decision 2 lists the same chapters; change both together.
OUT_OF_SCOPE = {28, 32, 33, 35, 36}  # video, Max for Live (+ its devices), Push 1, Push 2


def weighted(rows):
    """Page-weighted coverage of a selection of chapter rows."""
    num = sum(p * c for _, _, p, c, _, _ in rows if c is not None)
    den = sum(p for _, _, p, c, _, _ in rows if c is not None)
    return num / den, den


def section_metrics(work: Path):
    """Coverage weighted by numbered sections instead of pages.

    Falls back to the counts from the last full run (SECTIONS) when the heading map is not there,
    which is the normal case on a machine, or in CI, without a copy of the manual.
    """
    path = work / "headings.json"
    if not path.is_file():
        return dict(SECTIONS)
    headings = json.loads(path.read_text(encoding="utf-8"))

    def squeeze(text):
        prev = None
        while prev != text:
            prev = text
            text = re.sub(r"(?<=\d) (?=\d)", "", text)
        return text

    counts: dict = {}
    for entry in headings:
        if entry["kind"] != "section":
            continue
        chapter = int(squeeze(entry["num"]).split(".")[0])
        counts[chapter] = counts.get(chapter, 0) + 1

    coverage = {n: c for n, _, _, c, _, _ in CHAPTERS}
    total = sum(counts.values())
    zero = sum(v for k, v in counts.items() if not coverage.get(k))
    covered = sum(v * (coverage.get(k) or 0) for k, v in counts.items())
    return {"total": total, "zero": zero, "coverage": covered / total if total else 0.0}


def report(work: Path):
    scored = [r for r in CHAPTERS if r[3] is not None]
    all_cov, all_pages = weighted(scored)
    core = [r for r in scored if r[0] not in OUT_OF_SCOPE]
    core_cov, core_pages = weighted(core)
    scope = [r for r in core if r[0] != 41]
    scope_cov, scope_pages = weighted(scope)

    print(f"all chapters            {all_cov*100:5.1f}%  ({all_pages} pages)")
    print(f"without Push/M4L/video  {core_cov*100:5.1f}%  ({core_pages} pages)")
    print(f"roadmap scope           {scope_cov*100:5.1f}%  ({scope_pages} pages)")
    metrics = section_metrics(work)
    if metrics:
        print(f"by numbered sections    {metrics['coverage']*100:5.1f}%  "
              f"({metrics['total']} sections, {metrics['zero']} in untouched chapters)")
    print()
    for name, nums in AREAS.items():
        sel = [r for r in CHAPTERS if r[0] in nums and r[3] is not None]
        cov, pages = weighted(sel)
        print(f"{cov*100:5.1f}%  {pages:>4}p  {name}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dir", default=str(REPO / "target" / "parity"))
    args = parser.parse_args()

    report(Path(args.dir))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
