#[macro_use]
extern crate wayland_client;
extern crate wayland_kbd;
extern crate byteorder;
extern crate tempfile;

use byteorder::{WriteBytesExt, NativeEndian};

use std::os::unix::io::AsRawFd;
use std::io::Write;

use wayland_client::Event;
use wayland_client::wayland::{get_display, WaylandProtocolEvent};
use wayland_client::wayland::compositor::WlCompositor;
use wayland_client::wayland::shm::{WlShm, WlShmFormat};
use wayland_client::wayland::seat::{WlSeat, WlKeyboardKeyState};
use wayland_client::wayland::shell::{WlShell, WlShellSurfaceEvent};

use wayland_kbd::{MappedKeyboard, MappedKeyboardEvent};

wayland_env!(WaylandEnv,
    compositor: WlCompositor,
    seat: WlSeat,
    shm: WlShm,
    shell: WlShell
);

fn main() {
    let (display, event_iterator) = get_display().expect("Unable to connect to Wayland server.");

    let (env, mut event_iterator) = WaylandEnv::init(display, event_iterator);

    // quickly extract the global we need, and fail-fast if any is missing
    // should not happen, as these are supposed to always be implemented by
    // the compositor
    let compositor = env.compositor.as_ref().map(|o| &o.0).unwrap();
    let seat = env.seat.as_ref().map(|o| &o.0).unwrap();
    let shell = env.shell.as_ref().map(|o| &o.0).unwrap();
    let shm = env.shm.as_ref().map(|o| &o.0).unwrap();

    let surface = compositor.create_surface();
    let shell_surface = shell.get_shell_surface(&surface);
    shell_surface.set_toplevel();

    // create a tempfile to write on
    let mut tmp = tempfile::tempfile().ok().expect("Unable to create a tempfile.");
    // write the contents to it, lets put everything in dark red
    for _ in 0..10_000 {
        let _ = tmp.write_u32::<NativeEndian>(0xFFFFFFFF);
    }
    let _ = tmp.flush();
    let pool = shm.create_pool(tmp.as_raw_fd(), 40_000);
    let buffer = pool.create_buffer(0, 100, 100, 400, WlShmFormat::Argb8888);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();

    let mut mapped_keyboard = MappedKeyboard::new(seat, &env.display).ok().expect("libxkbcommon unavailable");

    event_iterator.sync_roundtrip().unwrap();

    loop {
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
        event_iterator.dispatch().expect("Connection with the compositor was lost.");
    }
}
