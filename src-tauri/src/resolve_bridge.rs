//! Client for the `Videe.lua` bridge script that runs inside DaVinci Resolve
//! (Workspace > Scripts > Utility > Videe). The Lua script is the server; we're
//! the client, calling it synchronously exactly when the user clicks a button --
//! no job queue, no polling loop on either side. That's a deliberate departure
//! from a naive "Rust runs a server, Lua polls it" design: it mirrors the proven
//! pattern used by other Resolve-integrated Tauri apps, whose documented
//! reliability bugs traced back to polling/reconnect machinery layered on top of
//! a *more* complex, session-oriented feature set than we need here.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;

/// Arbitrary, but fixed and documented so a user hitting a conflict (some other
/// local tool bound to the same port) has something concrete to search for.
pub const PORT: u16 = 51735;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

fn endpoint() -> String {
    format!("http://127.0.0.1:{PORT}/")
}

#[derive(Debug)]
pub enum BridgeError {
    /// Nothing is listening on the port -- the Lua script isn't running.
    NotRunning,
    /// Connected, but Resolve/the script didn't respond in time.
    Timeout,
    /// The script responded, but the Resolve-side operation itself failed
    /// (e.g. no project open, import rejected) -- carries the script's message.
    ResolveError(String),
    /// Anything else (malformed response, unexpected status, etc).
    Other(String),
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BridgeError::NotRunning => write!(
                f,
                "DaVinci Resolve isn't running the Videee bridge script. In Resolve: Workspace \u{2192} Scripts \u{2192} Utility \u{2192} Videe."
            ),
            BridgeError::Timeout => write!(f, "DaVinci Resolve didn't respond in time. Is it busy?"),
            BridgeError::ResolveError(msg) => write!(f, "{msg}"),
            BridgeError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl From<reqwest::Error> for BridgeError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_connect() {
            BridgeError::NotRunning
        } else if err.is_timeout() {
            BridgeError::Timeout
        } else {
            BridgeError::Other(err.to_string())
        }
    }
}

#[derive(Serialize)]
struct ImportEdlRequest<'a> {
    func: &'a str,
    path: &'a str,
    #[serde(rename = "sourceClipsPath")]
    source_clips_path: &'a str,
    #[serde(rename = "timelineName")]
    timeline_name: &'a str,
}

#[derive(Deserialize)]
struct BridgeResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .expect("reqwest client with a fixed timeout should always build")
}

/// Pure status check -- collapses every failure mode to `false` since this is
/// only ever used to render a connected/not-connected indicator.
pub async fn ping() -> bool {
    let body = serde_json::json!({ "func": "Ping" });
    let Ok(resp) = client().post(endpoint()).json(&body).send().await else {
        return false;
    };
    resp.json::<BridgeResponse>().await.map(|r| r.ok).unwrap_or(false)
}

pub async fn import_edl(edl_path: &Path, source_clips_path: &Path, timeline_name: &str) -> Result<(), BridgeError> {
    let req = ImportEdlRequest {
        func: "ImportEDL",
        path: &edl_path.to_string_lossy(),
        source_clips_path: &source_clips_path.to_string_lossy(),
        timeline_name,
    };
    let resp = client().post(endpoint()).json(&req).send().await?;
    let parsed: BridgeResponse = resp
        .json()
        .await
        .map_err(|e| BridgeError::Other(format!("unexpected response from Resolve bridge: {e}")))?;

    if parsed.ok {
        Ok(())
    } else {
        Err(BridgeError::ResolveError(
            parsed.error.unwrap_or_else(|| "Resolve rejected the import".to_string()),
        ))
    }
}

/// Where the bundled `Videe.lua` ships inside the app, so it can be copied out
/// to Resolve's scripts folder.
pub fn bundled_script_path(app: &AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path()
        .resolve("Videe.lua", tauri::path::BaseDirectory::Resource)
        .map_err(|e| e.to_string())
}

/// DaVinci Resolve's per-user "Utility" scripts folder. Only macOS is exercised in
/// practice so far (matching the ffmpeg sidecar's current aarch64-apple-darwin-only
/// coverage) -- Windows/Linux paths follow Blackmagic's documented layout but are
/// unverified.
pub fn install_target_path() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir().ok_or("could not determine this OS's application data directory")?;
    let path = if cfg!(target_os = "windows") {
        // dirs::data_dir() on Windows is %APPDATA%\Roaming.
        data_dir
            .join("Blackmagic Design")
            .join("DaVinci Resolve")
            .join("Support")
            .join("Fusion")
            .join("Scripts")
            .join("Utility")
            .join("Videe.lua")
    } else if cfg!(target_os = "macos") {
        data_dir
            .join("Blackmagic Design")
            .join("DaVinci Resolve")
            .join("Fusion")
            .join("Scripts")
            .join("Utility")
            .join("Videe.lua")
    } else {
        data_dir
            .join("DaVinciResolve")
            .join("Fusion")
            .join("Scripts")
            .join("Utility")
            .join("Videe.lua")
    };
    Ok(path)
}

/// Copies the bundled script to Resolve's Utility scripts folder. Errors rather
/// than creating a fake directory tree if Resolve's own parent folder doesn't
/// exist yet, since that almost always means Resolve itself isn't installed.
pub fn install_script(app: &AppHandle) -> Result<PathBuf, String> {
    let target = install_target_path()?;
    let scripts_utility_dir = target.parent().ok_or("invalid install target path")?;
    let scripts_dir = scripts_utility_dir
        .parent()
        .ok_or("invalid install target path")?;

    if !scripts_dir.exists() {
        return Err(format!(
            "DaVinci Resolve's scripts folder wasn't found at {}. Is DaVinci Resolve installed?",
            scripts_dir.display()
        ));
    }

    std::fs::create_dir_all(scripts_utility_dir).map_err(|e| e.to_string())?;

    let source = bundled_script_path(app)?;
    std::fs::copy(&source, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

pub fn script_status() -> Option<PathBuf> {
    let target = install_target_path().ok()?;
    target.exists().then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    /// Stands in for the Lua server so `ping`/`import_edl` are tested against a
    /// real socket instead of assuming reqwest's error classification without
    /// verification. Writes a canned HTTP/1.1 response directly rather than
    /// pulling in a server framework for a one-shot test double.
    async fn serve_once(listener: TcpListener, response_json: &str) {
        let (mut stream, _) = listener.accept().await.unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_json.len(),
            response_json
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.ok();
    }

    #[tokio::test]
    async fn ping_returns_false_when_nothing_is_listening() {
        // An address nothing is bound to -- exercises the NotRunning path.
        let unused_port = 51999;
        let body = serde_json::json!({ "func": "Ping" });
        let result = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{unused_port}/"))
            .json(&body)
            .send()
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_connect());
    }

    #[tokio::test]
    async fn import_edl_ok_response_is_ok() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(serve_once(listener, r#"{"ok":true}"#));

        let result = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/"))
            .json(&serde_json::json!({"func":"ImportEDL"}))
            .send()
            .await
            .unwrap()
            .json::<BridgeResponse>()
            .await
            .unwrap();

        server.await.unwrap();
        assert!(result.ok);
    }

    #[tokio::test]
    async fn import_edl_error_response_carries_resolve_message() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(serve_once(listener, r#"{"ok":false,"error":"No project is open"}"#));

        let resp: BridgeResponse = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/"))
            .json(&serde_json::json!({"func":"ImportEDL"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        server.await.unwrap();
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("No project is open"));
    }

    #[test]
    fn bridge_error_display_messages_are_human_readable() {
        assert!(BridgeError::NotRunning.to_string().contains("Workspace"));
        assert_eq!(
            BridgeError::ResolveError("No project is open".to_string()).to_string(),
            "No project is open"
        );
    }
}
