//! One caption session: `pw-record` taps a node, its PCM streams to a Myna
//! backend over the IE115 wire, and the transcription frames come back as
//! [`Event`]s. The session never commits on its own: the backend decides
//! where utterances end, and the stream runs until the node goes away or the
//! caller stops it.

use std::path::PathBuf;
use std::process::Stdio;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio_tungstenite::tungstenite::Message;

use crate::nodes::Kind;

pub const SAMPLE_RATE: u32 = 16_000;
/// 100 ms of mono S16LE.
const CHUNK_BYTES: usize = SAMPLE_RATE as usize / 10 * 2;

#[derive(Debug)]
pub enum Event {
    Status(String),
    Delta { item: String, text: String, unstable: bool },
    Completed { item: String, text: String },
    /// The session is over; `None` when it ended cleanly.
    Ended(Option<String>),
}

pub struct Spec {
    pub socket: PathBuf,
    pub serial: u64,
    pub kind: Kind,
    pub language: Option<String>,
}

/// Run the session until `stop` fires or either end goes away.
pub async fn run(spec: Spec, events: async_channel::Sender<Event>, stop: tokio::sync::oneshot::Receiver<()>) {
    eprintln!("myna-captions: captioning node {} through {}", spec.serial, spec.socket.display());
    let result = stream(&spec, &events, stop).await;
    match &result {
        Ok(()) => eprintln!("myna-captions: node {} finished", spec.serial),
        Err(e) => eprintln!("myna-captions: node {} failed: {e}", spec.serial),
    }
    let _ = events.send(Event::Ended(result.err())).await;
}

async fn stream(
    spec: &Spec,
    events: &async_channel::Sender<Event>,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), String> {
    let mut recorder = record(spec)?;
    let mut pcm = recorder.stdout.take().ok_or("pw-record has no stdout")?;

    let socket = UnixStream::connect(&spec.socket)
        .await
        .map_err(|e| format!("{}: {e}", spec.socket.display()))?;
    let (ws, _) = tokio_tungstenite::client_async("ws://localhost/", socket)
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    let (mut tx, mut rx) = ws.split();
    tx.send(Message::text(session_update(spec.language.as_deref()).to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut audio_open = true;
    loop {
        tokio::select! {
            _ = &mut stop, if audio_open => {
                audio_open = false;
                let _ = recorder.start_kill();
                commit(&mut tx).await?;
            }
            read = pcm.read(&mut buf), if audio_open => match read {
                Ok(0) | Err(_) => {
                    audio_open = false;
                    commit(&mut tx).await?;
                }
                Ok(n) => tx.send(Message::binary(buf[..n].to_vec())).await.map_err(|e| e.to_string())?,
            },
            frame = rx.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    if let Some(done) = dispatch(&text, events).await? {
                        if !audio_open && done {
                            return Ok(());
                        }
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.to_string()),
                None => return if audio_open { Err("the backend closed the connection".into()) } else { Ok(()) },
            },
        }
    }
}

fn record(spec: &Spec) -> Result<tokio::process::Child, String> {
    let mut command = Command::new("pw-record");
    command
        .args(["--target", &spec.serial.to_string()])
        .args(["--rate", &SAMPLE_RATE.to_string(), "--channels", "1", "--format", "s16"])
        .args(["--media-category", "Capture", "--media-role", "Accessibility"]);
    let mut props = String::from("{ node.dont-reconnect=true node.name=myna-captions node.description=\"Myna Captions\"");
    if spec.kind == Kind::Sink {
        props.push_str(" stream.capture.sink=true");
    }
    props.push_str(" }");
    command
        .args(["-P", &props, "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("pw-record: {e}"))
}

fn session_update(language: Option<&str>) -> Value {
    let mut input = json!({ "format": { "type": "audio/pcm", "rate": SAMPLE_RATE } });
    if let Some(language) = language {
        input["transcription"] = json!({ "language": language });
    }
    json!({ "type": "session.update", "session": { "type": "transcription", "audio": { "input": input } } })
}

async fn commit<S>(tx: &mut S) -> Result<(), String>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    tx.send(Message::text(json!({ "type": "input_audio_buffer.commit" }).to_string()))
        .await
        .map_err(|e| e.to_string())
}

/// Forward one server frame. `Some(true)` marks an item's terminal.
async fn dispatch(text: &str, events: &async_channel::Sender<Event>) -> Result<Option<bool>, String> {
    let Some(event) = parse(text)? else { return Ok(None) };
    let terminal = matches!(event, Event::Completed { .. });
    let _ = events.send(event).await;
    Ok(Some(terminal))
}

fn parse(text: &str) -> Result<Option<Event>, String> {
    let frame: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let str_of = |k: &str| frame.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    Ok(match frame.get("type").and_then(Value::as_str) {
        Some("status") => Some(Event::Status(str_of("state"))),
        Some("conversation.item.input_audio_transcription.delta") => Some(Event::Delta {
            item: str_of("item_id"),
            text: str_of("delta"),
            unstable: frame.get("disposition").and_then(Value::as_str) == Some("unstable"),
        }),
        Some("conversation.item.input_audio_transcription.completed") => Some(Event::Completed {
            item: str_of("item_id"),
            text: str_of("transcript"),
        }),
        Some("error") => {
            let error = frame.get("error").cloned().unwrap_or_default();
            return Err(format!(
                "{}: {}",
                error.get("code").and_then(Value::as_str).unwrap_or("error"),
                error.get("message").and_then(Value::as_str).unwrap_or("")
            ));
        }
        _ => None,
    })
}

/// The inference sockets installed Myna backends provide, best first.
pub fn backends() -> Vec<(String, PathBuf)> {
    let Ok(dirs) = std::fs::read_dir("/var/snap") else { return Vec::new() };
    let mut found: Vec<(String, PathBuf)> = dirs
        .flatten()
        .filter_map(|dir| {
            let socket = dir.path().join("common/share/provider/myna.sock");
            socket.exists().then(|| (dir.file_name().to_string_lossy().into_owned(), socket))
        })
        .collect();
    // Parakeet is the one family that streams out of the box; the others
    // answer only at the end of the audio, which for captions is never.
    found.sort_by_key(|(name, _)| (name != "myna-parakeet", name.clone()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_unstable_delta() {
        let event = parse(r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"i1","delta":"Hi","disposition":"unstable"}"#);
        assert!(matches!(event, Ok(Some(Event::Delta { unstable: true, .. }))));
    }

    #[test]
    fn a_delta_without_disposition_is_committed() {
        let event = parse(r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"i1","delta":"Hi"}"#);
        assert!(matches!(event, Ok(Some(Event::Delta { unstable: false, .. }))));
    }

    #[test]
    fn an_error_frame_ends_the_session() {
        let event = parse(r#"{"type":"error","error":{"code":"busy","message":"one at a time"}}"#);
        assert_eq!(event.err().as_deref(), Some("busy: one at a time"));
    }

    #[test]
    fn language_rides_in_transcription() {
        assert_eq!(session_update(Some("fr"))["session"]["audio"]["input"]["transcription"]["language"], "fr");
        assert!(session_update(None)["session"]["audio"]["input"].get("transcription").is_none());
    }
}
