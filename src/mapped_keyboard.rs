use ffi::{self, xkb_state_component};
use ffi::XKBCOMMON_HANDLE as XKBH;
use memmap::MmapOptions;
use std::env;
use std::ffi::CString;
use std::fs::File;
use std::os::raw::c_char;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::io::{FromRawFd, RawFd};
use std::ptr;
use wayland_client::EventQueueHandle;
use wayland_client::protocol::wl_keyboard::{self, KeyState, KeymapFormat, WlKeyboard};
use wayland_client::protocol::wl_surface::WlSurface;

struct KbState {
    xkb_context: *mut ffi::xkb_context,
    xkb_keymap: *mut ffi::xkb_keymap,
    xkb_state: *mut ffi::xkb_state,
    xkb_compose_table: *mut ffi::xkb_compose_table,
    xkb_compose_state: *mut ffi::xkb_compose_state,
    mods_state: ModifiersState,
    locked: bool,
}

/// Represents the current state of the keyboard modifiers
///
/// Each field of this struct represents a modifier and is `true` if this modifier is active.
///
/// For some modifiers, this means that the key is currently pressed, others are toggled
/// (like caps lock).
#[derive(Copy, Clone, Debug)]
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
    pub num_lock: bool,
}

impl ModifiersState {
    fn new() -> ModifiersState {
        ModifiersState {
            ctrl: false,
            alt: false,
            shift: false,
            caps_lock: false,
            logo: false,
            num_lock: false,
        }
    }

    fn update_with(&mut self, state: *mut ffi::xkb_state) {
        self.ctrl = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_CTRL.as_ptr() as *const c_char,
                xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
            ) > 0
        };
        self.alt = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_ALT.as_ptr() as *const c_char,
                xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
            ) > 0
        };
        self.shift = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_SHIFT.as_ptr() as *const c_char,
                xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
            ) > 0
        };
        self.caps_lock = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_CAPS.as_ptr() as *const c_char,
                xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
            ) > 0
        };
        self.logo = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_LOGO.as_ptr() as *const c_char,
                xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
            ) > 0
        };
        self.num_lock = unsafe {
            (XKBH.xkb_state_mod_name_is_active)(
                state,
                ffi::XKB_MOD_NAME_NUM.as_ptr() as *const c_char,
                xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
            ) > 0
        };
    }
}

unsafe impl Send for KbState {}

impl KbState {
    fn update_modifiers(&mut self, mods_depressed: u32, mods_latched: u32, mods_locked: u32, group: u32) {
        if !self.ready() {
            return;
        }
        let mask = unsafe {
            (XKBH.xkb_state_update_mask)(
                self.xkb_state,
                mods_depressed,
                mods_latched,
                mods_locked,
                0,
                0,
                group,
            )
        };
        if mask.contains(xkb_state_component::XKB_STATE_MODS_EFFECTIVE) {
            // effective value of mods have changed, we need to update our state
            self.mods_state.update_with(self.xkb_state);
        }
    }

    fn get_one_sym_raw(&mut self, keycode: u32) -> u32 {
        if !self.ready() {
            return 0;
        }
        unsafe { (XKBH.xkb_state_key_get_one_sym)(self.xkb_state, keycode + 8) }
    }

    fn get_utf8_raw(&mut self, keycode: u32) -> Option<String> {
        if !self.ready() {
            return None;
        }
        let size =
            unsafe { (XKBH.xkb_state_key_get_utf8)(self.xkb_state, keycode + 8, ptr::null_mut(), 0) } + 1;
        if size <= 1 {
            return None;
        };
        let mut buffer = Vec::with_capacity(size as usize);
        unsafe {
            buffer.set_len(size as usize);
            (XKBH.xkb_state_key_get_utf8)(
                self.xkb_state,
                keycode + 8,
                buffer.as_mut_ptr() as *mut _,
                size as usize,
            );
        };
        // remove the final `\0`
        buffer.pop();
        // libxkbcommon will always provide valid UTF8
        Some(unsafe { String::from_utf8_unchecked(buffer) })
    }

    fn compose_feed(&mut self, keysym: u32) -> Option<ffi::xkb_compose_feed_result> {
        if !self.ready() || self.xkb_compose_state.is_null() {
            return None;
        }
        Some(unsafe {
            (XKBH.xkb_compose_state_feed)(self.xkb_compose_state, keysym)
        })
    }

    fn compose_status(&mut self) -> Option<ffi::xkb_compose_status> {
        if !self.ready() || self.xkb_compose_state.is_null() {
            return None;
        }
        Some(unsafe {
            (XKBH.xkb_compose_state_get_status)(self.xkb_compose_state)
        })
    }

    fn compose_get_utf8(&mut self) -> Option<String> {
        if !self.ready() || self.xkb_compose_state.is_null() {
            return None;
        }
        let size =
            unsafe { (XKBH.xkb_compose_state_get_utf8)(self.xkb_compose_state, ptr::null_mut(), 0) } + 1;
        if size <= 1 {
            return None;
        };
        let mut buffer = Vec::with_capacity(size as usize);
        unsafe {
            buffer.set_len(size as usize);
            (XKBH.xkb_compose_state_get_utf8)(
                self.xkb_compose_state,
                buffer.as_mut_ptr() as *mut _,
                size as usize,
            );
        };
        // remove the final `\0`
        buffer.pop();
        // libxkbcommon will always provide valid UTF8
        Some(unsafe { String::from_utf8_unchecked(buffer) })
    }

    fn new() -> Result<KbState, MappedKeyboardError> {
        let xkbh = match ffi::XKBCOMMON_OPTION.as_ref() {
            Some(h) => h,
            None => return Err(MappedKeyboardError::XKBNotFound),
        };
        let xkb_context = unsafe { (xkbh.xkb_context_new)(ffi::xkb_context_flags::XKB_CONTEXT_NO_FLAGS) };
        if xkb_context.is_null() {
            return Err(MappedKeyboardError::XKBNotFound);
        }

        let mut me = KbState {
            xkb_context: xkb_context,
            xkb_keymap: ptr::null_mut(),
            xkb_state: ptr::null_mut(),
            xkb_compose_table: ptr::null_mut(),
            xkb_compose_state: ptr::null_mut(),
            mods_state: ModifiersState::new(),
            locked: false,
        };

        unsafe {
            me.init_compose();
        }

        Ok(me)
    }

    unsafe fn init_compose(&mut self) {
        let locale = env::var_os("LC_ALL")
            .or_else(|| env::var_os("LC_CTYPE"))
            .or_else(|| env::var_os("LANG"))
            .unwrap_or_else(|| "C".into());
        let locale = CString::new(locale.into_vec()).unwrap();

        let compose_table = (XKBH.xkb_compose_table_new_from_locale)(
            self.xkb_context,
            locale.as_ptr(),
            ffi::xkb_compose_compile_flags::XKB_COMPOSE_COMPILE_NO_FLAGS,
        );

        if compose_table.is_null() {
            // init of compose table failed, continue without compose
            return;
        }

        let compose_state = (XKBH.xkb_compose_state_new)(
            compose_table,
            ffi::xkb_compose_state_flags::XKB_COMPOSE_STATE_NO_FLAGS,
        );

        if compose_state.is_null() {
            // init of compose state failed, continue without compose
            (XKBH.xkb_compose_table_unref)(compose_table);
            return;
        }

        self.xkb_compose_table = compose_table;
        self.xkb_compose_state = compose_state;
    }

    unsafe fn post_init(&mut self, xkb_keymap: *mut ffi::xkb_keymap) {
        let xkb_state = (XKBH.xkb_state_new)(xkb_keymap);
        self.xkb_keymap = xkb_keymap;
        self.xkb_state = xkb_state;
        self.mods_state.update_with(xkb_state);
    }

    unsafe fn de_init(&mut self) {
        (XKBH.xkb_state_unref)(self.xkb_state);
        self.xkb_state = ptr::null_mut();
        (XKBH.xkb_keymap_unref)(self.xkb_keymap);
        self.xkb_keymap = ptr::null_mut();
    }

    unsafe fn init_with_fd(&mut self, fd: RawFd, size: usize) {
        let map = MmapOptions::new().len(size).map(&File::from_raw_fd(fd)).unwrap();

        let xkb_keymap = (XKBH.xkb_keymap_new_from_string)(
            self.xkb_context,
            map.as_ptr() as *const _,
            ffi::xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
            ffi::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
        );

        if xkb_keymap.is_null() {
            panic!("Received invalid keymap from compositor.");
        }

        self.post_init(xkb_keymap);
    }

    unsafe fn init_with_rmlvo(&mut self, names: ffi::xkb_rule_names) -> Result<(), MappedKeyboardError> {
        let xkb_keymap = (XKBH.xkb_keymap_new_from_names)(
            self.xkb_context,
            &names,
            ffi::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
        );

        if xkb_keymap.is_null() {
            return Err(MappedKeyboardError::BadNames);
        }

        self.post_init(xkb_keymap);

        Ok(())
    }

    #[inline]
    fn ready(&self) -> bool {
        !self.xkb_state.is_null()
    }
}

impl Drop for KbState {
    fn drop(&mut self) {
        unsafe {
            (XKBH.xkb_compose_state_unref)(self.xkb_compose_state);
            (XKBH.xkb_compose_table_unref)(self.xkb_compose_table);
            (XKBH.xkb_state_unref)(self.xkb_state);
            (XKBH.xkb_keymap_unref)(self.xkb_keymap);
            (XKBH.xkb_context_unref)(self.xkb_context);
        }
    }
}

#[derive(Debug)]
/// An error that occured while trying to initialize a mapped keyboard
pub enum MappedKeyboardError {
    /// libxkbcommon is not available
    XKBNotFound,
    /// Provided RMLVO sepcified a keymap that would not be loaded
    BadNames,
}

/// Register a keyboard with the implementation provided by this crate
///
/// This requires you to provide an implementation and its implementation data
/// to receive the events after they have been interpreted with the keymap.
///
/// The keymap information will be loaded from the events sent by the compositor,
/// as such you need to call this method as soon as you have created the keyboard
/// to make sure this event does not get lost.
///
/// Returns an error if xkbcommon could not be initialized.
pub fn register_kbd<ID: 'static>(evqh: &mut EventQueueHandle, kbd: &WlKeyboard,
                                 implem: MappedKeyboardImplementation<ID>, idata: ID)
                                 -> Result<(), MappedKeyboardError> {
    let mapped_kbd = KbState::new()?;
    evqh.register(
        kbd,
        wl_keyboard_implementation(),
        (mapped_kbd, implem, idata),
    );
    Ok(())
}

/// The RMLVO description of a keymap
///
/// All fiels are optional, and the system default
/// will be used if set to `None`.
pub struct RMLVO {
    /// The rules file to use
    pub rules: Option<String>,
    /// The keyboard model by which to interpret keycodes and LEDs
    pub model: Option<String>,
    /// A comma seperated list of layouts (languages) to include in the keymap
    pub layout: Option<String>,
    /// A comma seperated list of variants, one per layout, which may modify or
    /// augment the respective layout in various ways
    pub variant: Option<String>,
    /// A comma seprated list of options, through which the user specifies
    /// non-layout related preferences, like which key combinations are
    /// used for switching layouts, or which key is the Compose key.
    pub options: Option<String>,
}

/// Register a keyboard with the implementation provided by this crate
///
/// This requires you to provide an implementation and its implementation data
/// to receive the events after they have been interpreted with the keymap.
///
/// The keymap will be loaded from the provided RMLVO rules. Any keymap provided
/// by the compositor will be ignored.
///
/// Returns an error if xkbcommon could not be initialized.
pub fn register_kbd_from_rmlvo<ID: 'static>(evqh: &mut EventQueueHandle, kbd: &WlKeyboard,
                                            implem: MappedKeyboardImplementation<ID>, idata: ID,
                                            rmlvo: RMLVO)
                                            -> Result<(), MappedKeyboardError> {
    let mut mapped_kbd = KbState::new()?;

    fn to_cstring(s: Option<String>) -> Result<Option<CString>, MappedKeyboardError> {
        s.map_or(Ok(None), |s| CString::new(s).map(Option::Some))
            .map_err(|_| MappedKeyboardError::BadNames)
    }

    let rules = to_cstring(rmlvo.rules)?;
    let model = to_cstring(rmlvo.model)?;
    let layout = to_cstring(rmlvo.layout)?;
    let variant = to_cstring(rmlvo.variant)?;
    let options = to_cstring(rmlvo.options)?;

    let xkb_names = ffi::xkb_rule_names {
        rules: rules.map_or(ptr::null(), |s| s.as_ptr()),
        model: model.map_or(ptr::null(), |s| s.as_ptr()),
        layout: layout.map_or(ptr::null(), |s| s.as_ptr()),
        variant: variant.map_or(ptr::null(), |s| s.as_ptr()),
        options: options.map_or(ptr::null(), |s| s.as_ptr()),
    };

    unsafe {
        mapped_kbd.init_with_rmlvo(xkb_names)?;
    }

    mapped_kbd.locked = true;

    evqh.register(
        kbd,
        wl_keyboard_implementation(),
        (mapped_kbd, implem, idata),
    );
    Ok(())
}

pub struct MappedKeyboardImplementation<ID> {
    pub enter: fn(
     evqh: &mut EventQueueHandle,
     idata: &mut ID,
     keyboard: &WlKeyboard,
     serial: u32,
     surface: &WlSurface,
     mods: ModifiersState,
     rawkeys: &[u32],
     keysyms: &[u32],
    ),
    pub leave: fn(
     evqh: &mut EventQueueHandle,
     idata: &mut ID,
     keyboard: &WlKeyboard,
     serial: u32,
     surface: &WlSurface,
    ),
    pub key: fn(
     evqh: &mut EventQueueHandle,
     idata: &mut ID,
     keyboard: &WlKeyboard,
     serial: u32,
     time: u32,
     mods: ModifiersState,
     rawkey: u32,
     keysym: u32,
     state: KeyState,
     utf8: Option<String>,
    ),
    pub repeat_info:
        fn(evqh: &mut EventQueueHandle, idata: &mut ID, keyboard: &WlKeyboard, rate: i32, delay: i32),
}

fn wl_keyboard_implementation<ID: 'static>(
    )
    -> wl_keyboard::Implementation<(KbState, MappedKeyboardImplementation<ID>, ID)>
{
    wl_keyboard::Implementation {
        keymap: |_, &mut (ref mut state, _, _), _keyboard, format, fd, size| {
            if state.locked {
                // state is locked, ignore keymap updates
                return;
            }
            if state.ready() {
                // new keymap, we first deinit to free resources
                unsafe {
                    state.de_init();
                }
            }
            match format {
                KeymapFormat::XkbV1 => unsafe {
                    state.init_with_fd(fd, size as usize);
                },
                KeymapFormat::NoKeymap => {
                    // TODO: how to handle this (hopefully never occuring) case?
                }
            }
        },
        enter: |evqh, &mut (ref mut state, ref implem, ref mut idata), keyboard, serial, surface, keys| {
            let rawkeys: &[u32] =
                unsafe { ::std::slice::from_raw_parts(keys.as_ptr() as *const u32, keys.len() / 4) };
            let (keys, mods_state) = {
                let keys: Vec<u32> = rawkeys.iter().map(|k| state.get_one_sym_raw(*k)).collect();
                (keys, state.mods_state.clone())
            };
            (implem.enter)(
                evqh,
                idata,
                keyboard,
                serial,
                surface,
                mods_state,
                rawkeys,
                &keys,
            )
        },
        leave: |evqh, &mut (_, ref implem, ref mut idata), keyboard, serial, surface| {
            (implem.leave)(evqh, idata, keyboard, serial, surface)
        },
        key: |evqh,
              &mut (ref mut state, ref implem, ref mut idata),
              keyboard,
              serial,
              time,
              key,
              key_state| {
            let sym = state.get_one_sym_raw(key);
            let ignore_text = if key_state == KeyState::Pressed {
                state.compose_feed(sym) != Some(ffi::xkb_compose_feed_result::XKB_COMPOSE_FEED_ACCEPTED)
            } else {
                true
            };
            let utf8 = if ignore_text {
                None
            } else if let Some(status) = state.compose_status() {
                match status {
                    ffi::xkb_compose_status::XKB_COMPOSE_COMPOSED => state.compose_get_utf8(),
                    ffi::xkb_compose_status::XKB_COMPOSE_NOTHING => state.get_utf8_raw(key),
                    _ => None,
                }
            } else {
                state.get_utf8_raw(key)
            };
            let mods_state = state.mods_state.clone();
            (implem.key)(
                evqh,
                idata,
                keyboard,
                serial,
                time,
                mods_state,
                key,
                sym,
                key_state,
                utf8,
            )
        },
        modifiers: |_,
                    &mut (ref mut state, _, _),
                    _keyboard,
                    _,
                    mods_depressed,
                    mods_latched,
                    mods_locked,
                    group| { state.update_modifiers(mods_depressed, mods_latched, mods_locked, group) },
        repeat_info: |evqh, &mut (_, ref implem, ref mut idata), keyboard, rate, delay| {
            (implem.repeat_info)(evqh, idata, keyboard, rate, delay)
        },
    }
}
