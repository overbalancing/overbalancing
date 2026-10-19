#!/usr/bin/env bash
# Produce a simulator application; no App Store signing or distribution claim.
set -euo pipefail
if [[ "$(uname -m)" == arm64 ]]; then
  simulator_target=aarch64-apple-ios-sim
else
  simulator_target=x86_64-apple-ios
fi
rustup target add "$simulator_target"
cargo build --locked -p overpalancing-app --bin overpalancing --target "$simulator_target"
app=target/ios-simulator/Overpalancing.app
mkdir -p "$app"
cp "target/$simulator_target/debug/overpalancing" "$app/Overpalancing"
cat > "$app/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Overpalancing</string>
<key>CFBundleIdentifier</key><string>dev.overpalancing.game</string>
<key>CFBundleName</key><string>Overpalancing</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>LSRequiresIPhoneOS</key><true/>
<key>MinimumOSVersion</key><string>15.0</string>
<key>UILaunchScreen</key><dict/>
<key>UISupportedInterfaceOrientations</key><array><string>UIInterfaceOrientationPortrait</string><string>UIInterfaceOrientationLandscapeLeft</string><string>UIInterfaceOrientationLandscapeRight</string></array>
</dict></plist>
PLIST
codesign --force --sign - "$app"
tar -czf target/ios-simulator/Overpalancing.app.tar.gz -C target/ios-simulator Overpalancing.app
echo "Simulator application: $app"
