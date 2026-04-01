#!/bin/bash
identifier=dev.algus.krunner_zed
name=krunner-zed
desktop=krunner_zed.desktop

if [[ -n "$XDG_DATA_HOME" ]]; then
    dataHome="$XDG_DATA_HOME"
else
    dataHome=~/.local/share
fi

# Stop running instance
if pidof "$name" &>/dev/null; then
    kill "$(pidof "$name")" 2>/dev/null || true
fi

rm -f ~/.local/bin/"$name"
rm -f "$dataHome/krunner/dbusplugins/$desktop"
rm -f "$dataHome/dbus-1/services/$identifier.service"

echo "Uninstalled $name."

kquitapp6 krunner || true
