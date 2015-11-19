extern crate wayland_client;
extern crate wayland_kbd;

use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::io::Write;

use wayland_client::{EventIterator, Proxy, Event};
use wayland_client::wayland::{WlDisplay, WlRegistry, get_display};
use wayland_client::wayland::{WaylandProtocolEvent, WlRegistryEvent};
use wayland_client::wayland::compositor::WlCompositor;
use wayland_client::wayland::shm::{WlShm, WlShmFormat};
use wayland_client::wayland::seat::{WlSeat, WlKeyboardKeyState};
use wayland_client::wayland::shell::{WlShell, WlShellSurfaceEvent};

use wayland_kbd::{MappedKeyboard, MappedKeyboardEvent};

struct WaylandEnv {
    display: WlDisplay,
    registry: WlRegistry,
    compositor: Option<WlCompositor>,
    seat: Option<WlSeat>,
    shm: Option<WlShm>,
    shell: Option<WlShell>,
}

impl WaylandEnv {
    fn new(mut display: WlDisplay) -> WaylandEnv {
        let registry = display.get_registry();
        display.sync_roundtrip().unwrap();

        WaylandEnv {
            display: display,
            registry: registry,
            compositor: None,
            seat: None,
            shm: None,
            shell: None
        }
    }

    fn handle_global(&mut self, name: u32, interface: &str, _version: u32) {
        match interface {
            "wl_compositor" => self.compositor = Some(
                unsafe { self.registry.bind::<WlCompositor>(name, 1) }
            ),
            "wl_seat" => self.seat = Some(
                unsafe { self.registry.bind::<WlSeat>(name, 1) }
            ),
            "wl_shell" => self.shell = Some(
                unsafe { self.registry.bind::<WlShell>(name, 1) }
            ),
            "wl_shm" => self.shm = Some(
                unsafe { self.registry.bind::<WlShm>(name, 1) }
            ),
            _ => {}
        }
    }

    fn init(&mut self, iter: &mut EventIterator) {
        for evt in iter {
            match evt {
                Event::Wayland(WaylandProtocolEvent::WlRegistry(
                    _, WlRegistryEvent::Global(name, interface, version)
                )) => {
                    self.handle_global(name, &interface, version)
                }
                _ => {}
            }
        }
        if self.compositor.is_none() || self.seat.is_none() ||
            self.shell.is_none() || self.shm.is_none() {
            panic!("Missing some globals ???");
        }
    }
}

fn main() {
    let mut display = get_display().expect("Unable to connect to Wayland server.");
    let mut event_iterator = EventIterator::new();
    display.set_evt_iterator(&event_iterator);

    let mut env = WaylandEnv::new(display);
    // the only events to handle are the globals
    env.init(&mut event_iterator);

    // create a simple surface to get input
    let surface = env.compositor.as_ref().unwrap().create_surface();
    let shell_surface = env.shell.as_ref().unwrap().get_shell_surface(&surface);
    // Not a good way to create a shared buffer, but this will do for this example.
    let mut tmp = OpenOptions::new().read(true).write(true).create(true).truncate(true)
                            .open("shm.tmp").ok().expect("Unable to create a tempfile.");
    for _ in 0..10_000 {
        let _ = tmp.write(&[0xff, 0xff, 0xff, 0xff]);
    }
    let _ = tmp.flush();
    let pool = env.shm.as_ref().unwrap().create_pool(tmp.as_raw_fd(), 40_000);
    let buffer = pool.create_buffer(0, 100, 100, 400, WlShmFormat::Argb8888 as u32);
    shell_surface.set_toplevel();
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();

    let mut mapped_keyboard = MappedKeyboard::new(&env.seat.as_ref().unwrap()).ok().expect("libxkbcommon unavailable");

    env.display.sync_roundtrip().unwrap();

    loop {
        let _ = env.display.flush();
        env.display.dispatch().unwrap();
        for evt in &mut event_iterator {
            match evt {
                Event::Wayland(WaylandProtocolEvent::WlShellSurface(
                    _proxy, WlShellSurfaceEvent::Ping(p)
                )) => {
                    shell_surface.pong(p);
                },
                _ => { /* ignore all else */ }
            }
        }
        for evt in &mut mapped_keyboard {
            if let MappedKeyboardEvent::KeyEvent(evt) = evt {
                if let WlKeyboardKeyState::Pressed = evt.keystate {
                    if let Some(txt) = evt.as_utf8() {
                        print!("{}", txt);
                        ::std::io::stdout().flush().unwrap();
                    }
                }
            }
        }
    }
}
