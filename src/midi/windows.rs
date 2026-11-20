//! Windows: MIDI through the system's MIDI API (midir). Inputs are looked for every
//! moment or so, so controllers plugged in later are picked up and unplugged ones dropped.

use std::{
    collections::HashMap,
    sync::{atomic::Ordering::Relaxed, Arc},
    time::{Duration, SystemTime},
};

use midir::{MidiInput, MidiInputConnection};
use winit::event_loop::EventLoopProxy;

use crate::{
    devices::{Class, DevInfo},
    input::{micros, InEvent},
};

const RESCAN: Duration = Duration::from_millis(1500);

pub fn run(proxy: EventLoopProxy<InEvent>) -> anyhow::Result<()> {
    let mut open: HashMap<String, (MidiInputConnection<()>, Arc<str>)> = HashMap::new();
    loop {
        let lister = MidiInput::new("CUT")?;
        let names: Vec<String> = lister.ports().iter().filter_map(|p| lister.port_name(p).ok()).collect();
        // Unplugged.
        let gone: Vec<String> = open.keys().filter(|n| !names.contains(n)).cloned().collect();
        for name in gone {
            if let Some((conn, inst)) = open.remove(&name) {
                conn.close();
                let _ = proxy.send_event(InEvent::Gone(inst));
            }
        }
        // Plugged in (or there at start).
        let new: Vec<&String> = names.iter().filter(|n| !open.contains_key(*n)).collect();
        for name in new {
            let input = MidiInput::new("CUT")?;
            let Some(port) = input.ports().into_iter().find(|p| input.port_name(p).as_deref() == Ok(name.as_str())) else { continue };
            let info = DevInfo {
                inst: Arc::from(format!("midi:{name}")),
                id: format!("midi {name}"),
                name: name.clone(),
                class: Class::Midi,
                readable: true,
                vendor: 0,
                product: 0,
                ranges: HashMap::new(),
                grab: Arc::default(),
                forward: Arc::default(),
            };
            let (forward, inst, tx) = (info.forward.clone(), info.inst.clone(), proxy.clone());
            let conn = input.connect(
                &port,
                "CUT controllers in",
                move |_, msg, _| {
                    if !forward.load(Relaxed) {
                        return;
                    }
                    if let Some((ctl, value)) = super::parse(msg) {
                        let _ = tx.send_event(InEvent::Control { dev: inst.clone(), ctl, value, t: micros(SystemTime::now()) });
                    }
                },
                (),
            );
            match conn {
                Ok(conn) => {
                    let _ = proxy.send_event(InEvent::Device(info.clone()));
                    open.insert(name.clone(), (conn, info.inst));
                }
                // Another program may have it open: Windows lets only one at a time.
                Err(e) => eprintln!("MIDI {name}: {e}"),
            }
        }
        std::thread::sleep(RESCAN);
    }
}
