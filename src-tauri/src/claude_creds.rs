//! Reading and writing the Claude Code CLI's own login.
//!
//! Claude Code keeps the active login in two places:
//! * `<config dir>/.credentials.json` — key `claudeAiOauth` (access/refresh token, plan) next to
//!   unrelated keys such as `mcpOAuth` that must never be touched;
//! * `~/.claude.json` — key `oauthAccount` (email, organization, ...).
//!
//! Switching accounts replaces exactly those two values and nothing else.

use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct CliLogin {
    /// The `claudeAiOauth` object.
    pub oauth: Value,
    /// The `oauthAccount` object, when present.
    pub oauth_account: Option<Value>,
}

impl CliLogin {
    pub fn email(&self) -> Option<String> {
        self.oauth_account
            .as_ref()
            .and_then(|a| a.get("emailAddress"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
    }

    pub fn access_token(&self) -> Option<&str> {
        self.oauth.get("accessToken").and_then(Value::as_str)
    }


    pub fn expires_at_ms(&self) -> Option<i64> {
        self.oauth.get("expiresAt").and_then(Value::as_i64)
    }

    pub fn subscription(&self) -> String {
        self.oauth
            .get("subscriptionType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    pub fn rate_limit_tier(&self) -> String {
        self.oauth
            .get("rateLimitTier")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::home_dir().unwrap_or_default().join(".claude")
}

pub fn credentials_path() -> PathBuf {
    config_dir().join(".credentials.json")
}

/// With a custom CLAUDE_CONFIG_DIR the global config lives inside it, otherwise next to `~/.claude`.
pub fn claude_json_path() -> PathBuf {
    if std::env::var_os("CLAUDE_CONFIG_DIR").is_some_and(|v| !v.is_empty()) {
        config_dir().join(".claude.json")
    } else {
        dirs::home_dir().unwrap_or_default().join(".claude.json")
    }
}

fn read_json(path: &Path) -> io::Result<Option<Value>> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Write via a temp file in the same directory, then rename over the original.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.flipon-tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

// ---------------------------------------------------------------------------------------------
// Where Claude Code keeps the credentials JSON (`{"claudeAiOauth": …, "mcpOAuth": …}`):
// * Windows / Linux: `<config dir>/.credentials.json`;
// * macOS: the login Keychain, written and read with Apple's `security` tool exactly the way the
//   CLI does it (checked against Claude Code 2.1.301), with the file as a fallback.
// ---------------------------------------------------------------------------------------------

/// Keychain account Claude Code uses for its items (older versions used the login user name).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const KEYCHAIN_ACCOUNT: &str = "claude-code-user";

/// `Claude Code-credentials`, plus `-<first 8 hex of sha256(config dir)>` when the config dir is
/// not the default one — the same rule as Claude Code's own service-name function.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn keychain_service(secure_storage_dir: Option<&str>, claude_config_dir: Option<&str>, resolved_dir: &str) -> String {
    use sha2::{Digest, Sha256};
    // CLAUDE_SECURESTORAGE_CONFIG_DIR wins when set (an empty value means "the default").
    let (default, dir) = match secure_storage_dir {
        Some(d) => (d.is_empty(), d.to_string()),
        None => (claude_config_dir.is_none_or(str::is_empty), resolved_dir.to_string()),
    };
    if default {
        return "Claude Code-credentials".into();
    }
    let digest = Sha256::digest(dir.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("Claude Code-credentials-{}", &hex[..8])
}

/// The line `security -i` reads to store a value; the value travels hex-encoded on stdin, so the
/// secret never shows up in the process list.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn keychain_add_command(account: &str, service: &str, value: &[u8]) -> String {
    let hex: String = value.iter().map(|b| format!("{b:02x}")).collect();
    format!("add-generic-password -U -a \"{account}\" -s \"{service}\" -X \"{hex}\"\n")
}

/// What `security find-generic-password -w` printed, as text. It prints the value itself when it is
/// readable text and hex when it is not, so a hex dump that decodes to JSON is decoded.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn decode_keychain_value(printed: &str) -> String {
    let looks_hex = !printed.is_empty() && printed.len().is_multiple_of(2) && printed.bytes().all(|b| b.is_ascii_hexdigit());
    if looks_hex && serde_json::from_str::<Value>(printed).is_err() {
        let bytes: Option<Vec<u8>> = (0..printed.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&printed[i..i + 2], 16).ok())
            .collect();
        if let Some(text) = bytes.and_then(|b| String::from_utf8(b).ok()).filter(|t| serde_json::from_str::<Value>(t).is_ok()) {
            return text;
        }
    }
    printed.to_string()
}

#[cfg(target_os = "macos")]
mod keychain {
    use super::*;
    use std::io::{Read, Write};
    use std::process::{Command, Output, Stdio};
    use std::time::{Duration, Instant};

    /// `security` exits with 44 when the item does not exist.
    const NOT_FOUND: i32 = 44;
    /// If macOS asks the user to allow Keychain access, `security` waits for the answer. Give up after
    /// this long instead of blocking FlipOn's background loop forever.
    const TIMEOUT: Duration = Duration::from_secs(60);
    /// `security -i` reads one command per line from a fixed-size buffer; a longer line would be cut.
    /// Above this the value goes as an argument instead (briefly visible to `ps`, as with the CLI).
    const MAX_INTERACTIVE_LINE: usize = 4000;

    pub fn service() -> String {
        let env = |k: &str| std::env::var(k).ok();
        let resolved = config_dir().to_string_lossy().into_owned();
        keychain_service(
            env("CLAUDE_SECURESTORAGE_CONFIG_DIR").as_deref(),
            env("CLAUDE_CONFIG_DIR").as_deref(),
            &resolved,
        )
    }

    /// Runs `security`, feeding `input` on stdin, and kills it after `TIMEOUT`.
    fn run(args: &[&str], input: Option<&[u8]>) -> Result<Output, String> {
        let mut child = Command::new("/usr/bin/security")
            .args(args)
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not run the macOS security tool: {e}"))?;
        if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
            stdin
                .write_all(bytes)
                .map_err(|e| format!("Could not talk to the macOS security tool: {e}"))?;
            // stdin is dropped here, so `security -i` sees the end of its input
        }
        // read the pipes on threads so a large output cannot stall the child while we wait
        let out_pipe = child.stdout.take();
        let err_pipe = child.stderr.take();
        let drain = |pipe: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut p) = pipe {
                    let _ = p.read_to_end(&mut buf);
                }
                buf
            })
        };
        let out_reader = drain(out_pipe.map(|p| Box::new(p) as Box<dyn Read + Send>));
        let err_reader = drain(err_pipe.map(|p| Box::new(p) as Box<dyn Read + Send>));
        let started = Instant::now();
        let status = loop {
            match child.try_wait().map_err(|e| e.to_string())? {
                Some(status) => break status,
                None if started.elapsed() > TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("macOS is waiting for permission to use the Claude Code login in the Keychain. \
                                Allow it (\"Always Allow\") and try again."
                        .into());
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        };
        Ok(Output {
            status,
            stdout: out_reader.join().unwrap_or_default(),
            stderr: err_reader.join().unwrap_or_default(),
        })
    }

    fn find(service: &str, account: Option<&str>) -> Result<Option<String>, String> {
        let mut args = vec!["find-generic-password"];
        if let Some(a) = account {
            args.extend(["-a", a]);
        }
        args.extend(["-w", "-s", service]);
        let out = run(&args, None)?;
        match out.status.code() {
            Some(0) => Ok(Some(decode_keychain_value(String::from_utf8_lossy(&out.stdout).trim()))),
            Some(NOT_FOUND) => Ok(None),
            code => Err(format!(
                "Could not read the Claude Code login from the Keychain (security exited {}): {}",
                code.unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        }
    }

    /// The stored JSON, if Claude Code has a Keychain item. Items written by older CLI versions
    /// used the user name as account, so fall back to "any account".
    pub fn read() -> Result<Option<String>, String> {
        let service = service();
        match find(&service, Some(KEYCHAIN_ACCOUNT))? {
            Some(v) => Ok(Some(v)),
            None => find(&service, None),
        }
    }

    /// Stores the value, then reads it back: `security -i` can report success for a command it did
    /// not run, and a login that was not really saved must not look like a successful switch.
    pub fn write(value: &[u8]) -> Result<(), String> {
        let service = service();
        let line = keychain_add_command(KEYCHAIN_ACCOUNT, &service, value);
        let out = if line.len() <= MAX_INTERACTIVE_LINE {
            run(&["-i"], Some(line.as_bytes()))?
        } else {
            let hex: String = value.iter().map(|b| format!("{b:02x}")).collect();
            run(&["add-generic-password", "-U", "-a", KEYCHAIN_ACCOUNT, "-s", &service, "-X", &hex], None)?
        };
        if !out.status.success() {
            return Err(format!(
                "Could not save the Claude Code login to the Keychain: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let wanted: Option<Value> = serde_json::from_slice(value).ok();
        let stored = find(&service, Some(KEYCHAIN_ACCOUNT))?;
        let stored: Option<Value> = stored.as_deref().and_then(|s| serde_json::from_str(s).ok());
        if wanted.is_some() && stored == wanted {
            Ok(())
        } else {
            Err("The Claude Code login was not saved to the Keychain (it reads back different). Try again.".into())
        }
    }
}

/// The whole credentials object Claude Code stores (all keys, not just `claudeAiOauth`).
fn read_credentials() -> Result<Option<Map<String, Value>>, String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(text) = keychain::read()? {
            return match serde_json::from_str(&text) {
                Ok(Value::Object(map)) => Ok(Some(map)),
                _ => Err("The Claude Code Keychain item is not the expected JSON.".into()),
            };
        }
    }
    match read_json(&credentials_path()).map_err(|e| e.to_string())? {
        Some(Value::Object(map)) => Ok(Some(map)),
        _ => Ok(None),
    }
}

fn write_credentials_file(bytes: &[u8]) -> Result<(), String> {
    let path = credentials_path();
    write_atomic(&path, bytes).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

#[cfg(target_os = "macos")]
fn write_credentials(creds: &Map<String, Value>) -> Result<(), String> {
    let bytes = serde_json::to_vec(&Value::Object(creds.clone())).map_err(|e| e.to_string())?;
    keychain::write(&bytes)?;
    // Some setups (or older CLI versions) still read the plaintext file: keep it in step if present.
    if credentials_path().exists() {
        write_credentials_file(&bytes)?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn write_credentials(creds: &Map<String, Value>) -> Result<(), String> {
    let bytes = serde_json::to_vec(&Value::Object(creds.clone())).map_err(|e| e.to_string())?;
    write_credentials_file(&bytes)
}

pub fn read_cli_login() -> Result<Option<CliLogin>, String> {
    let Some(oauth) = read_credentials()?.and_then(|c| c.get("claudeAiOauth").cloned()) else {
        return Ok(None);
    };
    let oauth_account = read_json(&claude_json_path())
        .map_err(|e| e.to_string())?
        .and_then(|c| c.get("oauthAccount").cloned());
    Ok(Some(CliLogin { oauth, oauth_account }))
}

pub fn write_cli_login(login: &CliLogin) -> Result<(), String> {
    // credentials: keep every other key (mcpOAuth, ...), replace only claudeAiOauth
    let mut creds = read_credentials()?.unwrap_or_default();
    creds.insert("claudeAiOauth".into(), login.oauth.clone());
    write_credentials(&creds)?;

    // identity: replace only oauthAccount in ~/.claude.json
    if let Some(account) = &login.oauth_account {
        let path = claude_json_path();
        if let Some(Value::Object(mut map)) = read_json(&path).map_err(|e| e.to_string())? {
            map.insert("oauthAccount".into(), account.clone());
            let bytes = serde_json::to_vec_pretty(&Value::Object(map)).map_err(|e| e.to_string())?;
            write_atomic(&path, &bytes).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn switching_replaces_only_the_login_and_keeps_everything_else() {
        let dir = std::env::temp_dir().join(format!("flipon-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        // SAFETY: this is the only test that touches CLAUDE_CONFIG_DIR.
        unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", &dir) };

        fs::write(
            dir.join(".credentials.json"),
            json!({"claudeAiOauth": {"accessToken": "old"}, "mcpOAuth": {"server": {"token": "keep"}}}).to_string(),
        )
        .unwrap();
        fs::write(
            dir.join(".claude.json"),
            json!({"oauthAccount": {"emailAddress": "old@x.com"}, "numStartups": 5}).to_string(),
        )
        .unwrap();

        let new = CliLogin {
            oauth: json!({"accessToken": "new", "subscriptionType": "max"}),
            oauth_account: Some(json!({"emailAddress": "New@X.com"})),
        };
        write_cli_login(&new).unwrap();

        let creds: Value = serde_json::from_str(&fs::read_to_string(dir.join(".credentials.json")).unwrap()).unwrap();
        assert_eq!(creds["claudeAiOauth"]["accessToken"], "new");
        assert_eq!(creds["mcpOAuth"]["server"]["token"], "keep");
        let global: Value = serde_json::from_str(&fs::read_to_string(dir.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(global["oauthAccount"]["emailAddress"], "New@X.com");
        assert_eq!(global["numStartups"], 5);

        let read = read_cli_login().unwrap().unwrap();
        assert_eq!(read.email().as_deref(), Some("new@x.com"));
        assert_eq!(read.subscription(), "max");
        assert!(!dir.join(".credentials.json.flipon-tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod keychain_tests {
    use super::*;

    #[test]
    fn service_name_matches_claude_code() {
        // default config dir: no suffix
        assert_eq!(keychain_service(None, None, "/Users/a/.claude"), "Claude Code-credentials");
        assert_eq!(keychain_service(None, Some(""), "/Users/a/.claude"), "Claude Code-credentials");
        // custom CLAUDE_CONFIG_DIR: first 8 hex of sha256 of the resolved dir
        let dir = "/Users/a/work-claude";
        use sha2::{Digest, Sha256};
        let expected: String = Sha256::digest(dir.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect();
        assert_eq!(keychain_service(None, Some(dir), dir), format!("Claude Code-credentials-{expected}"));
        // CLAUDE_SECURESTORAGE_CONFIG_DIR wins; empty means default
        assert_eq!(keychain_service(Some(""), Some(dir), dir), "Claude Code-credentials");
        assert!(keychain_service(Some("/x"), None, "/y").starts_with("Claude Code-credentials-"));
    }

    #[test]
    fn keychain_output_is_decoded_when_printed_as_hex() {
        let json = r#"{"claudeAiOauth":{"accessToken":"t"}}"#;
        let hex: String = json.bytes().map(|b| format!("{b:02x}")).collect();
        assert_eq!(decode_keychain_value(json), json);
        assert_eq!(decode_keychain_value(&hex), json);
        // a value that happens to be hex but is not JSON stays as it is
        assert_eq!(decode_keychain_value("deadbeef"), "deadbeef");
    }

    #[test]
    fn add_command_hex_encodes_the_value() {
        let line = keychain_add_command("claude-code-user", "Claude Code-credentials", b"{\"a\":1}");
        assert_eq!(line, "add-generic-password -U -a \"claude-code-user\" -s \"Claude Code-credentials\" -X \"7b2261223a317d\"\n");
    }
}
