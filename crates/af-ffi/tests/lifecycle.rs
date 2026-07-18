use std::mem::{MaybeUninit, align_of, offset_of, size_of};
use std::ptr;
use std::thread;

use af_ffi::{
    AF_STATUS_INVALID_ARGUMENT, AF_STATUS_INVALID_HANDLE, AF_STATUS_OK, AF_STATUS_WRONG_THREAD,
    AfSessionHandle, AfStatus, AfVersion, af_abi_version, af_session_create, af_session_destroy,
};

#[test]
fn public_types_have_the_declared_c_layout() {
    assert_eq!(size_of::<AfStatus>(), size_of::<u32>());
    assert_eq!(size_of::<AfSessionHandle>(), size_of::<usize>());
    assert_eq!(size_of::<AfVersion>(), 6);
    assert_eq!(align_of::<AfVersion>(), 2);
    assert_eq!(offset_of!(AfVersion, major), 0);
    assert_eq!(offset_of!(AfVersion, minor), 2);
    assert_eq!(offset_of!(AfVersion, patch), 4);
}

#[test]
fn abi_version_is_exact_and_rejects_null() {
    let mut version = MaybeUninit::<AfVersion>::uninit();

    // SAFETY: MaybeUninit provides aligned writable storage for one AfVersion.
    assert_eq!(
        unsafe { af_abi_version(version.as_mut_ptr()) },
        AF_STATUS_OK
    );
    // SAFETY: The successful call above initialized the value.
    let version = unsafe { version.assume_init() };
    assert_eq!((version.major, version.minor, version.patch), (0, 7, 0));

    // SAFETY: Null is explicitly accepted as a checked error case.
    assert_eq!(
        unsafe { af_abi_version(ptr::null_mut()) },
        AF_STATUS_INVALID_ARGUMENT
    );
}

#[test]
fn session_lifecycle_rejects_null_zero_and_double_destroy() {
    // SAFETY: Null is explicitly accepted as a checked error case.
    assert_eq!(
        unsafe { af_session_create(ptr::null_mut()) },
        AF_STATUS_INVALID_ARGUMENT
    );

    let mut handle = 0;
    // SAFETY: `handle` is aligned writable storage for one AfSessionHandle.
    assert_eq!(unsafe { af_session_create(&mut handle) }, AF_STATUS_OK);
    assert_ne!(handle, 0);

    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
    assert_eq!(af_session_destroy(handle), AF_STATUS_INVALID_HANDLE);
    assert_eq!(af_session_destroy(0), AF_STATUS_INVALID_HANDLE);
}

#[test]
fn wrong_thread_does_not_destroy_the_session() {
    let mut handle = 0;
    // SAFETY: `handle` is aligned writable storage for one AfSessionHandle.
    assert_eq!(unsafe { af_session_create(&mut handle) }, AF_STATUS_OK);

    let other_status = thread::spawn(move || af_session_destroy(handle))
        .join()
        .expect("destroy thread should not panic");
    assert_eq!(other_status, AF_STATUS_WRONG_THREAD);
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}

#[test]
fn thread_exit_drops_sessions_and_owner_metadata() {
    let handle = thread::spawn(|| {
        let mut handle = 0;
        // SAFETY: `handle` is aligned writable storage for one AfSessionHandle.
        assert_eq!(unsafe { af_session_create(&mut handle) }, AF_STATUS_OK);
        handle
    })
    .join()
    .expect("owner thread should not panic");

    assert_eq!(af_session_destroy(handle), AF_STATUS_INVALID_HANDLE);
}

#[test]
fn header_tracks_the_public_contract() {
    let header = include_str!("../include/arccad.h");

    for required in [
        "typedef uint32_t AfStatus;",
        "typedef uintptr_t AfSessionHandle;",
        "#include <stddef.h>",
        "typedef struct AfUtf8Buffer",
        "typedef struct AfByteBuffer",
        "typedef struct AfF32Buffer",
        "const uint8_t *data;",
        "const float *data;",
        "size_t len;",
        "size_t capacity;",
        "uintptr_t owner;",
        "AF_STATUS_OK UINT32_C(0)",
        "AF_STATUS_INVALID_ARGUMENT UINT32_C(1)",
        "AF_STATUS_INVALID_HANDLE UINT32_C(2)",
        "AF_STATUS_WRONG_THREAD UINT32_C(3)",
        "AF_STATUS_INTERNAL UINT32_C(4)",
        "AF_STATUS_INVALID_UTF8 UINT32_C(5)",
        "AF_STATUS_PANIC UINT32_C(255)",
        "AF_ABI_VERSION_MAJOR UINT16_C(0)",
        "AF_ABI_VERSION_MINOR UINT16_C(7)",
        "AF_ABI_VERSION_PATCH UINT16_C(0)",
        "static inline int af_abi_version_matches(AfVersion version)",
        "static inline const char *af_status_message(AfStatus status)",
        "return \"ok\";",
        "return \"invalid argument\";",
        "return \"invalid handle\";",
        "return \"wrong thread\";",
        "return \"internal error\";",
        "return \"invalid UTF-8\";",
        "return \"panic\";",
        "return \"unknown status\";",
        "AfStatus af_abi_version(AfVersion *out_version);",
        "AfStatus af_session_create(AfSessionHandle *out_handle);",
        "AfStatus af_session_destroy(AfSessionHandle handle);",
        "AfStatus af_session_save_arcf(",
        "AfStatus af_session_open_arcf_json(",
        "AfStatus af_session_execute_json(",
        "AfStatus af_session_parse_input_json(",
        "AfStatus af_session_snap_json(",
        "AfStatus af_session_select_at_json(",
        "AfStatus af_session_render_delta(",
        "AfStatus af_session_render_full_json(",
        "AfStatus af_session_layers_json(",
        "AfStatus af_session_render_vertices(",
        "AfStatus af_utf8_buffer_free(AfUtf8Buffer *buffer);",
        "AfStatus af_byte_buffer_free(AfByteBuffer *buffer);",
        "AfStatus af_f32_buffer_free(AfF32Buffer *buffer);",
        "Passing NULL is idempotent and returns OK",
        "violating the remaining pointer preconditions is undefined behavior",
    ] {
        assert!(header.contains(required), "header is missing: {required}");
    }
    assert_eq!(header.matches("AfStatus af_").count(), 16);
}
