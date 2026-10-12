//! The C ABI of the Titania core. The contract is written out in `include/titania_core.h`.
//!
//! One entry point, `tt_run`, takes a JSON request and returns a JSON result. Progress, log
//! lines and cancellation go through a callback. Panics never cross the boundary.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::slice;
use titania_pgn::{Error, Sink, run_json};

/// Bumped whenever the functions or their meaning change incompatibly.
pub const TT_ABI_VERSION: u32 = 1;

pub const TT_OK: i32 = 0;
pub const TT_CANCELLED: i32 = 1;
pub const TT_ERROR: i32 = 2;
pub const TT_BAD_REQUEST: i32 = 3;
pub const TT_PANIC: i32 = 4;

pub const TT_EVENT_PROGRESS: i32 = 0;
pub const TT_EVENT_PHASE: i32 = 1;
pub const TT_EVENT_LOG: i32 = 2;

/// Called from the thread that called `tt_run`. `text` is UTF-8, not NUL-terminated, and valid
/// only during the call. Returning non-zero asks the operation to stop.
pub type TtCallback =
    unsafe extern "C" fn(context: *mut c_void, event: i32, done: u64, total: u64, games: u64, text: *const u8, text_len: usize) -> i32;

struct CallbackSink {
    callback: Option<TtCallback>,
    context: *mut c_void,
    cancelled: bool,
}

impl CallbackSink {
    fn send(&mut self, event: i32, done: u64, total: u64, games: u64, text: &str) {
        if let Some(callback) = self.callback {
            // SAFETY: the caller of tt_run guarantees the callback and its context are valid for the call.
            if unsafe { callback(self.context, event, done, total, games, text.as_ptr(), text.len()) } != 0 {
                self.cancelled = true;
            }
        }
    }
}

impl Sink for CallbackSink {
    fn progress(&mut self, done: u64, total: u64, games: u64) -> bool {
        self.send(TT_EVENT_PROGRESS, done, total, games, "");
        !self.cancelled
    }
    fn phase(&mut self, text: &str) -> bool {
        self.send(TT_EVENT_PHASE, 0, 0, 0, text);
        !self.cancelled
    }
    fn log(&mut self, text: &str) -> bool {
        self.send(TT_EVENT_LOG, 0, 0, 0, text);
        !self.cancelled
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tt_abi_version() -> u32 {
    TT_ABI_VERSION
}

/// The version of the core as a NUL-terminated string that lives as long as the library.
#[unsafe(no_mangle)]
pub extern "C" fn tt_version() -> *const u8 {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr()
}

/// Runs the JSON request in `request[..request_len]` and stores a JSON document in `*output`,
/// to be released with `tt_free`. On `TT_OK` it is the result; otherwise it is `{"error": "..."}`.
///
/// # Safety
/// `request` must point to `request_len` readable bytes, `output` and `output_len` must be valid
/// for writes, and `callback`, if not null, must be callable with `context` until this returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_run(
    request: *const u8,
    request_len: usize,
    callback: Option<TtCallback>,
    context: *mut c_void,
    output: *mut *mut u8,
    output_len: *mut usize,
) -> i32 {
    if output.is_null() || output_len.is_null() {
        return TT_BAD_REQUEST;
    }
    let (status, json) = if request.is_null() {
        (TT_BAD_REQUEST, error_json("Bad request: no request was given."))
    } else {
        // SAFETY: guaranteed by the caller.
        let request = unsafe { slice::from_raw_parts(request, request_len) };
        let mut sink = CallbackSink { callback, context, cancelled: false };
        match catch_unwind(AssertUnwindSafe(|| run_json(request, &mut sink))) {
            Ok(Ok(Ok(json))) => (TT_OK, json),
            Ok(Ok(Err(Error::Cancelled))) => (TT_CANCELLED, error_json("Cancelled")),
            Ok(Ok(Err(error))) => (TT_ERROR, error_json(&error.to_string())),
            Ok(Err(bad_request)) => (TT_BAD_REQUEST, error_json(&bad_request)),
            Err(panic) => {
                let detail = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned());
                (TT_PANIC, error_json(&format!("Internal error: {}", detail.as_deref().unwrap_or("unknown"))))
            }
        }
    };
    let json = json.into_boxed_slice();
    // SAFETY: both pointers were checked for null and are valid for writes per the contract.
    unsafe {
        *output_len = json.len();
        *output = Box::into_raw(json).cast::<u8>();
    }
    status
}

/// Releases a buffer returned by `tt_run`.
///
/// # Safety
/// `data` and `len` must be exactly what `tt_run` stored, and must not be released twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_free(data: *mut u8, len: usize) {
    if !data.is_null() {
        // SAFETY: the pair came from Box::into_raw in tt_run.
        drop(unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(data, len)) });
    }
}

fn error_json(message: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "error": message })).unwrap_or_else(|_| br#"{"error":"Unknown error"}"#.to_vec())
}
