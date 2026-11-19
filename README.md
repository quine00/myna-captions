# Myna Captions

Live captions for anything you can hear on the desktop. Pick an
application, an output device or a microphone, and its speech shows as
subtitles in a floating, translucent overlay. Speech is transcribed locally by
a [Myna](https://github.com/canonical/myna) engine.

Prototype. `FINDINGS.md` has what it taught us and the decisions so far.

## Trying it

Needs Ubuntu 26.04 or later (GTK 4.14, libadwaita 1.6), PipeWire and at
least one Myna engine snap.

```shell
sudo apt install cargo pkg-config libgtk-4-dev libadwaita-1-dev pipewire-bin
sudo snap install myna-nemotron --edge   # fetches its English model too
cargo run --release
```

Every installed Myna engine snap is listed as a method. `myna-nemotron` comes
first: it commits each word about half a second after it is spoken and never
rewrites one. `myna-parakeet` runs Myna's dictation loop, which commits at
pauses and is the slow baseline for captions; the other snaps answer only
when the audio ends (see `FINDINGS.md`).

- Pick a **method** in the Engine group, then press **Caption** on a source.
  An application is followed across tabs and tracks; the caption stays up
  while it is quiet and fades after 6 s without speech.
- Clicks go **through** the captions to whatever is underneath, except on
  their label pill. Point at the pill and the whole overlay wakes up: drag
  to move, corner grip to resize, close button or Escape to stop.
- **Words still being worked out**: show drafts at once (dimmed), only once
  two drafts agree, or only final text.

Flags:

- `--wayland` keeps GTK on Wayland. By default the app runs on XWayland,
  because GNOME Wayland offers no way for a window to stay above others.
- `--socket PATH` lists one more engine first.
- `--caption KEY` starts captioning a source at launch (`app:NAME:PID`,
  `sink:NODE`, `mic:NODE`; `MYNA_CAPTIONS_TRACE=1` prints every wire event).

## Developing

`cargo fmt --check`, `cargo clippy --all-targets` and `cargo test` are
expected to stay clean.

- `dev/fake_engine.py` is a scripted engine for overlay work without a model.
- `dev/serve-methods.sh` serves the experimental methods compared in
  `FINDINGS.md`, one socket each under `$XDG_RUNTIME_DIR/myna-captions/`,
  where the app lists them ahead of the snaps. It needs a Myna checkout with
  the unpublished `captions-live` branch and locally exported models.
- `dev/nested-drag.sh` runs the overlay in a nested GNOME Shell through
  Myna's `dev/nested-session.sh`.
