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

echo "Building $name..."
$(cd .. && cargo build --release)
cp "../target/release/$name" "./$name"

mkdir -p ~/.local/bin
mkdir -p "$dataHome/krunner/dbusplugins"
mkdir -p "$dataHome/dbus-1/services"

# Kill the running process so the binary can be replaced
# (transient D-Bus-activated units have no Restart= policy, so systemd will not
# bring it back up; KRunner will re-activate it on next use via D-Bus)
pkill -x "$name" || true
sleep 0.5

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
if command -v kquitapp6 &>/dev/null; then
	kquitapp6 krunner || true
elif command -v kquitapp5 &>/dev/null; then
	kquitapp5 krunner || true
fi

echo "KRunner will restart on next invocation."
