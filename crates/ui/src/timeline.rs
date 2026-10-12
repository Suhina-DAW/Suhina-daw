//! What the Arrangement view shows, derived from the project and the engine's monitor: one lane
//! per track with its clips placed in beats, and marks for the notes inside each clip. Pure data,
//! like [`crate::grid`].

use loopwerk_engine::LauncherMonitor;
use loopwerk_engine::model::{ChannelRole, Clip, ClipContent, LaneData, Project, Rgb, TrackId};

use crate::grid::{ColumnKind, TRACK_PALETTE, gain_to_db};

/// Note marks drawn per clip at most; the rest are left out.
const MAX_MARKS: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct NoteMark {
    /// Beats from the clip's start.
    pub start: f64,
    pub length: f64,
    pub key: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipBox {
    /// Index on the track, as `clip.move` and `project.clips` count it.
    pub index: usize,
    pub start: f64,
    pub length: f64,
    pub name: String,
    pub enabled: bool,
    pub audio: bool,
    pub notes: Vec<NoteMark>,
    /// The sub-row the clip is drawn in, and how many the lane has: more than one where clips
    /// overlap (stacked), so each stays visible.
    pub row: usize,
    pub rows: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    pub track: TrackId,
    pub name: String,
    pub color: Rgb,
    pub kind: ColumnKind,
    /// How many groups the track is inside.
    pub depth: usize,
    /// The track's place in the project's track order, as `track.move` counts it.
    pub position: usize,
    pub mute: bool,
    pub solo: bool,
    pub volume_db: f64,
    pub pan: f64,
    /// The group track this track plays into; `None` is the main output.
    pub output: Option<TrackId>,
    pub clips: Vec<ClipBox>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Timeline {
    /// Tracks in project order, then return tracks.
    pub lanes: Vec<Lane>,
    /// The main channel's volume, if the project has one.
    pub main_volume_db: Option<f64>,
    /// Where the last clip ends, in beats.
    pub end: f64,
    pub bar_beats: f64,
    pub running: bool,
    /// The playhead, in beats from the start.
    pub beat: f64,
}

impl Timeline {
    pub fn build(project: &Project, monitor: Option<&LauncherMonitor>) -> Self {
        let ts = project.transport.time_signature;
        let bar_beats =
            if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
        let mut lanes = Vec::new();
        let mut returns = Vec::new();
        for (position, track) in project.tracks.iter().enumerate() {
            let channel = track.channel.and_then(|id| project.channels.iter().find(|c| c.id == id));
            let kind = match channel.map(|c| c.role) {
                Some(ChannelRole::Master) => continue,
                Some(ChannelRole::Effect) => ColumnKind::Return,
                Some(ChannelRole::Submix) => ColumnKind::Group,
                _ => ColumnKind::Track,
            };
            let clips = project
                .arrangement
                .lanes
                .iter()
                .find(|l| l.track == Some(track.id))
                .map(|lane| {
                    lane.clips
                        .iter()
                        .enumerate()
                        .map(|(index, clip)| {
                            let content = project.content(clip.content);
                            ClipBox {
                                index,
                                start: clip.start,
                                length: clip.length,
                                name: content.map(|c| c.name.clone()).unwrap_or_default(),
                                enabled: clip.enabled,
                                audio: content
                                    .is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_)))),
                                notes: content.map(|c| note_marks(c, clip)).unwrap_or_default(),
                                row: 0,
                                rows: 1,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let clips = sub_rows(clips);
            let lane = Lane {
                track: track.id,
                name: track.name.clone(),
                color: track.color.unwrap_or(TRACK_PALETTE[position % TRACK_PALETTE.len()]),
                kind,
                depth: depth(project, track.parent),
                position,
                mute: channel.is_some_and(|c| c.mute),
                solo: channel.is_some_and(|c| c.solo),
                volume_db: channel.map_or(0.0, |c| gain_to_db(c.volume)),
                pan: channel.map_or(0.0, |c| c.pan),
                output: channel
                    .and_then(|c| c.output)
                    .and_then(|id| project.tracks.iter().find(|t| t.channel == Some(id)))
                    .map(|t| t.id),
                clips,
            };
            if kind == ColumnKind::Return { returns.push(lane) } else { lanes.push(lane) }
        }
        lanes.extend(returns);
        let end = lanes.iter().flat_map(|l| &l.clips).map(|c| c.start + c.length).fold(0.0, f64::max);
        let main_volume_db =
            project.channels.iter().find(|c| c.role == ChannelRole::Master).map(|c| gain_to_db(c.volume));
        let mut timeline = Self { lanes, main_volume_db, end, bar_beats, running: false, beat: 0.0 };
        timeline.follow(monitor);
        timeline
    }

    /// Takes the transport and playhead from the engine, leaving the rest as it was built.
    pub fn follow(&mut self, monitor: Option<&LauncherMonitor>) {
        let (running, beat) = monitor.map_or((false, 0.0), |m| (m.running(), m.beat()));
        self.running = running;
        self.beat = if beat.is_finite() { beat.max(0.0) } else { 0.0 };
    }
}

/// Puts each clip in the first sub-row free where it starts; every clip learns the lane's count.
fn sub_rows(mut clips: Vec<ClipBox>) -> Vec<ClipBox> {
    let mut ends: Vec<f64> = Vec::new();
    let mut order: Vec<usize> = (0..clips.len()).collect();
    let start = |i: &usize| clips.get(*i).map_or(0.0, |c| c.start);
    order.sort_by(|a, b| start(a).total_cmp(&start(b)));
    for i in order {
        let Some(clip) = clips.get_mut(i) else { continue };
        let end = clip.start + clip.length;
        match ends.iter().position(|e| *e <= clip.start + 1e-9) {
            Some(row) => {
                clip.row = row;
                if let Some(e) = ends.get_mut(row) {
                    *e = end;
                }
            }
            None => {
                clip.row = ends.len();
                ends.push(end);
            }
        }
    }
    let rows = ends.len().max(1);
    for clip in &mut clips {
        clip.rows = rows;
    }
    clips
}

/// How many groups a track is nested in; a broken chain stops counting.
fn depth(project: &Project, mut parent: Option<TrackId>) -> usize {
    let mut depth = 0;
    while let Some(id) = parent {
        depth += 1;
        if depth > project.tracks.len() {
            break;
        }
        parent = project.tracks.iter().find(|t| t.id == id).and_then(|t| t.parent);
    }
    depth
}

/// The notes a clip plays, from its start: the content from `offset`, then its loop again and
/// again until the clip ends.
pub fn note_marks(content: &ClipContent, clip: &Clip) -> Vec<NoteMark> {
    let notes: Vec<_> = content
        .lanes
        .iter()
        .filter_map(|l| match &l.data {
            LaneData::Notes(notes) => Some(notes),
            LaneData::Audio(_) => None,
        })
        .flatten()
        .collect();
    let mut marks = Vec::new();
    if notes.is_empty() || clip.length.is_nan() || clip.length <= 0.0 {
        return marks;
    }
    // Segments of content time, in the order the clip plays them.
    let (first_end, repeat) = match clip.loop_range {
        Some(r) if r.end > r.start => (r.end.max(clip.offset), Some((r.start, r.end))),
        _ => (clip.offset + clip.length, None),
    };
    let mut segment = (clip.offset, first_end);
    let mut at = 0.0;
    while at < clip.length && marks.len() < MAX_MARKS {
        let (from, to) = segment;
        for note in notes.iter().filter(|n| n.time >= from && n.time < to) {
            let start = at + note.time - from;
            if start < clip.length && marks.len() < MAX_MARKS {
                let length = note.duration.min(to - note.time).min(clip.length - start).max(0.0);
                marks.push(NoteMark { start, length, key: note.key });
            }
        }
        at += to - from;
        match repeat {
            Some(r) => segment = r,
            None => break,
        }
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::demo_project;

    #[test]
    fn lanes_follow_the_tracks_with_returns_last() {
        let timeline = Timeline::build(&demo_project(), None);
        let names: Vec<&str> = timeline.lanes.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Drums", "Bass", "Keys", "Vox", "A Reverb", "B Delay"]);
        assert_eq!(timeline.end, 192.0);
        assert_eq!(timeline.main_volume_db, Some(0.0));
        assert_eq!(timeline.lanes[0].clips[1].index, 1);
        assert_eq!(timeline.lanes[0].clips[1].name, "Beat B");
    }

    #[test]
    fn overlapping_clips_get_sub_rows() {
        let mut project = demo_project();
        let lane = &mut project.arrangement.lanes[0];
        lane.clips[1].start = 32.0; // over the first clip (0..64)
        let timeline = Timeline::build(&project, None);
        let rows: Vec<(usize, usize)> = timeline.lanes[0].clips.iter().map(|c| (c.row, c.rows)).collect();
        assert_eq!(rows[0], (0, 2));
        assert_eq!(rows[1], (1, 2));
        assert!(Timeline::build(&demo_project(), None).lanes.iter().flat_map(|l| &l.clips).all(|c| c.rows == 1));
    }

    #[test]
    fn a_looping_clip_repeats_its_notes() {
        let project = demo_project();
        let clip = &project.arrangement.lanes[0].clips[0];
        let content = project.content(clip.content).unwrap();
        let marks = note_marks(content, clip);
        // Four notes a bar, sixteen bars.
        assert_eq!(marks.len(), 64);
        assert_eq!(marks[4].start, 4.0);
        assert!(marks.iter().all(|m| m.start < clip.length));
    }

    #[test]
    fn a_clip_without_a_loop_shows_its_window_of_the_content() {
        let project = demo_project();
        let mut clip = project.arrangement.lanes[0].clips[0].clone();
        clip.loop_range = None;
        clip.offset = 1.0;
        clip.length = 2.0;
        let content = project.content(clip.content).unwrap();
        let starts: Vec<f64> = note_marks(content, &clip).iter().map(|m| m.start).collect();
        assert_eq!(starts, [0.0, 1.0]);
    }
}
