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

## Open questions for the grilling

Product

1. Who is it for: deaf and hard-of-hearing users (accessibility, must be
   reliable for hours), meeting notes, or language learners? It decides most
   of what follows.
2. Translation is the obvious next feature (caption French audio in
   English). In scope, and on which model?
3. Does the transcript persist (save to file, search) or is it ephemeral by
   design? Privacy posture either way.

Capture

4. Per-app taps need no consent today. Is that acceptable for a
   privacy-first project, or must selection go through the ScreenCast portal?
5. What does "Firefox" mean when it spawns a node per media element? Follow
   the application (`application.name` + pid) across nodes, or one node only?
6. Pause and silence: keep the session open for 20 minutes of a paused
   video, or end and restart?
7. Microphone too (captioning a live conversation in the room)?

Engine

8. Parakeet's growing re-decode needs a forced commit for captions. Client
   side (commit every N seconds or at a punctuation-stable prefix) or a
   server-side caption mode? The server is the right owner.
9. Non-speech: Whisper hallucinates over music. Gate on VAD client-side, or
   rely on the server?
10. Is a batch-only backend offered at all, or hidden?

Overlay

11. GNOME Wayland has no always-on-top. The prototype runs on XWayland
    (EWMH above). Alternatives: the Shell extension draws it (transcript
    crosses D-Bus into the compositor), or a picture-in-picture-style
    request upstream. Which?
12. Above fullscreen video? XWayland "above" loses to a fullscreen window.
13. Click-through while not hovered, so it never blocks the video's controls?
14. Revision display: dim tail (now), hold back unstable words, or
    word-by-word fade-in? Rolling lines or YouTube-style paged cues?
15. Multiple overlays at once (two calls), or one?
16. Settings persistence: GSettings, and per-node remembered position?

Library

17. Rust-only crate, or a C/GObject API so Python and GJS apps can use it?
18. Does the library include capture, or only engines and transcripts, with
    capture left to the app?
19. Lives in the Myna repo (dictation migrates onto it) or separately?
20. Packaging: snap (needs `audio-record` plus graph access to other apps'
    nodes, which strict confinement may not allow), deb, or Flatpak?
