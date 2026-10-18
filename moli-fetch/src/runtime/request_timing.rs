use std::{
    ffi::{c_char, c_int, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
};

use super::*;

// curl-sys does not expose these options yet. Both values are part of curl's
// public curl.h ABI, available since libcurl 7.80.0.
const CURLOPT_PREREQFUNCTION: curl_sys::CURLoption = curl_sys::CURLOPTTYPE_FUNCTIONPOINT + 312;
const CURLOPT_PREREQDATA: curl_sys::CURLoption = curl_sys::CURLOPTTYPE_OBJECTPOINT + 313;

pub(super) fn configure(easy: &mut Easy2<FetchTransferHandler>) -> Result<()> {
    let callback: extern "C" fn(*mut c_void, *mut c_char, *mut c_char, c_int, c_int) -> c_int =
        request_started;
    let handler = std::ptr::from_mut(easy.get_mut()).cast::<c_void>();
    // SAFETY: curl-rust keeps the handler in Easy2's boxed Inner allocation.
    // Moving Easy2 does not move the handler, and curl_easy_cleanup runs before
    // that allocation is freed. libcurl calls this synchronously on the same
    // thread as the other Handler callbacks, with no overlapping handler access.
    // Every attempt reconfigures these options after Easy2::reset; its collector
    // is bound to that attempt's ResponseProgress before the easy is submitted.
    let code = unsafe { curl_sys::curl_easy_setopt(easy.raw(), CURLOPT_PREREQDATA, handler) };
    if code != curl_sys::CURLE_OK {
        return Err(curl::Error::new(code)).context("failed to configure curl pre-request data");
    }
    // SAFETY: callback has the curl_prereq_callback signature from curl.h.
    let code = unsafe { curl_sys::curl_easy_setopt(easy.raw(), CURLOPT_PREREQFUNCTION, callback) };
    if code != curl_sys::CURLE_OK {
        return Err(curl::Error::new(code))
            .context("failed to configure curl pre-request callback");
    }
    Ok(())
}

extern "C" fn request_started(
    handler: *mut c_void,
    _remote_ip: *mut c_char,
    _local_ip: *mut c_char,
    _remote_port: c_int,
    _local_port: c_int,
) -> c_int {
    // Do not unwind across the C ABI. Abort this transfer if the callback fails.
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: configure installs a live FetchTransferHandler as PREREQDATA;
        // libcurl invokes callbacks serially while its Easy2 remains alive.
        let handler = unsafe { &*handler.cast::<FetchTransferHandler>() };
        if let Some(timing) = handler.response_progress() {
            timing.mark_request_started();
        }
        0 // CURL_PREREQFUNC_OK
    }))
    .unwrap_or(1) // CURL_PREREQFUNC_ABORT
}
