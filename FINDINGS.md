# Findings

What the 2026-11-01/02 prototype established, what it copied out of Myna (the
seed of a reusable library), and the questions still to settle. The parked
2026-08-25 design (`~/captioning-client.md`) is the earlier thinking; where it
conflicts with this file, this file has been measured.

## Overnight feasibility run (2026-11-02)

Question: can live captions feel snappy and accurate on this laptop?
**Yes, with a streaming model.** Nemotron (NVIDIA's cache-aware streaming
English model) commits every word within about a second, never rewrites
one, and is as accurate as Parakeet in batch.

Two clips, played in real time and scored against human references:
`nogaps` (5 min of LibriSpeech readings butted together, a talker who never
pauses) and `alestro` (6 min of a TV episode with music and crosstalk,
scored against its own human subtitles, which are edited and so put a ~10%
floor under every engine).

| clip | method | WER | commit delay p50 / p90 / max | first shown p50 | draft rewrites/min | CPU s per audio s |
|---|---|---|---|---|---|---|
| nogaps | Parakeet batch (reference) | 6.1% | - | - | - | - |
| nogaps | Parakeet shipped (pause cuts) | 12.2% | 36.8 / 59.6 / 70.0 s | 3.5 s | 361 | 1.79 |
| nogaps | Parakeet bounded, 1 s ticks | 8.2% | 3.2 / 9.2 / 20.3 s | 0.6 s | 188 | 1.24 |
| nogaps | Parakeet bounded, 0.5 s ticks | 12.5% | 1.7 / 4.7 / 14.5 s | 0.4 s | 121 | 1.86 |
| nogaps | **Nemotron, 1.12 s chunks** | **3.7%** | **0.9 / 1.3 / 1.8 s** | 0.9 s | **0** | 1.40 |
| nogaps | **Nemotron, 0.56 s chunks** | **3.7%** | **0.5 / 0.9 / 1.3 s** | 0.6 s | **0** | 2.34 |
| alestro | Parakeet batch (reference) | 10.7% | - | - | - | - |
| alestro | Parakeet shipped (pause cuts) | 9.6% | 24.0 / 53.1 / 66.5 s | 1.8 s | 751 | 1.40 |
| alestro | Parakeet bounded, 1 s ticks | 11.4% | 3.6 / 12.2 / 19.3 s | 0.6 s | 338 | 0.96 |
| alestro | Parakeet bounded, 0.5 s ticks | 16.4% | 1.5 / 3.6 / 18.8 s | 0.3 s | 250 | 1.26 |
| alestro | **Nemotron, 1.12 s chunks** | 12.2% | **0.9 / 1.4 / 1.9 s** | 0.9 s | **0** | 1.40 |
| alestro | **Nemotron, 0.56 s chunks** | 11.0% | **0.6 / 0.9 / 1.3 s** | 0.6 s | **0** | 2.35 |
| nogaps | Nemotron int8, 2 threads, 1.12 s | 3.0% | 0.7 / 1.2 / 1.7 s | 0.7 s | 0 | **0.43** |
| nogaps | Nemotron int8, 2 threads, 0.56 s | 3.7% | **0.3 / 0.7 / 1.0 s** | 0.4 s | 0 | **0.51** |
| alestro | Nemotron int8, 2 threads, 1.12 s | 11.8% | 0.7 / 1.2 / 1.8 s | 0.7 s | 0 | **0.42** |
| alestro | Nemotron int8, 2 threads, 0.56 s | 11.3% | **0.4 / 0.7 / 1.2 s** | 0.4 s | 0 | **0.50** |

How to read it:

- **Shipped Parakeet fails as captions.** Pause-free speech waits up to 70 s
  for a commit, and it doubles WER there: the 60 s hard cut decodes long
  windows that drop whole stretches (80 deletions against Nemotron's 3).
- **The bounded window fixes the delay but not the feel.** Commits arrive
  about 10x sooner and pause-free WER drops to 8.2%, but drafts still churn
  (190-340 rewrites/min), and the 0.5 s cadence costs accuracy.
- **Nemotron is the method.** Every word is final when shown, so the
  "words still being worked out" setting is moot for it. On TV it trails
  shipped Parakeet by 1.4-2.6 points; the gap is mostly verbatim fillers
  ("uh", "um") the subtitlers dropped, not hallucination over music
  (insertions 47 against Parakeet's 40).
- **Its cost is half a core.** fp32 torch on 4 threads took 1.4-2.3
  cores, mostly spinning. Dynamic int8 on the encoder's Linear layers, on 2
  threads, takes 0.42-0.51 and commits sooner, with word-identical output on
  a 60 s check. An int8 ONNX export with a torch-free loop (as Parakeet
  has; the multilingual sibling measured 14.9x real-time headroom that way,
  `myna/server/.kb/model-evaluation.md`) is still the shipping route, since
  torch has no place in a snap, but cost no longer blocks the product.

Caveats: one run per cell; another session's builds shared the machine for
part of the night, so CPU and latency carry some noise, not enough to move
the ranking. The harness, clips recipe and scorer are on Myna's
`captions-live` branch under `dev/captions_eval/`.

### French (2026-11-02, day)

`nogaps_fr` is 338 s of FLEURS French readings butted together (1094
reference words). Its absolute WERs read high: the normaliser leaves French
numbers spelt one way in the reference and another in the output ("6 6" /
"six six") and deletes parenthesised reference text the model did say, so
compare rows, not against published figures.

| method | WER | commit delay p50 / p90 / max | CPU s per audio s |
|---|---|---|---|
| Parakeet batch (reference) | 17.3% | - | - |
| Parakeet bounded, 1 s ticks | 22.4% | 4.1 / 11.8 / 19.7 s | 1.28 |
| **Nemotron 3.5 multilingual, fr-FR, 0.56 s, int8** | **15.5%** | **0.5 / 0.8 / 1.3 s** | **0.65** |

- **Nemotron 3.5 brings the same feel to French**, more accurate than
  Parakeet even in batch. It covers 19 transcription-ready locales. On
  English it trails the English model (7.0% against 3.7% pause-free, 13.0%
  against 11.3% on TV), so English stays on the English model.
- **Kyutai STT 1B en/fr reads French best of all** (punctuated, cased,
  near the reference) but is too heavy for this CPU under torch: 30 s of
  audio took 64.5 s in fp32, about 30 s in int8 (2 cores), and bf16 was
  slower still; the 1B step is memory-bandwidth bound every 80 ms. Kyutai's
  Rust runtime with 4-bit weights is the way to try it again. Parked.
- Voxtral Realtime (Mistral, Apache 2.0) was not tried: several times
  larger again.

### Seen but not fixed

- **Nemotron's punctuation is sparse** on conversational speech: captions
  read as run-on text. A punctuation pass (the cached sherpa online punct
  model) or a model that punctuates is the fix to try.
- **Parakeet's bounded window can drop words at the commit/draft seam**:
  once on screen, "anything for | And after" lost "a customer". Not chased,
  since Nemotron is the pick.

### What the prototype does now

- Engines: `dev/serve-methods.sh` serves Nemotron (0.56 s and 1.12 s) and
  Parakeet's bounded window on sockets the app lists under **Method**,
  fastest first; installed Myna snaps follow.
- Sources: applications (followed across their nodes), microphones, and
  output devices.
- Overlay: kept above other windows (XWayland), fades after 6 s of quiet.
  Clicks pass through except on a pill naming the source; pointing at it
  wakes the whole overlay (move, resize, close). A held-Ctrl design was
  built first and dropped: under XWayland an X client cannot see the
  pointer or modifiers while the pointer is over a Wayland window.
  **Unverified on the real GNOME session** (tested on Xvfb only): the
  first thing to check.
- Verified end to end overnight on a private X display: a TV clip played
  through PipeWire, tapped from the player's stream, captioned by each
  method; click-through, the pill, fade and close checked by driving the
  pointer; a SIGKILLed app takes its pw-record tap with it.

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
- **A new node per track ended the session** (fixed 2026-11-02): `pw-play`
  (and Firefox, per media element) creates a fresh node for each item. A
  source is now the application, re-resolved to its next node.

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
