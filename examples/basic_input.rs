extern crate wayland_client as wayland;
extern crate wayland_kbd;

use std::fs::OpenOptions;
use std::io::Write;

use wayland::core::{default_display, ShmFormat, KeyState};

use wayland_kbd::MappedKeyboard;

fn main() {
    let display = default_display().expect("Unable to connect to Wayland server.");

    let registry = display.get_registry();
    display.sync_roundtrip();

    let compositor = registry.get_compositor().expect("Unable to get the compositor.");

    let seat = registry.get_seats().into_iter().next().expect("Unable to get the seat.");

    // create a simple surface to get input
    let surface = compositor.create_surface();
    let shell = registry.get_shell().expect("Unable to get the shell.");
    let shell_surface = shell.get_shell_surface(surface);
    let shm = registry.get_shm().expect("Unable to get the shm.");
    // Not a good way to create a shared buffer, but this will do for this example.
    let mut tmp = OpenOptions::new().read(true).write(true).create(true).truncate(true)
                            .open("shm.tmp").ok().expect("Unable to create a tempfile.");
    for _ in 0..10_000 {
        let _ = tmp.write(&[0xff, 0xff, 0xff, 0xff]);
    }
    let _ = tmp.flush();
    let pool = shm.pool_from_fd(&tmp, 40_000);
    let buffer = pool.create_buffer(0, 100, 100, 400, ShmFormat::WL_SHM_FORMAT_ARGB8888)
                     .expect("Could not create buffer.");
    shell_surface.set_toplevel();
    shell_surface.attach(&buffer, 0, 0);
    shell_surface.commit();

    display.sync_roundtrip();

    let keyboard = seat.get_keyboard().expect("Unable to get the keyboard.");

    // sync for the keyboard to retrieve its keymap
    display.sync_roundtrip();

    let mapped_keyboard = MappedKeyboard::new(keyboard).ok().expect("libxkbcommon unavailable");

    mapped_keyboard.set_key_action(|kbstate, _, _, keycode, keystate| {
        if keystate == KeyState::WL_KEYBOARD_KEY_STATE_PRESSED {
            if let Some(txt) = kbstate.get_utf8(keycode) {
                print!("{}", txt);
            }
            let _ = ::std::io::stdout().flush();
        }
    });
    
    loop {
        let _ = display.flush();
        display.dispatch();
    }
}
