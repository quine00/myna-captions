#!/bin/bash
# Drag the overlay's pill in a headless nested GNOME Shell under XWayland,
# and print where the window was before and after.
#
#   cd ~/Projects/myna && dev/nested-session.sh \
#       ~/Projects/myna-captions/dev/nested-drag.sh SOURCE-KEY
#
# Run it only through Myna's dev/nested-session.sh (private bus and runtime
# dir; see that script for why). Needs a debug build of myna-captions.
#
# Known limit (2026-11-02): pointer motion injected through Mutter's
# RemoteDesktop API reaches XWayland windows, but presses did not, so this
# reproduces hover and the input region, not a drag. Confirm drags by hand.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
KEY=${1:?source key, e.g. sink:alsa_output...}
W=$(mktemp -d)
export XDG_CONFIG_HOME=$W/config  # keep the nested Shell off the real dconf
gnome-shell --headless --wayland --virtual-monitor 1600x900 > "$W/shell.log" 2>&1 &
SHELL_PID=$!
XD=""
for _ in $(seq 60); do
    XD=$(grep -oE "Using public X11 display :[0-9]+" "$W/shell.log" | grep -oE ":[0-9]+" | head -1)
    [ -n "$XD" ] && break; sleep 1
done
[ -n "$XD" ] || { echo "no XWayland display; see $W/shell.log"; kill $SHELL_PID; exit 1; }
export XAUTHORITY=$(ls "$XDG_RUNTIME_DIR"/.mutter-Xwaylandauth.* | head -1)
python3 "$HERE/fake_engine.py" "$W/fake.sock" 1.2 >/dev/null 2>&1 &
ENGINE=$!
sleep 1
PIPEWIRE_RUNTIME_DIR=/run/user/$(id -u) DISPLAY=$XD GDK_BACKEND=x11 GTK_A11Y=none \
    MYNA_CAPTIONS_TRACE=1 "$HERE/../target/debug/myna-captions" --socket "$W/fake.sock" \
    --caption "$KEY" > "$W/app.log" 2>&1 &
APP=$!
sleep 6
WIN=$(DISPLAY=$XD xdotool search --name "Captions –" | head -1)
echo "before: $(DISPLAY=$XD xdotool getwindowgeometry "$WIN" | tr '\n' ' ')"
eval "$(DISPLAY=$XD xdotool getwindowgeometry --shell "$WIN" | sed 's/^/G_/')"
/usr/bin/python3 "$HERE/rd_drag.py" $((G_X + 30)) $((G_Y + 12)) 300 200
echo "after:  $(DISPLAY=$XD xdotool getwindowgeometry "$WIN" | tr '\n' ' ')"
echo "app log: $W/app.log"
kill $APP $ENGINE $SHELL_PID
