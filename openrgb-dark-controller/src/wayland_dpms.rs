use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use tokio::sync::mpsc;
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum,
    protocol::{wl_output, wl_registry},
};

mod kde_dpms {
    #![allow(non_camel_case_types, unused, clippy::all)]

    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/dpms.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/dpms.xml");
}

mod wlr_power {
    #![allow(non_camel_case_types, unused, clippy::all)]

    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "protocols/wlr-output-power-management-unstable-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!(
        "protocols/wlr-output-power-management-unstable-v1.xml"
    );
}

use kde_dpms::{org_kde_kwin_dpms, org_kde_kwin_dpms_manager};
use wlr_power::{zwlr_output_power_manager_v1, zwlr_output_power_v1};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScreenPowerState {
    On,
    Off,
}

enum DpmsBackend {
    Kde(org_kde_kwin_dpms_manager::OrgKdeKwinDpmsManager),
    Wlr(zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1),
}

struct DpmsData {
    tx: mpsc::UnboundedSender<ScreenPowerState>,
    backend: Option<DpmsBackend>,
    outputs: Vec<wl_output::WlOutput>,
    last_state: Option<ScreenPowerState>,
    initial_done: bool,
}

impl DpmsData {
    fn handle_mode_change(&mut self, on: bool) {
        let new_state = if on {
            ScreenPowerState::On
        } else {
            ScreenPowerState::Off
        };
        debug!("DPMS state: {:?}", new_state);
        if self.initial_done && self.last_state != Some(new_state) {
            self.last_state = Some(new_state);
            let _ = self.tx.send(new_state);
        }
        if !self.initial_done {
            self.last_state = Some(new_state);
        }
    }
}

// -- Registry: discover globals and bind whichever DPMS manager is available --

impl Dispatch<wl_registry::WlRegistry, ()> for DpmsData {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "org_kde_kwin_dpms_manager" if state.backend.is_none() => {
                    let manager: org_kde_kwin_dpms_manager::OrgKdeKwinDpmsManager =
                        registry.bind(name, version.min(1), qh, ());
                    info!("Bound KDE DPMS manager (org_kde_kwin_dpms_manager v{})", version.min(1));
                    state.backend = Some(DpmsBackend::Kde(manager));
                }
                "zwlr_output_power_manager_v1" if state.backend.is_none() => {
                    let manager: zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1 =
                        registry.bind(name, version.min(1), qh, ());
                    info!("Bound wlroots power manager (zwlr_output_power_manager_v1 v{})", version.min(1));
                    state.backend = Some(DpmsBackend::Wlr(manager));
                }
                "wl_output" => {
                    let output: wl_output::WlOutput =
                        registry.bind(name, version.min(4), qh, ());
                    state.outputs.push(output);
                }
                _ => {}
            }
        }
    }
}

// -- wl_output: we only need the object, not its events --

impl Dispatch<wl_output::WlOutput, ()> for DpmsData {
    fn event(
        _state: &mut Self,
        _proxy: &wl_output::WlOutput,
        _event: wl_output::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

// -- KDE DPMS protocol --

impl Dispatch<org_kde_kwin_dpms_manager::OrgKdeKwinDpmsManager, ()> for DpmsData {
    fn event(
        _state: &mut Self,
        _proxy: &org_kde_kwin_dpms_manager::OrgKdeKwinDpmsManager,
        _event: org_kde_kwin_dpms_manager::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<org_kde_kwin_dpms::OrgKdeKwinDpms, ()> for DpmsData {
    fn event(
        state: &mut Self,
        _proxy: &org_kde_kwin_dpms::OrgKdeKwinDpms,
        event: org_kde_kwin_dpms::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            org_kde_kwin_dpms::Event::Mode { mode } => {
                // KDE: 0=On, 1=Standby, 2=Suspend, 3=Off
                state.handle_mode_change(mode == 0);
            }
            org_kde_kwin_dpms::Event::Done => {
                state.initial_done = true;
            }
            org_kde_kwin_dpms::Event::Supported { supported } => {
                debug!("KDE DPMS supported: {}", supported != 0);
            }
        }
    }
}

// -- wlroots output power protocol --

impl Dispatch<zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1, ()> for DpmsData {
    fn event(
        _state: &mut Self,
        _proxy: &zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1,
        _event: zwlr_output_power_manager_v1::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_output_power_v1::ZwlrOutputPowerV1, ()> for DpmsData {
    fn event(
        state: &mut Self,
        _proxy: &zwlr_output_power_v1::ZwlrOutputPowerV1,
        event: zwlr_output_power_v1::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_power_v1::Event::Mode { mode } => {
                // wlroots: Off=0, On=1
                let on = matches!(mode, WEnum::Value(zwlr_output_power_v1::Mode::On));
                state.handle_mode_change(on);
                // wlroots has no "done" batching event — each mode event is final
                state.initial_done = true;
            }
            zwlr_output_power_v1::Event::Failed => {
                warn!("wlroots output power control failed (output may not support DPMS)");
            }
        }
    }
}

// -- Event loop --

fn run(tx: mpsc::UnboundedSender<ScreenPowerState>) -> Result<()> {
    let conn = Connection::connect_to_env()
        .context("Failed to connect to Wayland compositor")?;

    let display = conn.display();
    let mut queue = conn.new_event_queue::<DpmsData>();
    let qh = queue.handle();

    let _registry = display.get_registry(&qh, ());

    let mut data = DpmsData {
        tx,
        backend: None,
        outputs: Vec::new(),
        last_state: None,
        initial_done: false,
    };

    // Discover globals
    queue.roundtrip(&mut data)
        .context("Initial Wayland roundtrip failed")?;

    let backend_name = match &data.backend {
        Some(DpmsBackend::Kde(manager)) => {
            for output in &data.outputs {
                manager.get(output, &qh, ());
            }
            "KDE (org_kde_kwin_dpms)"
        }
        Some(DpmsBackend::Wlr(manager)) => {
            for output in &data.outputs {
                manager.get_output_power(output, &qh, ());
            }
            "wlroots (zwlr_output_power_management)"
        }
        None => {
            anyhow::bail!(
                "No DPMS protocol available from compositor \
                 (tried org_kde_kwin_dpms_manager, zwlr_output_power_manager_v1)"
            );
        }
    };

    // Fetch initial state
    queue.roundtrip(&mut data)
        .context("DPMS state roundtrip failed")?;

    info!(
        "Wayland DPMS monitor active: {} backend, {} output(s), initial state: {:?}",
        backend_name,
        data.outputs.len(),
        data.last_state
    );

    loop {
        queue.blocking_dispatch(&mut data)
            .context("Wayland event dispatch failed")?;
    }
}

pub fn start(tx: mpsc::UnboundedSender<ScreenPowerState>) {
    std::thread::Builder::new()
        .name("wayland-dpms".into())
        .spawn(move || {
            if let Err(e) = run(tx) {
                error!("Wayland DPMS monitor: {:#}", e);
            }
        })
        .expect("Failed to spawn Wayland DPMS thread");
}
