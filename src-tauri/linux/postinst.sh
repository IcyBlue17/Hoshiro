#!/bin/sh
# Apply the udev rule shipped with the package to devices that are already connected.
modprobe uinput 2>/dev/null || true
if command -v udevadm >/dev/null 2>&1; then
  udevadm control --reload-rules 2>/dev/null || true
  udevadm trigger --subsystem-match=input --subsystem-match=misc --action=change 2>/dev/null || true
fi
exit 0
