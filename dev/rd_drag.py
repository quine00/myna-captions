"""Drag with Mutter's RemoteDesktop API in absolute logical pixels:
move to (x, y), press, move by (dx, dy) in steps, release."""
import sys, time
from gi.repository import Gio, GLib
x, y, dx, dy = map(int, sys.argv[1:5])
bus = Gio.bus_get_sync(Gio.BusType.SESSION)
def call(dest, path, iface, method, args=None, sig=None):
    return bus.call_sync(dest, path, iface, method, args, sig and GLib.VariantType(sig), 0, -1, None)
RDD, SCD = "org.gnome.Mutter.RemoteDesktop", "org.gnome.Mutter.ScreenCast"
rd = call(RDD, "/org/gnome/Mutter/RemoteDesktop", RDD, "CreateSession", None, "(o)").unpack()[0]
sid = bus.call_sync(RDD, rd, "org.freedesktop.DBus.Properties", "Get",
                    GLib.Variant("(ss)", (RDD + ".Session", "SessionId")), None, 0, -1, None).unpack()[0]
sc = call(SCD, "/org/gnome/Mutter/ScreenCast", SCD, "CreateSession",
          GLib.Variant("(a{sv})", ({"remote-desktop-session-id": GLib.Variant("s", sid)},)), "(o)").unpack()[0]
stream = call(SCD, sc, SCD + ".Session", "RecordMonitor",
              GLib.Variant("(sa{sv})", ("", {})), "(o)").unpack()[0]
S = RDD + ".Session"
call(RDD, rd, S, "Start")
time.sleep(0.5)
# A fresh Shell opens in the overview, which takes the pointer: leave it.
for down in (True, False):
    call(RDD, rd, S, "NotifyKeyboardKeysym", GLib.Variant("(ub)", (0xFF1B, down)))
time.sleep(1.0)
def to(a, b):
    call(RDD, rd, S, "NotifyPointerMotionAbsolute", GLib.Variant("(sdd)", (stream, a, b))); time.sleep(0.04)
to(x, y); time.sleep(0.6)
call(RDD, rd, S, "NotifyPointerButton", GLib.Variant("(ib)", (272, True))); time.sleep(0.15)
for i in range(1, 21):
    to(x + dx * i / 20, y + dy * i / 20)
time.sleep(0.2)
call(RDD, rd, S, "NotifyPointerButton", GLib.Variant("(ib)", (272, False))); time.sleep(0.5)
call(RDD, rd, S, "Stop")
