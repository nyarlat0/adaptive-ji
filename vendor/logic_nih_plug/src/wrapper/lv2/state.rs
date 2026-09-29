//! LV2 state extension implementation for preset save/load.

use std::ffi::c_void;
use std::os::raw::c_char;

use crate::wrapper::state::PluginState;

/// LV2 State interface flags
pub const LV2_STATE_IS_POD: u32 = 1 << 0;
pub const LV2_STATE_IS_PORTABLE: u32 = 1 << 1;

/// LV2 State store function type
pub type Lv2StateStoreFunction = extern "C" fn(
    handle: *mut c_void,
    key: u32,
    value: *const c_void,
    size: usize,
    type_: u32,
    flags: u32,
) -> i32;

/// LV2 State retrieve function type
pub type Lv2StateRetrieveFunction = extern "C" fn(
    handle: *mut c_void,
    key: u32,
    size: *mut usize,
    type_: *mut u32,
    flags: *mut u32,
) -> *const c_void;

/// LV2 State interface
#[repr(C)]
pub struct Lv2StateInterface {
    pub save: extern "C" fn(
        instance: *mut c_void,
        store: Lv2StateStoreFunction,
        handle: *mut c_void,
        flags: u32,
        features: *const *const c_void,
    ) -> i32,
    pub restore: extern "C" fn(
        instance: *mut c_void,
        retrieve: Lv2StateRetrieveFunction,
        handle: *mut c_void,
        flags: u32,
        features: *const *const c_void,
    ) -> i32,
}

/// State key for storing plugin state
/// This would typically be a URID, but we'll use a constant for simplicity
pub const STATE_KEY_PLUGIN_STATE: u32 = 1;

/// Save plugin state using the LV2 state extension
pub fn save_state(
    plugin_state: &PluginState,
    store: Lv2StateStoreFunction,
    handle: *mut c_void,
) -> Result<(), String> {
    // Serialize the plugin state to JSON
    let state_json = serde_json::to_string(plugin_state)
        .map_err(|e| format!("Failed to serialize state: {}", e))?;

    let state_bytes = state_json.as_bytes();

    // Store the state using the LV2 store function
    // The type would typically be a URID for a string or binary blob
    let result = store(
        handle,
        STATE_KEY_PLUGIN_STATE,
        state_bytes.as_ptr() as *const c_void,
        state_bytes.len(),
        0, // Type URID (would be atom:String or similar)
        LV2_STATE_IS_POD | LV2_STATE_IS_PORTABLE,
    );

    if result == 0 {
        Ok(())
    } else {
        Err(format!("LV2 store function returned error: {}", result))
    }
}

/// Restore plugin state using the LV2 state extension
pub fn restore_state(
    retrieve: Lv2StateRetrieveFunction,
    handle: *mut c_void,
) -> Result<PluginState, String> {
    let mut size: usize = 0;
    let mut type_: u32 = 0;
    let mut flags: u32 = 0;

    // Retrieve the state data
    let data_ptr = retrieve(handle, STATE_KEY_PLUGIN_STATE, &mut size, &mut type_, &mut flags);

    if data_ptr.is_null() || size == 0 {
        return Err("No state data found".to_string());
    }

    // Convert the data to a string
    let state_bytes = unsafe { std::slice::from_raw_parts(data_ptr as *const u8, size) };
    let state_json = std::str::from_utf8(state_bytes)
        .map_err(|e| format!("Failed to decode state as UTF-8: {}", e))?;

    // Deserialize the plugin state
    let plugin_state: PluginState = serde_json::from_str(state_json)
        .map_err(|e| format!("Failed to deserialize state: {}", e))?;

    Ok(plugin_state)
}

/// Create the LV2 state interface for a wrapper
pub fn create_state_interface<W>() -> Lv2StateInterface
where
    W: Lv2StateHandler,
{
    Lv2StateInterface {
        save: lv2_state_save::<W>,
        restore: lv2_state_restore::<W>,
    }
}

/// Trait for types that can handle LV2 state save/restore
pub trait Lv2StateHandler {
    fn get_state(&self) -> PluginState;
    fn set_state(&mut self, state: PluginState);
}

/// LV2 state save callback
extern "C" fn lv2_state_save<W: Lv2StateHandler>(
    instance: *mut c_void,
    store: Lv2StateStoreFunction,
    handle: *mut c_void,
    _flags: u32,
    _features: *const *const c_void,
) -> i32 {
    if instance.is_null() {
        return 1; // Error
    }

    let wrapper = unsafe { &*(instance as *const W) };
    let state = wrapper.get_state();

    match save_state(&state, store, handle) {
        Ok(()) => 0, // Success
        Err(e) => {
            log::error!("Failed to save LV2 state: {}", e);
            1 // Error
        }
    }
}

/// LV2 state restore callback
extern "C" fn lv2_state_restore<W: Lv2StateHandler>(
    instance: *mut c_void,
    retrieve: Lv2StateRetrieveFunction,
    handle: *mut c_void,
    _flags: u32,
    _features: *const *const c_void,
) -> i32 {
    if instance.is_null() {
        return 1; // Error
    }

    let wrapper = unsafe { &mut *(instance as *mut W) };

    match restore_state(retrieve, handle) {
        Ok(state) => {
            wrapper.set_state(state);
            0 // Success
        }
        Err(e) => {
            log::error!("Failed to restore LV2 state: {}", e);
            1 // Error
        }
    }
}
