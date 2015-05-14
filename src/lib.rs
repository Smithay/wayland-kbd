#[macro_use] extern crate bitflags;
#[macro_use] extern crate lazy_static;
extern crate libc;
extern crate mmap;
extern crate wayland_client as wayland;

mod ffi;
mod mapped_keyboard;

pub use ffi::keysyms;
pub use mapped_keyboard::{MappedKeyboard, KbState};