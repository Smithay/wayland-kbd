//! Keyboard mapping utility for `wayland-client` using libxkbcommon.
//!
//! This library provides a simple handler for the wayland Keyboard objects,
//! handling all keymap issues using libxkbcommon in a dynamic way (loading the
//! library dynamically and thus not being linked to it).
//!
//! To use it, create your backend handler implementing the `wayland_kbd::Handler` trait,
//! and provide a `MappedKeyboard<YourHandler>` to the event queue. The MappedKeyboard
//! will translate the events into utf8 and forward them to you.


#[macro_use] extern crate bitflags;
#[macro_use] extern crate dlib;
#[macro_use] extern crate lazy_static;
extern crate memmap;
extern crate wayland_client;

mod ffi;
mod mapped_keyboard;

pub use ffi::keysyms;
pub use mapped_keyboard::{MappedKeyboard, MappedKeyboardError, Handler, ModifiersState};
