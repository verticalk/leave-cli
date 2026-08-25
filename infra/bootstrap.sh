#!/bin/sh
# One command from a fresh checkout to Leave Setup on macOS, Linux, or WSL2.
#
# Everything is installed for this user account only. Nothing here needs
# administrator rights, and nothing is downloaded from outside the official
# Rust, Node.js, and Leave sources.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_dir=$(dirname -- "$script_dir")
install_prefix=${LEAVE_INSTALL_PREFIX:-"$HOME/.local"}
toolchain_dir=${LEAVE_TOOLCHAIN_DIR:-"$HOME/.local/share/leave/toolchain"}
node_version=$(cat "$repository_dir/.nvmrc")
assume_yes=0
open_setup=1

usage() {
  cat <<USAGE
Usage: infra/bootstrap.sh [--yes] [--no-setup] [--prefix DIR]

  --yes        Install the missing prerequisites without asking.
  --no-setup   Install Leave but do not open Leave Setup afterwards.
  --prefix DIR Install Leave under DIR instead of $HOME/.local.
USAGE
}

while [ $# -gt 0 ]; do
  case "$1" in
    -y|--yes) assume_yes=1 ;;
    --no-setup) open_setup=0 ;;
    --prefix) shift; install_prefix=$1 ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'Unknown option: %s\n\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

say() { printf '%s\n' "$1"; }
step() { printf '\n==> %s\n' "$1"; }
fail() { printf 'Leave setup stopped: %s\n' "$1" >&2; exit 1; }

have() { command -v "$1" >/dev/null 2>&1; }

confirm() {
  [ "$assume_yes" -eq 1 ] && return 0
  printf '%s [Y/n] ' "$1"
  read -r answer </dev/tty || fail "no terminal is available; re-run with --yes"
  case "$answer" in
    ""|y|Y|yes|YES) return 0 ;;
    *) return 1 ;;
  esac
}

download() {
  # $1 url, $2 destination
  if have curl; then
    curl -fsSL --proto '=https' --tlsv1.2 "$1" -o "$2"
  elif have wget; then
    wget -qO "$2" "$1"
  else
    fail "this computer has neither curl nor wget; install one and run this again"
  fi
}

node_major_ok() {
  have node || return 1
  major=$(node -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0)
  minor=$(node -p 'process.versions.node.split(".")[1]' 2>/dev/null || echo 0)
  [ "$major" -gt 22 ] && return 0
  [ "$major" -eq 22 ] && [ "$minor" -ge 12 ]
}

node_platform() {
  case "$(uname -s)" in
    Darwin) printf 'darwin' ;;
    Linux) printf 'linux' ;;
    *) fail "unsupported operating system: $(uname -s). Use the Windows script infra/bootstrap.ps1." ;;
  esac
}

node_arch() {
  case "$(uname -m)" in
    x86_64|amd64) printf 'x64' ;;
    arm64|aarch64) printf 'arm64' ;;
    *) fail "unsupported processor: $(uname -m)" ;;
  esac
}

install_rust() {
  step "Installing Rust for your user account"
  confirm "Install the official Rust toolchain from https://sh.rustup.rs?" ||
    fail "Rust is required to build Leave"
  tmp=$(mktemp)
  download "https://sh.rustup.rs" "$tmp"
  sh "$tmp" -y --profile minimal --no-modify-path >/dev/null
  rm -f "$tmp"
  say "Rust installed under $HOME/.cargo"
}

pnpm_pin() {
  # The pinned version lives in package.json, so one edit moves every install.
  sed -n 's/.*"packageManager": *"pnpm@\([^"]*\)".*/\1/p' "$repository_dir/package.json" | head -1
}

ensure_pnpm() {
  wanted=$(pnpm_pin)
  [ -n "$wanted" ] || fail "package.json does not pin a pnpm version"

  if have pnpm && [ "$(pnpm --version 2>/dev/null)" = "$wanted" ]; then
    say "pnpm: found $wanted"
    return 0
  fi

  pnpm_home="$toolchain_dir/pnpm"
  if [ -x "$pnpm_home/bin/pnpm" ] && [ "$("$pnpm_home/bin/pnpm" --version 2>/dev/null)" = "$wanted" ]; then
    PATH="$pnpm_home/bin:$PATH"
    export PATH
    say "pnpm: found $wanted"
    return 0
  fi

  step "Installing pnpm $wanted for your user account"
  have npm || fail "npm is missing from this Node.js installation"
  # Deliberately not corepack. The corepack bundled with several Node
  # releases carries npm registry signing keys that have since rotated, and
  # it fails with "Cannot find matching keyid" before it ever reads the
  # pinned version above. npm installs that exact version without needing
  # those keys, or an administrator.
  npm install -g --silent --prefix "$pnpm_home" "pnpm@$wanted" >/dev/null ||
    fail "could not install pnpm $wanted"
  PATH="$pnpm_home/bin:$PATH"
  export PATH
  have pnpm || fail "pnpm was installed but is not on PATH"
  say "pnpm installed under $pnpm_home"
}

install_node() {
  step "Installing Node.js $node_version for your user account"
  confirm "Download Node.js $node_version from https://nodejs.org?" ||
    fail "Node.js $node_version or newer is required to build the Leave app"
  platform=$(node_platform)
  arch=$(node_arch)
  archive="node-v$node_version-$platform-$arch.tar.gz"
  tmp=$(mktemp -d)
  download "https://nodejs.org/dist/v$node_version/$archive" "$tmp/$archive"
  mkdir -p "$toolchain_dir"
  rm -rf "$toolchain_dir/node"
  tar -xzf "$tmp/$archive" -C "$tmp"
  mv "$tmp/node-v$node_version-$platform-$arch" "$toolchain_dir/node"
  rm -rf "$tmp"
  say "Node.js installed under $toolchain_dir/node"
}

step "Checking this computer"
[ -f "$repository_dir/Cargo.toml" ] || fail "run this script from a Leave checkout"
have git || say "git is not installed. Leave's Git features need it later."

if have cargo; then
  say "Rust: found $(cargo --version)"
elif [ -x "$HOME/.cargo/bin/cargo" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
  say "Rust: found $(cargo --version)"
else
  say "Rust: not installed"
  install_rust
  PATH="$HOME/.cargo/bin:$PATH"
fi

if [ -x "$toolchain_dir/node/bin/node" ] && ! node_major_ok; then
  PATH="$toolchain_dir/node/bin:$PATH"
fi

if node_major_ok; then
  say "Node.js: found $(node --version)"
else
  say "Node.js: not installed or older than v$node_version"
  install_node
  PATH="$toolchain_dir/node/bin:$PATH"
fi
export PATH

ensure_pnpm

step "Building Leave (this takes a few minutes the first time)"
LEAVE_INSTALL_PREFIX="$install_prefix" "$script_dir/install-local.sh"

binary="$install_prefix/bin/leave"
[ -x "$binary" ] || fail "the build finished but $binary is missing"

case ":${PATH:-}:" in
  *":$install_prefix/bin:"*) ;;
  *)
    step "Adding Leave to your PATH"
    for profile in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.profile"; do
      [ -f "$profile" ] || continue
      if ! grep -Fq "$install_prefix/bin" "$profile"; then
        printf '\n# Added by Leave\nexport PATH="%s/bin:$PATH"\n' "$install_prefix" >>"$profile"
        say "Updated $profile. Open a new terminal to pick it up."
      fi
    done
    ;;
esac

step "Leave is installed"
say "Leave Setup guides you through Devin sign-in, choosing a folder, and phone access."
say "You can reopen it any time from your applications menu, or run: $binary setup"

if [ "$open_setup" -eq 1 ]; then
  step "Opening Leave Setup"
  exec "$binary" setup
fi
