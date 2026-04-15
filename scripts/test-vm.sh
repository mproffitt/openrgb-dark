#!/usr/bin/env bash
#
# Spin up test VMs for the DPMS detection code in different desktop
# environments. Uses quickemu/quickget.
#
# Each target corresponds to a distinct screen-power-off detection path in
# the controller:
#   gnome    -> GNOME on Wayland (mutter) – D-Bus org.freedesktop.ScreenSaver
#   sway     -> wlroots-based compositor  – zwlr_output_power_management_v1
#   hyprland -> wlroots-based compositor  – zwlr_output_power_management_v1
#
# Usage:
#   scripts/test-vm.sh fetch <target>    Download the image
#   scripts/test-vm.sh run   <target>    Boot the VM
#   scripts/test-vm.sh list              Show available targets
#   scripts/test-vm.sh clean <target>    Remove the VM data
#
# After first boot you'll need to install the OS interactively, then copy
# the controller binary into the guest via the shared folder (see
# $SHARED_DIR below, which is mounted inside the VM).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VM_DIR="${OPENRGB_VM_DIR:-$HOME/.cache/openrgb-dark-vms}"
SHARED_DIR="${OPENRGB_VM_SHARED:-$VM_DIR/shared}"
BINARY="$REPO_ROOT/openrgb-dark-controller/target/release/openrgb-dark-controller"

# target name => "os release edition"
declare -A TARGETS=(
    [gnome]="fedora 42 Workstation"
    [sway]="fedora 42 Sway"
    [hyprland]="cachyos latest desktop"
)

# target name => config filename quickget writes
declare -A CONFIGS=(
    [gnome]="fedora-42-Workstation.conf"
    [sway]="fedora-42-Sway.conf"
    [hyprland]="cachyos-latest-desktop.conf"
)

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "missing dependency: $1"; }

resolve_target() {
    local target="${1:-}"
    [[ -n "$target" ]] || die "target required (one of: ${!TARGETS[*]})"
    [[ -n "${TARGETS[$target]:-}" ]] || die "unknown target: $target"
    echo "$target"
}

ensure_shared() {
    mkdir -p "$SHARED_DIR"
    if [[ -x "$BINARY" ]]; then
        cp -u "$BINARY" "$SHARED_DIR/openrgb-dark-controller"
    else
        echo "note: release binary not found at $BINARY — build with 'cargo build --release' to stage it for the VM" >&2
    fi
    # Stage the systemd user unit too so it can be installed inside the VM.
    cp -u "$REPO_ROOT/openrgb-dark.service" "$SHARED_DIR/" 2>/dev/null || true
}

cmd_fetch() {
    local target; target=$(resolve_target "${1:-}")
    mkdir -p "$VM_DIR"
    cd "$VM_DIR"
    # shellcheck disable=SC2086 # intentional word split of the spec
    quickget ${TARGETS[$target]}
}

cmd_run() {
    local target; target=$(resolve_target "${1:-}")
    local conf="${CONFIGS[$target]}"
    [[ -f "$VM_DIR/$conf" ]] || die "config $conf not found — run: $0 fetch $target"

    ensure_shared

    # Inject the shared dir into the config if it isn't already set.
    if ! grep -q '^shared_dir=' "$VM_DIR/$conf"; then
        printf '\nshared_dir="%s"\n' "$SHARED_DIR" >> "$VM_DIR/$conf"
    fi

    cd "$VM_DIR"
    quickemu --vm "$conf"
}

cmd_clean() {
    local target; target=$(resolve_target "${1:-}")
    local stem="${CONFIGS[$target]%.conf}"
    cd "$VM_DIR"
    rm -rf -- "$stem" "$stem.conf"
    echo "removed $stem"
}

cmd_list() {
    cat <<EOF
Targets:
  gnome     GNOME on Wayland (Fedora Workstation)       — D-Bus ScreenSaver path
  sway      Sway / wlroots (Fedora Sway spin)            — zwlr_output_power_management_v1
  hyprland  CachyOS (Hyprland available in installer)    — zwlr_output_power_management_v1

VM data:      $VM_DIR
Shared dir:   $SHARED_DIR   (mounted inside VM — drop the binary here)

Post-install inside the VM:
  1. Install the OS (interactive on first boot).
  2. Open the shared folder, copy openrgb-dark-controller to ~/.local/bin/.
  3. sudo dnf install -y openrgb   (or equivalent on CachyOS/Arch).
  4. Run the controller in a terminal and observe logs while the screen
     idles out, or force DPMS via:
       KDE:       kscreen-doctor output.1 dpms off
       Sway:      swaymsg 'output * power off'
       Hyprland:  hyprctl dispatch dpms off
EOF
}

need quickemu
need quickget

case "${1:-}" in
    fetch) cmd_fetch "${2:-}" ;;
    run)   cmd_run   "${2:-}" ;;
    clean) cmd_clean "${2:-}" ;;
    list|"") cmd_list ;;
    *) die "unknown command: $1 (try: fetch|run|clean|list)" ;;
esac
