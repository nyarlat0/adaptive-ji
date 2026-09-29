//! Utility functions for the LV2 wrapper.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Convert a Rust string to a C string pointer.
/// The caller is responsible for freeing the memory.
pub unsafe fn string_to_c_char(s: &str) -> *mut c_char {
    let c_string = CString::new(s).unwrap_or_else(|_| CString::new("").unwrap());
    c_string.into_raw()
}

/// Convert a C string pointer to a Rust string slice.
pub unsafe fn c_char_to_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok()
}

/// Free a C string that was allocated by `string_to_c_char`.
pub unsafe fn free_c_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = CString::from_raw(ptr);
    }
}
