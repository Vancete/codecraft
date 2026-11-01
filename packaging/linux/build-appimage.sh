#!/bin/sh
# Packs target/release/codecraft into an AppImage: one file that runs on most Linux distributions.
# Usage: packaging/linux/build-appimage.sh <output.AppImage>
# Needs appimagetool on PATH (the release workflow downloads it).
set -eu

out="$1"
app="$(mktemp -d)/codecraft.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"

cp target/release/codecraft "$app/usr/bin/codecraft"
strip "$app/usr/bin/codecraft"
cp packaging/codecraft.desktop "$app/usr/share/applications/codecraft.desktop"
cp assets/app-icon/codecraft-256.png "$app/usr/share/icons/hicolor/256x256/apps/codecraft.png"

# What appimagetool looks for at the top of the AppDir.
cp packaging/codecraft.desktop "$app/codecraft.desktop"
cp assets/app-icon/codecraft-256.png "$app/codecraft.png"
ln -s codecraft.png "$app/.DirIcon"
cat > "$app/AppRun" <<'EOF'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/codecraft" "$@"
EOF
chmod +x "$app/AppRun"

ARCH=x86_64 appimagetool "$app" "$out"
