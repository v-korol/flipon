//! Locating the `claude` executable and driving `claude auth login` in the background.

use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc::UnboundedSender, oneshot, Mutex};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn command(path: &Path) -> Command {
    let mut cmd = Command::new(path);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    // Apps started from Finder or at login get a bare PATH (/usr/bin:/bin:…). An npm-installed
    // `claude` is a node script, so put its own folder and the usual tool folders in front.
    #[cfg(not(windows))]
    cmd.env("PATH", child_path(path));
    cmd.kill_on_drop(true);
    cmd
}

/// PATH for the `claude` child on macOS/Linux: claude's folder, common install folders, then the
/// inherited PATH.
#[cfg(not(windows))]
fn child_path(claude: &Path) -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = claude.parent() {
        dirs.push(parent.to_path_buf());
    }
    dirs.extend(extra_unix_dirs());
    if let Some(inherited) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&inherited));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    std::env::join_paths(dirs).unwrap_or_default()
}

/// Where tools usually live on a Mac (Homebrew, the native Claude Code installer, nvm, volta…).
#[cfg_attr(windows, allow(dead_code))]
fn extra_unix_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".claude").join("local"));
        dirs.push(home.join(".npm-global").join("bin"));
        dirs.push(home.join(".bun").join("bin"));
        dirs.push(home.join(".volta").join("bin"));
        // every node version installed with nvm, newest first
        if let Ok(read) = std::fs::read_dir(home.join(".nvm").join("versions").join("node")) {
            let mut versions: Vec<PathBuf> = read.flatten().map(|e| e.path().join("bin")).collect();
            versions.sort();
            versions.reverse();
            dirs.extend(versions);
        }
    }
    dirs
}

/// Explicit path from Settings first, then PATH, then the usual installer directories.
pub fn find_claude(custom: &str) -> Option<PathBuf> {
    let custom = custom.trim();
    if !custom.is_empty() {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Some(p);
        }
        // allow pointing at the install directory
        for name in exe_names() {
            let candidate = p.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        return None;
    }
    if let Ok(p) = which::which("claude") {
        return Some(p);
    }
    let mut dirs_to_try: Vec<PathBuf> = extra_unix_dirs();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs_to_try.push(PathBuf::from(appdata).join("npm"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs_to_try.push(PathBuf::from(local).join("Programs").join("claude"));
    }
    for dir in dirs_to_try {
        for name in exe_names() {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn exe_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    }
}

/// `claude --version` → "2.1.301"
pub async fn version(path: &Path) -> Option<String> {
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        command(path).arg("--version").stdin(Stdio::null()).output(),
    )
    .await
    .ok()?
    .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace().next().map(str::to_string).filter(|v| !v.is_empty())
}

#[derive(Debug)]
pub enum LoginEvent {
    Url(String),
    NeedsCode,
    Exited { success: bool, message: String },
}

pub struct LoginProc {
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    kill: Option<oneshot::Sender<()>>,
}

/// Handle for forwarding the pasted authorization code to the running login process.
#[derive(Clone)]
pub struct CodeWriter(Arc<Mutex<Option<ChildStdin>>>);

impl CodeWriter {
    pub async fn submit(&self, code: &str) -> Result<(), String> {
        let mut guard = self.0.lock().await;
        let stdin = guard.as_mut().ok_or("The login process is no longer running.")?;
        stdin
            .write_all(format!("{}
", code.trim()).as_bytes())
            .await
            .map_err(|e| format!("Could not send the code to Claude: {e}"))?;
        stdin.flush().await.map_err(|e| e.to_string())
    }
}

impl LoginProc {
    pub fn writer(&self) -> CodeWriter {
        CodeWriter(self.stdin.clone())
    }

    pub fn cancel(&mut self) {
        if let Some(tx) = self.kill.take() {
            let _ = tx.send(());
        }
    }
}

fn ansi_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)").unwrap())
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"https?://[^\s"'<>]+"#).unwrap())
}

/// Starts `claude auth login`. stdin stays open so the pasted code can be forwarded later.
pub fn spawn_login(
    path: &Path,
    email: Option<&str>,
    events: UnboundedSender<LoginEvent>,
) -> Result<LoginProc, String> {
    let mut cmd = command(path);
    cmd.args(["auth", "login", "--claudeai"]);
    if let Some(email) = email.filter(|e| !e.is_empty()) {
        cmd.args(["--email", email]);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child: Child = cmd.spawn().map_err(|e| format!("Could not start the Claude CLI: {e}"))?;

    let stdin = Arc::new(Mutex::new(child.stdin.take()));
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let tail = Arc::new(std::sync::Mutex::new(String::new()));

    for stream in [stdout.map(Stream::Out), stderr.map(Stream::Err)].into_iter().flatten() {
        let events = events.clone();
        let tail = tail.clone();
        tokio::spawn(async move {
            let mut reader = stream;
            let mut buf = [0u8; 4096];
            let mut window = String::new();
            let mut url_sent = false;
            let mut code_sent = false;
            loop {
                let n = match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                let clean = ansi_re().replace_all(&chunk, "").into_owned();
                if let Ok(mut t) = tail.lock() {
                    t.push_str(&clean);
                    let len = t.len();
                    if len > 2000 {
                        let mut cut = len - 2000;
                        while !t.is_char_boundary(cut) {
                            cut += 1;
                        }
                        t.drain(..cut);
                    }
                }
                window.push_str(&clean);
                if window.len() > 8192 {
                    let mut cut = window.len() - 8192;
                    while !window.is_char_boundary(cut) {
                        cut += 1;
                    }
                    window.drain(..cut);
                }
                if !url_sent {
                    if let Some(m) = url_re().find(&window) {
                        // wait for the end of the line so a URL split across reads is not cut short
                        let end = m.end();
                        if window[end..].contains(['\n', '\r']) || window[end..].contains("Paste") {
                            url_sent = true;
                            let _ = events.send(LoginEvent::Url(m.as_str().to_string()));
                        }
                    }
                }
                if !code_sent && window.to_lowercase().contains("paste code") {
                    code_sent = true;
                    let _ = events.send(LoginEvent::NeedsCode);
                }
            }
        });
    }

    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let tail_for_exit = tail.clone();
    tokio::spawn(async move {
        let result = tokio::select! {
            status = child.wait() => status.ok(),
            _ = kill_rx => {
                let _ = child.kill().await;
                None
            }
        };
        // give the output readers a moment to drain
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let message = tail_for_exit.lock().map(|t| last_meaningful_line(&t)).unwrap_or_default();
        let success = result.is_some_and(|s| s.success());
        let _ = events.send(LoginEvent::Exited { success, message });
    });

    Ok(LoginProc { stdin, kill: Some(kill_tx) })
}

enum Stream {
    Out(tokio::process::ChildStdout),
    Err(tokio::process::ChildStderr),
}

impl Stream {
    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Out(s) => s.read(buf).await,
            Stream::Err(s) => s.read(buf).await,
        }
    }
}

fn last_meaningful_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("http") && !l.to_lowercase().contains("paste code"))
        .next_back()
        .unwrap_or_default()
        .to_string()
}
