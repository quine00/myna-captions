//! One caption session: `pw-record` taps a source, its PCM streams to a
//! Myna backend over the IE115 wire, and the transcription frames come back
//! as [`Event`]s. The session never commits on its own: the backend decides
//! where utterances end. A source whose node goes away (a tab closed, the
//! next track) is followed to its next node, and the session stays armed
//! meanwhile; only the caller ends it.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::nodes::{self, Kind};

pub const SAMPLE_RATE: u32 = 16_000;
/// 100 ms of mono S16LE.
const CHUNK_BYTES: usize = SAMPLE_RATE as usize / 10 * 2;
/// How long a stopped session waits for the backend's last words.
const DRAIN: Duration = Duration::from_secs(5);
/// How often a source with no node is looked for again.
const FOLLOW_POLL: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum Event {
    Status(String),
    Delta {
        item: String,
        text: String,
        unstable: bool,
    },
    Completed {
        item: String,
        text: String,
    },
    /// The session is over; `None` when it ended cleanly.
    Ended(Option<String>),
}

pub struct Spec {
    pub socket: PathBuf,
    pub key: String,
    pub language: Option<String>,
}

/// Run the session until `stop` fires or the backend goes away.
pub async fn run(
    spec: Spec,
    events: async_channel::Sender<Event>,
    stop: tokio::sync::oneshot::Receiver<()>,
) {
    eprintln!(
        "myna-captions: captioning {} through {}",
        spec.key,
        spec.socket.display()
    );
    let result = stream(&spec, &events, stop).await;
    match &result {
        Ok(()) => eprintln!("myna-captions: {} finished", spec.key),
        Err(e) => eprintln!("myna-captions: {} failed: {e}", spec.key),
    }
    let _ = events.send(Event::Ended(result.err())).await;
}

async fn stream(
    spec: &Spec,
    events: &async_channel::Sender<Event>,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), String> {
    let socket = UnixStream::connect(&spec.socket)
        .await
        .map_err(|e| format!("{}: {e}", spec.socket.display()))?;
    let (ws, _) = tokio_tungstenite::client_async("ws://localhost/", socket)
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    let (mut tx, mut rx) = ws.split();
    tx.send(Message::text(
        session_update(spec.language.as_deref()).to_string(),
    ))
    .await
    .map_err(|e| e.to_string())?;

    let (pcm_tx, mut pcm) = mpsc::channel::<Vec<u8>>(64);
    let capture = tokio::spawn(follow(spec.key.clone(), pcm_tx));
    let mut audio_open = true;
    let drain = tokio::time::sleep(Duration::MAX);
    tokio::pin!(drain);
    let result = loop {
        tokio::select! {
            _ = &mut stop, if audio_open => {
                audio_open = false;
                capture.abort();
                commit(&mut tx).await?;
                drain.as_mut().reset(tokio::time::Instant::now() + DRAIN);
            }
            _ = &mut drain => break Ok(()),
            block = pcm.recv(), if audio_open => match block {
                Some(block) => tx.send(Message::binary(block)).await.map_err(|e| e.to_string())?,
                None => break Err("capture stopped".into()),
            },
            frame = rx.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    if let Some(done) = dispatch(&text, events).await? {
                        if !audio_open && done {
                            break Ok(());
                        }
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => break Err(e.to_string()),
                None => break if audio_open { Err("the backend closed the connection".into()) } else { Ok(()) },
            },
        }
    };
    capture.abort();
    result
}

/// Tap whichever node `key` resolves to, again each time it goes away.
async fn follow(key: String, pcm: mpsc::Sender<Vec<u8>>) {
    loop {
        let found = tokio::task::spawn_blocking({
            let key = key.clone();
            move || nodes::resolve(&nodes::list(), &key)
        })
        .await
        .ok()
        .flatten();
        let Some(node) = found else {
            tokio::time::sleep(FOLLOW_POLL).await;
            continue;
        };
        let Ok(mut recorder) = record(node.serial, &node.kind) else {
            tokio::time::sleep(FOLLOW_POLL).await;
            continue;
        };
        let Some(mut out) = recorder.stdout.take() else {
            continue;
        };
        let mut buf = vec![0u8; CHUNK_BYTES];
        loop {
            match out.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if pcm.send(buf[..n].to_vec()).await.is_err() {
                        return;
                    }
                }
            }
        }
        let _ = recorder.kill().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn record(serial: u64, kind: &Kind) -> Result<tokio::process::Child, String> {
    let mut command = Command::new("pw-record");
    command
        .args(["--target", &serial.to_string()])
        .args([
            "--rate",
            &SAMPLE_RATE.to_string(),
            "--channels",
            "1",
            "--format",
            "s16",
        ])
        .args([
            "--media-category",
            "Capture",
            "--media-role",
            "Accessibility",
        ]);
    let mut props = String::from(
        "{ node.dont-reconnect=true node.name=myna-captions node.description=\"Myna Captions\"",
    );
    if *kind == Kind::Sink {
        props.push_str(" stream.capture.sink=true");
    }
    props.push_str(" }");
    // The kernel ends the tap with the app, however the app ends: dropping
    // the child only covers an orderly exit, and an orphaned pw-record
    // would go on capturing.
    // SAFETY: prctl is async-signal-safe and touches only this process.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
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
    tx.send(Message::text(
        json!({ "type": "input_audio_buffer.commit" }).to_string(),
    ))
    .await
    .map_err(|e| e.to_string())
}

/// Forward one server frame. `Some(true)` marks an item's terminal.
async fn dispatch(
    text: &str,
    events: &async_channel::Sender<Event>,
) -> Result<Option<bool>, String> {
    let Some(event) = parse(text)? else {
        return Ok(None);
    };
    let terminal = matches!(event, Event::Completed { .. });
    let _ = events.send(event).await;
    Ok(Some(terminal))
}

fn parse(text: &str) -> Result<Option<Event>, String> {
    let frame: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let str_of = |k: &str| {
        frame
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
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

/// The engines to offer, best first: caption methods served locally under
/// `$XDG_RUNTIME_DIR/myna-captions/` (each `NAME.sock` labelled by an
/// optional `NAME.label`), then the installed Myna backend snaps.
pub fn backends() -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    if let Some(dir) =
        std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("myna-captions"))
    {
        let mut local: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "sock"))
            .map(|socket| {
                let label = std::fs::read_to_string(socket.with_extension("label"))
                    .map(|l| l.trim().to_owned())
                    .unwrap_or_else(|_| {
                        socket
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    });
                (label, socket)
            })
            .collect();
        local.sort();
        found.extend(local);
    }
    let mut snaps: Vec<(String, PathBuf)> = std::fs::read_dir("/var/snap")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|dir| {
            let socket = dir.path().join("common/share/provider/myna.sock");
            socket
                .exists()
                .then(|| (dir.file_name().to_string_lossy().into_owned(), socket))
        })
        .collect();
    snaps.sort_by_key(|(name, _)| (snap_rank(name), name.clone()));
    found.extend(snaps);
    found
}

/// Nemotron commits each word as it is spoken, Parakeet at pauses; the other
/// snaps answer only at the end of the audio, which for captions is never.
fn snap_rank(name: &str) -> u8 {
    match name {
        "myna-nemotron" => 0,
        "myna-parakeet" => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_unstable_delta() {
        let event = parse(
            r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"i1","delta":"Hi","disposition":"unstable"}"#,
        );
        assert!(matches!(
            event,
            Ok(Some(Event::Delta { unstable: true, .. }))
        ));
    }

    #[test]
    fn a_delta_without_disposition_is_committed() {
        let event = parse(
            r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"i1","delta":"Hi"}"#,
        );
        assert!(matches!(
            event,
            Ok(Some(Event::Delta {
                unstable: false,
                ..
            }))
        ));
    }

    #[test]
    fn an_error_frame_ends_the_session() {
        let event = parse(r#"{"type":"error","error":{"code":"busy","message":"one at a time"}}"#);
        assert_eq!(event.err().as_deref(), Some("busy: one at a time"));
    }

    #[test]
    fn language_rides_in_transcription() {
        assert_eq!(
            session_update(Some("fr"))["session"]["audio"]["input"]["transcription"]["language"],
            "fr"
        );
        assert!(session_update(None)["session"]["audio"]["input"]
            .get("transcription")
            .is_none());
    }

    #[test]
    fn streaming_snaps_are_offered_first() {
        let mut names = [
            "myna-whisper",
            "myna-parakeet",
            "myna-funasr",
            "myna-nemotron",
        ];
        names.sort_by_key(|name| (snap_rank(name), *name));
        assert_eq!(
            names,
            [
                "myna-nemotron",
                "myna-parakeet",
                "myna-funasr",
                "myna-whisper"
            ]
        );
    }
}
