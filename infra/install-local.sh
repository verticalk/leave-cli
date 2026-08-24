#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_dir=$(dirname -- "$script_dir")
install_prefix=${LEAVE_INSTALL_PREFIX:-"$HOME/.local"}
binary_dir="$install_prefix/bin"
web_dir="$install_prefix/share/leave/web"

cd "$repository_dir"
corepack pnpm install --frozen-lockfile
corepack pnpm --filter @leave/web build
cargo build --release -p leave

install -d "$binary_dir" "$web_dir"
install -m 0755 target/release/leave "$binary_dir/leave"
cp -R apps/web/dist/. "$web_dir/"

case "$(uname -s)" in
  Darwin)
    app_dir="$HOME/Applications/Leave Setup.app"
    install -d "$app_dir/Contents/MacOS"
    install -d "$app_dir/Contents/Resources"
    cp apps/web/public/favicon.svg "$app_dir/Contents/Resources/leave.svg"
    printf '#!/bin/sh\nexec "%s" setup\n' "$binary_dir/leave" > "$app_dir/Contents/MacOS/leave-setup"
    chmod 0755 "$app_dir/Contents/MacOS/leave-setup"
    printf '%s\n' \
      '<?xml version="1.0" encoding="UTF-8"?>' \
      '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
      '<plist version="1.0"><dict>' \
      '<key>CFBundleExecutable</key><string>leave-setup</string>' \
      '<key>CFBundleIdentifier</key><string>dev.leave.setup</string>' \
      '<key>CFBundleName</key><string>Leave Setup</string>' \
      '<key>CFBundleDisplayName</key><string>Leave Setup</string>' \
      '<key>CFBundlePackageType</key><string>APPL</string>' \
      '<key>CFBundleShortVersionString</key><string>0.1.0</string>' \
      '<key>LSUIElement</key><true/>' \
      '</dict></plist>' > "$app_dir/Contents/Info.plist"
    launcher_detail="Open Leave Setup from your Applications folder."
    ;;
  *)
    applications_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}/applications
    icons_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}/icons/hicolor/scalable/apps
    install -d "$applications_dir" "$icons_dir"
    install -m 0644 apps/web/public/favicon.svg "$icons_dir/leave.svg"
    printf '%s\n' \
      '[Desktop Entry]' \
      'Type=Application' \
      'Version=1.0' \
      'Name=Leave Setup' \
      'Comment=Connect Devin and private phone access' \
      "Exec=$binary_dir/leave setup" \
      'Icon=leave' \
      'Terminal=false' \
      'Categories=Development;Utility;' \
      'StartupNotify=true' > "$applications_dir/leave-setup.desktop"
    chmod 0644 "$applications_dir/leave-setup.desktop"
    launcher_detail="Open Leave Setup from your applications menu."
    ;;
esac

printf '%s\n' "Leave installed locally at $binary_dir/leave"
case ":${PATH:-}:" in
  *":$binary_dir:"*) ;;
  *) printf '%s\n' "Add $binary_dir to PATH, then open a new terminal." ;;
esac
printf '%s\n' "$launcher_detail"
printf '%s\n' "Command-line fallback: $binary_dir/leave setup"
