use wayland_client::{Event, EventIterator, Proxy};
use wayland_client::wayland::seat::{WlSeat, WlKeyboard, WlKeyboardEvent, WlKeyboardKeyState};

use std::iter::Iterator;
use std::ptr;
use std::os::unix::io::RawFd;
use std::sync::{Arc, Mutex};

use mmap::{MemoryMap, MapOption};

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
        Some(String::from_utf8(buffer).unwrap())
    }
}

pub enum MappedKeyboardEvent {
    KeyEvent(KeyEvent),
    Other(WlKeyboardEvent)
}

pub struct KeyEvent {
    keycode: u32,
    state: Arc<Mutex<KbState>>,
    pub serial: u32,
    pub time: u32,
    pub keystate: WlKeyboardKeyState
}

impl KeyEvent {
    /// Tries to retrieve the key event as an UTF8 sequence
    pub fn as_utf8(&self) -> Option<String> {
        self.state.lock().unwrap().get_utf8(self.keycode)
    }

    // Tries to match this key event as a key symbol according to current keyboard state.
    ///
    /// Returns 0 if not possible (meaning that this keycode maps to more than one key symbol).
    pub fn as_symbol(&self) -> Option<u32> {
        let val = self.state.lock().unwrap().get_one_sym(self.keycode);
        if val == 0 {
            None
        } else {
            Some(val)
        }
    }
}

#[doc(hidden)]
unsafe impl Send for KbState {}

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
pub struct MappedKeyboard {
    wkb: WlKeyboard,
    state: Arc<Mutex<KbState>>,
    iter: EventIterator
}

pub enum MappedKeyboardError {
    XKBNotFound,
    NoKeyboardOnSeat
}

impl MappedKeyboard {
    /// Creates a mapped keyboard by extracting the keyboard from a seat.
    ///
    /// Make sure the initialization phase of the keyboard is finished
    /// (with `Display::sync_roundtrip()` for example), otherwise the
    /// keymap won't be available.
    ///
    /// Will return Err() if `libxkbcommon.so` is not available or the
    /// keyboard had no associated keymap.
    pub fn new(seat: &WlSeat) -> Result<MappedKeyboard, MappedKeyboardError> {
        let mut keyboard = seat.get_keyboard();
        let xkbh = match ffi::XKBCOMMON_OPTION.as_ref() {
            Some(h) => h,
            None => return Err(MappedKeyboardError::XKBNotFound)
        };
        let xkb_context = unsafe {
            (xkbh.xkb_context_new)(ffi::xkb_context_flags::XKB_CONTEXT_NO_FLAGS)
        };
        if xkb_context.is_null() { return Err(MappedKeyboardError::XKBNotFound) }

        let iter = EventIterator::new();

        keyboard.set_evt_iterator(&iter);

        Ok(MappedKeyboard {
            wkb: keyboard,
            state: Arc::new(Mutex::new(KbState {
                xkb_context: xkb_context,
                xkb_keymap: ptr::null_mut(),
                xkb_state: ptr::null_mut()
            })),
            iter: iter
        })
    }


    fn init(&mut self, fd: RawFd, size: usize) {
        let mut state = self.state.lock().unwrap();

        let map = MemoryMap::new(
            size as usize,
            &[MapOption::MapReadable, MapOption::MapFd(fd)]
        ).unwrap();

        let xkb_keymap = {
            unsafe {
                (XKBH.xkb_keymap_new_from_string)(
                    state.xkb_context,
                    map.data() as *const _,
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

impl Iterator for MappedKeyboard {
    type Item = MappedKeyboardEvent;

    fn next(&mut self) -> Option<MappedKeyboardEvent> {
        use wayland_client::wayland::WaylandProtocolEvent;
        use wayland_client::wayland::seat::WlKeyboardEvent;
        loop {
            match self.iter.next() {
                None => return None,
                Some(Event::Wayland(WaylandProtocolEvent::WlKeyboard(proxy, event))) => {
                    if proxy == self.wkb.id() {
                        match event {
                            WlKeyboardEvent::Keymap(_format, fd, size) => {
                                self.init(fd, size as usize);
                                continue
                            },
                            WlKeyboardEvent::Modifiers(_, mods_d, mods_la, mods_lo, group) => {
                                self.state.lock().unwrap().update_modifiers(mods_d,mods_la, mods_lo, group);
                                continue;
                            }
                            WlKeyboardEvent::Key(serial, time, key, keystate) => {
                                return Some(MappedKeyboardEvent::KeyEvent(KeyEvent {
                                    keycode: key,
                                    state: self.state.clone(),
                                    time: time,
                                    serial: serial,
                                    keystate: keystate
                                }));
                            }
                            _ => return Some(MappedKeyboardEvent::Other(event))
                        }
                    } else {
                        // should never happen, actually...
                        continue
                    }
                },
                // should never happen, actually...
                _ => continue
            }
        }
    }
}
