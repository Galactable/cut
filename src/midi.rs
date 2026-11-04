//! MIDI controllers (DJ controllers, pad and knob boxes) through the ALSA sequencer,
//! which PipeWire and JACK setups share. Every hardware MIDI output is listened to,
//! including ones plugged in later; the app decides what the notes and CCs mean.

use std::{
    collections::HashMap,
    ffi::CString,
    sync::Arc,
    time::SystemTime,
};

use alsa::seq::{Addr, ClientIter, EvCtrl, EvNote, EventType, PortCap, PortIter, PortSubscribe, PortType, Seq};
use winit::event_loop::EventLoopProxy;

use crate::{
    devices::{Class, Ctl, DevInfo},
    input::{micros, InEvent},
};

pub fn start(proxy: EventLoopProxy<InEvent>) {
    std::thread::spawn(move || {
        if let Err(e) = run(proxy) {
            eprintln!("MIDI unavailable: {e}");
        }
    });
}

struct Port {
    info: DevInfo,
}

fn run(proxy: EventLoopProxy<InEvent>) -> anyhow::Result<()> {
    let seq = Seq::open(None, Some(alsa::Direction::Capture), false)?;
    seq.set_client_name(&CString::new("CUT")?)?;
    let me = seq.client_id()?;
    let my_port = seq.create_simple_port(
        &CString::new("controllers in")?,
        PortCap::WRITE | PortCap::SUBS_WRITE,
        PortType::MIDI_GENERIC | PortType::APPLICATION,
    )?;
    let dest = Addr { client: me, port: my_port };
    // New ports (controllers plugged in later) are announced here.
    connect(&seq, Addr::system_announce(), dest);

    let mut ports: HashMap<(i32, i32), Port> = HashMap::new();
    for client in ClientIter::new(&seq) {
        for port in PortIter::new(&seq, client.get_client()) {
            let addr = Addr { client: port.get_client(), port: port.get_port() };
            add(&seq, &proxy, &mut ports, addr, me, dest);
        }
    }

    let mut input = seq.input();
    loop {
        let ev = input.event_input()?;
        let src = ev.get_source();
        let ctl_value = match ev.get_type() {
            EventType::PortStart => {
                if let Some(a) = ev.get_data::<Addr>() {
                    add(&seq, &proxy, &mut ports, a, me, dest);
                }
                continue;
            }
            EventType::PortExit => {
                if let Some(a) = ev.get_data::<Addr>() {
                    if let Some(p) = ports.remove(&(a.client, a.port)) {
                        let _ = proxy.send_event(InEvent::Gone(p.info.inst));
                    }
                }
                continue;
            }
            EventType::Noteon => ev.get_data::<EvNote>().map(|n| (Ctl::Note(n.channel & 15, n.note & 127), n.velocity as i32)),
            EventType::Noteoff => ev.get_data::<EvNote>().map(|n| (Ctl::Note(n.channel & 15, n.note & 127), 0)),
            EventType::Controller => ev.get_data::<EvCtrl>().map(|c| (Ctl::Cc(c.channel & 15, (c.param & 127) as u8), c.value)),
            EventType::Pitchbend => ev.get_data::<EvCtrl>().map(|c| (Ctl::Bend(c.channel & 15), c.value)),
            _ => None,
        };
        let Some((ctl, value)) = ctl_value else { continue };
        let Some(p) = ports.get(&(src.client, src.port)) else { continue };
        if !p.info.forward.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let m = InEvent::Control { dev: p.info.inst.clone(), ctl, value, t: micros(SystemTime::now()) };
        if proxy.send_event(m).is_err() {
            return Ok(());
        }
    }
}

fn connect(seq: &Seq, sender: Addr, dest: Addr) -> bool {
    let Ok(sub) = PortSubscribe::empty() else { return false };
    sub.set_sender(sender);
    sub.set_dest(dest);
    seq.subscribe_port(&sub).is_ok()
}

/// Listens to a port if it's a controller's MIDI output.
fn add(seq: &Seq, proxy: &EventLoopProxy<InEvent>, ports: &mut HashMap<(i32, i32), Port>, a: Addr, me: i32, dest: Addr) {
    // 0 is the system's own client; 14 is "Midi Through", which only echoes software.
    if a.client == me || a.client == 0 || a.client == 14 || ports.contains_key(&(a.client, a.port)) {
        return;
    }
    let Ok(info) = seq.get_any_port_info(a) else { return };
    let caps = info.get_capability();
    if !caps.contains(PortCap::READ | PortCap::SUBS_READ) || caps.contains(PortCap::NO_EXPORT) {
        return;
    }
    let ty = info.get_type();
    if !ty.intersects(PortType::MIDI_GENERIC | PortType::HARDWARE) {
        return;
    }
    let client = seq.get_any_client_info(a.client).ok().and_then(|c| c.get_name().ok().map(str::to_string)).unwrap_or_default();
    let port = info.get_name().unwrap_or("").to_string();
    // Most controllers have one port named after the client; show the port when it adds something.
    let name = if port.is_empty() || port.starts_with(&client) { client.clone() } else { format!("{client} - {port}") };
    if !connect(seq, a, dest) {
        return;
    }
    let dinfo = DevInfo {
        inst: Arc::from(format!("midi:{}:{}", a.client, a.port)),
        id: format!("midi {name}"),
        name,
        class: Class::Midi,
        readable: true,
        vendor: 0,
        product: 0,
        ranges: HashMap::new(),
        grab: Arc::default(),
        forward: Arc::default(),
    };
    let _ = proxy.send_event(InEvent::Device(dinfo.clone()));
    ports.insert((a.client, a.port), Port { info: dinfo });
}
