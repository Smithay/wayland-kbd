//! Keyboard mapping utility for `wayland-client` using libxkbcommon.
//!
//! This library provides a simple wrapper for the wayland Keyboard objects,
//! handling all keymap issues using libxkbcommon in a dynamic way (loading the
//! library dynamically and thus not being linked to it).
//!
//! To use it, simply call `MappedKeyboard::new(..)` to wrap you keyboard object
//! and set the key_action callback. This callback will be provided the keycode,
//! the new state of the key (up or down), the keyboard ID,
//! as well as a `KbState` handle.
//!
//! This handle will allow you to retrive the keysym associated to the keycode
//! and compare it to the values defined in the `keysyms` module, or directly
//! restrieve an (utf8) String representation of this character.


#[macro_use] extern crate bitflags;
#[macro_use] extern crate dlib;
#[macro_use] extern crate lazy_static;
extern crate memmap;
extern crate wayland_client;

mod ffi;
mod mapped_keyboard;

pub use ffi::keysyms;
pub use mapped_keyboard::{MappedKeyboard, MappedKeyboardEvent, KeyEvent, MappedKeyboardError};
