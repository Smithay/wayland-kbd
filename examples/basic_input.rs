extern crate byteorder;
extern crate tempfile;
#[macro_use]
extern crate wayland_client;
extern crate wayland_kbd;

use byteorder::{NativeEndian, WriteBytesExt};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use wayland_client::EnvHandler;
use wayland_client::protocol::{wl_compositor, wl_seat, wl_shell, wl_shell_surface, wl_shm};
use wayland_kbd::{register_kbd, MappedKeyboardImplementation};

wayland_env!(
    WaylandEnv,
    compositor: wl_compositor::WlCompositor,
    seat: wl_seat::WlSeat,
    shm: wl_shm::WlShm,
    shell: wl_shell::WlShell
);

fn shell_surface_implementation() -> wl_shell_surface::Implementation<()> {
    wl_shell_surface::Implementation {
        ping: |_, _, shell_surface, serial| shell_surface.pong(serial),
        configure: |_, _, _, _, _, _| { /* not used in this example */ },
        popup_done: |_, _, _| { /* not used in this example */ },
    }
}

fn kbd_implementation() -> MappedKeyboardImplementation<()> {
    MappedKeyboardImplementation {
        enter: |_, _, _, _, _, mods, _, keysyms| {
            println!(
                "Gained focus while {} keys pressed and modifiers are {:?}.",
                keysyms.len(),
                mods
            );
        },
        leave: |_, _, _, _, _| {
            println!("Lost focus.");
        },
        key: |_, _, _, _, _, _, _, sym, state, utf8| {
            println!("Key {:?}: {:x}.", state, sym);
            if let Some(txt) = utf8 {
                println!("Received text \"{}\".", txt,);
            }
        },
        repeat_info: |_, _, _, rate, delay| {
            println!(
                "Received repeat info: start repeating every {}ms after an initial delay of {}ms",
                rate,
                delay
            );
        },
    }
}

fn main() {
    let (display, mut event_queue) = match wayland_client::default_connect() {
        Ok(ret) => ret,
        Err(e) => panic!("Cannot connect to wayland server: {:?}", e),
    };

    let registry = display.get_registry();
    let env_token = EnvHandler::<WaylandEnv>::init(&mut event_queue, &registry);
    event_queue.sync_roundtrip().unwrap();

    // create a tempfile to write the conents of the window on
    let mut tmp = tempfile::tempfile()
        .ok()
        .expect("Unable to create a tempfile.");
    // write the contents to it, lets put a red background
    for _ in 0..10_000 {
        let _ = tmp.write_u32::<NativeEndian>(0xFFFF0000);
    }
    let _ = tmp.flush();

    // prepare the wayland surface
    let (shell_surface, keyboard) = {
        // introduce a new scope because .state() borrows the event_queue
        let state = event_queue.state();
        // retrieve the EnvHandler
        let env = state.get(&env_token);
        let surface = env.compositor.create_surface();
        let shell_surface = env.shell.get_shell_surface(&surface);

        let pool = env.shm.create_pool(tmp.as_raw_fd(), 40_000);
        // match a buffer on the part we wrote on
        let buffer = pool.create_buffer(0, 100, 100, 400, wl_shm::Format::Argb8888)
            .expect("The pool cannot be already dead");

        // make our surface as a toplevel one
        shell_surface.set_toplevel();
        // attach the buffer to it
        surface.attach(Some(&buffer), 0, 0);
        // commit
        surface.commit();

        let keyboard = env.seat
            .get_keyboard()
            .expect("Seat cannot be already destroyed.");

        // we can let the other objects go out of scope
        // their associated wyland objects won't automatically be destroyed
        // and we don't need them in this example
        (shell_surface, keyboard)
    };

    register_kbd(&mut event_queue, &keyboard, kbd_implementation(), ()).unwrap();

    event_queue.register(&shell_surface, shell_surface_implementation(), ());

    loop {
        display.flush().unwrap();
        event_queue.dispatch().unwrap();
    }
}
