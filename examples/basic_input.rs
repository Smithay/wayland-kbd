#[macro_use]
extern crate wayland_client;
extern crate wayland_kbd;
extern crate byteorder;
extern crate tempfile;

use byteorder::{WriteBytesExt, NativeEndian};

use std::os::unix::io::AsRawFd;
use std::io::Write;

use wayland_client::{EventQueueHandle, EnvHandler};
use wayland_client::protocol::{wl_compositor, wl_shell, wl_shm, wl_shell_surface,
                               wl_seat, wl_keyboard};

use wayland_kbd::{MappedKeyboard, ModifiersState};

wayland_env!(WaylandEnv,
    compositor: wl_compositor::WlCompositor,
    seat: wl_seat::WlSeat,
    shm: wl_shm::WlShm,
    shell: wl_shell::WlShell
);

struct ShellHandler;

impl wl_shell_surface::Handler for ShellHandler {
    // required to avoid being marked as "unresponsive"
    fn ping(&mut self, _: &mut EventQueueHandle, me: &wl_shell_surface::WlShellSurface, serial: u32) {
        me.pong(serial);
    }
}

declare_handler!(ShellHandler, wl_shell_surface::Handler, wl_shell_surface::WlShellSurface);

struct KbdHandler;

impl wayland_kbd::Handler for KbdHandler {
    fn key(&mut self, _: &mut EventQueueHandle, _: &wl_keyboard::WlKeyboard, _: u32, _: u32,
            _: &ModifiersState,_: u32, _: u32, state: wl_keyboard::KeyState, utf8: Option<String>) {
        if let wl_keyboard::KeyState::Pressed = state {
            if let Some(txt) = utf8 {
                print!("{}", txt);
                ::std::io::stdout().flush().unwrap();
            }
        }
    }
}

fn main() {
    let (display, mut event_queue) = match wayland_client::default_connect() {
        Ok(ret) => ret,
        Err(e) => panic!("Cannot connect to wayland server: {:?}", e)
    };

    event_queue.add_handler(EnvHandler::<WaylandEnv>::new());
    let registry = display.get_registry();
    event_queue.register::<_, EnvHandler<WaylandEnv>>(&registry,0);
    event_queue.sync_roundtrip().unwrap();

    // create a tempfile to write the conents of the window on
    let mut tmp = tempfile::tempfile().ok().expect("Unable to create a tempfile.");
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
        let env = state.get_handler::<EnvHandler<WaylandEnv>>(0);
        let surface = env.compositor.create_surface();
        let shell_surface = env.shell.get_shell_surface(&surface);

        let pool = env.shm.create_pool(tmp.as_raw_fd(), 40_000);
        // match a buffer on the part we wrote on
        let buffer = pool.create_buffer(0, 100, 100, 400, wl_shm::Format::Argb8888).expect("The pool cannot be already dead");

        // make our surface as a toplevel one
        shell_surface.set_toplevel();
        // attach the buffer to it
        surface.attach(Some(&buffer), 0, 0);
        // commit
        surface.commit();

        let keyboard = env.seat.get_keyboard().expect("Seat cannot be already destroyed.");

        // we can let the other objects go out of scope
        // their associated wyland objects won't automatically be destroyed
        // and we don't need them in this example
        (shell_surface, keyboard)
    };

    let shell_handler = event_queue.add_handler(ShellHandler);
    event_queue.register::<_, ShellHandler>(&shell_surface, shell_handler);
    let kbd_handler = event_queue.add_handler(MappedKeyboard::new(KbdHandler).ok().expect("libxkbcommon is missing!"));
    event_queue.register::<_, MappedKeyboard<KbdHandler>>(&keyboard, kbd_handler);

    loop {
        display.flush().unwrap();
        event_queue.dispatch().unwrap();
}
}
