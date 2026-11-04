//! Adding and editing the sources an IRL scene is built from, from the
//! dashboard: media sources (`ffmpeg_source`), VLC video sources
//! (`vlc_source`, when OBS has the VLC plugin), browser sources and text
//! sources (GDI+ on Windows, FreeType 2 elsewhere: one set of settings for
//! both), plus moving any item up or down its scene or group.
//!
//! OBS names some kinds with a version (`text_gdiplus_v3`): they're matched
//! without it, and new sources get the newest version OBS has.
//!
//! Only the settings below can be set, each checked, never raw OBS settings:
//! what a source plays must be a network URL, so nobody can put a file from the
//! streamer's computer on stream (or in the preview). A source that already
//! plays one keeps it unless it's replaced with a URL; its path isn't shown.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    ClientTransport, Error, ObsClient,
    error::Result,
    placement::{Anchor, Fit, Placement},
    types::{
        CreateInput, GetInputDefaultSettings, GetInputKindList, GetInputSettings, RequestKind,
    },
};

/// The longest source name, in characters.
pub const MAX_NAME_CHARS: usize = 256;
pub const MAX_URL_CHARS: usize = 4096;
pub const MAX_CSS_CHARS: usize = 20_000;
pub const MAX_PLAYLIST: usize = 50;
/// A browser source's size, in pixels (OBS's own limits).
pub const MAX_BROWSER_SIZE: u32 = 8192;
pub const MAX_BROWSER_FPS: u32 = 60;
/// A media source's network buffering, MB.
pub const MAX_BUFFERING_MB: u32 = 16;
/// A media source's reconnect delay, seconds.
pub const RECONNECT_DELAY_SEC: std::ops::RangeInclusive<u32> = 1..=60;
/// VLC's network caching, ms.
pub const NETWORK_CACHING_MS: std::ops::RangeInclusive<u32> = 100..=60_000;
pub const MAX_TEXT_CHARS: usize = 10_000;
pub const MAX_FONT_FACE_CHARS: usize = 128;
/// A text source's font size, in points.
pub const FONT_SIZE: std::ops::RangeInclusive<u32> = 1..=1024;

/// OBS's font flags.
const FONT_BOLD: i64 = 1;
const FONT_ITALIC: i64 = 2;

/// What media and VLC sources may play: streams, never files.
const STREAM_SCHEMES: [&str; 11] = [
    "srt", "rist", "rtmp", "rtmps", "rtsp", "rtsps", "rtp", "udp", "tcp", "http", "https",
];
/// What browser sources may show.
const WEB_SCHEMES: [&str; 2] = ["http", "https"];

/// A kind of source that can be added and edited here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum EditableKind {
    /// OBS's Media Source.
    Media,
    /// VLC Video Source (needs OBS's VLC plugin).
    Vlc,
    Browser,
    /// Text (GDI+) on Windows, Text (FreeType 2) elsewhere.
    Text,
}

impl EditableKind {
    pub const ALL: [Self; 4] = [Self::Media, Self::Vlc, Self::Browser, Self::Text];

    /// Where a new one goes: feeds and pages fill the screen (keeping their
    /// shape, whatever their resolution), text sits in the bottom left.
    pub fn default_placement(self) -> Placement {
        let (fit, anchor) = match self {
            Self::Text => (Fit::Original, Anchor::BottomLeft),
            _ => (Fit::Fit, Anchor::Center),
        };
        Placement::Preset {
            fit,
            anchor,
            size_percent: 100,
            margin_percent: 3,
        }
    }

    /// OBS's kinds for it, unversioned, the one to add first.
    fn obs_kinds(self) -> &'static [&'static str] {
        match self {
            Self::Media => &["ffmpeg_source"],
            Self::Vlc => &["vlc_source"],
            Self::Browser => &["browser_source"],
            Self::Text => &[GDIPLUS, FREETYPE],
        }
    }

    /// What an input of OBS kind `kind` (versioned or not) is here.
    pub fn from_input_kind(kind: &str) -> Option<Self> {
        let kind = unversioned(kind);
        Self::ALL
            .into_iter()
            .find(|editable| editable.obs_kinds().contains(&kind))
    }

    /// The OBS kind to add one as, from OBS's (versioned) kinds: the first of
    /// [`Self::obs_kinds`] it has, in its newest version.
    fn pick(self, available: &[String]) -> Option<&str> {
        self.obs_kinds().iter().find_map(|wanted| {
            available
                .iter()
                .filter(|kind| unversioned(kind) == *wanted)
                .max_by_key(|kind| version(kind))
                .map(String::as_str)
        })
    }
}

const GDIPLUS: &str = "text_gdiplus";
const FREETYPE: &str = "text_ft2_source";

/// `text_gdiplus_v3` → `text_gdiplus`.
fn unversioned(kind: &str) -> &str {
    match kind.rsplit_once("_v") {
        Some((base, number))
            if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) =>
        {
            base
        }
        _ => kind,
    }
}

/// `text_gdiplus_v3` → 3; unversioned kinds are version 1.
fn version(kind: &str) -> u32 {
    kind.strip_prefix(unversioned(kind))
        .and_then(|rest| rest.strip_prefix("_v"))
        .and_then(|number| number.parse().ok())
        .unwrap_or(1)
}

/// A source's settings, by kind. Reading gives every field; when writing, a
/// `None` URL or playlist keeps what the source plays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceSettings {
    Media(MediaSettings),
    Vlc(VlcSettings),
    Browser(BrowserSettings),
    Text(TextSettings),
}

impl SourceSettings {
    pub fn kind(&self) -> EditableKind {
        match self {
            Self::Media(_) => EditableKind::Media,
            Self::Vlc(_) => EditableKind::Vlc,
            Self::Browser(_) => EditableKind::Browser,
            Self::Text(_) => EditableKind::Text,
        }
    }
}

/// What GDI+ (Windows) and FreeType 2 (Linux, macOS) text sources share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TextSettings {
    /// What it says. Read as `null` while it reads a file on the OBS
    /// computer; writing text stops that.
    pub text: Option<String>,
    /// The font, as the OBS computer names it. Empty keeps the font it has.
    pub font_face: String,
    /// 1 to 1024.
    pub font_size: u32,
    pub bold: bool,
    pub italic: bool,
    /// `#rrggbb`. A FreeType 2 source's gradient is kept unless it's changed.
    pub color: String,
    pub outline: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MediaSettings {
    /// The stream it plays (`srt://…`, `rtmp://…`, …). Read as `null` when it
    /// plays something else (a file on the OBS computer).
    pub url: Option<String>,
    /// ffmpeg's input format (e.g. `mpegts`); empty to detect it.
    pub input_format: String,
    /// Network buffering, 0 to 16 MB.
    pub buffering_mb: u32,
    /// Seconds before reconnecting after the stream drops, 1 to 60.
    pub reconnect_delay_sec: u32,
    /// Decode on the GPU when it can.
    pub hw_decode: bool,
    /// Restart playback when the source becomes active.
    pub restart_on_activate: bool,
    /// Close the stream while the source isn't shown.
    pub close_when_inactive: bool,
    /// Show nothing once playback ends (rather than the last frame).
    pub clear_on_media_end: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VlcSettings {
    /// The streams it plays, in order. Read as `null` when it plays
    /// something else (files or folders on the OBS computer).
    pub playlist: Option<Vec<String>>,
    /// Start the playlist over at its end.
    pub loop_playlist: bool,
    pub shuffle: bool,
    pub playback: VlcPlayback,
    /// Network caching, 100 to 60,000 ms.
    pub network_caching_ms: u32,
}

/// What a VLC source does while it isn't shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum VlcPlayback {
    /// Stop, and restart when shown (OBS's default).
    StopRestart,
    PauseUnpause,
    AlwaysPlay,
}

impl VlcPlayback {
    fn as_obs(self) -> &'static str {
        match self {
            Self::StopRestart => "stop_restart",
            Self::PauseUnpause => "pause_unpause",
            Self::AlwaysPlay => "always_play",
        }
    }

    fn from_obs(value: &str) -> Self {
        match value {
            "pause_unpause" => Self::PauseUnpause,
            "always_play" => Self::AlwaysPlay,
            _ => Self::StopRestart,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BrowserSettings {
    /// The page it shows. Read as `null` when it shows a file on the OBS computer.
    pub url: Option<String>,
    /// 1 to 8192 px.
    pub width: u32,
    pub height: u32,
    /// A frame rate of its own, 1 to 60; `null` to match OBS's.
    pub fps: Option<u32>,
    /// CSS added to the page.
    pub css: String,
    /// Shut the page down while the source isn't shown.
    pub shutdown: bool,
    /// Reload the page when its scene goes on air.
    pub restart_when_active: bool,
    /// Send the page's audio through OBS (it then shows on the Audio Mixer).
    pub reroute_audio: bool,
}

/// What's wrong with a source's name or settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    #[error(
        "a source needs a name of at most {MAX_NAME_CHARS} characters, without control characters"
    )]
    Name,
    #[error("a new source needs a URL")]
    MissingUrl,
    #[error("media and VLC sources play stream URLs (srt://, rtmp://, https://, …)")]
    StreamUrl,
    #[error("browser sources show http:// or https:// pages")]
    WebUrl,
    #[error("a VLC source plays 1 to {MAX_PLAYLIST} URLs")]
    Playlist,
    #[error("network buffering is 0 to {MAX_BUFFERING_MB} MB")]
    Buffering,
    #[error("the reconnect delay is 1 to 60 seconds")]
    ReconnectDelay,
    #[error("the input format is letters, digits and _ only")]
    InputFormat,
    #[error("network caching is 100 to 60,000 ms")]
    NetworkCaching,
    #[error("a browser source is 1 to {MAX_BROWSER_SIZE} pixels wide and high")]
    BrowserSize,
    #[error("a browser source's frame rate is 1 to {MAX_BROWSER_FPS}")]
    BrowserFps,
    #[error("custom CSS is at most {MAX_CSS_CHARS} characters")]
    Css,
    #[error("a text source says at most {MAX_TEXT_CHARS} characters")]
    Text,
    #[error("a font's name is at most {MAX_FONT_FACE_CHARS} characters")]
    FontFace,
    #[error("the font size is 1 to 1024")]
    FontSize,
    #[error("a color is written #rrggbb")]
    Color,
}

/// `#rrggbb` as OBS stores colors (`0xAABBGGRR`), opaque.
fn obs_color(hex: &str) -> Option<i64> {
    let digits = hex.strip_prefix('#').filter(|digits| digits.len() == 6)?;
    let rgb = u32::from_str_radix(digits, 16).ok()?;
    let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    Some(i64::from(0xff00_0000 | b << 16 | g << 8 | r))
}

/// An OBS color as `#rrggbb` (its alpha left out).
fn hex_color(value: &Value) -> String {
    let abgr = value.as_i64().unwrap_or(0xffff_ffff);
    let (r, g, b) = (abgr & 0xff, (abgr >> 8) & 0xff, (abgr >> 16) & 0xff);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// A source's name, trimmed, or why it can't be one.
pub fn clean_name(name: &str) -> std::result::Result<&str, Invalid> {
    let name = name.trim();
    let fine = !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.chars().any(char::is_control);
    fine.then_some(name).ok_or(Invalid::Name)
}

fn url_allowed(url: &str, schemes: &[&str]) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    url.len() <= MAX_URL_CHARS
        && !rest.is_empty()
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
        && schemes.contains(&scheme.to_ascii_lowercase().as_str())
}

fn check_url(url: &str, schemes: &[&str], invalid: Invalid) -> std::result::Result<(), Invalid> {
    url_allowed(url, schemes).then_some(()).ok_or(invalid)
}

impl SourceSettings {
    /// Checks every field; a new source (`creating`) must say what it plays.
    pub fn check(&self, creating: bool) -> std::result::Result<(), Invalid> {
        match self {
            Self::Media(media) => {
                match &media.url {
                    Some(url) => check_url(url, &STREAM_SCHEMES, Invalid::StreamUrl)?,
                    None if creating => return Err(Invalid::MissingUrl),
                    None => {}
                }
                if media.buffering_mb > MAX_BUFFERING_MB {
                    return Err(Invalid::Buffering);
                }
                if !RECONNECT_DELAY_SEC.contains(&media.reconnect_delay_sec) {
                    return Err(Invalid::ReconnectDelay);
                }
                let format_fine = media.input_format.len() <= 64
                    && media
                        .input_format
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_');
                if !format_fine {
                    return Err(Invalid::InputFormat);
                }
            }
            Self::Vlc(vlc) => {
                match &vlc.playlist {
                    Some(urls) => {
                        if urls.is_empty() || urls.len() > MAX_PLAYLIST {
                            return Err(Invalid::Playlist);
                        }
                        for url in urls {
                            check_url(url, &STREAM_SCHEMES, Invalid::StreamUrl)?;
                        }
                    }
                    None if creating => return Err(Invalid::MissingUrl),
                    None => {}
                }
                if !NETWORK_CACHING_MS.contains(&vlc.network_caching_ms) {
                    return Err(Invalid::NetworkCaching);
                }
            }
            Self::Browser(browser) => {
                match &browser.url {
                    Some(url) => check_url(url, &WEB_SCHEMES, Invalid::WebUrl)?,
                    None if creating => return Err(Invalid::MissingUrl),
                    None => {}
                }
                let size = 1..=MAX_BROWSER_SIZE;
                if !size.contains(&browser.width) || !size.contains(&browser.height) {
                    return Err(Invalid::BrowserSize);
                }
                if browser
                    .fps
                    .is_some_and(|fps| !(1..=MAX_BROWSER_FPS).contains(&fps))
                {
                    return Err(Invalid::BrowserFps);
                }
                if browser.css.chars().count() > MAX_CSS_CHARS {
                    return Err(Invalid::Css);
                }
            }
            Self::Text(text) => {
                let says_fine = text.text.as_deref().is_none_or(|says| {
                    says.chars().count() <= MAX_TEXT_CHARS
                        && !says
                            .chars()
                            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                });
                if !says_fine {
                    return Err(Invalid::Text);
                }
                let face = text.font_face.trim();
                if face.chars().count() > MAX_FONT_FACE_CHARS || face.chars().any(char::is_control)
                {
                    return Err(Invalid::FontFace);
                }
                if !FONT_SIZE.contains(&text.font_size) {
                    return Err(Invalid::FontSize);
                }
                if obs_color(&text.color).is_none() {
                    return Err(Invalid::Color);
                }
            }
        }
        Ok(())
    }

    /// The OBS settings to write (on top of the source's own) for an input of
    /// OBS kind `input_kind`, whose settings now (or its kind's defaults, for
    /// a new one) are `base`. Call [`Self::check`] first.
    pub fn to_obs(&self, input_kind: &str, base: &Value) -> Value {
        let mut out = Map::new();
        match self {
            Self::Media(media) => {
                if let Some(url) = &media.url {
                    out.insert("is_local_file".into(), false.into());
                    out.insert("input".into(), url.as_str().into());
                }
                out.insert("input_format".into(), media.input_format.as_str().into());
                out.insert("buffering_mb".into(), media.buffering_mb.into());
                out.insert(
                    "reconnect_delay_sec".into(),
                    media.reconnect_delay_sec.into(),
                );
                out.insert("hw_decode".into(), media.hw_decode.into());
                out.insert(
                    "restart_on_activate".into(),
                    media.restart_on_activate.into(),
                );
                out.insert(
                    "close_when_inactive".into(),
                    media.close_when_inactive.into(),
                );
                out.insert("clear_on_media_end".into(), media.clear_on_media_end.into());
            }
            Self::Vlc(vlc) => {
                if let Some(urls) = &vlc.playlist {
                    let playlist: Vec<Value> = urls
                        .iter()
                        .map(|url| json!({ "value": url, "hidden": false, "selected": false }))
                        .collect();
                    out.insert("playlist".into(), playlist.into());
                }
                out.insert("loop".into(), vlc.loop_playlist.into());
                out.insert("shuffle".into(), vlc.shuffle.into());
                out.insert("playback_behavior".into(), vlc.playback.as_obs().into());
                out.insert("network_caching".into(), vlc.network_caching_ms.into());
            }
            Self::Browser(browser) => {
                if let Some(url) = &browser.url {
                    out.insert("is_local_file".into(), false.into());
                    out.insert("url".into(), url.as_str().into());
                }
                out.insert("width".into(), browser.width.into());
                out.insert("height".into(), browser.height.into());
                out.insert("fps_custom".into(), browser.fps.is_some().into());
                if let Some(fps) = browser.fps {
                    out.insert("fps".into(), fps.into());
                }
                out.insert("css".into(), browser.css.as_str().into());
                out.insert("shutdown".into(), browser.shutdown.into());
                out.insert(
                    "restart_when_active".into(),
                    browser.restart_when_active.into(),
                );
                out.insert("reroute_audio".into(), browser.reroute_audio.into());
            }
            Self::Text(text) => {
                let freetype = unversioned(input_kind) == FREETYPE;
                if let Some(says) = &text.text {
                    out.insert("text".into(), says.as_str().into());
                    let from_file = if freetype {
                        "from_file"
                    } else {
                        "read_from_file"
                    };
                    out.insert(from_file.into(), false.into());
                }

                // OBS replaces the whole font, so start from the one it has.
                let mut font = base["font"].as_object().cloned().unwrap_or_default();
                let face = text.font_face.trim();
                if !face.is_empty() {
                    font.insert("face".into(), face.into());
                }
                font.insert("size".into(), text.font_size.into());
                let others = font.get("flags").and_then(Value::as_i64).unwrap_or(0)
                    & !(FONT_BOLD | FONT_ITALIC);
                let flags = others
                    | if text.bold { FONT_BOLD } else { 0 }
                    | if text.italic { FONT_ITALIC } else { 0 };
                font.insert("flags".into(), flags.into());
                let style = match (text.bold, text.italic) {
                    (false, false) => "Regular",
                    (true, false) => "Bold",
                    (false, true) => "Italic",
                    (true, true) => "Bold Italic",
                };
                font.insert("style".into(), style.into());
                out.insert("font".into(), Value::Object(font));

                let color = obs_color(&text.color).unwrap_or(0xffff_ffff);
                if freetype {
                    // Its color is a gradient from color1 to color2.
                    if hex_color(&base["color1"]) != text.color.to_ascii_lowercase() {
                        out.insert("color1".into(), color.into());
                        out.insert("color2".into(), color.into());
                    }
                } else {
                    out.insert("color".into(), color.into());
                }
                out.insert("outline".into(), text.outline.into());
            }
        }
        Value::Object(out)
    }

    /// Reads the settings of an input of OBS kind `input_kind` (its own merged
    /// over its kind's defaults). Whatever isn't a network URL comes out as
    /// `None`, unshown, like the text of a text source reading a file.
    pub fn from_obs(kind: EditableKind, input_kind: &str, settings: &Value) -> Self {
        let text = |key: &str| settings[key].as_str().unwrap_or_default().to_owned();
        let flag = |key: &str, default: bool| settings[key].as_bool().unwrap_or(default);
        let number = |key: &str, default: u32| {
            settings[key]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(default)
        };
        let url = |key: &str, schemes: &[&str]| {
            let url = text(key);
            (!flag("is_local_file", false) && url_allowed(&url, schemes)).then_some(url)
        };

        match kind {
            EditableKind::Media => Self::Media(MediaSettings {
                // OBS's default is a local file.
                url: (!flag("is_local_file", true))
                    .then(|| text("input"))
                    .filter(|url| url_allowed(url, &STREAM_SCHEMES)),
                input_format: text("input_format"),
                buffering_mb: number("buffering_mb", 2),
                reconnect_delay_sec: number("reconnect_delay_sec", 10),
                hw_decode: flag("hw_decode", false),
                restart_on_activate: flag("restart_on_activate", true),
                close_when_inactive: flag("close_when_inactive", false),
                clear_on_media_end: flag("clear_on_media_end", true),
            }),
            EditableKind::Vlc => {
                let entries: Vec<String> = settings["playlist"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .map(|item| item["value"].as_str().unwrap_or_default().to_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                let streams_only = entries
                    .iter()
                    .all(|entry| url_allowed(entry, &STREAM_SCHEMES));
                Self::Vlc(VlcSettings {
                    playlist: streams_only.then_some(entries),
                    loop_playlist: flag("loop", true),
                    shuffle: flag("shuffle", false),
                    playback: VlcPlayback::from_obs(&text("playback_behavior")),
                    network_caching_ms: number("network_caching", 400),
                })
            }
            EditableKind::Browser => Self::Browser(BrowserSettings {
                url: url("url", &WEB_SCHEMES),
                width: number("width", 800),
                height: number("height", 600),
                fps: flag("fps_custom", false).then(|| number("fps", 30)),
                css: text("css"),
                shutdown: flag("shutdown", false),
                restart_when_active: flag("restart_when_active", false),
                reroute_audio: flag("reroute_audio", false),
            }),
            EditableKind::Text => {
                let freetype = unversioned(input_kind) == FREETYPE;
                let from_file = flag(
                    if freetype {
                        "from_file"
                    } else {
                        "read_from_file"
                    },
                    false,
                );
                let font = &settings["font"];
                let flags = font["flags"].as_i64().unwrap_or(0);
                Self::Text(TextSettings {
                    text: (!from_file).then(|| text("text")),
                    font_face: font["face"].as_str().unwrap_or_default().to_owned(),
                    font_size: font["size"]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .unwrap_or(256),
                    bold: flags & FONT_BOLD != 0,
                    italic: flags & FONT_ITALIC != 0,
                    color: hex_color(&settings[if freetype { "color1" } else { "color" }]),
                    outline: flag("outline", false),
                })
            }
        }
    }
}

/// An editable source as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct InputDetails {
    pub name: String,
    pub settings: SourceSettings,
}

/// OBS's answer when a name is taken (by any source or scene).
pub const RESOURCE_ALREADY_EXISTS: u32 = 601;

impl<CT: ClientTransport> ObsClient<CT> {
    /// OBS's input kinds, versioned (`text_gdiplus_v3`).
    async fn input_kinds(&self) -> Result<Vec<String>> {
        let list: GetInputKindList = self
            .send_request(RequestKind::GetInputKindList { unversioned: false })
            .await?;
        Ok(list.input_kinds)
    }

    async fn default_settings(&self, input_kind: &str) -> Result<Value> {
        let defaults: GetInputDefaultSettings = self
            .send_request(RequestKind::GetInputDefaultSettings { input_kind })
            .await?;
        Ok(defaults.default_input_settings)
    }

    /// The kinds this OBS can add (VLC needs its plugin).
    pub async fn editable_kinds(&self) -> Result<Vec<EditableKind>> {
        let available = self.input_kinds().await?;
        Ok(EditableKind::ALL
            .into_iter()
            .filter(|kind| kind.pick(&available).is_some())
            .collect())
    }

    /// An input's OBS kind and its settings over its kind's defaults (OBS
    /// only gives what differs). Objects, like a text source's font, are merged too.
    async fn current_settings(&self, name: &str) -> Result<(String, Value)> {
        let current: GetInputSettings = self
            .send_request(RequestKind::GetInputSettings { input_name: name })
            .await?;
        let mut merged = self.default_settings(&current.input_kind).await?;
        if let (Some(merged), Value::Object(own)) = (merged.as_object_mut(), current.input_settings)
        {
            for (key, value) in own {
                match (merged.get_mut(&key), value) {
                    (Some(Value::Object(default)), Value::Object(own)) => default.extend(own),
                    (_, value) => {
                        merged.insert(key, value);
                    }
                }
            }
        }
        Ok((current.input_kind, merged))
    }

    /// An input's settings, if it's a kind that can be edited here (`None`
    /// otherwise).
    pub async fn input_details(&self, name: &str) -> Result<Option<InputDetails>> {
        let (input_kind, settings) = self.current_settings(name).await?;
        Ok(
            EditableKind::from_input_kind(&input_kind).map(|kind| InputDetails {
                name: name.to_owned(),
                settings: SourceSettings::from_obs(kind, &input_kind, &settings),
            }),
        )
    }

    /// Adds a new source to `scene` (not a group: OBS can't), shown or not.
    /// `Ok(None)` if this OBS doesn't have that kind.
    pub async fn create_input(
        &self,
        scene: &str,
        name: &str,
        settings: &SourceSettings,
        enabled: bool,
    ) -> Result<Option<CreateInput>> {
        let available = self.input_kinds().await?;
        let Some(input_kind) = settings.kind().pick(&available) else {
            return Ok(None);
        };
        let defaults = self.default_settings(input_kind).await?;
        let created = self
            .send_request(RequestKind::CreateInput {
                scene_name: scene,
                input_name: name,
                input_kind,
                input_settings: settings.to_obs(input_kind, &defaults),
                scene_item_enabled: enabled,
            })
            .await?;
        Ok(Some(created))
    }

    /// Writes `settings` over an input's own (what isn't set here stays).
    /// `Ok(false)`, changing nothing, if the input isn't of their kind.
    pub async fn update_input(&self, name: &str, settings: &SourceSettings) -> Result<bool> {
        let (input_kind, current) = self.current_settings(name).await?;
        if EditableKind::from_input_kind(&input_kind) != Some(settings.kind()) {
            return Ok(false);
        }
        self.command(RequestKind::SetInputSettings {
            input_name: name,
            input_settings: settings.to_obs(&input_kind, &current),
        })
        .await?;
        Ok(true)
    }

    pub async fn rename_input(&self, name: &str, new_name: &str) -> Result<()> {
        self.command(RequestKind::SetInputName {
            input_name: name,
            new_input_name: new_name,
        })
        .await
    }

    /// Moves item `item_id` of `scene` (a group when `group`) to `position`
    /// among its items, counted from the top like OBS's Sources dock (0: drawn
    /// over all the others); past the end means the bottom. `Ok(None)` if it's
    /// gone, `Ok(Some(false))` if it's there already.
    pub async fn move_scene_item(
        &self,
        scene: &str,
        group: bool,
        item_id: i64,
        position: usize,
    ) -> Result<Option<bool>> {
        // Bottom first: an item's OBS index is its place in this list.
        let items = self.scene_items(scene, group).await?;
        let Some(index) = items.iter().position(|item| item.scene_item_id == item_id) else {
            return Ok(None);
        };
        let to = items.len() - 1 - position.min(items.len() - 1);
        if to == index {
            return Ok(Some(false));
        }
        self.command(RequestKind::SetSceneItemIndex {
            scene_name: scene,
            scene_item_id: item_id,
            scene_item_index: to,
        })
        .await?;
        Ok(Some(true))
    }

    /// Removes item `item_id` from `scene` (a scene, not a group), as OBS's
    /// Sources dock does; OBS deletes the source once no scene uses it.
    /// Groups aren't removed here: everything in them would go too.
    pub async fn remove_scene_item(&self, scene: &str, item_id: i64) -> Result<Removal> {
        let items = self.scene_items(scene, false).await?;
        let Some(item) = items.into_iter().find(|item| item.scene_item_id == item_id) else {
            return Ok(Removal::Gone);
        };
        if item.is_group == Some(true) {
            return Ok(Removal::Group);
        }
        self.command(RequestKind::RemoveSceneItem {
            scene_name: scene,
            scene_item_id: item_id,
        })
        .await?;
        Ok(Removal::Removed(item.source_name))
    }
}

/// What [`ObsClient::remove_scene_item`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// Removed the item of this source.
    Removed(String),
    /// It's a group, so nothing was removed.
    Group,
    /// It isn't in the scene (anymore).
    Gone,
}

/// Whether OBS refused because the name is taken.
pub fn is_name_taken(error: &Error) -> bool {
    matches!(
        error,
        Error::ObsResponseError {
            code: RESOURCE_ALREADY_EXISTS,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::fake::{client, input};

    fn media(url: Option<&str>) -> SourceSettings {
        SourceSettings::Media(MediaSettings {
            url: url.map(Into::into),
            input_format: "mpegts".into(),
            buffering_mb: 1,
            reconnect_delay_sec: 2,
            hw_decode: true,
            restart_on_activate: false,
            close_when_inactive: false,
            clear_on_media_end: true,
        })
    }

    fn browser(url: Option<&str>) -> BrowserSettings {
        BrowserSettings {
            url: url.map(Into::into),
            width: 1920,
            height: 1080,
            fps: None,
            css: String::new(),
            shutdown: true,
            restart_when_active: false,
            reroute_audio: false,
        }
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(clean_name("  IRL cam "), Ok("IRL cam"));
        assert_eq!(clean_name("   "), Err(Invalid::Name));
        assert_eq!(clean_name("a\nb"), Err(Invalid::Name));
        assert!(clean_name(&"é".repeat(MAX_NAME_CHARS)).is_ok());
        assert_eq!(
            clean_name(&"é".repeat(MAX_NAME_CHARS + 1)),
            Err(Invalid::Name)
        );
    }

    #[test]
    fn only_network_urls_can_be_played() {
        for url in [
            "srt://ingest:4000?streamid=play/live",
            "RTMP://ingest/live/key",
            "https://example.com/hls.m3u8",
            "udp://0.0.0.0:9000",
        ] {
            assert_eq!(media(Some(url)).check(true), Ok(()), "{url}");
        }
        for url in [
            "file:///C:/Users/kate/secret.mp4",
            "C:\\video.mp4",
            "/home/kate/video.mp4",
            "concat:a.mp4|b.mp4",
            "srt://",
            "srt://host /x",
        ] {
            assert_eq!(
                media(Some(url)).check(false),
                Err(Invalid::StreamUrl),
                "{url}"
            );
        }
        assert_eq!(media(None).check(true), Err(Invalid::MissingUrl));
        assert_eq!(media(None).check(false), Ok(()));

        let page = |url| SourceSettings::Browser(browser(Some(url))).check(true);
        assert_eq!(page("https://streamelements.com/overlay/x"), Ok(()));
        assert_eq!(page("http://localhost:3000"), Ok(()));
        assert_eq!(page("file:///C:/overlay.html"), Err(Invalid::WebUrl));
        assert_eq!(page("srt://ingest:4000"), Err(Invalid::WebUrl));

        let vlc = |playlist: Vec<&str>| {
            SourceSettings::Vlc(VlcSettings {
                playlist: Some(playlist.into_iter().map(Into::into).collect()),
                loop_playlist: true,
                shuffle: false,
                playback: VlcPlayback::AlwaysPlay,
                network_caching_ms: 400,
            })
            .check(true)
        };
        assert_eq!(vlc(vec!["srt://a:1", "rtmp://b/live"]), Ok(()));
        assert_eq!(vlc(vec![]), Err(Invalid::Playlist));
        assert_eq!(vlc(vec!["srt://a:1", "D:/clips"]), Err(Invalid::StreamUrl));
    }

    #[test]
    fn numbers_stay_in_obs_ranges() {
        let mut settings = browser(Some("https://x.y"));
        settings.width = 0;
        assert_eq!(
            SourceSettings::Browser(settings.clone()).check(true),
            Err(Invalid::BrowserSize)
        );
        settings.width = 800;
        settings.fps = Some(61);
        assert_eq!(
            SourceSettings::Browser(settings).check(true),
            Err(Invalid::BrowserFps)
        );

        let SourceSettings::Media(mut settings) = media(Some("srt://a:1")) else {
            unreachable!()
        };
        settings.reconnect_delay_sec = 0;
        assert_eq!(
            SourceSettings::Media(settings.clone()).check(true),
            Err(Invalid::ReconnectDelay)
        );
        settings.reconnect_delay_sec = 10;
        settings.input_format = "mpegts -i x".into();
        assert_eq!(
            SourceSettings::Media(settings).check(true),
            Err(Invalid::InputFormat)
        );
    }

    #[test]
    fn a_url_makes_media_play_the_network_and_none_keeps_what_it_plays() {
        let written = media(Some("srt://ingest:4000")).to_obs("ffmpeg_source", &json!({}));
        assert_eq!(written["is_local_file"], false);
        assert_eq!(written["input"], "srt://ingest:4000");

        let kept = media(None).to_obs("ffmpeg_source", &json!({}));
        assert!(kept.get("is_local_file").is_none() && kept.get("input").is_none());
        assert_eq!(kept["buffering_mb"], 1);
    }

    #[test]
    fn files_on_the_obs_computer_are_never_read_out() {
        let local = SourceSettings::from_obs(
            EditableKind::Media,
            "ffmpeg_source",
            &json!({ "is_local_file": true, "local_file": "C:/Users/kate/intro.mp4" }),
        );
        let SourceSettings::Media(local) = local else {
            unreachable!()
        };
        assert_eq!(local.url, None);

        let SourceSettings::Media(file_url) = SourceSettings::from_obs(
            EditableKind::Media,
            "ffmpeg_source",
            &json!({ "is_local_file": false, "input": "file:///C:/intro.mp4" }),
        ) else {
            unreachable!()
        };
        assert_eq!(file_url.url, None);

        let SourceSettings::Vlc(vlc) = SourceSettings::from_obs(
            EditableKind::Vlc,
            "vlc_source",
            &json!({ "playlist": [{ "value": "srt://a:1" }, { "value": "D:/clips" }] }),
        ) else {
            unreachable!()
        };
        assert_eq!(vlc.playlist, None);

        let SourceSettings::Browser(page) = SourceSettings::from_obs(
            EditableKind::Browser,
            "browser_source",
            &json!({ "is_local_file": true, "url": "https://x.y", "local_file": "C:/a.html" }),
        ) else {
            unreachable!()
        };
        assert_eq!(page.url, None);
    }

    fn text(says: Option<&str>, color: &str) -> SourceSettings {
        SourceSettings::Text(TextSettings {
            text: says.map(Into::into),
            font_face: String::new(),
            font_size: 96,
            bold: true,
            italic: false,
            color: color.into(),
            outline: true,
        })
    }

    #[test]
    fn text_kinds_are_known_by_any_version_and_added_in_the_newest() {
        assert_eq!(unversioned("text_gdiplus_v3"), "text_gdiplus");
        assert_eq!(unversioned("text_ft2_source_v2"), "text_ft2_source");
        assert_eq!(unversioned("browser_source"), "browser_source");
        assert_eq!(
            EditableKind::from_input_kind("text_gdiplus_v3"),
            Some(EditableKind::Text)
        );
        assert_eq!(
            EditableKind::from_input_kind("text_ft2_source"),
            Some(EditableKind::Text)
        );
        assert_eq!(EditableKind::from_input_kind("image_source"), None);

        let kinds = |list: &[&str]| list.iter().map(|k| k.to_string()).collect::<Vec<_>>();
        let windows = kinds(&[
            "text_gdiplus",
            "text_gdiplus_v2",
            "text_gdiplus_v3",
            "text_ft2_source_v2",
        ]);
        assert_eq!(EditableKind::Text.pick(&windows), Some("text_gdiplus_v3"));
        let linux = kinds(&["text_ft2_source", "text_ft2_source_v2", "ffmpeg_source"]);
        assert_eq!(EditableKind::Text.pick(&linux), Some("text_ft2_source_v2"));
        assert_eq!(EditableKind::Media.pick(&linux), Some("ffmpeg_source"));
        assert_eq!(EditableKind::Vlc.pick(&linux), None);
    }

    #[test]
    fn colors_go_to_obs_as_opaque_abgr() {
        assert_eq!(obs_color("#ff8000"), Some(0xff00_80ff));
        assert_eq!(hex_color(&json!(0xff00_80ff_u32)), "#ff8000");
        // GDI+ stores its color without alpha.
        assert_eq!(hex_color(&json!(0x00ff_ffff)), "#ffffff");
        assert_eq!(obs_color("ff8000"), None);
        assert_eq!(obs_color("#ff80"), None);
        assert_eq!(text(Some("Hi"), "red").check(true), Err(Invalid::Color));
    }

    #[test]
    fn gdiplus_text_keeps_the_font_s_other_flags() {
        let base =
            json!({ "font": { "face": "Arial", "size": 256, "style": "Regular", "flags": 4 } });
        let written = text(Some("BRB 5 min"), "#ffffff").to_obs("text_gdiplus_v3", &base);
        assert_eq!(written["text"], "BRB 5 min");
        assert_eq!(written["read_from_file"], false);
        assert_eq!(written["color"], 0xffff_ffff_u32);
        assert_eq!(
            written["font"],
            json!({ "face": "Arial", "size": 96, "style": "Bold", "flags": 5 })
        );
        assert!(written.get("color1").is_none());

        let read = SourceSettings::from_obs(
            EditableKind::Text,
            "text_gdiplus_v3",
            &json!({
                "text": "BRB", "read_from_file": false, "color": 0x0000_80ff,
                "font": { "face": "Arial", "size": 48, "flags": 3 }, "outline": true,
            }),
        );
        assert_eq!(
            read,
            SourceSettings::Text(TextSettings {
                text: Some("BRB".into()),
                font_face: "Arial".into(),
                font_size: 48,
                bold: true,
                italic: true,
                color: "#ff8000".into(),
                outline: true,
            })
        );
    }

    #[test]
    fn freetype_text_keeps_its_gradient_unless_the_color_changes() {
        let base = json!({ "color1": 0xffff_ffff_u32, "color2": 0xff00_00ff_u32, "font": {} });
        let same = text(None, "#FFFFFF").to_obs("text_ft2_source_v2", &base);
        assert!(same.get("color1").is_none() && same.get("color2").is_none());
        assert!(same.get("text").is_none() && same.get("from_file").is_none());

        let changed = text(Some("Hi"), "#ff8000").to_obs("text_ft2_source_v2", &base);
        assert_eq!(changed["color1"], 0xff00_80ff_u32);
        assert_eq!(changed["color2"], 0xff00_80ff_u32);
        assert_eq!(changed["from_file"], false);
        assert!(changed.get("read_from_file").is_none());

        let SourceSettings::Text(reading) = SourceSettings::from_obs(
            EditableKind::Text,
            "text_ft2_source_v2",
            &json!({ "from_file": true, "text_file": "C:/now-playing.txt", "text": "old" }),
        ) else {
            unreachable!()
        };
        assert_eq!(reading.text, None, "the file's text and path aren't shown");
    }

    #[test]
    fn vlc_settings_round_trip() {
        let settings = SourceSettings::Vlc(VlcSettings {
            playlist: Some(vec!["srt://a:1".into()]),
            loop_playlist: false,
            shuffle: true,
            playback: VlcPlayback::PauseUnpause,
            network_caching_ms: 1000,
        });
        assert_eq!(
            SourceSettings::from_obs(
                EditableKind::Vlc,
                "vlc_source",
                &settings.to_obs("vlc_source", &json!({}))
            ),
            settings
        );
    }

    fn obs(kind: &str, data: &Value) -> Value {
        match kind {
            "GetInputKindList" => {
                json!({ "inputKinds": ["ffmpeg_source", "browser_source", "image_source"] })
            }
            "GetInputSettings" => match data["inputName"].as_str() {
                Some("Overlay") => json!({
                    "inputKind": "browser_source",
                    "inputSettings": { "url": "https://overlay.example/abc", "width": 1920 },
                }),
                _ => json!({ "inputKind": "image_source", "inputSettings": {} }),
            },
            "GetInputDefaultSettings" => json!({ "defaultInputSettings": {
                "url": "https://obsproject.com/browser-source", "width": 800, "height": 600,
                "fps": 30, "fps_custom": false, "css": "body {}", "shutdown": false,
                "restart_when_active": false, "reroute_audio": false, "is_local_file": false,
            } }),
            "CreateInput" => json!({ "sceneItemId": 9 }),
            "GetSceneItemList" => json!({ "sceneItems": [
                input(7, "Camera", "ffmpeg_source", true),
                input(3, "Overlay", "browser_source", true),
                input(5, "Chat", "browser_source", true),
            ] }),
            _ => Value::Null,
        }
    }

    #[tokio::test]
    async fn only_kinds_this_obs_has_can_be_added() {
        let obs = client(obs);
        assert_eq!(
            obs.editable_kinds().await.unwrap(),
            [EditableKind::Media, EditableKind::Browser]
        );
    }

    #[tokio::test]
    async fn details_merge_the_source_over_its_defaults() {
        let obs = client(obs);
        let details = obs.input_details("Overlay").await.unwrap().unwrap();
        let SourceSettings::Browser(page) = details.settings else {
            panic!("not a browser source")
        };
        assert_eq!(page.url.as_deref(), Some("https://overlay.example/abc"));
        assert_eq!((page.width, page.height), (1920, 600));
        assert_eq!(page.css, "body {}");

        assert_eq!(obs.input_details("Logo").await.unwrap(), None);
    }

    #[tokio::test]
    async fn removing_takes_the_item_off_its_scene_but_never_a_group() {
        fn obs(kind: &str, _: &Value) -> Value {
            match kind {
                "GetSceneItemList" => json!({ "sceneItems": [
                    input(3, "Chat", "browser_source", true),
                    crate::fake::scene(4, "Overlay", true, true),
                ] }),
                _ => Value::Null,
            }
        }
        let obs = client(obs);
        assert_eq!(
            obs.remove_scene_item("LIVE", 3).await.unwrap(),
            Removal::Removed("Chat".into())
        );
        assert_eq!(
            obs.remove_scene_item("LIVE", 4).await.unwrap(),
            Removal::Group
        );
        assert_eq!(
            obs.remove_scene_item("LIVE", 9).await.unwrap(),
            Removal::Gone
        );
        assert_eq!(
            obs.asked(&["RemoveSceneItem"]),
            [(
                "RemoveSceneItem".to_owned(),
                json!({ "sceneName": "LIVE", "sceneItemId": 3 })
            )]
        );
    }

    #[tokio::test]
    async fn moving_counts_from_the_top_like_the_sources_dock() {
        let obs = client(obs);
        // OBS lists bottom first: Camera 0, Overlay 1, Chat 2. From the top:
        // Chat 0, Overlay 1, Camera 2.
        let to = |item_id, position| obs.move_scene_item("LIVE", false, item_id, position);
        assert_eq!(to(3, 0).await.unwrap(), Some(true), "Overlay to the top");
        assert_eq!(to(3, 2).await.unwrap(), Some(true), "Overlay to the bottom");
        assert_eq!(
            to(5, 0).await.unwrap(),
            Some(false),
            "Chat is on top already"
        );
        assert_eq!(
            to(7, 9).await.unwrap(),
            Some(false),
            "past the end: the bottom"
        );
        assert_eq!(to(99, 0).await.unwrap(), None);
        assert_eq!(
            obs.asked(&["SetSceneItemIndex"]),
            [
                (
                    "SetSceneItemIndex".to_owned(),
                    json!({ "sceneName": "LIVE", "sceneItemId": 3, "sceneItemIndex": 2 })
                ),
                (
                    "SetSceneItemIndex".to_owned(),
                    json!({ "sceneName": "LIVE", "sceneItemId": 3, "sceneItemIndex": 0 })
                ),
            ]
        );
    }

    #[tokio::test]
    async fn new_sources_are_created_with_their_checked_settings() {
        let obs = client(obs);
        let settings = SourceSettings::Browser(browser(Some("https://chat.example")));
        let created = obs
            .create_input("LIVE", "Chat 2", &settings, false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(created.scene_item_id, 9);
        let asked = obs.asked(&["CreateInput"]);
        assert_eq!(asked[0].1["inputKind"], "browser_source");
        assert_eq!(asked[0].1["sceneItemEnabled"], false);
        assert_eq!(asked[0].1["inputSettings"]["url"], "https://chat.example");
        assert_eq!(asked[0].1["inputSettings"]["fps_custom"], false);
    }
}
