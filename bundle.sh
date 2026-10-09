#!/bin/sh
# Builds GitLance (release) and wraps it in target/GitLance.app with its icon.
#   ./bundle.sh           builds target/GitLance.app
#   ./bundle.sh --open    builds it and opens it
set -eu
cd "$(dirname "$0")"
if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
    . "$HOME/.cargo/env"
fi
cargo build --release
app=target/GitLance.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/gitlance "$app/Contents/MacOS/GitLance"
cp assets/GitLance.icns "$app/Contents/Resources/GitLance.icns"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>GitLance</string>
<key>CFBundleDisplayName</key><string>GitLance</string>
<key>CFBundleIdentifier</key><string>dev.gitlance.app</string>
<key>CFBundleExecutable</key><string>GitLance</string>
<key>CFBundleIconFile</key><string>GitLance</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
echo "built $app"
[ "${1:-}" = "--open" ] && open "$app"
exit 0
