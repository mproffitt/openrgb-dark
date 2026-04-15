use anyhow::Result;
use log::{info, error, debug};
use openrgb2::{OpenRgbClient, Color};
use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tokio::time::sleep;
use zbus::Connection;
use futures_util::stream::StreamExt;
use clap::Parser;

mod wayland_dpms;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Turn off all lights immediately
    #[arg(long)]
    dark: bool,

    /// Turn on lights and effects immediately
    #[arg(long)]
    light: bool,

    /// Hardware profile to load when "light" (default: base)
    #[arg(long, default_value = "base")]
    profile: String,

    /// Effects profile to load when "light" (default: base)
    #[arg(long, default_value = "base")]
    effects_profile: String,

    /// Time in seconds to wait after screen goes dark (overrides TIMEOUT env)
    #[arg(short, long)]
    timeout: Option<u64>,
}

mod fdo {
    use zbus::proxy;
    #[proxy(
        interface = "org.freedesktop.ScreenSaver",
        default_service = "org.freedesktop.ScreenSaver",
        default_path = "/ScreenSaver"
    )]
    pub trait ScreenSaver {
        #[zbus(signal)]
        fn active_changed(&self, active: bool) -> zbus::Result<()>;
    }
}

mod kde {
    use zbus::proxy;
    #[proxy(
        interface = "org.kde.screensaver",
        default_service = "org.kde.screensaver",
        default_path = "/ScreenSaver"
    )]
    pub trait ScreenSaver {
        #[zbus(signal)]
        fn active_changed(&self, active: bool) -> zbus::Result<()>;
    }
}

async fn load_openrgb_profile(name: &str) -> Result<()> {
    let client = OpenRgbClient::connect().await?;
    client.load_profile(name).await?;
    info!("Loaded OpenRGB profile: {}", name);
    Ok(())
}

fn send_plugin_command(plugin_idx: u32, cmd_id: u32, payload: &[u8]) -> Result<Vec<u8>> {
    let mut stream = TcpStream::connect("127.0.0.1:6742")?;
    
    let mut data = Vec::new();
    data.extend_from_slice(&cmd_id.to_le_bytes()); 
    data.extend_from_slice(payload);
    
    let mut header = Vec::new();
    header.extend_from_slice(b"ORGB");
    header.extend_from_slice(&plugin_idx.to_le_bytes()); 
    header.extend_from_slice(&201u32.to_le_bytes()); 
    header.extend_from_slice(&(data.len() as u32).to_le_bytes()); 
    
    stream.write_all(&header)?;
    stream.write_all(&data)?;

    let mut resp_hdr = [0u8; 16];
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    if stream.read_exact(&mut resp_hdr).is_ok() {
        let data_size = u32::from_le_bytes(resp_hdr[12..16].try_into()?);
        if data_size > 0 {
            let mut data = vec![0u8; data_size as usize];
            let _ = stream.read_exact(&mut data);
            return Ok(data);
        }
    }
    Ok(Vec::new())
}

fn get_active_effects() -> Result<Vec<String>> {
    let data = send_plugin_command(0, 0, &[])?;
    if data.len() < 10 { return Ok(Vec::new()); }
    
    let mut effects = Vec::new();
    // Offset 0-3: Subcommand ID
    // Offset 4-7: Plugin-specific header/version
    // Offset 8-9: Count (u16)
    let count = u16::from_le_bytes(data[8..10].try_into()?);
    debug!("Plugin reported {} effects in list", count);
    
    let mut offset = 10;
    for _ in 0..count {
        if offset + 2 > data.len() { break; }
        let name_len = u16::from_le_bytes(data[offset..offset+2].try_into()?);
        if name_len == 0 {
            offset += 2;
            continue;
        }
        if offset + 2 + name_len as usize > data.len() { break; }
        let mut name_bytes = data[offset+2..offset+2+name_len as usize].to_vec();
        if let Some(0) = name_bytes.last() { name_bytes.pop(); }
        let name = String::from_utf8_lossy(&name_bytes).to_string();
        effects.push(name);
        offset += 2 + name_len as usize;
        
        // Skip description and enabled flag
        if offset + 2 > data.len() { break; }
        let desc_len = u16::from_le_bytes(data[offset..offset+2].try_into()?);
        offset += 2 + desc_len as usize;
        
        if offset + 1 > data.len() { break; }
        // Skip enabled flag (1 byte)
        offset += 1;
    }
    Ok(effects)
}

fn stop_effect_by_name(name: &str) -> Result<()> {
    let mut effect_bytes = name.as_bytes().to_vec();
    effect_bytes.push(0); 
    let mut payload = Vec::new();
    payload.extend_from_slice(&(effect_bytes.len() as u16).to_le_bytes());
    payload.extend_from_slice(&effect_bytes);
    let _ = send_plugin_command(0, 21, &payload);
    Ok(())
}

fn stop_all_effects() -> Result<()> {
    if let Ok(effects) = get_active_effects() {
        for effect in effects {
            info!("Stopping active effect: {}", effect);
            let _ = stop_effect_by_name(&effect);
        }
    }
    // Master Off
    let _ = send_plugin_command(0, 10, &[]);
    Ok(())
}

fn load_effect_profile(name: &str) -> Result<()> {
    let mut profile_bytes = name.as_bytes().to_vec();
    profile_bytes.push(0); 
    let mut payload = Vec::new();
    payload.extend_from_slice(&(profile_bytes.len() as u16).to_le_bytes());
    payload.extend_from_slice(&profile_bytes);
    let _ = send_plugin_command(0, 23, &payload);
    info!("Loaded effect profile: {}", name);
    Ok(())
}

async fn sequence_darken(base_profile: &str) -> Result<()> {
    info!("Applying dark sequence...");
    
    // 1. Stop all plugin effects
    let _ = stop_all_effects();
    
    // 2. Load base profile to ensure we are in a mode that accepts colors (like Static)
    // This is critical because some hardware modes ignore SDK color updates.
    if let Err(e) = load_openrgb_profile(base_profile).await {
        error!("Failed to load base profile during darken: {}", e);
    }

    sleep(Duration::from_secs(1)).await;

    // 3. Set all LEDs to black
    let client = OpenRgbClient::connect().await?;
    let controllers = client.get_all_controllers().await?;
    for controller in controllers {
        let mut cmd = controller.cmd();
        cmd.set_leds(vec![Color::new(0, 0, 0); controller.num_leds()])?;
        let _ = cmd.execute().await;
    }
    
    info!("Dark sequence complete");
    Ok(())
}

async fn sequence_lighten(profile: &str, effects_profile: &str) -> Result<()> {
    info!("Applying light sequence (profile={}, effects={})...", profile, effects_profile);
    let _ = load_openrgb_profile(profile).await;
    let _ = load_effect_profile(effects_profile);
    info!("Light sequence complete");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    let args = Args::parse();

    if args.dark {
        return sequence_darken(&args.profile).await;
    }

    if args.light {
        return sequence_lighten(&args.profile, &args.effects_profile).await;
    }

    let timeout_secs = args.timeout.unwrap_or_else(|| {
        env::var("TIMEOUT")
            .unwrap_or_else(|_| "30".to_string())
            .parse::<u64>()
            .unwrap_or(30)
    });
    
    info!("Starting OpenRGB Dark Controller (TIMEOUT={}s, PROFILE={}, EFFECTS={})", 
        timeout_secs, args.profile, args.effects_profile);

    let connection = Connection::session().await?;

    // Listen to both standard and KDE-specific ScreenSaver signals (GNOME, KDE+X11)
    let proxy = fdo::ScreenSaverProxy::new(&connection).await?;
    let mut signals = proxy.receive_active_changed().await?;

    let kde_proxy = kde::ScreenSaverProxy::new(&connection).await?;
    let mut kde_signals = kde_proxy.receive_active_changed().await?;

    // Listen to Wayland DPMS state changes (KDE Plasma 6, sway/Hyprland/wlroots)
    let (dpms_tx, mut dpms_rx) = tokio::sync::mpsc::unbounded_channel();
    wayland_dpms::start(dpms_tx);

    let mut timeout_handle: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        let active = tokio::select! {
            Some(signal) = signals.next() => {
                let a = signal.args()?.active;
                info!("org.freedesktop.ScreenSaver active changed: {}", a);
                Some(a)
            }
            Some(signal) = kde_signals.next() => {
                let a = signal.args()?.active;
                info!("org.kde.screensaver active changed: {}", a);
                Some(a)
            }
            Some(power_state) = dpms_rx.recv() => {
                match power_state {
                    wayland_dpms::ScreenPowerState::On => {
                        info!("Wayland DPMS: screen on");
                        Some(false)
                    }
                    wayland_dpms::ScreenPowerState::Off => {
                        info!("Wayland DPMS: screen off");
                        Some(true)
                    }
                }
            }
            else => break,
        };

        if let Some(active) = active {
            if active {
                if let Some(handle) = timeout_handle.take() {
                    handle.abort();
                }
                let args_clone = args.clone();
                timeout_handle = Some(tokio::spawn(async move {
                    info!("Screen dark, waiting {}s before turning off lights", timeout_secs);
                    sleep(Duration::from_secs(timeout_secs)).await;
                    let _ = sequence_darken(&args_clone.profile).await;
                }));
            } else {
                if let Some(handle) = timeout_handle.take() {
                    handle.abort();
                    info!("Timeout aborted, screen woke up");
                }
                let _ = sequence_lighten(&args.profile, &args.effects_profile).await;
            }
        }
    }
    Ok(())
}
