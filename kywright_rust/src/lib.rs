mod brave_filter;
mod filter;
mod dom;

use filter::StreamFilter;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

#[no_mangle]
pub unsafe extern "C" fn filter_html(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return ptr::null_mut();
    }

    let c_str = CStr::from_ptr(input);
    let input_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    let result = catch_unwind(AssertUnwindSafe(|| filter::filter_html_str(input_str)));

    match result {
        Ok(filtered) => match CString::new(filtered) {
            Ok(cstring) => cstring.into_raw(),
            Err(_) => ptr::null_mut(),
        },
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn kywright_free_string(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    drop(CString::from_raw(s));
}

pub struct KywrightSession {
    inner: StreamFilter,
}

#[no_mangle]
pub unsafe extern "C" fn kywright_session_new() -> *mut KywrightSession {
    let session = Box::new(KywrightSession {
        inner: StreamFilter::new(),
    });
    Box::into_raw(session)
}

#[no_mangle]
pub unsafe extern "C" fn kywright_session_feed(
    handle: *mut KywrightSession,
    chunk: *const c_char,
) -> i32 {
    if handle.is_null() || chunk.is_null() {
        return -1;
    }
    let session = &mut *handle;
    let c_str = CStr::from_ptr(chunk);
    let chunk_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        session.inner.feed(chunk_str);
    }));

    match result {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn kywright_session_take_output(
    handle: *mut KywrightSession,
) -> *mut c_char {
    if handle.is_null() {
        return ptr::null_mut();
    }
    let session = &mut *handle;
    let result = catch_unwind(AssertUnwindSafe(|| session.inner.take_output()));
    match result {
        Ok(s) => CString::new(s).map(|c| c.into_raw()).unwrap_or(ptr::null_mut()),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn kywright_session_finish(
    handle: *mut KywrightSession,
) -> *mut c_char {
    if handle.is_null() {
        return ptr::null_mut();
    }
    let session = &mut *handle;
    let result = catch_unwind(AssertUnwindSafe(|| {
        session.inner.finish();
        session.inner.take_output()
    }));
    match result {
        Ok(s) => CString::new(s).map(|c| c.into_raw()).unwrap_or(ptr::null_mut()),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn kywright_session_free(handle: *mut KywrightSession) {
    if handle.is_null() {
        return;
    }
    drop(Box::from_raw(handle));
}

#[cfg(test)]
mod ffi_tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn one_shot_roundtrip() {
        unsafe {
            let input = CString::new("<p>hi</p><script>bad()</script>").unwrap();
            let out_ptr = filter_html(input.as_ptr());
            assert!(!out_ptr.is_null());
            let out = CStr::from_ptr(out_ptr).to_str().unwrap().to_string();
            assert!(out.contains("hi"));
            assert!(!out.contains("bad()"));
            kywright_free_string(out_ptr);
        }
    }

    #[test]
    fn null_input_returns_null() {
        unsafe {
            let out = filter_html(ptr::null());
            assert!(out.is_null());
        }
    }

    #[test]
    fn session_streaming_roundtrip() {
        unsafe {
            let handle = kywright_session_new();
            assert!(!handle.is_null());

            let c1 = CString::new("<p>hello wor").unwrap();
            assert_eq!(kywright_session_feed(handle, c1.as_ptr()), 0);

            let c2 = CString::new("ld</p><scr").unwrap();
            assert_eq!(kywright_session_feed(handle, c2.as_ptr()), 0);

            let c3 = CString::new("ipt>evil()</script>").unwrap();
            assert_eq!(kywright_session_feed(handle, c3.as_ptr()), 0);

            let final_ptr = kywright_session_finish(handle);
            assert!(!final_ptr.is_null());
            let final_str = CStr::from_ptr(final_ptr).to_str().unwrap().to_string();
            assert!(final_str.contains("hello world"));
            assert!(!final_str.contains("evil()"));

            kywright_free_string(final_ptr);
            kywright_session_free(handle);
        }
    }

    #[test]
    fn double_free_guard_on_null_is_safe() {
        unsafe {
            kywright_free_string(ptr::null_mut());
            kywright_session_free(ptr::null_mut());
        }
    }
}
