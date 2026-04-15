# OpenRGB Dark Controller

A lightweight, event-driven Rust service that automatically manages RGB lighting based on screen power state. Originally built for KDE Plasma 6 on Wayland, with coverage for GNOME, KDE (X11 and Wayland), and wlroots-based compositors such as sway and Hyprland.

## Features

- **Event-driven detection:** listens on D-Bus and directly on the Wayland compositor for screen-power changes, with no polling.
- **Cross-desktop:** works on GNOME (D-Bus ScreenSaver), KDE Plasma 5/6 (D-Bus + Wayland DPMS), and wlroots compositors (sway, Hyprland) via `zwlr_output_power_management_v1`.
- **Robust effect management:** discovers and stops active effects in the OpenRGB Effects Plugin before darkening.
- **Minimal footprint:** compiled Rust binary using ~2.5 MB of RAM.
- **Configurable:** customise hardware profiles, effect profiles, and timeouts via CLI flags.
- **Systemd integration:** runs as a user systemd service.

## Prerequisites

- [OpenRGB](https://openrgb.org/) with the SDK server enabled.
- [OpenRGB Effects Plugin](https://gitlab.com/OpenRGBDevelopers/OpenRGBEffectsPlugin) (optional, but supported).
- Rust toolchain (for building).

## Installation

### 1. Build the Project

```bash
cd openrgb-dark-controller
cargo build --release
```

### 2. Install the Binary

```bash
mkdir -p ~/.local/bin
install -m 755 target/release/openrgb-dark-controller ~/.local/bin/
```

### 3. Install the Systemd Service

Create the directory if it doesn't exist:
```bash
mkdir -p ~/.config/systemd/user/
```

Copy the service file:
```bash
cp ../openrgb-dark.service ~/.config/systemd/user/
```

### 4. Enable and Start

```bash
systemctl --user daemon-reload
systemctl --user enable openrgb-dark.service
systemctl --user start openrgb-dark.service
```

## Usage

### Command Line Flags

You can run the controller manually to test your sequences:

- `--dark`: Immediately trigger the "Darken" sequence (stop effects, set LEDs to black).
- `--light`: Immediately trigger the "Lighten" sequence (restore profiles).
- `--profile <name>`: Specify the hardware profile to load on wake (default: `base`).
- `--effects-profile <name>`: Specify the effects profile to load on wake (default: `base`).
- `--timeout <seconds>`: Override the wait time after the screen goes dark (default: `30`).

**Example:**
```bash
openrgb-dark-controller --light --profile my_custom_profile --effects-profile party_mode
```

## How It Works

1.  **Detection.** The controller listens on multiple event sources in parallel so it can pick up a screen-off transition regardless of the active desktop:

    | Source | Covers |
    |---|---|
    | `org.freedesktop.ScreenSaver` (D-Bus session) | GNOME, most freedesktop-compliant environments |
    | `org.kde.screensaver` (D-Bus session) | KDE Plasma (X11) |
    | `org_kde_kwin_dpms` (Wayland protocol) | KDE Plasma 6 (Wayland) |
    | `zwlr_output_power_management_v1` (Wayland protocol) | sway, Hyprland, river, and other wlroots-based compositors |

    The Wayland protocols are preferred on their respective compositors because modern KDE and wlroots compositors perform DPMS entirely over Wayland and emit no D-Bus signal when the screen blanks.

2.  **Darken sequence:**
    - Queries the Effects Plugin for active effects and stops them by name.
    - Sends a "Master Off" command to the Effects Plugin.
    - Loads the `base` profile to return hardware to a known state.
    - Explicitly sets all hardware LEDs to black via the SDK.

3.  **Lighten sequence:**
    - Loads the configured hardware profile.
    - Loads the configured effects profile in the plugin.

## Testing in VMs

A helper script in `scripts/test-vm.sh` uses [`quickemu`/`quickget`](https://github.com/quickemu-project/quickemu) to spin up VMs for each supported compositor family:

```bash
scripts/test-vm.sh fetch gnome      # Fedora Workstation (GNOME / D-Bus path)
scripts/test-vm.sh fetch sway       # Fedora Sway spin (wlroots path)
scripts/test-vm.sh fetch hyprland   # CachyOS, select Hyprland in the installer
scripts/test-vm.sh run <target>     # boot the VM
scripts/test-vm.sh list             # show all targets and post-install notes
```

The release binary is automatically staged into the VM's shared folder.

## License

This project is licensed under the [MIT License](LICENSE).
