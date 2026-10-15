//! The user's settings: what they are, their defaults, and the text file they are kept in
//! (`key = value` lines, so a person can read and fix it). The app decides where the file lives.

/// Which side of the timeline the track headers are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    Left,
    #[default]
    Right,
}

/// What opening a project does while another is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnOpen {
    /// Ask each time, in the window.
    #[default]
    Ask,
    /// Open it in a new tab.
    NewTab,
    /// Close the active project and open it in its place.
    Replace,
}

/// Where the playhead goes when the transport stops (`transport.stop`'s `return`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StopReturn {
    /// Back to the play start marker, where it started.
    #[default]
    Marker,
    /// It stays where it stopped: the marker moves there.
    Here,
}

impl StopReturn {
    /// The value `transport.stop` takes for it.
    pub fn param(self) -> &'static str {
        match self {
            Self::Marker => "marker",
            Self::Here => "here",
        }
    }
}

/// How the detail area lays out a device chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceLayout {
    /// Devices side by side in a row under the view.
    #[default]
    Row,
    /// Devices one under another in a column beside the view.
    Stack,
}

/// Where a plug-in's editor opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorPlacement {
    /// In a window of its own, owned by Suhina's: above it, and minimised with it.
    #[default]
    Window,
    /// Inside the device panel, as part of the device chain (Windows and X11; elsewhere the
    /// editor opens in a window).
    DeviceView,
}

/// How the window is drawn. Takes effect the next time the app starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Graphics {
    /// wgpu: Vulkan, Metal or Direct3D 12.
    #[default]
    Gpu,
    /// OpenGL, through glow.
    OpenGl,
}

/// How samples with a known tempo are stretched when dropped (`clip.addFiles`' `warp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WarpMode {
    /// Keeps transients on the grid.
    #[default]
    Rhythmic,
    Tonal,
    Texture,
    Smooth,
    HighQuality,
    /// Speed and pitch change together.
    Repitch,
}

impl WarpMode {
    pub const ALL: [WarpMode; 6] =
        [Self::Rhythmic, Self::Tonal, Self::Texture, Self::Smooth, Self::HighQuality, Self::Repitch];

    /// The value `clip.addFiles` takes, and the settings file keeps.
    pub fn param(self) -> &'static str {
        match self {
            Self::Rhythmic => "rhythmic",
            Self::Tonal => "tonal",
            Self::Texture => "texture",
            Self::Smooth => "smooth",
            Self::HighQuality => "high-quality",
            Self::Repitch => "repitch",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Rhythmic => "Rhythmic",
            Self::Tonal => "Tonal",
            Self::Texture => "Texture",
            Self::Smooth => "Smooth",
            Self::HighQuality => "High quality",
            Self::Repitch => "Re-pitch",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.param() == text)
    }
}

/// The browser panel's narrowest and widest, and its width at first.
pub const BROWSER_WIDTHS: (u32, u32) = (200, 600);
pub const BROWSER_WIDTH: u32 = 300;
/// The preview's level, in dB: its range and its default.
/// The longest sample the sample map shows by default, and the range it may be set to.
pub const MAP_MAX_SECONDS: u32 = 10;
pub const MAP_MAX_SECONDS_RANGE: (u32, u32) = (1, 60);
pub const PREVIEW_VOLUMES: (i32, i32) = (-48, 6);
/// The cue volumes a settings file may hold, in dB; the lowest is silence.
pub const CUE_VOLUMES: (i32, i32) = (-60, 12);
pub const PREVIEW_VOLUME: i32 = -6;
/// The size of the whole window, in percent: the smallest and largest, the default, and how much
/// one Zoom in or Zoom out changes it.
pub const ZOOMS: (u32, u32) = (50, 200);
pub const ZOOM: u32 = 100;
pub const ZOOM_STEP: u32 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub animations: bool,
    pub track_headers: Side,
    pub graphics: Graphics,
    /// Audio clips moved over others stack (all play) instead of replacing them.
    pub sample_stacking: bool,
    /// MIDI clips moved over others stack instead of replacing them.
    pub midi_stacking: bool,
    pub on_open: OnOpen,
    /// Where Stop leaves the playhead.
    pub stop: StopReturn,
    pub device_layout: DeviceLayout,
    /// Where plug-in editors open.
    pub plugin_editors: EditorPlacement,
    /// The theme in use: [`crate::theme::BUILT_IN`] or the name of a file in the themes folder,
    /// without `.json`.
    pub theme: String,
    /// Where new projects are saved and templates are kept; empty for the defaults the app gives
    /// (`App::set_default_folders`).
    pub projects_folder: String,
    pub templates_folder: String,
    /// The browser panel shows, on which side, and how wide (`docs/spec/browser.md`).
    pub browser_open: bool,
    pub browser_side: Side,
    pub browser_width: u32,
    /// Selecting a sample or MIDI file previews it.
    pub preview_autoplay: bool,
    /// The preview's level, in dB.
    pub preview_volume: i32,
    /// Several dropped files go one track each without Ctrl (and onto one track with it).
    pub drop_separate: bool,
    pub warp_mode: WarpMode,
    /// The sample map (`docs/spec/sample-map.md`): grid rather than cloud, preview on hover,
    /// and the longest sample it shows, in seconds.
    pub map_grid: bool,
    pub map_hover_preview: bool,
    pub map_max_seconds: u32,
    /// The sample map is in a window of its own (`docs/spec/sample-map.md` › Where the map is
    /// shown), and where that window was: its left, top, width and height in points, on the
    /// whole desktop, so a second screen counts.
    pub map_window: bool,
    pub map_window_rect: Option<[i32; 4]>,
    /// Waveforms in decibels, so quiet parts show; linear otherwise.
    pub waveform_db: bool,
    /// The audio output (Settings › Audio, `audio.setOutput`): host and device by name, empty
    /// for the default; sample rate and buffer size, 0 for the device's own.
    pub audio_host: String,
    pub audio_device: String,
    pub audio_sample_rate: u32,
    pub audio_buffer: u32,
    /// WASAPI exclusive mode; off, so other programs stay heard.
    pub audio_exclusive: bool,
    /// The output pair the cue plays on (`output.setCuePair`): 1 is channels 3 and 4; 0 plays
    /// it with the main output.
    pub audio_cue_pair: u16,
    /// The cue volume in dB (`output.setCueVolume`); the lowest of [`CUE_VOLUMES`] is silence.
    pub audio_cue_volume: i32,
    /// The size of the whole window, in percent (`ZOOMS`): everything is drawn this much larger
    /// or smaller.
    pub zoom: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            animations: true,
            track_headers: Side::Right,
            graphics: Graphics::Gpu,
            sample_stacking: false,
            midi_stacking: false,
            on_open: OnOpen::Ask,
            stop: StopReturn::Marker,
            device_layout: DeviceLayout::Row,
            plugin_editors: EditorPlacement::Window,
            theme: crate::theme::BUILT_IN.into(),
            projects_folder: String::new(),
            templates_folder: String::new(),
            browser_open: true,
            browser_side: Side::Left,
            browser_width: BROWSER_WIDTH,
            preview_autoplay: true,
            preview_volume: PREVIEW_VOLUME,
            drop_separate: false,
            warp_mode: WarpMode::Rhythmic,
            map_grid: false,
            map_hover_preview: true,
            map_max_seconds: MAP_MAX_SECONDS,
            map_window: false,
            map_window_rect: None,
            waveform_db: false,
            audio_host: String::new(),
            audio_device: String::new(),
            audio_sample_rate: 0,
            audio_buffer: 0,
            audio_exclusive: false,
            audio_cue_pair: 0,
            audio_cue_volume: 0,
            zoom: ZOOM,
        }
    }
}

impl Settings {
    /// The audio output these settings choose.
    pub fn output_choice(&self) -> suhina_session::OutputChoice {
        let text = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
        suhina_session::OutputChoice {
            host: text(&self.audio_host),
            device: text(&self.audio_device),
            sample_rate: (self.audio_sample_rate > 0).then_some(self.audio_sample_rate),
            buffer_frames: (self.audio_buffer > 0).then_some(self.audio_buffer),
            exclusive: self.audio_exclusive,
            cue_pair: (self.audio_cue_pair > 0).then_some(self.audio_cue_pair),
            cue_gain: if self.audio_cue_volume <= CUE_VOLUMES.0 {
                0.0
            } else {
                10f64.powf(f64::from(self.audio_cue_volume) / 20.0) as f32
            },
        }
    }

    /// Keeps `choice` as the audio output.
    pub fn set_output_choice(&mut self, choice: &suhina_session::OutputChoice) {
        self.audio_host = choice.host.clone().unwrap_or_default();
        self.audio_device = choice.device.clone().unwrap_or_default();
        self.audio_sample_rate = choice.sample_rate.unwrap_or(0);
        self.audio_buffer = choice.buffer_frames.unwrap_or(0);
        self.audio_exclusive = choice.exclusive;
        self.audio_cue_pair = choice.cue_pair.unwrap_or(0);
        self.audio_cue_volume = if choice.cue_gain > 0.0 {
            ((20.0 * f64::from(choice.cue_gain).log10()).round() as i32).clamp(CUE_VOLUMES.0, CUE_VOLUMES.1)
        } else {
            CUE_VOLUMES.0
        };
    }

    /// Makes the window `steps` zoom steps larger (or smaller, when negative), onto the steps and
    /// within the limits. Returns whether the zoom changed.
    pub fn step_zoom(&mut self, steps: i32) -> bool {
        let step = i64::from(ZOOM_STEP);
        let zoom = i64::from(self.zoom);
        // From the step on the side it moves away from, so a hand-edited 115 goes to 120 or 110.
        let from = match steps.signum() {
            1 => zoom / step * step,
            -1 => (zoom + step - 1) / step * step,
            _ => return false,
        };
        let wanted = from.saturating_add(i64::from(steps).saturating_mul(step));
        let zoom = u32::try_from(wanted.clamp(i64::from(ZOOMS.0), i64::from(ZOOMS.1))).unwrap_or(ZOOM);
        let changed = zoom != self.zoom;
        self.zoom = zoom;
        changed
    }

    /// The zoom as the factor everything is drawn at (1.0 for 100 %).
    pub fn zoom_factor(&self) -> f32 {
        self.zoom.clamp(ZOOMS.0, ZOOMS.1) as f32 / 100.0
    }

    /// The settings as the lines of a settings file.
    pub fn to_text(&self) -> String {
        let headers = match self.track_headers {
            Side::Left => "left",
            Side::Right => "right",
        };
        let graphics = match self.graphics {
            Graphics::Gpu => "wgpu",
            Graphics::OpenGl => "glow",
        };
        let on = |b: bool| if b { "on" } else { "off" };
        let devices = match self.device_layout {
            DeviceLayout::Row => "row",
            DeviceLayout::Stack => "stack",
        };
        let on_open = match self.on_open {
            OnOpen::Ask => "ask",
            OnOpen::NewTab => "tab",
            OnOpen::Replace => "replace",
        };
        let editors = match self.plugin_editors {
            EditorPlacement::Window => "window",
            EditorPlacement::DeviceView => "device",
        };
        format!(
            "# Suhina settings; changed from Options > Settings.\nanimations = {}\ntrack_headers = {headers}\nrenderer = {graphics}\nsample_stacking = {}\nmidi_stacking = {}\nopen_projects = {on_open}\nstop = {stop}\ndevice_chain = {devices}\nplugin_editors = {editors}\ntheme = {}\nprojects_folder = {}\ntemplates_folder = {}\nbrowser = {}\nbrowser_side = {browser_side}\nbrowser_width = {}\npreview_autoplay = {}\npreview_volume = {}\ndrop_files = {drop}\nwarp_mode = {}\nmap_layout = {map_layout}\nmap_hover_preview = {}\nmap_max_seconds = {}\nmap_window = {}\nmap_window_rect = {}\naudio_host = {}\naudio_device = {}\naudio_sample_rate = {}\naudio_buffer = {}\naudio_exclusive = {}\naudio_cue_pair = {}\naudio_cue_volume = {}\nzoom = {}\nwaveform_scale = {waveform_scale}\n",
            on(self.animations),
            on(self.sample_stacking),
            on(self.midi_stacking),
            self.theme,
            self.projects_folder,
            self.templates_folder,
            on(self.browser_open),
            self.browser_width,
            on(self.preview_autoplay),
            self.preview_volume,
            self.warp_mode.param(),
            on(self.map_hover_preview),
            self.map_max_seconds,
            on(self.map_window),
            self.map_window_rect.map_or_else(|| "none".to_string(), |[x, y, w, h]| format!("{x}, {y}, {w}, {h}")),
            self.audio_host,
            self.audio_device,
            self.audio_sample_rate,
            self.audio_buffer,
            on(self.audio_exclusive),
            self.audio_cue_pair,
            self.audio_cue_volume,
            self.zoom,
            map_layout = if self.map_grid { "grid" } else { "cloud" },
            waveform_scale = if self.waveform_db { "db" } else { "linear" },
            stop = self.stop.param(),
            browser_side = match self.browser_side {
                Side::Left => "left",
                Side::Right => "right",
            },
            drop = if self.drop_separate { "separate" } else { "one_track" },
        )
    }

    /// Reads a settings file. Unknown keys and values are skipped with a warning and leave the
    /// default, so an old or hand-edited file never stops the app from starting.
    pub fn from_text(text: &str) -> (Self, Vec<String>) {
        let mut settings = Self::default();
        let mut warnings = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                warnings.push(format!("settings line {}: expected key = value", number + 1));
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match (key, value) {
                ("animations", "on" | "off") => settings.animations = value == "on",
                ("sample_stacking", "on" | "off") => settings.sample_stacking = value == "on",
                ("midi_stacking", "on" | "off") => settings.midi_stacking = value == "on",
                ("track_headers", "left") => settings.track_headers = Side::Left,
                ("track_headers", "right") => settings.track_headers = Side::Right,
                ("device_chain", "row") => settings.device_layout = DeviceLayout::Row,
                ("device_chain", "stack") => settings.device_layout = DeviceLayout::Stack,
                ("plugin_editors", "window") => settings.plugin_editors = EditorPlacement::Window,
                ("plugin_editors", "device") => settings.plugin_editors = EditorPlacement::DeviceView,
                ("projects_folder", folder) => settings.projects_folder = folder.to_string(),
                ("templates_folder", folder) => settings.templates_folder = folder.to_string(),
                ("open_projects", "ask") => settings.on_open = OnOpen::Ask,
                ("open_projects", "tab") => settings.on_open = OnOpen::NewTab,
                ("open_projects", "replace") => settings.on_open = OnOpen::Replace,
                ("stop", "marker") => settings.stop = StopReturn::Marker,
                ("stop", "here") => settings.stop = StopReturn::Here,
                ("renderer", "wgpu") => settings.graphics = Graphics::Gpu,
                ("renderer", "glow") => settings.graphics = Graphics::OpenGl,
                ("theme", id) if is_theme_id(id) => settings.theme = id.to_string(),
                ("browser", "on" | "off") => settings.browser_open = value == "on",
                ("browser_side", "left") => settings.browser_side = Side::Left,
                ("browser_side", "right") => settings.browser_side = Side::Right,
                ("browser_width", width) if width.parse::<u32>().is_ok() => {
                    settings.browser_width =
                        width.parse::<u32>().unwrap_or(BROWSER_WIDTH).clamp(BROWSER_WIDTHS.0, BROWSER_WIDTHS.1);
                }
                ("preview_autoplay", "on" | "off") => settings.preview_autoplay = value == "on",
                ("preview_volume", db) if db.parse::<i32>().is_ok() => {
                    settings.preview_volume =
                        db.parse::<i32>().unwrap_or(PREVIEW_VOLUME).clamp(PREVIEW_VOLUMES.0, PREVIEW_VOLUMES.1);
                }
                ("map_layout", "cloud" | "grid") => settings.map_grid = value == "grid",
                ("map_hover_preview", "on" | "off") => settings.map_hover_preview = value == "on",
                ("waveform_scale", "linear" | "db") => settings.waveform_db = value == "db",
                ("map_max_seconds", seconds) if seconds.parse::<u32>().is_ok() => {
                    settings.map_max_seconds = seconds
                        .parse::<u32>()
                        .unwrap_or(MAP_MAX_SECONDS)
                        .clamp(MAP_MAX_SECONDS_RANGE.0, MAP_MAX_SECONDS_RANGE.1);
                }
                ("map_window", "on" | "off") => settings.map_window = value == "on",
                ("map_window_rect", "none") => settings.map_window_rect = None,
                ("map_window_rect", rect) if parse_window_rect(rect).is_some() => {
                    settings.map_window_rect = parse_window_rect(rect);
                }
                ("zoom", percent) if percent.parse::<u32>().is_ok() => {
                    settings.zoom = percent.parse::<u32>().unwrap_or(ZOOM).clamp(ZOOMS.0, ZOOMS.1);
                }
                ("drop_files", "one_track") => settings.drop_separate = false,
                ("drop_files", "separate") => settings.drop_separate = true,
                ("warp_mode", mode) if WarpMode::parse(mode).is_some() => {
                    settings.warp_mode = WarpMode::parse(mode).unwrap_or_default();
                }
                ("audio_host", host) => settings.audio_host = host.to_string(),
                ("audio_device", device) => settings.audio_device = device.to_string(),
                ("audio_sample_rate", rate) if rate.parse::<u32>().is_ok_and(|r| r <= MAX_SAMPLE_RATE) => {
                    settings.audio_sample_rate = rate.parse().unwrap_or(0);
                }
                ("audio_buffer", frames)
                    if frames
                        .parse::<u32>()
                        .is_ok_and(|f| f == 0 || (MIN_BUFFER_FRAMES..=MAX_BUFFER_FRAMES).contains(&f)) =>
                {
                    settings.audio_buffer = frames.parse().unwrap_or(0);
                }
                ("audio_exclusive", "on" | "off") => settings.audio_exclusive = value == "on",
                ("audio_cue_pair", pair) if pair.parse::<u16>().is_ok_and(|p| p <= MAX_CUE_PAIR) => {
                    settings.audio_cue_pair = pair.parse().unwrap_or(0);
                }
                ("audio_cue_volume", db) if db.parse::<i32>().is_ok() => {
                    settings.audio_cue_volume = db.parse::<i32>().unwrap_or(0).clamp(CUE_VOLUMES.0, CUE_VOLUMES.1);
                }
                _ => {
                    warnings.push(format!("settings line {}: {key} = {value} is not a known setting", number + 1));
                }
            }
        }
        (settings, warnings)
    }
}

use suhina_session::audio::{MAX_BUFFER_FRAMES, MAX_CUE_PAIR, MAX_SAMPLE_RATE, MIN_BUFFER_FRAMES};

/// Reads a window's place, `left, top, width, height` in points. A position far off any screen
/// or a size outside [`WINDOW_SIZES`] is refused, so a broken file cannot open a window nowhere.
fn parse_window_rect(text: &str) -> Option<[i32; 4]> {
    let parts: Vec<i32> = text.split(',').map(|p| p.trim().parse::<i32>()).collect::<Result<_, _>>().ok()?;
    let [x, y, w, h] = <[i32; 4]>::try_from(parts).ok()?;
    let on_desktop = |v: i32| (-WINDOW_POSITION..=WINDOW_POSITION).contains(&v);
    let sized = |v: i32| (WINDOW_SIZES.0..=WINDOW_SIZES.1).contains(&v);
    (on_desktop(x) && on_desktop(y) && sized(w) && sized(h)).then_some([x, y, w, h])
}

/// The smallest and largest a window of its own may be, in points, and how far from the origin
/// of the desktop its corner may be.
pub const WINDOW_SIZES: (i32, i32) = (200, 10_000);
const WINDOW_POSITION: i32 = 30_000;

/// Whether `id` can name a theme file: not empty and not a path.
pub fn is_theme_id(id: &str) -> bool {
    !id.is_empty() && !id.contains(['/', '\\', ':']) && id != "." && id != ".."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_round_trip() {
        let settings = Settings {
            animations: false,
            track_headers: Side::Left,
            graphics: Graphics::OpenGl,
            sample_stacking: true,
            midi_stacking: false,
            on_open: OnOpen::Replace,
            stop: StopReturn::Here,
            device_layout: DeviceLayout::Stack,
            plugin_editors: EditorPlacement::DeviceView,
            theme: "Night blue".into(),
            projects_folder: "C:/Music/Songs = mine".into(),
            templates_folder: "/home/me/templates".into(),
            browser_open: false,
            browser_side: Side::Right,
            browser_width: 420,
            preview_autoplay: false,
            preview_volume: -12,
            drop_separate: true,
            warp_mode: WarpMode::Tonal,
            map_grid: true,
            map_hover_preview: false,
            map_max_seconds: 30,
            waveform_db: true,
            audio_host: "ASIO".into(),
            audio_device: "Universal Audio Thunderbolt".into(),
            audio_sample_rate: 48_000,
            audio_buffer: 256,
            audio_exclusive: true,
            audio_cue_pair: 2,
            audio_cue_volume: -6,
            zoom: 130,
            map_window: true,
            map_window_rect: Some([-1600, 40, 900, 700]),
        };
        assert_eq!(Settings::from_text(&settings.to_text()), (settings, Vec::new()));
        assert_eq!(Settings::from_text(&Settings::default().to_text()).0, Settings::default());
    }

    #[test]
    fn browser_values_out_of_range_are_limited() {
        let (settings, warnings) =
            Settings::from_text("browser_width = 5000\npreview_volume = -900\nwarp_mode = elastic\n");
        assert_eq!((settings.browser_width, settings.preview_volume), (BROWSER_WIDTHS.1, PREVIEW_VOLUMES.0));
        assert_eq!(settings.warp_mode, WarpMode::Rhythmic);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn the_audio_output_is_a_choice_and_nonsense_keeps_the_default() {
        let (settings, warnings) = Settings::from_text(
            "audio_host = WASAPI\naudio_device = Speakers (Interface)\naudio_sample_rate = 44100\naudio_buffer = 512\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let choice = settings.output_choice();
        assert_eq!(choice.host.as_deref(), Some("WASAPI"));
        assert_eq!(choice.device.as_deref(), Some("Speakers (Interface)"));
        assert_eq!((choice.sample_rate, choice.buffer_frames, choice.exclusive), (Some(44_100), Some(512), false));
        let mut back = Settings::default();
        back.set_output_choice(&choice);
        assert_eq!(back.output_choice(), choice);
        assert_eq!(Settings::default().output_choice(), suhina_session::OutputChoice::default());

        let (settings, warnings) = Settings::from_text(
            "audio_buffer = 17\naudio_sample_rate = lots\naudio_exclusive = maybe\naudio_cue_pair = 40\n",
        );
        assert_eq!(settings, Settings::default());
        assert_eq!(warnings.len(), 4);

        // The cue: its pair, and its volume within range, lowest is silence.
        let (settings, warnings) = Settings::from_text("audio_cue_pair = 1\naudio_cue_volume = 40\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        let choice = settings.output_choice();
        assert_eq!(choice.cue_pair, Some(1));
        assert!((choice.cue_gain - 10f32.powf(12.0 / 20.0)).abs() < 1e-5);
        let mut back = Settings::default();
        back.set_output_choice(&choice);
        assert_eq!((back.audio_cue_pair, back.audio_cue_volume), (1, 12));
        let silent = Settings::from_text("audio_cue_volume = -100\n").0.output_choice();
        assert_eq!(silent.cue_gain, 0.0);
    }

    #[test]
    fn a_map_window_out_of_bounds_or_misread_is_not_kept() {
        let (settings, warnings) = Settings::from_text(
            "map_window = on
map_window_rect = 10, 20, 800, 600
",
        );
        assert_eq!((settings.map_window, settings.map_window_rect), (true, Some([10, 20, 800, 600])));
        assert!(warnings.is_empty());
        for bad in ["10, 20, 800", "10, 20, 50, 600", "99999, 0, 800, 600", "a, b, c, d", "1,2,3,4,5"] {
            let (settings, warnings) = Settings::from_text(&format!(
                "map_window_rect = {bad}
"
            ));
            assert_eq!((settings.map_window_rect, warnings.len()), (None, 1), "{bad}");
        }
    }

    #[test]
    fn the_zoom_steps_within_its_limits_and_a_saved_one_is_limited_too() {
        let mut settings = Settings::default();
        assert!(settings.step_zoom(1));
        assert_eq!((settings.zoom, settings.zoom_factor()), (110, 1.1));
        assert!(settings.step_zoom(-3));
        assert_eq!(settings.zoom, 80);
        assert!(settings.step_zoom(-100));
        assert_eq!(settings.zoom, ZOOMS.0);
        assert!(!settings.step_zoom(-1), "already the smallest");
        assert!(settings.step_zoom(i32::MAX));
        assert_eq!(settings.zoom, ZOOMS.1);
        settings.zoom = 115;
        assert!(settings.step_zoom(1));
        assert_eq!(settings.zoom, 120, "onto the next step");
        settings.zoom = 115;
        assert!(settings.step_zoom(-1));
        assert_eq!(settings.zoom, 110);

        let (settings, warnings) = Settings::from_text(
            "zoom = 900
",
        );
        assert_eq!((settings.zoom, warnings.len()), (ZOOMS.1, 0));
        let (settings, warnings) = Settings::from_text(
            "zoom = big
",
        );
        assert_eq!((settings.zoom, warnings.len()), (ZOOM, 1));
    }

    #[test]
    fn unknown_lines_keep_the_defaults() {
        let (settings, warnings) = Settings::from_text("animations = sometimes\ncolour = pink\nnonsense\n\n# fine\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(warnings.len(), 3);
    }
}
