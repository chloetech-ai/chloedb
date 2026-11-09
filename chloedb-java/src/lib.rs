//! Java bindings for Strata, the database in-process: the native library
//! (`libstrata_jni`) under the `ai.chloetech.strata` package
//! (`src/main/java`), loaded by its class `Native`.
//!
//! The boundary is two functions. `call(handle, method, args)` runs one of
//! the methods of [`calls`] with its arguments as one value and returns its
//! answer as another, both in the binary form of [`codec`]; `free(handle)`
//! frees a database's handle once Java no longer holds it. A call does not
//! hold the JVM: Java threads may query one database at once.

mod calls;
mod codec;

use std::panic::{catch_unwind, AssertUnwindSafe};

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jbyteArray, jlong};
use jni::JNIEnv;

use calls::{Fail, Handle};

/// What Java throws for a failure.
fn throw(env: &mut JNIEnv<'_>, f: Fail) {
    let (class, message) = match f {
        Fail::Arg(m) => ("java/lang/IllegalArgumentException", m),
        Fail::Closed => ("java/lang/IllegalStateException", "the database is closed".to_string()),
        Fail::Db(e) => ("ai/chloetech/strata/StrataException", e.to_string()),
    };
    // A failure to throw leaves the exception JNI raised pending, which
    // Java then sees.
    let _ = env.throw_new(class, message);
}

/// `byte[] Native.call(long handle, String method, byte[] args)`: the
/// handle is 0 for the calls that open a database, or need none.
#[no_mangle]
pub extern "system" fn Java_ai_chloetech_strata_Native_call<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong, method: JString<'l>, args: JByteArray<'l>) -> jbyteArray {
    let out = catch_unwind(AssertUnwindSafe(|| -> Result<Vec<u8>, Fail> {
        let method: String = env.get_string(&method).map_err(|e| Fail::Arg(format!("the method's name: {e}")))?.into();
        let args = env.convert_byte_array(&args).map_err(|e| Fail::Arg(format!("the arguments: {e}")))?;
        let args = codec::decode(&args).map_err(|e| Fail::Arg(format!("{method}: the arguments: {e}")))?;
        // SAFETY: a handle Java holds is one `calls` gave it (`Box::into_raw`
        // of a `Handle`), which only `free` frees, when the Java object
        // holding it is unreachable and so in no call.
        let h = (handle != 0).then(|| unsafe { &*(handle as *const Handle) });
        calls::call(h, &method, args)
    }));
    let failed = match out {
        Ok(Ok(bytes)) => match env.byte_array_from_slice(&bytes) {
            Ok(a) => return a.into_raw(),
            Err(e) => Fail::Db(chloedb::Error::Query(format!("the answer does not fit a byte[]: {e}"))),
        },
        Ok(Err(f)) => f,
        Err(panic) => {
            let why = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            Fail::Db(chloedb::Error::Query(format!("ChloeDB's native code panicked: {why}")))
        }
    };
    throw(&mut env, failed);
    std::ptr::null_mut()
}

/// `void Native.free(long handle)`: drops a database's handle, closing the
/// database if `close` did not.
#[no_mangle]
pub extern "system" fn Java_ai_chloetech_strata_Native_free<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, handle: jlong) {
    if handle != 0 {
        // SAFETY: as in `call`; Java frees each handle once.
        let _ = catch_unwind(|| drop(unsafe { Box::from_raw(handle as *mut Handle) }));
    }
}
