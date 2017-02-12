use wayland_client::EventQueueHandle;
use wayland_client::protocol::wl_keyboard::{self, WlKeyboard, KeyState, KeymapFormat};
use wayland_client::protocol::wl_surface::WlSurface;

use std::fs::File;
use std::ptr;
use std::os::unix::io::{FromRawFd, RawFd};

use std::os::raw::c_char;

use memmap::{Mmap, Protection};

use ffi;
use ffi::XKBCOMMON_HANDLE as XKBH;

struct KbState {
    xkb_context: *mut ffi::xkb_context,
    xkb_keymap: *mut ffi::xkb_keymap,
    xkb_state: *mut ffi::xkb_state,
    mods_state: ModifiersState
}

/// Represents the current state of the keyboard modifiers
///
/// Each field of this struct represents a modifier and is `true` if this modifier is active.
///
/// For some modifiers, this means that the key is currently pressed, others are toggled
/// (like caps lock).
#[derive(Copy,Clone,Debug)]
pub struct ModifiersState {
    /// The "control" key
    pub ctrl: bool,
    /// The "alt" key
    pub alt: bool,
    /// The "shift" key
    pub shift: bool,
    /// The "Caps lock" key
    pub caps_lock: bool,
    /// The "logo" key
    ///
    /// Also known as the "windows" key on most keyboards
    pub logo: bool,
    /// The "Num lock" key
    pub num_lock: bool
}

impl ModifiersState {
    fn new() -> ModifiersState {
        ModifiersState {
            ctrl: false,
            alt: false,
            shift: false,
            caps_lock: false,
            logo: false,
            num_lock: false
        }
    }

    fn update_with(&mut self, state: *mut ffi::xkb_state) {
        self.ctrl = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_CTRL.as_ptr() as *const c_char,
                ffi::XKB_STATE_MODS_EFFECTIVE
            ) > 0
        };
        self.alt = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_ALT.as_ptr() as *const c_char,
                ffi::XKB_STATE_MODS_EFFECTIVE
            ) > 0
        };
        self.shift = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_SHIFT.as_ptr() as *const c_char,
                ffi::XKB_STATE_MODS_EFFECTIVE
            ) > 0
        };
        self.caps_lock = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_CAPS.as_ptr() as *const c_char,
                ffi::XKB_STATE_MODS_EFFECTIVE
            ) > 0
        };
        self.logo = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_LOGO.as_ptr() as *const c_char,
                ffi::XKB_STATE_MODS_EFFECTIVE
            ) > 0
        };
        self.num_lock = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_NUM.as_ptr() as *const c_char,
                ffi::XKB_STATE_MODS_EFFECTIVE
            ) > 0
        };
    }
}

unsafe impl Send for KbState { }

impl KbState {
    fn update_modifiers(&mut self, mods_depressed: u32, mods_latched: u32, mods_locked: u32, group: u32) {
        let mask = unsafe {
            (XKBH.xkb_state_update_mask)(
                self.xkb_state, mods_depressed, mods_latched, mods_locked, 0, 0, group)
        };
        if mask.contains(ffi::XKB_STATE_MODS_EFFECTIVE) {
            // effective value of mods have changed, we need to update our state
            self.mods_state.update_with(self.xkb_state);
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
                xkb_state: ptr::null_mut(),
                mods_state: ModifiersState::new()
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

        state.mods_state.update_with(xkb_state);
    }

    pub fn handler(&mut self) -> &mut H {
        &mut self.handler
    }
}

#[allow(unused_variables)]
pub trait Handler {
    fn enter(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, surface: &WlSurface, mods: &ModifiersState, rawkeys: &[u32], keysyms: &[u32]) {
    }

    fn leave(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, surface: &WlSurface) {
    }

    fn key(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, time: u32, mods: &ModifiersState, rawkey: u32, keysym: u32, state: KeyState, utf8: Option<String>) {
    }

    fn repeat_info(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, rate: i32, delay: i32) {
    }
}

impl<H: Handler> wl_keyboard::Handler for MappedKeyboard<H> {
    fn keymap(&mut self, _: &mut EventQueueHandle, _: &WlKeyboard, _: KeymapFormat, fd: RawFd, size: u32) {
        self.init(fd, size as usize)
    }

    fn enter(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, surface: &WlSurface, keys: Vec<u8>) {
        let rawkeys: &[u32] = unsafe { ::std::slice::from_raw_parts(keys.as_ptr() as *const u32, keys.len()/4) };
        let keys: Vec<u32> = rawkeys.iter().map(|k| self.state.get_one_sym(*k)).collect();
        self.handler.enter(evqh, proxy, serial, surface, &self.state.mods_state, rawkeys, &keys)
    }

    fn leave(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, surface: &WlSurface) {
        self.handler.leave(evqh, proxy, serial, surface)
    }

    fn key(&mut self, evqh: &mut EventQueueHandle, proxy: &WlKeyboard, serial: u32, time: u32, key: u32, state: KeyState) {
        let sym = self.state.get_one_sym(key);
        let utf8 = self.state.get_utf8(key);
        self.handler.key(evqh, proxy, serial, time, &self.state.mods_state, key, sym, state, utf8)
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
