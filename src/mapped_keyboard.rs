use wayland_client::EventQueueHandle;
use wayland_client::protocol::wl_keyboard::{self, WlKeyboard, KeyState, KeymapFormat};
use wayland_client::protocol::wl_surface::WlSurface;

use std::fs::File;
use std::ptr;
use std::os::unix::io::{FromRawFd, RawFd};

use memmap::{Mmap, Protection};

use ffi;
use ffi::XKBCOMMON_HANDLE as XKBH;

struct KbState {
    xkb_context: *mut ffi::xkb_context,
    xkb_keymap: *mut ffi::xkb_keymap,
    xkb_state: *mut ffi::xkb_state
}

impl KbState {
    fn update_modifiers(&mut self, mods_depressed: u32, mods_latched: u32, mods_locked: u32, group: u32) {
        unsafe {
            (XKBH.xkb_state_update_mask)(
                self.xkb_state, mods_depressed, mods_latched, mods_locked, 0, 0, group);
        }
    }

    pub fn get_one_sym(&self, keycode: u32) -> u32 {
        unsafe {
            (XKBH.xkb_state_key_get_one_sym)(
                self.xkb_state, keycode + 8)
        }
    }

    pub fn get_utf8(&self, keycode: u32) -> Option<String> {
        let size = unsafe {
            (XKBH.xkb_state_key_get_utf8)(self.xkb_state, keycode + 8, ptr::null_mut(), 0)
        } + 1;
        if size <= 1 { return None };
        let mut buffer = Vec::with_capacity(size as usize);
        unsafe {
            buffer.set_len(size as usize);
            (XKBH.xkb_state_key_get_utf8)(
                self.xkb_state, keycode + 8, buffer.as_mut_ptr() as *mut _, size as usize);
        };
        // remove the final `\0`
        buffer.pop();
        // libxkbcommon will always provide valid UTF8
        Some(unsafe { String::from_utf8_unchecked(buffer) } )
    }
}

impl Drop for KbState {
    fn drop(&mut self) {
        unsafe {
            (XKBH.xkb_state_unref)(self.xkb_state);
            (XKBH.xkb_keymap_unref)(self.xkb_keymap);
            (XKBH.xkb_context_unref)(self.xkb_context);
        }
    }
}

/// A wayland keyboard mapped to its keymap
///
/// It wraps an event iterator on this keyboard, catching the Keymap, Key, and Modifiers
/// events of the keyboard to handle them using libxkbcommon. All other events are directly
/// forwarded.
pub struct MappedKeyboard<H: Handler> {
    state: KbState,
    handler: H
}

pub enum MappedKeyboardError {
    XKBNotFound,
    NoKeyboardOnSeat
}

impl<H: Handler> MappedKeyboard<H> {
    pub fn new(handler: H) -> Result<MappedKeyboard<H>, MappedKeyboardError> {
        let xkbh = match ffi::XKBCOMMON_OPTION.as_ref() {
            Some(h) => h,
            None => return Err(MappedKeyboardError::XKBNotFound)
        };
        let xkb_context = unsafe {
            (xkbh.xkb_context_new)(ffi::xkb_context_flags::XKB_CONTEXT_NO_FLAGS)
        };
        if xkb_context.is_null() { return Err(MappedKeyboardError::XKBNotFound) }

        Ok(MappedKeyboard {
            state: KbState {
                xkb_context: xkb_context,
                xkb_keymap: ptr::null_mut(),
                xkb_state: ptr::null_mut()
            },
            handler: handler
        })
    }


    fn init(&mut self, fd: RawFd, size: usize) {
        let mut state = &mut self.state;

        let map = unsafe {
            Mmap::open_with_offset(&File::from_raw_fd(fd), Protection::Read, 0, size).unwrap()
        };

        let xkb_keymap = {
            unsafe {
                (XKBH.xkb_keymap_new_from_string)(
                    state.xkb_context,
                    map.ptr() as *const _,
                    ffi::xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
                    ffi::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS
                )
            }
        };

        if xkb_keymap.is_null() {
            panic!("Failed to load keymap!");
        }

        let xkb_state = unsafe {
            (XKBH.xkb_state_new)(xkb_keymap)
        };

        state.xkb_keymap = xkb_keymap;
        state.xkb_state = xkb_state;
    }
}

#[allow(unused_variables)]
pub trait Handler {
    fn enter(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, surface: &WlSurface, keysyms: Vec<u32>) {
    }

    fn leave(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, surface: &WlSurface) {
    }

    fn key(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, time: u32, keysym: u32, state: KeyState, utf8: Option<String>) {
    }

    fn repeat_info(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, rate: i32, delay: i32) {
    }
}

impl<H: Handler> wl_keyboard::Handler for MappedKeyboard<H> {
    fn keymap(&mut self, _: &mut EventQueueHandle, _: &WlKeyboard, _: KeymapFormat, fd: RawFd, size: u32) {
        self.init(fd, size as usize)
    }

    fn enter(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, surface: &WlSurface, keys: Vec<u8>) {
        let keys: &[u32] = unsafe { ::std::slice::from_raw_parts(keys.as_ptr() as *const u32, keys.len()/4) };
        let keys = keys.iter().map(|k| self.state.get_one_sym(*k)).collect();
        self.handler.enter(evqh, proxy, serial, surface, keys)
    }

    fn leave(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, surface: &WlSurface) {
        self.handler.leave(evqh, proxy, serial, surface)
    }

    fn key(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, time: u32, key: u32, state: KeyState) {
        let sym = self.state.get_one_sym(key);
        let utf8 = self.state.get_utf8(key);
        self.handler.key(evqh, proxy, serial, time, sym, state, utf8)
    }

    fn modifiers(&mut self, _: &mut EventQueueHandle, _: &WlKeyboard, _: u32, mods_depressed: u32, mods_latched: u32, mods_locked: u32, group: u32) {
        self.state.update_modifiers(mods_depressed, mods_latched, mods_locked, group)
    }

    fn repeat_info(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, rate: i32, delay: i32) {
        self.handler.repeat_info(evqh, proxy, rate, delay)
    }
}

unsafe impl<H: Handler> ::wayland_client::Handler<WlKeyboard> for MappedKeyboard<H> {
    unsafe fn message(&mut self, evq: &mut EventQueueHandle, proxy: &WlKeyboard, opcode: u32, args: *const ::wayland_client::sys::wl_argument) -> Result<(),()> {
        <MappedKeyboard<H> as ::wayland_client::protocol::wl_keyboard::Handler>::__message(self, evq, proxy, opcode, args)
    }
}
