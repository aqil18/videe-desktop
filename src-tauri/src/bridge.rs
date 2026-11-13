//! Client for the bridges that run inside the NLEs: `Videe.lua` in DaVinci
//! Resolve (Workspace > Scripts > Utility > Videe) and the `com.videe.bridge`
//! CEP panel in Premiere Pro (Window > Extensions > Videe). The NLE side is the
//! server; we're the client, calling it synchronously exactly when the user
//! clicks a button -- no job queue, no polling loop on either side. That's a
//! deliberate departure from a naive "Rust runs a server, the NLE polls it"
//! design: it mirrors the proven pattern used by other Resolve-integrated Tauri
//! apps, whose documented reliability bugs traced back to polling/reconnect
//! machinery layered on top of a *more* complex, session-oriented feature set
//! than we need here.
//!
//! Both bridges speak the same protocol: an HTTP POST of a flat JSON object
//! (`func` plus string fields), answered with `{"ok": bool, "error"?: string}`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nle {
    Resolve,
    Premiere,
}

impl Nle {
    /// Arbitrary, but fixed and documented so a user hitting a conflict (some
    /// other local tool bound to the same port) has something concrete to search
    /// for. Separate ports so both NLEs can be open at once.
    pub fn port(self) -> u16 {
        match self {
            Nle::Resolve => 51735,
            Nle::Premiere => 51736,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Nle::Resolve => "DaVinci Resolve",
            Nle::Premiere => "Premiere Pro",
        }
    }

    fn not_running_message(self) -> &'static str {
        match self {
            Nle::Resolve => {
                "DaVinci Resolve isn't running the Videe bridge script. In Resolve: Workspace \u{2192} Scripts \u{2192} Utility \u{2192} Videe."
            }
            Nle::Premiere => {
                "Premiere Pro isn't running the Videe panel. In Premiere: Window \u{2192} Extensions \u{2192} Videe. If it's not listed, click Install Extension in Videe and restart Premiere."
            }
        }
    }

    fn outdated_message(self) -> &'static str {
        match self {
            Nle::Resolve => {
                "The Videe script running in DaVinci Resolve is out of date. Click Install Script in Videe, then in Resolve run Workspace \u{2192} Scripts \u{2192} Utility \u{2192} Videe again. If Resolve's console (Workspace \u{2192} Console) says an older copy is still running, follow the steps it prints."
            }
            Nle::Premiere => {
                "The Videe panel in Premiere Pro is out of date. Click Install Extension in Videe, then close and reopen the Videe panel in Premiere (Window \u{2192} Extensions \u{2192} Videe)."
            }
        }
    }
}

#[derive(Debug)]
pub enum BridgeError {
    /// Nothing is listening on the port -- the bridge isn't running.
    NotRunning(Nle),
    /// Connected, but the NLE didn't respond in time.
    Timeout(Nle),
    /// The bridge responded, but the NLE-side operation itself failed (e.g. no
    /// project open, import rejected) -- carries the bridge's message.
    NleError(String),
    /// Anything else (malformed response, unexpected status, etc).
    Other(String),
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BridgeError::NotRunning(nle) => write!(f, "{}", nle.not_running_message()),
            BridgeError::Timeout(nle) => write!(f, "{} didn't respond in time. Is it busy?", nle.name()),
            BridgeError::NleError(msg) => write!(f, "{msg}"),
            BridgeError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

fn classify(err: reqwest::Error, nle: Nle) -> BridgeError {
    if err.is_connect() {
        BridgeError::NotRunning(nle)
    } else if err.is_timeout() {
        BridgeError::Timeout(nle)
    } else {
        BridgeError::Other(err.to_string())
    }
}

#[derive(Serialize)]
struct CreateTimelineRequest<'a> {
    func: &'a str,
    /// See `encode_clip_paths`.
    clips: String,
    #[serde(rename = "timelineName")]
    timeline_name: &'a str,
    /// See `encode_markers`.
    markers: String,
}

/// A timestamp to add as a marker on the new timeline, positioned relative to
/// the clip it belongs to rather than as an absolute timeline position. The
/// bridge resolves it against where the NLE actually placed that clip, so it
/// doesn't depend on the timeline's start timecode or on the timeline frame
/// rate matching the clip's.
pub struct ClipMarker {
    /// 0-based index into the clip list sent with the request.
    pub clip_index: usize,
    /// Seconds from the start of the clip.
    pub offset_seconds: f64,
    pub label: String,
}

/// The Lua bridge's JSON decoder only handles flat string fields, so the clip
/// list travels as one string, an absolute path per line. A path containing a
/// line break can't be framed that way, so it's rejected rather than mangled.
fn encode_clip_paths(paths: &[PathBuf]) -> Result<String, BridgeError> {
    let mut lines = Vec::with_capacity(paths.len());
    for path in paths {
        let s = path.to_string_lossy();
        if s.contains(['\n', '\r']) {
            return Err(BridgeError::Other(format!("can't send a file whose path contains a line break: {s:?}")));
        }
        lines.push(s.into_owned());
    }
    Ok(lines.join("\n"))
}

/// Markers travel the same way: a line per marker of `<1-based clip>\t<offset
/// seconds>\t<label>`. Tabs, newlines and other control characters in labels
/// would break that framing, so they're replaced with spaces.
fn encode_markers(markers: &[ClipMarker]) -> String {
    markers
        .iter()
        .map(|m| {
            let label: String = m.label.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
            format!("{}\t{:.6}\t{}", m.clip_index + 1, m.offset_seconds.max(0.0), label)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The running bridge predates `CreateTimeline` -- point the user at
/// reinstalling instead of echoing the bridge's message.
fn explain_nle_error(nle: Nle, msg: String) -> BridgeError {
    if msg.starts_with("unknown func") {
        BridgeError::NleError(nle.outdated_message().to_string())
    } else {
        BridgeError::NleError(msg)
    }
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

fn endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

/// Pure status check -- collapses every failure mode to `false` since this is
/// only ever used to render a connected/not-connected indicator.
pub async fn ping(nle: Nle) -> bool {
    ping_at(nle.port()).await
}

async fn ping_at(port: u16) -> bool {
    let body = serde_json::json!({ "func": "Ping" });
    let Ok(resp) = client().post(endpoint(port)).json(&body).send().await else {
        return false;
    };
    resp.json::<BridgeResponse>().await.map(|r| r.ok).unwrap_or(false)
}

/// Asks the NLE to import `clip_paths` (reusing any already in the project),
/// build a timeline named `timeline_name` from them in order, and add `markers`.
pub async fn create_timeline(nle: Nle, clip_paths: &[PathBuf], timeline_name: &str, markers: &[ClipMarker]) -> Result<(), BridgeError> {
    create_timeline_at(nle, nle.port(), clip_paths, timeline_name, markers).await
}

async fn create_timeline_at(
    nle: Nle,
    port: u16,
    clip_paths: &[PathBuf],
    timeline_name: &str,
    markers: &[ClipMarker],
) -> Result<(), BridgeError> {
    let req = CreateTimelineRequest {
        func: "CreateTimeline",
        clips: encode_clip_paths(clip_paths)?,
        timeline_name,
        markers: encode_markers(markers),
    };
    let resp = client().post(endpoint(port)).json(&req).send().await.map_err(|e| classify(e, nle))?;
    let parsed: BridgeResponse = resp
        .json()
        .await
        .map_err(|e| BridgeError::Other(format!("unexpected response from the {} bridge: {e}", nle.name())))?;

    if parsed.ok {
        Ok(())
    } else {
        Err(explain_nle_error(
            nle,
            parsed.error.unwrap_or_else(|| format!("{} rejected the import", nle.name())),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Stands in for the NLE bridge so `ping`/`create_timeline` are tested against
    /// a real socket instead of assuming reqwest's error classification without
    /// verification. Writes a canned HTTP/1.1 response directly rather than
    /// pulling in a server framework for a one-shot test double, and returns the
    /// raw request it received.
    async fn serve_once(listener: TcpListener, response_json: &'static str) -> String {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        // Read until the headers and the declared body have arrived.
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            request.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&request);
            if let Some(header_end) = text.find("\r\n\r\n") {
                let content_length = text[..header_end]
                    .lines()
                    .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_json.len(),
            response_json
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.ok();
        String::from_utf8_lossy(&request).into_owned()
    }

    async fn listener() -> (TcpListener, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    #[tokio::test]
    async fn not_running_when_nothing_is_listening() {
        // Bind then drop to get a port nothing is listening on.
        let (l, port) = listener().await;
        drop(l);
        assert!(!ping_at(port).await);
        let err = create_timeline_at(Nle::Premiere, port, &[PathBuf::from("/a.mov")], "t", &[]).await.unwrap_err();
        assert!(matches!(err, BridgeError::NotRunning(Nle::Premiere)), "{err:?}");
    }

    #[tokio::test]
    async fn ping_true_when_bridge_answers_ok() {
        let (l, port) = listener().await;
        let server = tokio::spawn(serve_once(l, r#"{"ok":true}"#));
        assert!(ping_at(port).await);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn create_timeline_sends_json_with_flat_string_fields() {
        let (l, port) = listener().await;
        let server = tokio::spawn(serve_once(l, r#"{"ok":true}"#));
        let markers = [ClipMarker { clip_index: 0, offset_seconds: 2.0, label: "Take".to_string() }];
        create_timeline_at(Nle::Premiere, port, &[PathBuf::from("/lib/a.mov")], "Videe Export", &markers)
            .await
            .unwrap();
        let request = server.await.unwrap();
        // The Premiere panel rejects anything that isn't a JSON POST to 127.0.0.1:<port>.
        let lower = request.to_ascii_lowercase();
        assert!(request.starts_with("POST / HTTP/1.1"), "{request}");
        assert!(lower.contains("content-type: application/json"), "{request}");
        assert!(lower.contains(&format!("host: 127.0.0.1:{port}")), "{request}");
        assert!(!lower.contains("origin:"), "{request}");
        assert!(request.contains(r#""func":"CreateTimeline""#), "{request}");
        assert!(request.contains(r#""clips":"/lib/a.mov""#), "{request}");
        assert!(request.contains(r#""markers":"1\t2.000000\tTake""#), "{request}");
    }

    #[tokio::test]
    async fn create_timeline_error_response_carries_nle_message() {
        let (l, port) = listener().await;
        let server = tokio::spawn(serve_once(l, r#"{"ok":false,"error":"No project is open"}"#));
        let err = create_timeline_at(Nle::Resolve, port, &[PathBuf::from("/a.mov")], "t", &[]).await.unwrap_err();
        server.await.unwrap();
        assert_eq!(err.to_string(), "No project is open");
    }

    #[test]
    fn markers_encode_as_one_line_each_with_1_based_clips_and_safe_labels() {
        let encoded = encode_markers(&[
            ClipMarker { clip_index: 0, offset_seconds: 1.5, label: "Best take".to_string() },
            ClipMarker { clip_index: 2, offset_seconds: 12.0, label: "tab\there\nnewline".to_string() },
        ]);
        assert_eq!(encoded, "1\t1.500000\tBest take\n3\t12.000000\ttab here newline");
        assert_eq!(encode_markers(&[]), "");
    }

    #[test]
    fn clip_paths_encode_one_per_line_and_reject_line_breaks() {
        let encoded = encode_clip_paths(&[PathBuf::from("/lib/a b.mov"), PathBuf::from("/lib/sub/IMG_1628.MOV")]).unwrap();
        assert_eq!(encoded, "/lib/a b.mov\n/lib/sub/IMG_1628.MOV");
        assert!(encode_clip_paths(&[PathBuf::from("/lib/bad\nname.mov")]).is_err());
    }

    #[test]
    fn outdated_bridge_error_tells_user_to_reinstall() {
        let err = explain_nle_error(Nle::Resolve, "unknown func: CreateTimeline".to_string()).to_string();
        assert!(err.contains("out of date") && err.contains("Install Script"), "{err}");
        let err = explain_nle_error(Nle::Premiere, "unknown func: CreateTimeline".to_string()).to_string();
        assert!(err.contains("out of date") && err.contains("Install Extension"), "{err}");
        assert_eq!(explain_nle_error(Nle::Resolve, "No project is open".to_string()).to_string(), "No project is open");
    }

    #[test]
    fn bridge_error_display_messages_are_human_readable() {
        assert!(BridgeError::NotRunning(Nle::Resolve).to_string().contains("Workspace"));
        assert!(BridgeError::NotRunning(Nle::Premiere).to_string().contains("Window"));
        assert_eq!(BridgeError::NleError("No project is open".to_string()).to_string(), "No project is open");
    }

    #[test]
    fn nles_use_distinct_ports() {
        assert_ne!(Nle::Resolve.port(), Nle::Premiere.port());
    }
}
