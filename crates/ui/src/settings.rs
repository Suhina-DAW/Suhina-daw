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
    /// In a window of its own, owned by Loopwerk's: above it, and minimised with it.
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
        }
    }
}

impl Settings {
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
            "# Loopwerk settings; changed from Options > Settings.\nanimations = {}\ntrack_headers = {headers}\nrenderer = {graphics}\nsample_stacking = {}\nmidi_stacking = {}\nopen_projects = {on_open}\nstop = {stop}\ndevice_chain = {devices}\nplugin_editors = {editors}\ntheme = {}\nprojects_folder = {}\ntemplates_folder = {}\n",
            on(self.animations),
            on(self.sample_stacking),
            on(self.midi_stacking),
            self.theme,
            self.projects_folder,
            self.templates_folder,
            stop = self.stop.param(),
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
                _ => {
                    warnings.push(format!("settings line {}: {key} = {value} is not a known setting", number + 1));
                }
            }
        }
        (settings, warnings)
    }
}

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
        };
        assert_eq!(Settings::from_text(&settings.to_text()), (settings, Vec::new()));
        assert_eq!(Settings::from_text(&Settings::default().to_text()).0, Settings::default());
    }

    #[test]
    fn unknown_lines_keep_the_defaults() {
        let (settings, warnings) = Settings::from_text("animations = sometimes\ncolour = pink\nnonsense\n\n# fine\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(warnings.len(), 3);
    }
}
