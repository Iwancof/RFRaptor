use std::{ffi::CStr, ptr::NonNull};

use liquid_dsp_sys::liquid_error_info;

pub(crate) fn liquid_get_pointer<Ret, F: FnOnce() -> *mut Ret>(
    f: F,
) -> anyhow::Result<NonNull<Ret>> {
    let ret = f();

    if let Some(ptr) = NonNull::new(ret) {
        return Ok(ptr);
    }

    let reason = unsafe { CStr::from_ptr(liquid_error_info(0)) }
        .to_str()
        .expect("Could not get error info");

    anyhow::bail!("[{}] at [{}]", 0, reason);
}

pub(crate) fn liquid_do_int<F: FnOnce() -> i32>(f: F) -> anyhow::Result<()> {
    let ret = f() as _; // not capturing stderr due to performance reason

    if ret == liquid_dsp_sys::liquid_error_code_LIQUID_OK {
        return Ok(());
    }

    let reason = unsafe { CStr::from_ptr(liquid_error_info(ret)) }
        .to_str()
        .expect("Could not get error info");

    anyhow::bail!("[{}] at [{}]", ret, reason);
}
