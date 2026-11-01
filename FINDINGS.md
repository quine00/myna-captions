# Findings

What the 2026-11-01 prototype established, what it copied out of Myna (the
seed of a reusable library), and the questions still to settle. The parked
2026-08-25 design (`~/captioning-client.md`) is the earlier thinking; where it
conflicts with this file, this file has been measured.

## Measured

- **Tapping an application stream works.** `pw-record --target <serial>`
  links to a `Stream/Output/Audio` node and PipeWire resamples to 16 kHz mono.
  No portal, no consent dialog, no sink monitor needed for per-app capture.
- **Parakeet streams; Whisper as installed does not.** Parakeet sends an
  `unstable` hypothesis about every 2 s. The Whisper snap is in batch mode:
  only `status` pings until the end of the audio, which for a live caption
  never comes.
- **Parakeet's continuous-speech cost grows.** Over 34 s of unbroken speech
  it never committed, and the gap between hypotheses grew from 2.1 s to
  6.3 s: each update re-decodes the whole open utterance. The hypothesis also
  rewrites earlier words ("We've" / "Uh we've", "going to" / "gonna"), which
  reads as flicker.
- **Two sessions run concurrently on one backend.** Two 12 s Parakeet
  sessions in parallel kept identical cadence (first delta 2.2 s, terminal
  14.4 s). Captions and dictation can share a backend, CPU permitting.
  Longer sessions and dictation latency under load are unmeasured.
- **A new node per track ends the session.** `pw-play` (and Firefox, per
  media element) creates a fresh node for each item, and
  `node.dont-reconnect` ends the capture with it.

## Copied from Myna: the library candidates

Each of these is a reimplementation in this repo of something Myna already
has, coupled there to dictation. Together they are the library.

| Piece | Here | In Myna | Why it could not be reused |
|---|---|---|---|
| IE115 wire client | `session.rs` | `myna-orchestrator` `backend/ws_unix_ie115.rs` | Behind the dictation FSM's `BackendClient` trait, one commit per connection |
| Backend discovery | `session::backends` | myna-desktop / myna-config, `provider.env`, `myna_core::streams_by_default` | Scattered; no single "list installed engines and what they can do" call |
| Transcript assembly (items, committed vs unstable) | `transcript.rs` | the controller's commit/preedit handling | Shaped around injecting into a text field |
| Capture of an arbitrary node | `pw-record` subprocess | `myna-audio::native` | Source-only by construction, `Communication` role, fail-fast lifecycle |
| Streaming capability | sort by snap name | `Capabilities.streaming` | Needs a socket query; the prototype guesses |

A first cut of the library is roughly: `Engines::discover()`, an async
`Session` that takes PCM and yields `Event`s, a `Transcript` that folds
events into display text, and a capture `Tap` with node selectors and a
lifecycle policy (follow, survive idle, end with the node).

## Live decoding: why Parakeet stalls, and the methods

- **Myna today (Parakeet, `SilenceCut`):** commits only at a pause once the
  window is 15 s long (`SC_ARM_S`), or at a 60 s hard cut. Between cuts each
  draft re-decodes the whole open window (0.2 s at 10 s, 2.4 s at 60 s),
  throttled to half of real time (`PARTIAL_BUDGET`), so drafts slow as the
  window grows. A fast talker who never pauses runs the window to 60 s.
- **WhisperLive is no cheaper:** it re-decodes everything since the last
  commit. It commits when Whisper splits the window into segments, or when the
  output stays the same for 11 steps. Its caps (45 s trim, 25 s `clip_audio`)
  drop audio without committing it.
- **Methods that bound the cost of each step:**
  1. **A bounded window with timestamp commits.** Decode a few seconds of
     audio plus some audio before it for context. Commit the words that end
     more than about 1.5 s before "now", by token time, then slide the window
     up to them. This works with the shipped Parakeet TDT.
  2. **A cache-aware streaming model** (Nemotron or FastConformer
     cache-aware, a sherpa streaming transducer). The cost per step stays
     constant, but it is a new model family to ship.

## Decisions (grilling, 2026-11-01)

Product
- **Audience:** accessibility first (deaf and hard-of-hearing users). That
  sets the bar for reliability over hours, readability and low flicker.
- **Translation:** later. Reserve a target-language field in the API.
- **Transcript:** ephemeral by default. There is a scrollback view and an
  explicit "Save transcript" action.

Capture
- **Consent:** tap directly. The user's pick in our UI is the consent; no
  portal.
- **Following an app:** select by application and follow it. Tap every
  output node it creates and mix them.
- **Silence:** stay armed. Captions fade after a few seconds of silence, and
  the session closes only when the user closes it.
- **Microphone:** a source in v1, listed alongside the outputs.

Engine and API
- **Decoding methods:** build both the bounded window (1) and native
  streaming (2), and make it easy to switch between methods from the UI.
- **Wire API:** the app states an intent (dictation or captions) plus bounds
  (latency, revision span, CPU share). The server advertises the methods the
  loaded model supports in `session.created`, and an app may name one
  explicitly. That gives the UI a method drop-down built from what the
  backend reports.
- **Dictation:** must not lose accuracy. The pause-cut loop stays its
  default; the new methods are an advanced-settings choice.
- **Non-speech gating:** done in the server as part of the method (VAD plus
  the segment's no-speech score), advertised as a bound.
- **Choosing an engine:** behind a seam. The app asks for a capability (say
  `speech-to-text/live`) and gets the best model that provides it. Today a
  resolver scans the installed Myna snaps and their advertised methods.
  Later it is the Ubuntu platform's inference-snaps manager. Batch-only
  backends never answer a live request.

Library
- **Language:** Rust crate first. Add a GObject layer only when Python or
  GJS users appear.
- **Scope:** engines, sessions, transcript assembly and capture (node, app,
  mic and sink-monitor selectors, with fail-fast or stay-armed lifecycles).
  Dictation's `myna-audio` capture moves onto it.
- **Location:** the library lives in the Myna workspace. Myna Captions stays
  its own repo and depends on it.

Overlay
- **GNOME:** a separate Shell extension renders the captions, with its own
  D-Bus contract for transcript text. myna-shell keeps its rule against
  carrying transcript text. Elsewhere the overlay is a layer-shell or X11
  window.
- **Clicks:** pass through to whatever is underneath. A modifier or a handle
  reveals move and resize.
- **Multiple overlays:** one per source, each labelled.
- **Revisions:** a user setting. Options are a dim tail (as now), holding
  back unstable words, or committed text only.
- **Packaging:** a deb in the Myna PPA, like Myna Settings, with the
  extension alongside.

## Still open

- **Engine-selection seam:** its exact shape (D-Bus service, library trait,
  or both), and who owns "best" (accuracy, latency, hardware).
- **Settings:** stored in GSettings; whether position is remembered per
  source.
- **Above fullscreen video:** whether the Shell extension draws over it.
- **Evaluation:** a caption-shaped corpus (fast continuous speech, music beds,
  calls) to score commit latency, CPU and WER against batch for each method.
