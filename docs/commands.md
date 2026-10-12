# Commands

Every command Loopwerk has, generated from the command registry in `crates/session` by
`loopwerk-cli commands --markdown`; do not edit by hand. A workflow regenerates it on `main`
after every merge, so pull requests leave it alone. The CLI, the MCP server and the UI run
these same commands ([ADR 0002](decisions/0002-commands.md), [`mcp.md`](mcp.md)).

| Command | Kind | What it does | Parameters |
|---|---|---|---|
| `projects.list` | projects | List the open projects in tab order with id, name, source and whether each is active | none |
| `projects.switch` | projects | Make an open project the active one; the previous one stops playing | `project` (integer): project id, from projects.list |
| `projects.next` | projects | Make the next open project active, wrapping around after the last | none |
| `projects.previous` | projects | Make the previous open project active, wrapping around before the first | none |
| `projects.close` | projects | Close an open project and its undo history; a neighbour becomes active if it was | `project` (integer): project id, from projects.list |
| `plugins.list` | plugins | List the plug-ins the last scan found (id, name, vendor, version, features, file) and the files that failed | none |
| `plugins.scan` | plugins | Look for installed plug-ins in the standard folders and the added ones; reads only new and changed files unless all is true | `all` (bool, optional): read every file again, failed ones too (default false) |
| `plugins.folders` | plugins | List the folders a scan looks in: the system's standard ones and the ones added | none |
| `plugins.addFolder` | plugins | Add a folder for scans to look in, besides the standard ones | `folder` (text): a folder that holds plug-ins |
| `plugins.removeFolder` | plugins | Stop looking in a folder that was added | `folder` (text): a folder that holds plug-ins |
| `plugins.check` | plugins | List a project's plug-in devices and the installed plug-in found for each: how it was found (same, declared, vst2-id, same-name, newer-version) and whether its saved settings are promised to carry over | `project` (integer, optional): project id, from projects.list (default the active one) |
| `commands.list` | read | List every command with its parameters | none |
| `project.info` | read | Summarise the open project: tempo, time signature, counts of tracks, scenes and clips, undo state | none |
| `project.tracks` | read | List tracks with id, name, volume (dB), pan, mute and devices | none |
| `project.clips` | read | List arrangement clips with track, index, start and length in beats, and note count | `track` (track, optional): only clips on this track (id or name) |
| `transport.setTempo` | edit | Set the project tempo | `bpm` (number): beats per minute, 20 to 999 |
| `track.setVolume` | edit | Set a track's volume | `track` (track): track id or name, from project.tracks; `db` (decibels): level in dB, at most +6; "-inf" or below -150 is silence |
| `track.setPan` | edit | Set a track's pan | `track` (track): track id or name, from project.tracks; `pan` (number): -1 is full left, 0 centre, 1 full right |
| `track.setMute` | edit | Mute or unmute a track | `track` (track): track id or name, from project.tracks; `mute` (bool): true mutes the track |
| `track.group` | edit | Put tracks into a new group track, which plays into where they played | `tracks` (tracks): the tracks to group (ids or names), all in the same group or none; `name` (text, optional): the group's name ("Group" if left out) |
| `track.ungroup` | edit | Dissolve a group track: its tracks stay, playing into where the group played | `track` (track): track id or name, from project.tracks |
| `track.move` | edit | Move a track (a group with its tracks) to another place in the track order | `track` (track): track id or name, from project.tracks; `to` (integer): its new position, from 0, in project.tracks order |
| `track.rename` | edit | Rename a track | `track` (track): track id or name, from project.tracks; `name` (text): the new name, not empty |
| `track.setColor` | edit | Set a track's colour | `track` (track): track id or name, from project.tracks; `color` (text): "#rrggbb", or "none" for the default |
| `track.delete` | edit | Delete a track with its clips; a deleted group's tracks stay | `track` (track): track id or name, from project.tracks |
| `clip.delete` | edit | Delete an arrangement clip | `track` (track): track id or name, from project.tracks; `index` (integer): clip index on the track, from project.clips |
| `clip.setGain` | edit | Set an audio clip's gain (every clip that plays the same audio content follows) | `track` (track): track id or name, from project.tracks; `index` (integer): clip index on the track, from project.clips; `db` (decibels): gain in dB, at most +24; "-inf" or below -150 is silence |
| `clip.copy` | read | Copy an arrangement clip, with what it plays, for clip.paste | `track` (track): track id or name, from project.tracks; `index` (integer): clip index on the track, from project.clips |
| `clip.cut` | edit | Copy an arrangement clip for clip.paste and delete it | `track` (track): track id or name, from project.tracks; `index` (integer): clip index on the track, from project.clips |
| `clip.paste` | edit | Place the copied clip on a track, replacing what it covers unless stacked | `track` (track): track id or name, from project.tracks; `start` (number): where it starts, in beats from the beginning, at least 0; `stack` (bool, optional): true leaves clips under it as they are |
| `clip.duplicate` | edit | Place a copy of an arrangement clip right after it, replacing what the copy covers | `track` (track): track id or name, from project.tracks; `index` (integer): clip index on the track, from project.clips |
| `track.setOutput` | edit | Route a track's audio to the main output or into a group track | `track` (track): track id or name, from project.tracks; `to` (track): "main", or a group track (id or name) to play into |
| `track.setSolo` | edit | Solo a track or end its solo: while any track is soloed, only soloed tracks, the groups they play into and return tracks are heard | `track` (track): track id or name, from project.tracks; `solo` (bool): true solos the track; `exclusive` (bool, optional): true also ends every other solo (false by default) |
| `clip.move` | edit | Move an arrangement clip to a new start position, and to another track if asked; it replaces what it covers unless stacked | `track` (track): track id or name, from project.tracks; `index` (integer): clip index on the track, from project.clips; `start` (number): new start in beats from the beginning, at least 0; `to_track` (track, optional): track to move the clip to (id or name); it must take the clip's kind: notes or audio; `stack` (bool, optional): true leaves clips under it as they are (they all play); false (default) shortens, splits or removes them |
| `project.launcher` | read | List the clip launcher: scenes (name, tempo) and each track's slots (clip name, stop button) | none |
| `launcher.state` | transport | What the audio engine is playing: transport, tempo, position, and each track's playing and queued clip | none |
| `transport.play` | transport | Start the transport from the beginning: the arrangement plays, launches follow the beat | none |
| `transport.stop` | transport | Stop the transport, the arrangement and every launched clip at once | none |
| `transport.setMetronome` | transport | Turn the metronome on or off: a click on every beat while playing, higher on the bar | `on` (bool): true turns the click on; `db` (decibels, optional): click level in dB, at most +6; keeps the current level (-6 at first) if left out |
| `clip.launch` | transport | Launch the clip in a track's slot at the next quantisation boundary (an empty slot with a stop button stops the track) | `track` (track): track id or name, from project.tracks; `scene` (integer): scene number, from 1, as in project.launcher |
| `scene.launch` | transport | Launch a scene: every track launches its slot in that row at the next quantisation boundary | `scene` (integer): scene number, from 1, as in project.launcher |
| `track.stop` | transport | Stop the clip a track is playing, at the next quantisation boundary | `track` (track): track id or name, from project.tracks |
| `launcher.stopAll` | transport | Stop every launched clip at the next quantisation boundary | none |
| `launcher.setQuantization` | transport | Set the grid launches wait for (default 1 bar) | `value` (text): none, 8 bars, 4 bars, 2 bars, 1 bar, 1/2, 1/2T, 1/4, 1/4T, 1/8, 1/8T, 1/16, 1/16T or 1/32 |
| `launcher.backToArrangement` | transport | Return every track that launched a clip to playing its arrangement clips | none |
| `devices.list` | read | List the built-in devices (id, name, effect or instrument) with their parameters (id, range, default, unit, options) | none |
| `track.devices` | read | List the devices on a track in order, with every built-in device's parameter values | `track` (track): track id or name, from project.tracks |
| `device.add` | edit | Add a built-in device to a track; an instrument goes first and plays the track's notes, an effect goes last unless a position is given | `track` (track): track id or name, from project.tracks; `id` (text): built-in device id, from devices.list; `position` (integer, optional): where in the chain, from 0 (default: last; an instrument is always first) |
| `device.remove` | edit | Remove a device from a track | `track` (track): track id or name, from project.tracks; `device` (integer): device index on the track, from 0, as in track.devices |
| `device.setEnabled` | edit | Switch a device on or off; a device that is off passes the sound through | `track` (track): track id or name, from project.tracks; `device` (integer): device index on the track, from 0, as in track.devices; `enabled` (bool): false switches the device off |
| `device.setParam` | edit | Set a parameter of a built-in device on a track; the value is limited to the parameter's range | `track` (track): track id or name, from project.tracks; `device` (integer): device index on the track, from 0, as in track.devices; `param` (text): parameter id, from devices.list; `value` (number): value in the parameter's unit (dB, Hz, 0..1 for percentages, option index for choices) |
| `edit.undo` | history | Undo the last edit | none |
| `edit.redo` | history | Redo the last undone edit | none |
