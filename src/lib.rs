//! Keyboard mapping utility for `wayland-client` using libxkbcommon.
//!
//! This library provide a simple implemenation for wl_keyboard objects
//! that use libxkbcommon to interpret the keyboard input according to the
//! keymap provided by the compositor.
//!
//! ## Usage
//!
//! To intialize a wl_keyboard with this crate, simply use the provided
//! `register_kbd` function. See its documentation for details.

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate dlib;
#[macro_use]
extern crate lazy_static;
extern crate memmap;
extern crate wayland_client;

mod ffi;
mod mapped_keyboard;

pub use ffi::keysyms;
pub use mapped_keyboard::{register_kbd, register_kbd_from_rmlvo, MappedKeyboardError,
                          MappedKeyboardImplementation, ModifiersState, RMLVO};
