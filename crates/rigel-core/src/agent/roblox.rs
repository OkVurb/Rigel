//! Roblox Studio, for every model Rigel runs.
//!
//! Studio ships its own MCP server, so Rigel does not invent a plugin or a protocol: the
//! native agent gets [`call`] behind the `roblox_studio` tool, and external harnesses get the
//! same stdio server in `.mcp.json`. Studio's server already generates textured meshes,
//! materials and procedural models, and its `execute_luau` tool is the animation path.
//!
//! Studio does not generate audio. [`write_sound`] fills that gap locally: a deterministic
//! PCM WAV from a small preset vocabulary, ready to import in Studio's Asset Manager. No API
//! key, network request or third-party service is involved.
//!
//! The process boundary is deliberately narrow. A call launches the server, completes
//! `initialize`, asks for one tool, and kills the child. Output text is capped and image
//! payloads are replaced by a size note, so a viewport capture cannot spend the turn's context.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Command as AsyncCommand;

pub const SERVER_NAME: &str = "roblox_studio";
/// The companion skill pack: 23 MIT-licensed `roblox-*` skills indexed through the shelf.
pub const SKILLS_REPO: &str = "EL4CTEO/roblox-skills";
pub const DOCS_URL: &str = "https://create.roblox.com/docs/studio/mcp";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(180);
/// Enough for a large script or asset listing, small enough that a chatty tool cannot crowd
/// out the rest of the turn.
const MAX_RESULT_CHARS: usize = 24_000;
const SAMPLE_RATE: u32 = 44_100;
const MAX_SOUND_MS: u32 = 10_000;
const MIN_SOUND_MS: u32 = 100;

/// The tools that only read. Everything else is treated as a write, including a tool name
/// Roblox adds after this list was written: an unknown capability is asked for, never assumed
/// harmless.
pub const READ_ONLY_TOOLS: &[&str] = &[
    "get_console_output",
    "get_studio_state",
    "http_get",
    "list_roblox_studios",
    "screen_capture",
    "script_grep",
    "script_read",
    "script_search",
    "search_asset",
    "skill",
    "store_image",
    "wait_job_finished",
];

pub const SOUND_PRESETS: &[&str] = &[
    "ambient",
    "click",
    "explosion",
    "footstep",
    "jump",
    "laser",
    "pickup",
    "whoosh",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub command: String,
    pub args: Vec<String>,
}

impl Launch {
    pub fn command_line(&self) -> String {
        std::iter::once(self.command.clone())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn mcp_server(&self) -> Value {
        json!({ "command": self.command, "args": self.args })
    }
}

/// The Studio MCP launch recipe, if Studio (or an explicit override) is installed.
///
/// `RIGEL_ROBLOX_MCP` wins so a portable or preview Studio install can be named without
/// editing the project. A configured path that does not exist is reported as missing rather
/// than silently falling back to a different Studio.
pub fn launch() -> Option<Launch> {
    if let Some(configured) = std::env::var_os("RIGEL_ROBLOX_MCP") {
        let configured = PathBuf::from(configured);
        if configured.is_file() {
            return Some(launch_for(configured));
        }
        if configured.components().count() == 1 {
            return Some(Launch {
                command: configured.to_string_lossy().to_string(),
                args: Vec::new(),
            });
        }
        return None;
    }
    default_path().filter(|path| path.is_file()).map(launch_for)
}

pub fn mcp_server() -> Option<Value> {
    launch().map(|launch| launch.mcp_server())
}

pub fn missing_message() -> String {
    format!(
        "Roblox Studio's MCP server was not found. Install the latest Roblox Studio, open a \
         place, then turn on Assistant > … > Manage MCP Servers > Enable Studio as MCP server. \
         Set RIGEL_ROBLOX_MCP to the server path if Studio is in a non-standard location. \
         Docs: {DOCS_URL}"
    )
}

fn launch_for(path: PathBuf) -> Launch {
    #[cfg(windows)]
    {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(extension.as_str(), "bat" | "cmd") {
            let directory = path.parent().unwrap_or_else(|| Path::new("."));
            let file = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            // Studio resolves its server files relative to this folder. Running the batch file
            // by absolute path from the project makes tools/list come back empty, so cd first.
            let directory = if std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .map(|local| local.join("Roblox"))
                .as_deref()
                == Some(directory)
            {
                "%LOCALAPPDATA%\\Roblox".to_string()
            } else {
                format!("\"{}\"", directory.display())
            };
            let file = if file.contains(char::is_whitespace) {
                format!("\"{file}\"")
            } else {
                format!(".\\{file}")
            };
            return Launch {
                command: "cmd.exe".to_string(),
                args: vec!["/c".to_string(), format!("cd /d {directory} && {file}")],
            };
        }
    }
    Launch {
        command: path.display().to_string(),
        args: Vec::new(),
    }
}

#[cfg(windows)]
fn default_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join("Roblox").join("mcp.bat"))
}

#[cfg(target_os = "macos")]
fn default_path() -> Option<PathBuf> {
    Some(PathBuf::from(
        "/Applications/RobloxStudio.app/Contents/MacOS/StudioMCP",
    ))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn default_path() -> Option<PathBuf> {
    None
}

pub fn is_read_only(tool: &str) -> bool {
    READ_ONLY_TOOLS.contains(&tool)
}

/// Ask the installed server for its live tool list. This is also the only honest answer to
/// "did Studio update and add something": the list comes from the process, not this file.
pub async fn list_tools() -> Result<Vec<Value>, String> {
    let launch = launch().ok_or_else(missing_message)?;
    let mut child = spawn(&launch)?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Roblox Studio MCP has no stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Roblox Studio MCP has no stdout".to_string())?;
    let result = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        mcp_list(tokio::io::BufReader::new(stdout), stdin),
    )
    .await
    .map_err(|_| "Roblox Studio did not answer tools/list in time".to_string())?;
    let _ = child.start_kill();
    let _ = child.wait().await;
    result
}

/// Run one Studio tool. `studio_id` is passed through, because Studio requires every tool call
/// to name the instance it targets; use `list_roblox_studios` first when more than one is open.
pub async fn call(tool: &str, arguments: &Value, studio_id: Option<&str>) -> Result<Value, String> {
    let launch = launch().ok_or_else(missing_message)?;
    let mut child = spawn(&launch)?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Roblox Studio MCP has no stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Roblox Studio MCP has no stdout".to_string())?;
    let mut arguments = arguments.clone();
    if !arguments.is_object() {
        arguments = json!({});
    }
    if let Some(studio_id) = studio_id {
        arguments["studio_id"] = json!(studio_id);
    }
    let result = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        mcp_call(tokio::io::BufReader::new(stdout), stdin, tool, arguments),
    )
    .await
    .map_err(|_| {
        format!(
            "Roblox Studio did not answer `{tool}` within {} seconds",
            DEFAULT_TIMEOUT.as_secs()
        )
    })?;
    let _ = child.start_kill();
    let _ = child.wait().await;
    result
}

fn spawn(launch: &Launch) -> Result<tokio::process::Child, String> {
    let mut command = AsyncCommand::new(&launch.command);
    command
        .args(&launch.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    crate::process::hide_tokio(&mut command);
    command
        .spawn()
        .map_err(|error| format!("could not start Roblox Studio MCP: {error}"))
}

async fn mcp_list<R, W>(reader: R, mut writer: W) -> Result<Vec<Value>, String>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = reader.lines();
    initialize(&mut lines, &mut writer).await?;
    send(
        &mut writer,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    )
    .await?;
    let response = wait_for(&mut lines, 2, "tools/list").await?;
    if let Some(error) = response.get("error") {
        return Err(format!("Roblox Studio refused tools/list: {error}"));
    }
    Ok(response
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

async fn mcp_call<R, W>(
    reader: R,
    mut writer: W,
    tool: &str,
    arguments: Value,
) -> Result<Value, String>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = reader.lines();
    initialize(&mut lines, &mut writer).await?;
    send(
        &mut writer,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        }),
    )
    .await?;
    let response = wait_for(&mut lines, 2, "tools/call").await?;
    if let Some(error) = response.get("error") {
        return Err(format!("Roblox Studio refused `{tool}`: {error}"));
    }
    let result = response.get("result").cloned().unwrap_or(Value::Null);
    if result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(content_text(&result).unwrap_or_else(|| format!("`{tool}` failed in Studio")));
    }
    Ok(trim_result(result))
}

async fn initialize<R, W>(lines: &mut tokio::io::Lines<R>, writer: &mut W) -> Result<(), String>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    send(
        writer,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "rigel", "version": env!("CARGO_PKG_VERSION") }
            }
        }),
    )
    .await?;
    let response = wait_for(lines, 1, "initialize").await?;
    if let Some(error) = response.get("error") {
        return Err(format!("Roblox Studio refused initialize: {error}"));
    }
    send(
        writer,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await
}

async fn wait_for<R>(lines: &mut tokio::io::Lines<R>, id: u64, label: &str) -> Result<Value, String>
where
    R: AsyncBufRead + Unpin,
{
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|error| format!("could not read from Roblox Studio: {error}"))?
    {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if message.get("id") == Some(&json!(id)) {
            return Ok(message);
        }
    }
    Err(format!(
        "Roblox Studio closed before {label}. Open a place and enable Studio as MCP server in \
         Assistant > … > Manage MCP Servers. Docs: {DOCS_URL}"
    ))
}

async fn send<W>(writer: &mut W, message: Value) -> Result<(), String>
where
    W: AsyncWrite + Unpin,
{
    let mut line = message.to_string();
    line.push('\n');
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|error| format!("could not write to Roblox Studio: {error}"))?;
    writer
        .flush()
        .await
        .map_err(|error| format!("could not flush to Roblox Studio: {error}"))
}

fn content_text(result: &Value) -> Option<String> {
    let parts = result
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn trim_result(mut result: Value) -> Value {
    let Some(items) = result.get_mut("content").and_then(Value::as_array_mut) else {
        return result;
    };
    let mut remaining = MAX_RESULT_CHARS;
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                let Some(text) = item.get("text").and_then(Value::as_str) else {
                    continue;
                };
                let chars = text.chars().count();
                if chars > remaining {
                    let cut: String = text.chars().take(remaining).collect();
                    item["text"] = json!(format!("{cut}\n… Studio output truncated"));
                    remaining = 0;
                } else {
                    remaining = remaining.saturating_sub(chars);
                }
            }
            Some("image") => {
                let bytes = item
                    .get("data")
                    .and_then(Value::as_str)
                    .map(str::len)
                    .unwrap_or(0);
                item["data"] = json!(format!(
                    "[{bytes} bytes of image data omitted; use Studio's viewport to inspect it]"
                ));
            }
            _ => {}
        }
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundFile {
    pub relative: String,
    pub bytes: u64,
    pub duration_ms: u32,
    pub sample_rate: u32,
    pub preset: String,
}

/// Synthesize one sound effect into `.rigel/roblox/sounds`. The name is reduced to a safe
/// filename, and an existing file is replaced: a rerun is the same deterministic effect.
pub fn write_sound(
    root: &Path,
    name: &str,
    preset: &str,
    duration_ms: u32,
    pitch: f32,
    seed: u64,
) -> Result<SoundFile, String> {
    if !SOUND_PRESETS.contains(&preset) {
        return Err(format!(
            "unknown sound preset `{preset}`; use one of {}",
            SOUND_PRESETS.join(", ")
        ));
    }
    let duration_ms = duration_ms.clamp(MIN_SOUND_MS, MAX_SOUND_MS);
    let pitch = pitch.clamp(0.5, 2.0);
    let samples = synthesize(preset, duration_ms, pitch, seed);
    let bytes = wav(&samples);
    let relative = format!(".rigel/roblox/sounds/{}.wav", slug(name));
    let path = root.join(".rigel").join("roblox").join("sounds");
    std::fs::create_dir_all(&path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    let path = path.join(format!("{}.wav", slug(name)));
    std::fs::write(&path, &bytes)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(SoundFile {
        relative,
        bytes: bytes.len() as u64,
        duration_ms,
        sample_rate: SAMPLE_RATE,
        preset: preset.to_string(),
    })
}

fn slug(value: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            dash = false;
            if slug.len() >= 48 {
                break;
            }
        } else {
            dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "sound".to_string()
    } else if matches!(
        slug.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        format!("sfx-{slug}")
    } else {
        slug.to_string()
    }
}

fn synthesize(preset: &str, duration_ms: u32, pitch: f32, seed: u64) -> Vec<f32> {
    let count = (SAMPLE_RATE as u64 * duration_ms as u64 / 1_000) as usize;
    let mut out = vec![0.0f32; count];
    let mut noise = Noise::new(seed);
    let mut phase = 0.0f32;
    let mut low = 0.0f32;
    for (index, sample) in out.iter_mut().enumerate() {
        let t = index as f32 / SAMPLE_RATE as f32;
        let progress = if count <= 1 {
            0.0
        } else {
            index as f32 / (count - 1) as f32
        };
        let n = noise.next();
        *sample = match preset {
            "laser" => {
                let frequency =
                    1_800.0 * pitch * (1.0 - progress).powf(2.2) + 180.0 * pitch * (1.0 - progress);
                phase += std::f32::consts::TAU * frequency / SAMPLE_RATE as f32;
                let envelope = (1.0 - progress).powf(1.8) * (progress / 0.01).min(1.0);
                (phase.sin() * 0.72 + (phase * 2.0).sin() * 0.18 + n * 0.10) * envelope
            }
            "explosion" => {
                low += (n - low) * 0.045;
                let body = (std::f32::consts::TAU * 52.0 * pitch * t).sin() * 0.45;
                let envelope = (-4.5 * progress).exp();
                (low * 0.9 + body) * envelope
            }
            "pickup" => {
                let notes = [659.25f32, 830.61, 987.77];
                let step = 0.18;
                let note = ((t / step) as usize).min(notes.len() - 1);
                let local = t - note as f32 * step;
                let envelope = (1.0 - local / step).max(0.0).powf(1.4);
                (std::f32::consts::TAU * notes[note] * pitch * local).sin() * envelope * 0.55
            }
            "jump" => {
                let frequency = 220.0 * pitch + 700.0 * pitch * progress;
                phase += std::f32::consts::TAU * frequency / SAMPLE_RATE as f32;
                phase.sin() * (1.0 - progress).powf(1.3) * 0.55
            }
            "click" => {
                let envelope = (-70.0 * t).exp();
                ((std::f32::consts::TAU * 1_600.0 * pitch * t).sin() * 0.55 + n * 0.25) * envelope
            }
            "whoosh" => {
                let cutoff = 0.02 + 0.28 * (std::f32::consts::PI * progress).sin();
                low += (n - low) * cutoff;
                low * (std::f32::consts::PI * progress).sin().powi(2) * 1.8
            }
            "footstep" => {
                low += (n - low) * 0.08;
                let first = if (0.04..0.18).contains(&t) {
                    (-24.0 * (t - 0.04)).exp()
                } else {
                    0.0
                };
                let second = if (0.26..0.42).contains(&t) {
                    (-28.0 * (t - 0.26)).exp() * 0.65
                } else {
                    0.0
                };
                low * (first + second) * 1.6
            }
            "ambient" => {
                let fade = (progress / 0.08).min(1.0) * ((1.0 - progress) / 0.12).min(1.0);
                let hum = (std::f32::consts::TAU * 110.0 * pitch * t).sin() * 0.22
                    + (std::f32::consts::TAU * 165.0 * pitch * t).sin() * 0.13
                    + (std::f32::consts::TAU * 220.0 * pitch * t).sin() * 0.06;
                low += (n - low) * 0.004;
                (hum + low * 0.08) * fade * (0.75 + 0.25 * (std::f32::consts::TAU * 0.37 * t).sin())
            }
            _ => 0.0,
        }
        .clamp(-1.0, 1.0);
    }
    out
}

fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

struct Noise(u64);

impl Noise {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let unit = (self.0 >> 40) as f32 / 16_777_215.0;
        unit * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn the_mcp_client_initializes_and_returns_one_tool_result() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        let (server_read, mut server_write) = tokio::io::split(server);
        let fake = tokio::spawn(async move {
            let mut lines = BufReader::new(server_read).lines();
            let init = lines.next_line().await.unwrap().unwrap();
            assert!(init.contains("\"initialize\""));
            server_write
                .write_all(
                    b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-06-18\"}}\n",
                )
                .await
                .unwrap();
            let notification = lines.next_line().await.unwrap().unwrap();
            assert!(notification.contains("notifications/initialized"));
            let call = lines.next_line().await.unwrap().unwrap();
            assert!(call.contains("\"tools/call\""));
            assert!(call.contains("\"list_roblox_studios\""));
            server_write
                .write_all(
                    b"{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"[]\"}]}}\n",
                )
                .await
                .unwrap();
        });
        let result = mcp_call(
            BufReader::new(client_read),
            client_write,
            "list_roblox_studios",
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(result["content"][0]["text"], "[]");
        fake.await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires Roblox Studio's local MCP server"]
    async fn live_studio_lists_the_generation_tools() {
        let tools = list_tools()
            .await
            .expect("Studio's local MCP server should answer");
        let names = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        for expected in [
            "execute_luau",
            "generate_material",
            "generate_mesh",
            "generate_procedural_model",
            "insert_asset",
            "search_asset",
        ] {
            assert!(
                names.contains(&expected),
                "Studio did not advertise {expected}: {names:?}"
            );
        }
        println!("Studio MCP tools: {names:?}");
        match call("list_roblox_studios", &json!({}), None).await {
            Ok(studios) => println!("Connected Studio windows: {studios}"),
            Err(error) => println!("No Studio window answered yet: {error}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_batch_server_starts_in_its_own_directory() {
        let launch = launch_for(PathBuf::from(r"C:\Tools\Roblox\mcp.bat"));
        assert_eq!(launch.command, "cmd.exe");
        assert_eq!(
            launch.args,
            vec![
                "/c".to_string(),
                r#"cd /d "C:\Tools\Roblox" && .\mcp.bat"#.to_string()
            ]
        );
    }

    #[test]
    fn unknown_tools_are_writes_not_reads() {
        assert!(is_read_only("script_read"));
        assert!(is_read_only("search_asset"));
        assert!(!is_read_only("execute_luau"));
        assert!(!is_read_only("generate_mesh"));
        assert!(
            !is_read_only("a_tool_roblox_adds_later"),
            "a new tool must ask before it changes the place"
        );
    }

    #[test]
    fn a_sound_is_a_pcm_wav_with_the_requested_duration() {
        let dir = tempfile::tempdir().unwrap();
        let sound = write_sound(dir.path(), "Pickup Chime", "pickup", 250, 1.0, 7).unwrap();
        assert_eq!(sound.duration_ms, 250);
        assert_eq!(sound.sample_rate, 44_100);
        assert_eq!(sound.relative, ".rigel/roblox/sounds/pickup-chime.wav");
        let bytes = std::fs::read(dir.path().join(&sound.relative)).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(bytes.len(), 44 + 22_050);
        assert!(bytes[44..].iter().any(|byte| *byte != 0));
    }

    #[test]
    fn the_same_seed_is_the_same_sound() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_sound(dir.path(), "laser", "laser", 120, 1.2, 42).unwrap();
        let b = write_sound(dir.path(), "laser", "laser", 120, 1.2, 42).unwrap();
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(
            std::fs::read(dir.path().join(&a.relative)).unwrap(),
            std::fs::read(dir.path().join(&b.relative)).unwrap()
        );
    }

    #[test]
    fn unknown_presets_and_path_like_names_do_not_escape_the_project() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write_sound(dir.path(), "x", "bagpipes", 100, 1.0, 1).is_err());
        let sound = write_sound(dir.path(), "../../x", "click", 100, 1.0, 1).unwrap();
        assert_eq!(sound.relative, ".rigel/roblox/sounds/x.wav");
        assert!(dir.path().join(&sound.relative).is_file());
    }
}
