#!/bin/bash
set -e

identifier=dev.algus.krunner_zed
name=krunner-zed
desktop=krunner_zed.desktop

cd "$(dirname "$0")"

if [[ -n "$XDG_DATA_HOME" ]]; then
	dataHome="$XDG_DATA_HOME"
else
	dataHome=~/.local/share
fi

mkdir -p ~/.local/bin
mkdir -p "$dataHome/krunner/dbusplugins"
mkdir -p "$dataHome/dbus-1/services"

# Install binary
cp "$name" ~/.local/bin/"$name"
chmod +x ~/.local/bin/"$name"

# Install D-Bus service file with Exec path filled in
executableFullPath=$(readlink -m ~/.local/bin/"$name")
sed "s|Exec=|Exec=$executableFullPath|" "$identifier.service" \
	>"$dataHome/dbus-1/services/$identifier.service"

# Install KRunner desktop file
cp "$desktop" "$dataHome/krunner/dbusplugins/$desktop"

echo "Installed $name."

# Reload KRunner
kquitapp6 krunner || true

echo "KRunner will restart on next invocation."
