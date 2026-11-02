# Myna Captions

Live captions for anything you can hear on the desktop. Pick an
application, an output device or a microphone, and its speech shows as
subtitles in a floating, translucent overlay. Speech is transcribed locally by
a [Myna](https://github.com/canonical/myna) engine.

Prototype. `FINDINGS.md` has what it taught us and the decisions so far.

## Trying it

Needs PipeWire (`pw-dump`, `pw-record`) and at least one engine.

```shell
# Engines: the caption methods under evaluation, one socket each. Needs a
# Myna checkout on the captions-live branch (MYNA_TREE); NeMo and torch go
# into their own venv under ~/.cache/myna on first run.
dev/serve-methods.sh                     # every method
dev/serve-methods.sh nemotron-1120       # just one

cargo run --release
```

Installed Myna snaps (`myna-parakeet` and friends) are listed as engines too,
after the locally served methods. They run Myna's dictation loop, which is
the slow baseline for captions (see `FINDINGS.md`).

- Pick a **method** in the Engine group, then press **Caption** on a source.
  An application is followed across tabs and tracks; the caption stays up
  while it is quiet and fades after 6 s without speech.
- **Hold Ctrl** over the captions to drag, resize (corner grip) or close
  them. Otherwise clicks go through to whatever is underneath.
- **Words still being worked out**: show drafts at once (dimmed), only once
  two drafts agree, or only final text.

Flags:

- `--wayland` keeps GTK on Wayland. By default the app runs on XWayland,
  because GNOME Wayland offers no way for a window to stay above others.
- `--socket PATH` lists one more engine first.
- `--caption KEY` starts captioning a source at launch (`app:NAME:PID`,
  `sink:NODE`, `mic:NODE`; `MYNA_CAPTIONS_TRACE=1` prints every wire event).
