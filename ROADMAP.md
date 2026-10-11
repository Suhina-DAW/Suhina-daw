# Roadmap

Loopwerk is pre-alpha. Milestones are in order; each one builds on the previous.

| | Milestone | Status |
|---|---|---|
| M0 | **Foundation:** workspace, GPL-3.0-or-later, clean-room rules, DAW-neutral model (ADR 0001) | done |
| M1 | **First sound:** offline renderer; `.als` reader for tempo, time signature, tracks, groups, mixer, sends, scenes, MIDI clips with start/end/loop markers, and audio clips and device chains as data (not yet played); `loopwerk-cli render` | done |
| M2 | **Real-time playback:** audio output (ALSA, PipeWire, JACK, WASAPI, CoreAudio), transport, `loopwerk-cli play` | done: `play` verified by ear on Windows (WASAPI) with a Live 12 set |
| M3 | **Commands and agents:** every action is an engine command with an id, params and undo; the CLI and an **MCP server** drive the same commands, so an agent can open, inspect, edit, play and render a project headlessly | in progress: registry, first commands and `loopwerk-cli mcp` done; more commands to come |
| M4 | **Clip launching:** launch quantisation, scenes, stop buttons, follow actions | in progress: the engine launches clips and scenes on a quantised grid (bars or note values, following the time signature) with stop buttons, scene tempos and launched clips taking their track over from the arrangement; commands for the CLI (`play --launcher`) and MCP (`--audio`, and offline launch scripts in `project_render`); follow actions and launch modes still to come |
| M5 | **Audio playback:** play imported audio clips with gain, transpose, warping and time-stretching | in progress: clips play from WAV/AIFF/FLAC/MP3/Ogg with gain, loops and warp markers; warped clips still change pitch with speed (no time-stretch yet) and transpose applies to unwarped clips only |
| M6 | **Plug-ins:** CLAP hosting, sandboxed out of process (AGENTS.md rule 5); VST3 next, using the state already imported from Live Sets | |
| M7 | **More formats:** DAWproject import and export | |
| M8 | **UI:** Session and Arrangement views on top of the commands; familiar workflow, Loopwerk's own look | |

Later, not ordered: FL Studio projects (`.flp`), built-in instruments and effects, automation,
sends and returns in the mix (groups and the master already apply volume and mute), MIDI controllers and Push (from Ableton's published Push interface docs),
Max for Live devices as placeholders, ARA, bridging Windows plug-ins.

Linux is the primary platform; Windows and macOS are supported.
