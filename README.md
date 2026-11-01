# Myna Captions

Live captions for any audio playing on the desktop. Pick a PipeWire output (an
application's stream, or a whole device's mix), and its speech shows as
subtitles in a floating, translucent overlay you can drag and resize. Speech is
transcribed locally by an installed [Myna](https://github.com/canonical/myna)
inference backend.

Prototype. See `FINDINGS.md` for what it taught us and what is still open.

## Running

Needs PipeWire with `pw-dump` and `pw-record`, and a Myna backend snap
(`myna-parakeet` is the one that streams by default).

```shell
cargo run --release
cargo run --release -- --caption <object.serial>    # start one straight away
MYNA_CAPTIONS_TRACE=1 cargo run -- ...                # print every wire event
```

- `--wayland` keeps GTK on Wayland. By default the app runs on XWayland,
  because GNOME Wayland offers no way for a window to stay above others.
- `--socket PATH` adds a backend socket ahead of the discovered ones.

## Using the overlay

- Drag anywhere to move it.
- Drag the corner grip to resize; its height sets how many lines show.
- The close button or Escape stops the captions.
- Text still being revised is drawn dimmer than settled text.
