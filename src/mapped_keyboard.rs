use wayland::core::{Keyboard, KeyState};

use libc::size_t;

use std::ptr;
use std::sync::{Arc, Mutex};

use mmap::{MemoryMap, MapOption};

use ffi;
use ffi::XKBCOMMON_HANDLE as XKBH;

pub struct KbState {
    xkb_contex: *mut ffi::xkb_context,
    xkb_keymap: *mut ffi::xkb_keymap,
    xkb_state: *mut ffi::xkb_state
}

#[doc(hidden)]
unsafe impl Send for KbState {}

impl KbState {
    fn update_modifiers(&mut self, mods_depressed: u32, mods_latched: u32, mods_locked: u32, group: u32) {
        unsafe {
            (XKBH.xkb_state_update_mask)(
                self.xkb_state, mods_depressed, mods_latched, mods_locked, 0, 0, group);
        }
    }

    /// Tries to match this keycode as a key symbol according to current keyboard state.
    ///
    /// Returns 0 if not possible (meaning that this keycode maps to more than one key symbol).
    pub fn get_one_sym(&self, keycode: u32) -> u32 {
        unsafe { 
            (XKBH.xkb_state_key_get_one_sym)(
                self.xkb_state, keycode)
        }
    }

    /// Tries to retrieve the generated keycode as an UTF8 sequence
    pub fn get_utf8(&self, keycode: u32) -> Option<String> {
        let size = unsafe {
            (XKBH.xkb_state_key_get_utf8)(self.xkb_state, keycode, ptr::null_mut(), 0)
        } + 1;
        if size <= 1 { return None };
        let mut buffer = Vec::with_capacity(size as usize);
        unsafe {
            buffer.set_len(size as usize);
            (XKBH.xkb_state_key_get_utf8)(
                self.xkb_state, keycode, buffer.as_mut_ptr() as *mut _, size as size_t);
        };
        // remove the final `\0`
        buffer.pop();
        // libxkbcommon will always provide valid UTF8
        Some(String::from_utf8(buffer).unwrap())
    }
}

impl Drop for KbState {
    fn drop(&mut self) {
        unsafe {
            (XKBH.xkb_state_unref)(self.xkb_state);
            (XKBH.xkb_keymap_unref)(self.xkb_keymap);
            (XKBH.xkb_context_unref)(self.xkb_contex);
        }
    }
}

/// A wayland keyboard mapped to its keymap
pub struct MappedKeyboard {
    wkb: Keyboard,
    _state: Arc<Mutex<KbState>>,
    keyaction: Arc<Mutex<Box<Fn(&KbState, u32, KeyState) + Send + Sync + 'static>>>
}

impl MappedKeyboard {
    /// Creates a mapped keyboard from a regular wayland keyboard.
    ///
    /// Make sure the initialization phase of the keyboard is finished
    /// (with `Display::sync_roundtrip()` for example), otherwise the
    /// keymap won't be available.
    ///
    /// Will return Err() and hand back the untouched keyboard if 
    /// `libxkbcommon.so` is not available or the keyboard had no
    /// associated keymap.
    pub fn new(mut keyboard: Keyboard) -> Result<MappedKeyboard, Keyboard> {
        let xkbh = match ffi::XKBCOMMON_OPTION.as_ref() {
            Some(h) => h,
            None => return Err(keyboard)
        };
        let xkb_context = unsafe {
            (xkbh.xkb_context_new)(ffi::xkb_context_flags::XKB_CONTEXT_NO_FLAGS)
        };
        if xkb_context.is_null() { return Err(keyboard) }
        let (fd, size) = match keyboard.keymap_fd() {
            Some((fd, size)) => (fd, size),
            None => return Err(keyboard)
        };

        let map = MemoryMap::new(
            size as usize,
            &[MapOption::MapReadable, MapOption::MapFd(fd)]
        ).unwrap();

        let xkb_keymap = {
            unsafe {
                (xkbh.xkb_keymap_new_from_string)(
                    xkb_context,
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
            (xkbh.xkb_state_new)(xkb_keymap)
        };

        let state = Arc::new(Mutex::new(KbState {
            xkb_contex: xkb_context,
            xkb_keymap : xkb_keymap,
            xkb_state: xkb_state
        }));

        let sma_state = state.clone();
        keyboard.set_modifiers_action(move |_, mods_d, mods_la, mods_lo, group| {
            sma_state.lock().unwrap().update_modifiers(mods_d, mods_la, mods_lo, group)
        });

        let keyaction = Arc::new(Mutex::new(
            Box::new(move |_: &_, _, _|{}) as Box<Fn(&KbState, u32, KeyState) + Send + Sync + 'static>
        ));
        let ska_action = keyaction.clone();
        let ska_state  = state.clone();
        keyboard.set_key_action(move |_, _, keycode, keystate| {
            let state = ska_state.lock().unwrap();
            let action = ska_action.lock().unwrap();
            action(&*state, keycode + 8, keystate);
        });

        Ok(MappedKeyboard {
            wkb: keyboard,
            _state: state,
            keyaction: keyaction
        })
    }

    /// Releases the keyboard from this MappedKeyboard and returns it.
    pub fn release(mut self) -> Keyboard {
        self.wkb.set_key_action(move |_, _, _, _| {});
        self.wkb.set_modifiers_action(move |_, _, _, _, _| {});
        self.wkb
    }

    /// Sets the action to perform when a key is pressed or released.
    ///
    /// The closure is given an handle to a `KbState` that will allow to
    /// translate the keycode into a key symbol or an UTF8 sequence.
    pub fn set_key_action<F>(&self, f: F)
        where F: Fn(&KbState, u32, KeyState) + Send + Sync + 'static
    {
        let mut action = self.keyaction.lock().unwrap();
        *action = Box::new(f);
    }
}