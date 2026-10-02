//! Tolerant `Settings.ini` parser for the RSDKv4/Origins asset tree.
//!
//! The assets shipped with Sonic 1/2 (Origins rev03) use lowercase keys in the `[Game]` and
//! `[Video]` sections, while RSDKv4-Decompilation's own `WriteSettings` emits `[Dev]`,
//! `[Game]`, `[Window]` and `[Keyboard 1]` sections with different capitalisation. This parser
//! accepts both: section names and keys are matched ASCII case-insensitively, comments
//! (`;`/`#`) and blank lines are skipped, unknown keys are retained in [`Settings::raw`] and
//! never cause an error, and integer values may be decimal or `0x` hexadecimal (the keyboard
//! maps use hex scancodes). Boolean values accept the `iniparser` spellings
//! `y/yes/t/true/1` and `n/no/f/false/0`.

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::Serialize;

use crate::error::FormatError;
use retro_io::DataSource;

/// Engine game release type, from `[Game] gameType`.
///
/// `gameType` controls script behaviour: `0` is a standalone/original release, `1` is the
/// Origins release used by this asset set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GameType {
    /// `gameType=0`
    Standalone,
    /// `gameType=1` (the default used by the Origins-era engine).
    #[default]
    Origins,
    /// Any other value.
    Other(i32),
}

impl GameType {
    /// Builds a [`GameType`] from the raw integer.
    pub fn from_id(id: i32) -> Self {
        match id {
            0 => Self::Standalone,
            1 => Self::Origins,
            other => Self::Other(other),
        }
    }

    /// The raw engine value.
    pub fn id(self) -> i32 {
        match self {
            Self::Standalone => 0,
            Self::Origins => 1,
            Self::Other(other) => other,
        }
    }
}

impl From<i32> for GameType {
    fn from(id: i32) -> Self {
        Self::from_id(id)
    }
}

impl From<GameType> for i32 {
    fn from(game_type: GameType) -> Self {
        game_type.id()
    }
}

impl PartialEq<i32> for GameType {
    fn eq(&self, other: &i32) -> bool {
        self.id() == *other
    }
}

/// `[Game]` section fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GameSettings {
    /// `dataFile`: pack file name (`Data.rsdk`).
    pub data_file: Option<String>,
    /// `devMenu` (or `[Dev] DevMenu`): enables the developer menu.
    pub dev_menu: bool,
    /// `faceButtonFlip`: swaps confirm/cancel face buttons.
    pub face_button_flip: bool,
    /// `enableControllerDebugging`: prints controller input to the log.
    pub enable_controller_debugging: bool,
    /// `disableFocusPause`; the older engine treats this as a 0..3 mode, the Origins engine as a
    /// boolean, so it is kept as an integer with `y`/`n` mapped to 1/0.
    pub disable_focus_pause: i32,
    /// `fastForwardSpeed`: multiplier applied while fast-forwarding.
    pub fast_forward_speed: i32,
    /// `region`: `-1` lets the game decide, `>= 0` forces a region.
    pub region: i32,
    /// `txtScripts`: forces legacy stages to load scripts instead of bytecode.
    pub txt_scripts: bool,
    /// `gameType`.
    pub game_type: GameType,
    /// `language`: `0` = EN, `1` = FR, ... per the engine language enum.
    pub language: i32,
    /// `gameLogic`: base name of the game logic script.
    pub game_logic: Option<String>,
    /// `username`: Origins profile name.
    pub username: Option<String>,
    /// `skipStartMenu`: disables the start menu.
    pub skip_start_menu: bool,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            data_file: None,
            dev_menu: false,
            face_button_flip: false,
            enable_controller_debugging: false,
            disable_focus_pause: 0,
            fast_forward_speed: 8,
            region: -1,
            txt_scripts: false,
            game_type: GameType::default(),
            language: 0,
            game_logic: None,
            username: None,
            skip_start_menu: false,
        }
    }
}

/// `[Video]` (Origins) / `[Window]` (decomp) section fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VideoSettings {
    /// `windowed` (inverse of the decomp's `[Window] FullScreen`).
    pub windowed: bool,
    /// `border` (inverse of the decomp's `[Window] Borderless`).
    pub border: bool,
    /// `exclusiveFS`: requests exclusive fullscreen.
    pub exclusive_fs: bool,
    /// `vsync`.
    pub vsync: bool,
    /// `tripleBuffering`.
    pub triple_buffering: bool,
    /// `pixWidth`: base render width in pixels (`424`).
    pub pix_width: i32,
    /// `winWidth`: windowed-mode width.
    pub win_width: i32,
    /// `winHeight`: windowed-mode height.
    pub win_height: i32,
    /// `fsWidth`: optional explicit fullscreen width (absent means desktop resolution).
    pub fs_width: Option<i32>,
    /// `fsHeight`: optional explicit fullscreen height.
    pub fs_height: Option<i32>,
    /// `refreshRate`: target FPS.
    pub refresh_rate: i32,
    /// `shaderSupport`.
    pub shader_support: bool,
    /// `screenShader`: shader id.
    pub screen_shader: i32,
    /// `maxPixWidth`: maximum allowed render width, `0` disables the limit.
    pub max_pix_width: i32,
    /// Decomp `[Window] ScalingMode`: `0` nearest neighbour, `1` linear.
    pub scaling_mode: i32,
    /// Decomp `[Window] WindowScale`.
    pub window_scale: i32,
    /// Decomp `[Window] DimLimit` in seconds, `-1` disables dimming.
    pub dim_limit: i32,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            windowed: true,
            border: true,
            exclusive_fs: false,
            vsync: false,
            triple_buffering: false,
            pix_width: 424,
            win_width: 424,
            win_height: 240,
            fs_width: None,
            fs_height: None,
            refresh_rate: 60,
            shader_support: true,
            screen_shader: 0,
            max_pix_width: 424,
            scaling_mode: 0,
            window_scale: 2,
            dim_limit: 300,
        }
    }
}

/// `[Audio]` section fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AudioSettings {
    /// `streamsEnabled`: enables streamed (music) audio.
    pub streams_enabled: bool,
    /// `streamVolume` (decomp alias `BGMVolume`), `0.0..=1.0`.
    pub stream_volume: f32,
    /// `sfxVolume` (decomp alias `SFXVolume`), `0.0..=1.0`.
    pub sfx_volume: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            streams_enabled: true,
            stream_volume: 0.8,
            sfx_volume: 1.0,
        }
    }
}

/// Scancodes for one player, from `[Keyboard Map 1..4]`.
///
/// Values are `SDL_Scancode` numbers, usually written in hexadecimal (`up=0x26`). A missing or
/// unparsable key is `None`. L/R buttons from the decomp settings are not exposed yet because
/// the Origins assets never define them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct KeyboardMap {
    /// `up`
    pub up: Option<i32>,
    /// `down`
    pub down: Option<i32>,
    /// `left`
    pub left: Option<i32>,
    /// `right`
    pub right: Option<i32>,
    /// `buttonA`
    pub button_a: Option<i32>,
    /// `buttonB`
    pub button_b: Option<i32>,
    /// `buttonC`
    pub button_c: Option<i32>,
    /// `buttonX`
    pub button_x: Option<i32>,
    /// `buttonY`
    pub button_y: Option<i32>,
    /// `buttonZ`
    pub button_z: Option<i32>,
    /// `start`
    pub start: Option<i32>,
    /// `select`
    pub select: Option<i32>,
}

/// Parsed `Settings.ini`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    /// `[Game]`/`[Dev]` section.
    pub game: GameSettings,
    /// `[Video]`/`[Window]` section.
    pub video: VideoSettings,
    /// `[Audio]` section.
    pub audio: AudioSettings,
    /// The four `[Keyboard Map 1..4]` sections, in order. Missing sections produce empty maps.
    pub keyboard_maps: Vec<KeyboardMap>,
    /// Every section/key/value as parsed, lowercased, including keys this parser does not model.
    #[serde(skip)]
    pub raw: BTreeMap<String, BTreeMap<String, String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            game: GameSettings::default(),
            video: VideoSettings::default(),
            audio: AudioSettings::default(),
            keyboard_maps: vec![KeyboardMap::default(); 4],
            raw: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// Canonical asset path passed to [`Settings::load`].
    pub const PATH: &str = "Settings.ini";

    /// Parses `Settings.ini` text. This never fails: malformed lines are ignored like
    /// `iniparser` does.
    ///
    /// [`std::str::FromStr`] is implemented for `Settings` and delegates to this method.
    pub fn parse(text: &str) -> Result<Self, FormatError> {
        let raw = parse_ini(text);
        Ok(Self::from_raw(raw))
    }

    /// Parses `Settings.ini` bytes. Invalid UTF-8 is rejected with [`FormatError::Invalid`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        let text = std::str::from_utf8(bytes).map_err(|error| {
            FormatError::invalid(format!("Settings.ini is not valid UTF-8: {error}"))
        })?;
        Self::parse(text)
    }

    /// Reads [`Settings::PATH`] through `src` and parses it.
    pub fn load(src: &dyn DataSource) -> Result<Self, FormatError> {
        Self::from_bytes(&src.read(Self::PATH)?)
    }

    /// Looks up a raw value by section and key, case-insensitively.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.raw
            .get(&section.to_ascii_lowercase())
            .and_then(|entries| entries.get(&key.to_ascii_lowercase()))
            .map(String::as_str)
    }

    fn from_raw(raw: BTreeMap<String, BTreeMap<String, String>>) -> Self {
        let lookup = |sections: &[&str], keys: &[&str]| -> Option<&str> {
            for section in sections {
                for key in keys {
                    if let Some(value) = raw.get(*section).and_then(|entries| entries.get(*key)) {
                        return Some(value);
                    }
                }
            }
            None
        };

        let mut keyboard_maps = Vec::with_capacity(4);
        for index in 1..=4 {
            let section = format!("keyboard map {index}");
            let legacy_section = format!("keyboard {index}");
            let key = |names: &[&str]| -> Option<i32> {
                parse_int(lookup(&[section.as_str(), legacy_section.as_str()], names))
            };
            keyboard_maps.push(KeyboardMap {
                up: key(&["up"]),
                down: key(&["down"]),
                left: key(&["left"]),
                right: key(&["right"]),
                button_a: key(&["buttona", "a"]),
                button_b: key(&["buttonb", "b"]),
                button_c: key(&["buttonc", "c"]),
                button_x: key(&["buttonx", "x"]),
                button_y: key(&["buttony", "y"]),
                button_z: key(&["buttonz", "z"]),
                start: key(&["start", "startbutton"]),
                select: key(&["select", "selectbutton"]),
            });
        }

        let windowed = match lookup(&["video"], &["windowed"]) {
            Some(value) => parse_bool(Some(value), true),
            None => !parse_bool(lookup(&["window"], &["fullscreen"]), false),
        };
        let border = match lookup(&["video"], &["border"]) {
            Some(value) => parse_bool(Some(value), true),
            None => !parse_bool(lookup(&["window"], &["borderless"]), false),
        };

        Self {
            game: GameSettings {
                data_file: lookup(&["game", "dev"], &["datafile"]).map(str::to_owned),
                dev_menu: parse_bool(lookup(&["game", "dev"], &["devmenu"]), false),
                face_button_flip: parse_bool(
                    lookup(&["game"], &["facebuttonflip", "confirmbuttonflip"]),
                    false,
                ),
                enable_controller_debugging: parse_bool(
                    lookup(&["game"], &["enablecontrollerdebugging"]),
                    false,
                ),
                disable_focus_pause: parse_int_or_bool(
                    lookup(&["game"], &["disablefocuspause"]),
                    0,
                ),
                fast_forward_speed: parse_int(lookup(&["game", "dev"], &["fastforwardspeed"]))
                    .unwrap_or(8),
                region: parse_int(lookup(&["game"], &["region"])).unwrap_or(-1),
                txt_scripts: parse_bool(lookup(&["game", "dev"], &["txtscripts"]), false),
                game_type: GameType::from_id(
                    parse_int(lookup(&["game"], &["gametype", "gamereleaseid"])).unwrap_or(1),
                ),
                language: parse_int(lookup(&["game"], &["language"])).unwrap_or(0),
                game_logic: lookup(&["game"], &["gamelogic"]).map(str::to_owned),
                username: lookup(&["game"], &["username"]).map(str::to_owned),
                skip_start_menu: parse_bool(lookup(&["game"], &["skipstartmenu"]), false),
            },
            video: VideoSettings {
                windowed,
                border,
                exclusive_fs: parse_bool(lookup(&["video"], &["exclusivefs"]), false),
                vsync: parse_bool(lookup(&["video", "window"], &["vsync"]), false),
                triple_buffering: parse_bool(lookup(&["video"], &["triplebuffering"]), false),
                pix_width: parse_int(lookup(&["video"], &["pixwidth"]))
                    .or_else(|| parse_int(lookup(&["window"], &["screenwidth"])))
                    .unwrap_or(424),
                win_width: parse_int(lookup(&["video"], &["winwidth"])).unwrap_or(424),
                win_height: parse_int(lookup(&["video"], &["winheight"])).unwrap_or(240),
                fs_width: parse_int(lookup(&["video"], &["fswidth"])),
                fs_height: parse_int(lookup(&["video"], &["fsheight"])),
                refresh_rate: parse_int(lookup(&["video", "window"], &["refreshrate"]))
                    .unwrap_or(60),
                shader_support: parse_bool(lookup(&["video"], &["shadersupport"]), true),
                screen_shader: parse_int(lookup(&["video"], &["screenshader"])).unwrap_or(0),
                max_pix_width: parse_int(lookup(&["video"], &["maxpixwidth"])).unwrap_or(424),
                scaling_mode: parse_int(lookup(&["window"], &["scalingmode"])).unwrap_or(0),
                window_scale: parse_int(lookup(&["window"], &["windowscale"])).unwrap_or(2),
                dim_limit: parse_int(lookup(&["window"], &["dimlimit"])).unwrap_or(300),
            },
            audio: AudioSettings {
                streams_enabled: parse_bool(lookup(&["audio"], &["streamsenabled"]), true),
                stream_volume: parse_float(lookup(&["audio"], &["streamvolume", "bgmvolume"]), 0.8),
                sfx_volume: parse_float(lookup(&["audio"], &["sfxvolume"]), 1.0),
            },
            keyboard_maps,
            raw,
        }
    }
}

impl FromStr for Settings {
    type Err = FormatError;

    /// Equivalent to [`Settings::parse`]; parsing is infallible for valid UTF-8 input because
    /// unknown keys and malformed lines are ignored like `iniparser` does.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

fn parse_ini(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(inner) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = inner.trim().to_ascii_lowercase();
            sections.entry(section.clone()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=').or_else(|| line.split_once(':')) else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        let value = value.trim().to_owned();
        sections
            .entry(section.clone())
            .or_default()
            .insert(key, value);
    }
    sections
}

fn parse_int(value: Option<&str>) -> Option<i32> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let (digits, radix, negative) = if let Some(rest) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        (rest, 16, false)
    } else if let Some(rest) = value
        .strip_prefix("-0x")
        .or_else(|| value.strip_prefix("-0X"))
    {
        (rest, 16, true)
    } else {
        (value, 10, false)
    };
    let magnitude = i64::from_str_radix(digits, radix).ok()?;
    let signed = if negative { -magnitude } else { magnitude };
    i32::try_from(signed).ok()
}

fn parse_bool(value: Option<&str>, default: bool) -> bool {
    match value.map(str::trim) {
        Some(value) => match value.to_ascii_lowercase().as_str() {
            "y" | "yes" | "t" | "true" | "1" | "on" => true,
            "n" | "no" | "f" | "false" | "0" | "off" => false,
            _ => default,
        },
        None => default,
    }
}

fn parse_int_or_bool(value: Option<&str>, default: i32) -> i32 {
    match value.map(str::trim) {
        Some(value) => match value.to_ascii_lowercase().as_str() {
            "y" | "yes" | "t" | "true" | "on" => 1,
            "n" | "no" | "f" | "false" | "off" => 0,
            _ => parse_int(Some(value)).unwrap_or(default),
        },
        None => default,
    }
}

fn parse_float(value: Option<&str>, default: f32) -> f32 {
    value
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_io::MemorySource;
    use std::str::FromStr;

    const ASSET_STYLE: &str = "; Retro Engine Config File\n\
\n\
[Game]\n\
dataFile=Data.rsdk\n\
devMenu=y\n\
faceButtonFlip=n\n\
enableControllerDebugging=n\n\
; a comment\n\
disableFocusPause=n\n\
fastForwardSpeed=8\n\
region=-1\n\
txtScripts=n\n\
gameType=1\n\
language=0\n\
\n\
[Video]\n\
windowed=y\n\
border=y\n\
exclusiveFS=y\n\
vsync=y\n\
tripleBuffering=n\n\
pixWidth=424\n\
winWidth=848\n\
winHeight=480\n\
refreshRate=60\n\
shaderSupport=y\n\
screenShader=0\n\
maxPixWidth=424\n\
\n\
[Audio]\n\
streamsEnabled=y\n\
streamVolume=1.000000\n\
sfxVolume=1.000000\n\
\n\
[Keyboard Map 1]\n\
up=0x26\n\
down=0x28\n\
left=0x25\n\
right=0x27\n\
buttonA=0x41\n\
buttonB=0x53\n\
buttonC=0x44\n\
buttonX=0x51\n\
buttonY=0x57\n\
buttonZ=0x45\n\
start=0xd\n\
select=0x9\n\
\n\
[Keyboard Map 2]\n\
up=0x68\n\
\n\
[Unknown Section]\n\
mystery=42\n";

    #[test]
    fn parses_asset_style_settings() {
        let settings = Settings::from_str(ASSET_STYLE).unwrap();
        assert_eq!(settings.game.data_file.as_deref(), Some("Data.rsdk"));
        assert!(settings.game.dev_menu);
        assert!(!settings.game.face_button_flip);
        assert!(!settings.game.enable_controller_debugging);
        assert_eq!(settings.game.disable_focus_pause, 0);
        assert_eq!(settings.game.fast_forward_speed, 8);
        assert_eq!(settings.game.region, -1);
        assert!(!settings.game.txt_scripts);
        assert_eq!(settings.game.game_type, GameType::Origins);
        assert_eq!(settings.game.game_type, 1);
        assert_eq!(settings.game.game_type.id(), 1);
        assert_eq!(settings.game.language, 0);

        assert!(settings.video.windowed);
        assert!(settings.video.border);
        assert!(settings.video.exclusive_fs);
        assert!(settings.video.vsync);
        assert!(!settings.video.triple_buffering);
        assert_eq!(settings.video.pix_width, 424);
        assert_eq!(settings.video.win_width, 848);
        assert_eq!(settings.video.win_height, 480);
        assert_eq!(settings.video.refresh_rate, 60);
        assert!(settings.video.shader_support);
        assert_eq!(settings.video.screen_shader, 0);
        assert_eq!(settings.video.max_pix_width, 424);

        assert!(settings.audio.streams_enabled);
        assert_eq!(settings.audio.stream_volume, 1.0);
        assert_eq!(settings.audio.sfx_volume, 1.0);

        assert_eq!(settings.keyboard_maps.len(), 4);
        assert_eq!(settings.keyboard_maps[0].up, Some(0x26));
        assert_eq!(settings.keyboard_maps[0].down, Some(0x28));
        assert_eq!(settings.keyboard_maps[0].button_a, Some(0x41));
        assert_eq!(settings.keyboard_maps[0].start, Some(0x0D));
        assert_eq!(settings.keyboard_maps[0].select, Some(0x09));
        assert_eq!(settings.keyboard_maps[1].up, Some(0x68));
        assert_eq!(settings.keyboard_maps[1].down, None);
        assert_eq!(settings.keyboard_maps[2], KeyboardMap::default());
        assert_eq!(settings.keyboard_maps[3], KeyboardMap::default());

        assert_eq!(settings.get("Unknown Section", "mystery"), Some("42"));
        assert_eq!(settings.get("unknown SECTION", "MYSTERY"), Some("42"));
    }

    #[test]
    fn empty_input_yields_defaults() {
        let settings = Settings::from_str("").unwrap();
        let via_trait: Settings = "".parse().unwrap();
        assert_eq!(settings, via_trait);
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.game.game_type, GameType::Origins);
        assert!(settings.video.windowed);
        assert_eq!(settings.video.pix_width, 424);
        assert_eq!(settings.audio.stream_volume, 0.8);
    }

    #[test]
    fn keys_and_sections_are_case_insensitive() {
        let settings = Settings::from_str(
            "[gAmE]\n\
             DATAFILE=Custom.rsdk\n\
             DEVmenu=YES\n\
             TXTscripts=TRUE\n\
             GAMETYPE=0\n\
             LANGUAGE=5\n\
             [vIdEo]\n\
             PIXWIDTH=320\n\
             WINDOWED=No\n",
        )
        .unwrap();
        assert_eq!(settings.game.data_file.as_deref(), Some("Custom.rsdk"));
        assert!(settings.game.dev_menu);
        assert!(settings.game.txt_scripts);
        assert_eq!(settings.game.game_type, GameType::Standalone);
        assert_eq!(settings.game.language, 5);
        assert_eq!(settings.video.pix_width, 320);
        assert!(!settings.video.windowed);
    }

    #[test]
    fn supports_legacy_decomp_sections() {
        let settings = Settings::from_str(
            "[Dev]\n\
             DevMenu=y\n\
             TxtScripts=y\n\
             DataFile=Legacy.rsdk\n\
             [Game]\n\
             Language=2\n\
             GameType=0\n\
             [Window]\n\
             FullScreen=y\n\
             Borderless=y\n\
             VSync=y\n\
             ScalingMode=1\n\
             WindowScale=3\n\
             ScreenWidth=512\n\
             RefreshRate=50\n\
             DimLimit=-1\n\
             [Audio]\n\
             BGMVolume=0.5\n\
             SFXVolume=0.25\n\
             [Keyboard 1]\n\
             Up=200\n\
             A=-1\n\
             Start=13\n",
        )
        .unwrap();
        assert!(settings.game.dev_menu);
        assert!(settings.game.txt_scripts);
        assert_eq!(settings.game.data_file.as_deref(), Some("Legacy.rsdk"));
        assert_eq!(settings.game.language, 2);
        assert_eq!(settings.game.game_type, GameType::Standalone);
        assert!(!settings.video.windowed);
        assert!(!settings.video.border);
        assert!(settings.video.vsync);
        assert_eq!(settings.video.scaling_mode, 1);
        assert_eq!(settings.video.window_scale, 3);
        assert_eq!(settings.video.pix_width, 512);
        assert_eq!(settings.video.refresh_rate, 50);
        assert_eq!(settings.video.dim_limit, -1);
        assert_eq!(settings.audio.stream_volume, 0.5);
        assert_eq!(settings.audio.sfx_volume, 0.25);
        assert_eq!(settings.keyboard_maps[0].up, Some(200));
        assert_eq!(settings.keyboard_maps[0].button_a, Some(-1));
        assert_eq!(settings.keyboard_maps[0].start, Some(13));
    }

    #[test]
    fn malformed_lines_and_values_are_tolerated() {
        let settings = Settings::from_str(
            "not a section\n\
             =missing key\n\
             [Game]\n\
             dataFile\n\
             devMenu=maybe\n\
             fastForwardSpeed=abc\n\
             region=0x10\n\
             language=\n\
             [Video]\n\
             pixWidth=0oops\n\
             streamVolume=nope\n\
             [Audio]\n\
             streamVolume=NaN\n",
        )
        .unwrap();
        assert_eq!(settings.game.data_file, None);
        assert!(!settings.game.dev_menu);
        assert_eq!(settings.game.fast_forward_speed, 8);
        assert_eq!(settings.game.region, 16);
        assert_eq!(settings.game.language, 0);
        assert_eq!(settings.video.pix_width, 424);
        assert_eq!(settings.audio.stream_volume, 0.8);
    }

    #[test]
    fn integer_edge_cases() {
        let settings = Settings::from_str(
            "[Game]\n\
             fastForwardSpeed=-0x10\n\
             region=2147483647\n\
             language=2147483648\n\
             [Video]\n\
             pixWidth=0xFFFFFFFF\n",
        )
        .unwrap();
        assert_eq!(settings.game.fast_forward_speed, -16);
        assert_eq!(settings.game.region, i32::MAX);
        assert_eq!(settings.game.language, 0);
        assert_eq!(settings.video.pix_width, 424);
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert!(matches!(
            Settings::from_bytes(&[b'[', b'G', b'a', b'm', b'e', b']', 0xFF]),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn loads_through_data_source() {
        let mut source = MemorySource::new();
        source.insert("Settings.ini", ASSET_STYLE);
        let settings = Settings::load(&source).unwrap();
        assert_eq!(settings.game.game_type, GameType::Origins);
        assert!(matches!(
            Settings::load(&MemorySource::new()),
            Err(FormatError::Io(_))
        ));
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        let mut state = 0xC0FF_EE00u32;
        for length in 0..256usize {
            let text: String = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    match (state >> 24) & 0x0F {
                        0 => '[',
                        1 => ']',
                        2 => '=',
                        3 => '\n',
                        4 => ';',
                        value => char::from(b'0' + value as u8),
                    }
                })
                .collect();
            let _ = Settings::from_str(&text);
        }
    }
}
