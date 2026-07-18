#![deny(unsafe_op_in_unsafe_fn)]
//! Small, versioned C ABI for ArcCAD.
//!
//! Sessions remain thread-affine. Command results cross the boundary as owned
//! UTF-8 buffers whose allocation stays in Rust until the matching free call.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::slice;
use std::str;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::thread::{self, ThreadId};

use af_api::{ApiError, ApiSession, ParsedPoint};
use af_model::units::Units;

/// Fixed-width status returned by every ABI entrypoint.
pub type AfStatus = u32;

/// Process-local opaque session ID. Zero is always invalid.
pub type AfSessionHandle = usize;

/// Semantic version of the C ABI, independent from the scripting API version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct AfVersion {
    /// Breaking ABI generation.
    pub major: u16,
    /// Backwards-compatible feature generation.
    pub minor: u16,
    /// Backwards-compatible fixes.
    pub patch: u16,
}

/// Read-only UTF-8 allocation owned by the Rust buffer registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct AfUtf8Buffer {
    /// First result byte; not NUL-terminated.
    pub data: *const u8,
    /// Number of initialized UTF-8 bytes.
    pub len: usize,
    /// Rust allocation capacity, used to validate ownership metadata.
    pub capacity: usize,
    /// Opaque, process-local allocation token. Zero means empty.
    pub owner: usize,
}

/// Read-only byte allocation owned by the Rust buffer registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct AfByteBuffer {
    /// First result byte.
    pub data: *const u8,
    /// Number of initialized bytes.
    pub len: usize,
    /// Rust allocation capacity, used to validate ownership metadata.
    pub capacity: usize,
    /// Opaque, process-local allocation token. Zero means empty.
    pub owner: usize,
}

/// Read-only IEEE-754 binary32 allocation owned by the Rust buffer registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct AfF32Buffer {
    /// First float element; coordinates are interleaved `x, y`.
    pub data: *const f32,
    /// Number of initialized float elements, not bytes.
    pub len: usize,
    /// Rust allocation capacity in float elements.
    pub capacity: usize,
    /// Opaque, process-local allocation token. Zero means empty.
    pub owner: usize,
}

const EMPTY_BUFFER: AfUtf8Buffer = AfUtf8Buffer {
    data: ptr::null(),
    len: 0,
    capacity: 0,
    owner: 0,
};

const EMPTY_BYTE_BUFFER: AfByteBuffer = AfByteBuffer {
    data: ptr::null(),
    len: 0,
    capacity: 0,
    owner: 0,
};

const EMPTY_F32_BUFFER: AfF32Buffer = AfF32Buffer {
    data: ptr::null(),
    len: 0,
    capacity: 0,
    owner: 0,
};

/// Operation completed.
pub const AF_STATUS_OK: AfStatus = 0;
/// A required pointer was null or checked metadata was invalid.
pub const AF_STATUS_INVALID_ARGUMENT: AfStatus = 1;
/// A session handle or nonzero buffer owner was unknown or already released.
pub const AF_STATUS_INVALID_HANDLE: AfStatus = 2;
/// The session belongs to another thread.
pub const AF_STATUS_WRONG_THREAD: AfStatus = 3;
/// An internal invariant or finite resource prevented the operation.
pub const AF_STATUS_INTERNAL: AfStatus = 4;
/// An input byte range was not valid UTF-8.
pub const AF_STATUS_INVALID_UTF8: AfStatus = 5;
/// An unwinding Rust panic was caught at the ABI boundary.
pub const AF_STATUS_PANIC: AfStatus = 255;

const ABI_VERSION: AfVersion = AfVersion {
    major: 0,
    minor: 7,
    patch: 0,
};

enum OwnedBuffer {
    Utf8(Vec<u8>),
    Bytes(Vec<u8>),
    F32(Vec<f32>),
}

static NEXT_HANDLE: AtomicUsize = AtomicUsize::new(1);
static NEXT_BUFFER_OWNER: AtomicUsize = AtomicUsize::new(1);
static OWNERS: LazyLock<Mutex<HashMap<AfSessionHandle, ThreadId>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static BUFFERS: LazyLock<Mutex<HashMap<usize, OwnedBuffer>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct ThreadSessions {
    owner: ThreadId,
    sessions: HashMap<AfSessionHandle, ApiSession>,
}

impl ThreadSessions {
    fn new() -> Self {
        Self {
            owner: thread::current().id(),
            sessions: HashMap::new(),
        }
    }
}

impl Drop for ThreadSessions {
    fn drop(&mut self) {
        let mut owners = lock_owners();
        for handle in self.sessions.keys() {
            if owners.get(handle) == Some(&self.owner) {
                owners.remove(handle);
            }
        }
    }
}

thread_local! {
    static SESSIONS: RefCell<ThreadSessions> = RefCell::new(ThreadSessions::new());
}

fn lock_owners() -> MutexGuard<'static, HashMap<AfSessionHandle, ThreadId>> {
    OWNERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_buffers() -> MutexGuard<'static, HashMap<usize, OwnedBuffer>> {
    BUFFERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn allocate_handle(counter: &AtomicUsize) -> Result<usize, AfStatus> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            if current == 0 {
                None
            } else {
                current.checked_add(1)
            }
        })
        .map_err(|_| AF_STATUS_INTERNAL)
}

fn with_reserved_buffer_owner<T>(
    counter: &AtomicUsize,
    operation: impl FnOnce(usize) -> Result<T, AfStatus>,
) -> Result<T, AfStatus> {
    let owner = allocate_handle(counter)?;
    operation(owner)
}

fn with_reserved_buffer_owners<T>(
    counter: &AtomicUsize,
    operation: impl FnOnce(usize, usize) -> T,
) -> Result<T, AfStatus> {
    let first = allocate_handle(counter)?;
    let second = allocate_handle(counter)?;
    Ok(operation(first, second))
}

fn guard(operation: impl FnOnce() -> AfStatus) -> AfStatus {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(status) => status,
        Err(payload) => {
            // A custom panic payload may itself panic in Drop. Do not let that
            // second panic escape the C boundary.
            mem::forget(payload);
            AF_STATUS_PANIC
        }
    }
}

fn invalidate_session(handle: AfSessionHandle) {
    // Removing the public owner first makes the handle terminal even if
    // dropping the thread-local session were ever to fail.
    lock_owners().remove(&handle);
    let _ = SESSIONS.try_with(|sessions| {
        if let Ok(mut sessions) = sessions.try_borrow_mut() {
            drop(sessions.sessions.remove(&handle));
        }
    });
}

fn guard_terminal(operation: impl FnOnce() -> AfStatus, invalidate: impl FnOnce()) -> AfStatus {
    let status = guard(operation);
    if status == AF_STATUS_PANIC {
        // Invalidation may drop session-owned values. Contain a second panic
        // while preserving the original terminal PANIC result.
        let _ = guard(|| {
            invalidate();
            AF_STATUS_OK
        });
    }
    status
}

fn guard_session(handle: AfSessionHandle, operation: impl FnOnce() -> AfStatus) -> AfStatus {
    guard_terminal(operation, || invalidate_session(handle))
}

fn create_session() -> Result<AfSessionHandle, AfStatus> {
    let handle = allocate_handle(&NEXT_HANDLE)?;
    let session = ApiSession::new(Units::default());
    let owner = thread::current().id();

    SESSIONS.with_borrow_mut(|sessions| {
        sessions.sessions.insert(handle, session);
    });
    lock_owners().insert(handle, owner);
    Ok(handle)
}

fn session_access(handle: AfSessionHandle) -> Result<(), AfStatus> {
    if handle == 0 {
        return Err(AF_STATUS_INVALID_HANDLE);
    }

    match lock_owners().get(&handle) {
        None => Err(AF_STATUS_INVALID_HANDLE),
        Some(owner) if *owner != thread::current().id() => Err(AF_STATUS_WRONG_THREAD),
        Some(_) => Ok(()),
    }
}

fn destroy_session(handle: AfSessionHandle) -> AfStatus {
    if let Err(status) = session_access(handle) {
        return status;
    }

    let mut owners = lock_owners();
    let removed = SESSIONS.with_borrow_mut(|sessions| sessions.sessions.remove(&handle));
    owners.remove(&handle);
    drop(owners);

    if removed.is_some() {
        AF_STATUS_OK
    } else {
        AF_STATUS_INTERNAL
    }
}

fn execute_session(
    handle: AfSessionHandle,
    command: &str,
    args_json: &str,
) -> Result<String, AfStatus> {
    SESSIONS.with_borrow_mut(|sessions| {
        sessions
            .sessions
            .get_mut(&handle)
            .map(|session| session.execute_json(command, args_json))
            .ok_or(AF_STATUS_INTERNAL)
    })
}

fn parse_result_json(result: Result<ParsedPoint, ApiError>) -> String {
    let mut envelope = serde_json::Map::new();
    match result {
        Ok(point) => envelope.insert(
            "ok".to_string(),
            serde_json::to_value(point).expect("parsed point must serialize"),
        ),
        Err(error) => envelope.insert(
            "error".to_string(),
            serde_json::to_value(error).expect("API error must serialize"),
        ),
    };
    serde_json::to_string(&serde_json::Value::Object(envelope))
        .expect("point parse envelope must serialize")
}

fn parse_input_session(
    handle: AfSessionHandle,
    input: &str,
    base: Option<[f64; 2]>,
) -> Result<String, AfStatus> {
    SESSIONS.with_borrow(|sessions| {
        sessions
            .sessions
            .get(&handle)
            .map(|session| parse_result_json(session.parse_input(input, base)))
            .ok_or(AF_STATUS_INTERNAL)
    })
}

fn snap_session(handle: AfSessionHandle, x: f64, y: f64, radius: f64) -> Result<String, AfStatus> {
    SESSIONS.with_borrow(|sessions| {
        let session = sessions.sessions.get(&handle).ok_or(AF_STATUS_INTERNAL)?;
        serde_json::to_string(&session.snap(x, y, radius, &serde_json::Value::Null))
            .map_err(|_| AF_STATUS_INTERNAL)
    })
}

fn select_at_json(session: &mut ApiSession, x: f64, y: f64, tolerance: f64) -> String {
    match session.pick(x, y, tolerance).first().map(|hit| hit.id) {
        Some(id) => session.set_selection(&[id]),
        None => session.clear_selection(),
    }
    serde_json::to_string(&session.selection()).expect("selection IDs must serialize")
}

fn select_at_session(handle: AfSessionHandle, x: f64, y: f64, tolerance: f64) -> String {
    SESSIONS.with_borrow_mut(|sessions| {
        let session = sessions
            .sessions
            .get_mut(&handle)
            .expect("owner registry and thread session must agree");
        select_at_json(session, x, y, tolerance)
    })
}

fn render_delta_session(handle: AfSessionHandle) -> (String, Vec<f32>) {
    SESSIONS.with_borrow_mut(|sessions| {
        let session = sessions
            .sessions
            .get_mut(&handle)
            .expect("owner registry and thread session must agree");
        let mut delta = session.render_delta();
        let vertices = mem::take(&mut delta.vertices);
        assert!(
            vertices.len().is_multiple_of(2),
            "render delta vertices must contain x,y pairs"
        );
        let control = serde_json::to_string(&delta).expect("render delta control must serialize");
        (control, vertices)
    })
}

fn render_full_session(handle: AfSessionHandle) -> Result<String, AfStatus> {
    SESSIONS.with_borrow_mut(|sessions| {
        let session = sessions
            .sessions
            .get_mut(&handle)
            .ok_or(AF_STATUS_INTERNAL)?;
        serde_json::to_string(&session.render_full()).map_err(|_| AF_STATUS_INTERNAL)
    })
}

fn layers_session(handle: AfSessionHandle) -> Result<String, AfStatus> {
    SESSIONS.with_borrow(|sessions| {
        let session = sessions.sessions.get(&handle).ok_or(AF_STATUS_INTERNAL)?;
        serde_json::to_string(&session.layers()).map_err(|_| AF_STATUS_INTERNAL)
    })
}

fn render_vertices_session(handle: AfSessionHandle) -> Result<Vec<f32>, AfStatus> {
    SESSIONS.with_borrow(|sessions| {
        sessions
            .sessions
            .get(&handle)
            .map(ApiSession::render_vertices)
            .ok_or(AF_STATUS_INTERNAL)
    })
}

fn save_session(handle: AfSessionHandle) -> Result<Vec<u8>, AfStatus> {
    SESSIONS.with_borrow(|sessions| {
        sessions
            .sessions
            .get(&handle)
            .expect("owner registry and thread session must agree")
            .save()
            .map_err(|_| AF_STATUS_INTERNAL)
    })
}

fn open_result_json(result: Result<Vec<String>, ApiError>) -> String {
    let mut envelope = serde_json::Map::new();
    match result {
        Ok(warnings) => envelope.insert(
            "ok".to_string(),
            serde_json::to_value(warnings).expect("open warnings must serialize"),
        ),
        Err(error) => envelope.insert(
            "error".to_string(),
            serde_json::to_value(error).expect("API error must serialize"),
        ),
    };
    serde_json::to_string(&serde_json::Value::Object(envelope))
        .expect("open envelope must serialize")
}

fn open_session(handle: AfSessionHandle, bytes: &[u8]) -> String {
    SESSIONS.with_borrow_mut(|sessions| {
        let session = sessions
            .sessions
            .get_mut(&handle)
            .expect("owner registry and thread session must agree");
        open_result_json(session.open(bytes))
    })
}

fn register_utf8_buffer(owner: usize, value: String) -> Result<AfUtf8Buffer, AfStatus> {
    let bytes = value.into_bytes();
    if bytes.is_empty() {
        return Err(AF_STATUS_INTERNAL);
    }

    let buffer = AfUtf8Buffer {
        data: bytes.as_ptr(),
        len: bytes.len(),
        capacity: bytes.capacity(),
        owner,
    };
    match lock_buffers().entry(owner) {
        Entry::Vacant(entry) => {
            entry.insert(OwnedBuffer::Utf8(bytes));
            Ok(buffer)
        }
        Entry::Occupied(_) => Err(AF_STATUS_INTERNAL),
    }
}

fn register_byte_buffer(owner: usize, bytes: Vec<u8>) -> Result<AfByteBuffer, AfStatus> {
    if bytes.is_empty() {
        return Err(AF_STATUS_INTERNAL);
    }

    let buffer = AfByteBuffer {
        data: bytes.as_ptr(),
        len: bytes.len(),
        capacity: bytes.capacity(),
        owner,
    };
    match lock_buffers().entry(owner) {
        Entry::Vacant(entry) => {
            entry.insert(OwnedBuffer::Bytes(bytes));
            Ok(buffer)
        }
        Entry::Occupied(_) => Err(AF_STATUS_INTERNAL),
    }
}

fn free_utf8_buffer(buffer: &mut AfUtf8Buffer) -> AfStatus {
    if buffer.owner == 0 {
        return if *buffer == EMPTY_BUFFER {
            AF_STATUS_OK
        } else {
            AF_STATUS_INVALID_ARGUMENT
        };
    }

    let mut buffers = lock_buffers();
    let bytes = match buffers.get(&buffer.owner) {
        Some(OwnedBuffer::Utf8(bytes)) => bytes,
        Some(OwnedBuffer::Bytes(_) | OwnedBuffer::F32(_)) => {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        None => return AF_STATUS_INVALID_HANDLE,
    };
    if !ptr::eq(buffer.data, bytes.as_ptr())
        || buffer.len != bytes.len()
        || buffer.capacity != bytes.capacity()
    {
        return AF_STATUS_INVALID_ARGUMENT;
    }

    let allocation = buffers
        .remove(&buffer.owner)
        .expect("buffer checked present while registry lock is held");
    let OwnedBuffer::Utf8(bytes) = allocation else {
        unreachable!("buffer type checked while registry lock is held");
    };
    *buffer = EMPTY_BUFFER;
    drop(buffers);
    drop(bytes);
    AF_STATUS_OK
}

fn free_byte_buffer(buffer: &mut AfByteBuffer) -> AfStatus {
    if buffer.owner == 0 {
        return if *buffer == EMPTY_BYTE_BUFFER {
            AF_STATUS_OK
        } else {
            AF_STATUS_INVALID_ARGUMENT
        };
    }

    let mut buffers = lock_buffers();
    let bytes = match buffers.get(&buffer.owner) {
        Some(OwnedBuffer::Bytes(bytes)) => bytes,
        Some(OwnedBuffer::Utf8(_) | OwnedBuffer::F32(_)) => {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        None => return AF_STATUS_INVALID_HANDLE,
    };
    if !ptr::eq(buffer.data, bytes.as_ptr())
        || buffer.len != bytes.len()
        || buffer.capacity != bytes.capacity()
    {
        return AF_STATUS_INVALID_ARGUMENT;
    }

    let allocation = buffers
        .remove(&buffer.owner)
        .expect("buffer checked present while registry lock is held");
    let OwnedBuffer::Bytes(bytes) = allocation else {
        unreachable!("buffer type checked while registry lock is held");
    };
    *buffer = EMPTY_BYTE_BUFFER;
    drop(buffers);
    drop(bytes);
    AF_STATUS_OK
}

fn register_f32_buffer(owner: usize, values: Vec<f32>) -> Result<AfF32Buffer, AfStatus> {
    if values.is_empty() {
        return Ok(EMPTY_F32_BUFFER);
    }
    if !values.len().is_multiple_of(2) {
        return Err(AF_STATUS_INTERNAL);
    }

    let buffer = AfF32Buffer {
        data: values.as_ptr(),
        len: values.len(),
        capacity: values.capacity(),
        owner,
    };
    match lock_buffers().entry(owner) {
        Entry::Vacant(entry) => {
            entry.insert(OwnedBuffer::F32(values));
            Ok(buffer)
        }
        Entry::Occupied(_) => Err(AF_STATUS_INTERNAL),
    }
}

fn register_render_delta_buffers(
    control_owner: usize,
    control: String,
    vertices_owner: usize,
    vertices: Vec<f32>,
) -> (AfUtf8Buffer, AfF32Buffer) {
    assert_ne!(control_owner, 0, "reserved control owner must be nonzero");
    assert_ne!(vertices_owner, 0, "reserved vertices owner must be nonzero");
    assert_ne!(
        control_owner, vertices_owner,
        "render delta owners must be unique"
    );
    assert!(
        vertices.len().is_multiple_of(2),
        "render delta vertices must contain x,y pairs"
    );

    let control = control.into_bytes();
    assert!(
        !control.is_empty(),
        "render delta control must not be empty"
    );
    let control_buffer = AfUtf8Buffer {
        data: control.as_ptr(),
        len: control.len(),
        capacity: control.capacity(),
        owner: control_owner,
    };
    let vertices_buffer = if vertices.is_empty() {
        EMPTY_F32_BUFFER
    } else {
        AfF32Buffer {
            data: vertices.as_ptr(),
            len: vertices.len(),
            capacity: vertices.capacity(),
            owner: vertices_owner,
        }
    };

    let mut buffers = lock_buffers();
    assert!(
        !buffers.contains_key(&control_owner) && !buffers.contains_key(&vertices_owner),
        "reserved render delta owners must be vacant"
    );
    buffers
        .try_reserve(if vertices.is_empty() { 1 } else { 2 })
        .expect("render delta registry capacity must be available");
    assert!(
        buffers
            .insert(control_owner, OwnedBuffer::Utf8(control))
            .is_none(),
        "control owner was checked vacant"
    );
    if !vertices.is_empty() {
        assert!(
            buffers
                .insert(vertices_owner, OwnedBuffer::F32(vertices))
                .is_none(),
            "vertices owner was checked vacant"
        );
    }
    (control_buffer, vertices_buffer)
}

fn free_f32_buffer(buffer: &mut AfF32Buffer) -> AfStatus {
    if buffer.owner == 0 {
        return if *buffer == EMPTY_F32_BUFFER {
            AF_STATUS_OK
        } else {
            AF_STATUS_INVALID_ARGUMENT
        };
    }

    let mut buffers = lock_buffers();
    let values = match buffers.get(&buffer.owner) {
        Some(OwnedBuffer::F32(values)) => values,
        Some(OwnedBuffer::Utf8(_) | OwnedBuffer::Bytes(_)) => {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        None => return AF_STATUS_INVALID_HANDLE,
    };
    if !ptr::eq(buffer.data, values.as_ptr())
        || buffer.len != values.len()
        || buffer.capacity != values.capacity()
    {
        return AF_STATUS_INVALID_ARGUMENT;
    }

    let allocation = buffers
        .remove(&buffer.owner)
        .expect("buffer checked present while registry lock is held");
    let OwnedBuffer::F32(values) = allocation else {
        unreachable!("buffer type checked while registry lock is held");
    };
    *buffer = EMPTY_F32_BUFFER;
    drop(buffers);
    drop(values);
    AF_STATUS_OK
}

unsafe fn read_bytes<'a>(data: *const u8, len: usize) -> Result<&'a [u8], AfStatus> {
    if len == 0 {
        return Ok(&[]);
    }
    if data.is_null() || len > isize::MAX as usize {
        return Err(AF_STATUS_INVALID_ARGUMENT);
    }

    // SAFETY: The caller contract requires one immutable allocation containing
    // `len <= ISIZE_MAX` readable bytes with no address wrap for the full call.
    Ok(unsafe { slice::from_raw_parts(data, len) })
}

unsafe fn read_utf8<'a>(data: *const u8, len: usize) -> Result<&'a str, AfStatus> {
    // SAFETY: This function has the same pointer/length contract as read_bytes.
    let bytes = unsafe { read_bytes(data, len) }?;
    str::from_utf8(bytes).map_err(|_| AF_STATUS_INVALID_UTF8)
}

/// Writes the current C ABI version.
///
/// # Safety
///
/// `out_version` must be non-null, aligned and valid for writing one
/// [`AfVersion`] with exclusive access for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_abi_version(out_version: *mut AfVersion) -> AfStatus {
    guard(|| {
        if out_version.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }

        // SAFETY: The caller contract requires a non-null, aligned, writable
        // pointer to one AfVersion; null was checked above.
        unsafe { out_version.write(ABI_VERSION) };
        AF_STATUS_OK
    })
}

/// Creates an empty millimetre-based core session owned by the calling thread.
///
/// # Safety
///
/// `out_handle` must be non-null, aligned and valid for writing one
/// [`AfSessionHandle`] with exclusive access for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_create(out_handle: *mut AfSessionHandle) -> AfStatus {
    guard(|| {
        if out_handle.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }

        // SAFETY: The caller contract requires a non-null, aligned, writable
        // pointer to one handle; null was checked above.
        unsafe { out_handle.write(0) };

        match create_session() {
            Ok(handle) => {
                // SAFETY: The same validated output pointer remains valid for
                // the duration of this call.
                unsafe { out_handle.write(handle) };
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

/// Destroys a session on its owner thread.
#[unsafe(no_mangle)]
pub extern "C" fn af_session_destroy(handle: AfSessionHandle) -> AfStatus {
    guard_session(handle, || destroy_session(handle))
}

/// Serializes the native document through [`ApiSession::save`] into owned bytes.
///
/// # Safety
///
/// `out_bytes` must be non-null, aligned and valid for an exclusive write of
/// one [`AfByteBuffer`] for the full call. It must not own a live buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_save_arcf(
    handle: AfSessionHandle,
    out_bytes: *mut AfByteBuffer,
) -> AfStatus {
    guard_session(handle, || {
        if out_bytes.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: The caller provides exclusive writable output storage;
        // null was checked above.
        unsafe { out_bytes.write(EMPTY_BYTE_BUFFER) };

        if let Err(status) = session_access(handle) {
            return status;
        }

        let result = with_reserved_buffer_owner(&NEXT_BUFFER_OWNER, |owner| {
            register_byte_buffer(owner, save_session(handle)?)
        });
        match result {
            Ok(buffer) => {
                // SAFETY: The validated output remains exclusively writable.
                unsafe { out_bytes.write(buffer) };
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

/// Replaces the native document through [`ApiSession::open`] and returns its
/// stable JSON result envelope as owned UTF-8.
///
/// # Safety
///
/// `out_result` must be non-null, aligned, disjoint from the input and valid for
/// an exclusive write of one [`AfUtf8Buffer`] for the full call. It must not own
/// a live buffer. A non-empty input must be one immutable readable allocation
/// of at most `ISIZE_MAX` bytes with no address wrap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_open_arcf_json(
    handle: AfSessionHandle,
    bytes: *const u8,
    len: usize,
    out_result: *mut AfUtf8Buffer,
) -> AfStatus {
    guard_session(handle, || {
        if out_result.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: The caller provides exclusive writable output storage;
        // null was checked above.
        unsafe { out_result.write(EMPTY_BUFFER) };

        if let Err(status) = session_access(handle) {
            return status;
        }
        // SAFETY: The caller guarantees a valid immutable input range disjoint
        // from the output storage.
        let bytes = match unsafe { read_bytes(bytes, len) } {
            Ok(bytes) => bytes,
            Err(status) => return status,
        };

        let result = with_reserved_buffer_owner(&NEXT_BUFFER_OWNER, |owner| {
            register_utf8_buffer(owner, open_session(handle, bytes))
        });
        match result {
            Ok(buffer) => {
                // SAFETY: The validated output remains exclusively writable.
                unsafe { out_result.write(buffer) };
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

/// Executes a command through [`ApiSession::execute_json`] and returns its
/// structured JSON envelope as an owned UTF-8 buffer.
///
/// # Safety
///
/// `out_result` must be non-null, aligned, disjoint from both inputs and valid
/// for an exclusive write of one [`AfUtf8Buffer`] for the full call. It must not
/// currently own a live buffer. Each non-empty input must be one immutable,
/// readable allocation of at most `ISIZE_MAX` bytes with no address wrap. Null
/// input pointers are accepted only when their corresponding length is zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_execute_json(
    handle: AfSessionHandle,
    command_ptr: *const u8,
    command_len: usize,
    args_json_ptr: *const u8,
    args_json_len: usize,
    out_result: *mut AfUtf8Buffer,
) -> AfStatus {
    guard_session(handle, || {
        if out_result.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: The caller provides exclusive writable storage for one
        // disjoint output value; null was checked above.
        unsafe { out_result.write(EMPTY_BUFFER) };

        if let Err(status) = session_access(handle) {
            return status;
        }
        // SAFETY: The caller guarantees both non-empty ranges satisfy the slice
        // validity and immutability requirements documented above.
        let command = match unsafe { read_utf8(command_ptr, command_len) } {
            Ok(command) => command,
            Err(status) => return status,
        };
        // SAFETY: Same contract as the command range.
        let args_json = match unsafe { read_utf8(args_json_ptr, args_json_len) } {
            Ok(args_json) => args_json,
            Err(status) => return status,
        };

        let result = with_reserved_buffer_owner(&NEXT_BUFFER_OWNER, |owner| {
            let json = execute_session(handle, command, args_json)?;
            register_utf8_buffer(owner, json)
        });
        match result {
            Ok(buffer) => {
                // SAFETY: The validated output storage remains exclusively
                // writable and disjoint for the duration of the call.
                unsafe { out_result.write(buffer) };
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

/// Parses one command-line point through [`ApiSession::parse_input`] and returns
/// its structured result envelope as owned UTF-8.
///
/// # Safety
///
/// `out_result` must be non-null, aligned, disjoint from the input and valid for
/// an exclusive write of one [`AfUtf8Buffer`] for the full call. It must not own
/// a live buffer. A non-empty input must be one immutable readable allocation
/// of at most `ISIZE_MAX` bytes with no address wrap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_parse_input_json(
    handle: AfSessionHandle,
    input: *const u8,
    input_len: usize,
    has_base: u8,
    base_x: f64,
    base_y: f64,
    out_result: *mut AfUtf8Buffer,
) -> AfStatus {
    guard_session(handle, || {
        if out_result.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: The caller provides exclusive writable output storage;
        // null was checked above.
        unsafe { out_result.write(EMPTY_BUFFER) };

        if let Err(status) = session_access(handle) {
            return status;
        }
        let base = match has_base {
            0 => None,
            1 if base_x.is_finite() && base_y.is_finite() => Some([base_x, base_y]),
            _ => return AF_STATUS_INVALID_ARGUMENT,
        };
        // SAFETY: The caller guarantees a valid immutable input range disjoint
        // from the output storage.
        let input = match unsafe { read_utf8(input, input_len) } {
            Ok(input) => input,
            Err(status) => return status,
        };

        let result = with_reserved_buffer_owner(&NEXT_BUFFER_OWNER, |owner| {
            let json = parse_input_session(handle, input, base)?;
            register_utf8_buffer(owner, json)
        });
        match result {
            Ok(buffer) => {
                // SAFETY: The validated output remains exclusively writable.
                unsafe { out_result.write(buffer) };
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

/// Returns ranked default snaps from [`ApiSession::snap`] as owned plain JSON.
///
/// # Safety
///
/// `out_result` must be non-null, aligned and valid for an exclusive write of
/// one [`AfUtf8Buffer`] for the full call, and must not own a live buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_snap_json(
    handle: AfSessionHandle,
    x: f64,
    y: f64,
    radius: f64,
    out_result: *mut AfUtf8Buffer,
) -> AfStatus {
    guard_session(handle, || {
        if out_result.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: The caller provides exclusive writable output storage;
        // null was checked above.
        unsafe { out_result.write(EMPTY_BUFFER) };

        if let Err(status) = session_access(handle) {
            return status;
        }
        if !x.is_finite() || !y.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return AF_STATUS_INVALID_ARGUMENT;
        }

        let result = with_reserved_buffer_owner(&NEXT_BUFFER_OWNER, |owner| {
            let json = snap_session(handle, x, y, radius)?;
            register_utf8_buffer(owner, json)
        });
        match result {
            Ok(buffer) => {
                // SAFETY: The validated output remains exclusively writable.
                unsafe { out_result.write(buffer) };
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

/// Replaces the runtime selection with the best [`ApiSession::pick`] hit and
/// returns the resulting, group-expanded selection as owned plain JSON.
///
/// # Safety
///
/// `out_selection` must be non-null, aligned and valid for an exclusive write
/// of one [`AfUtf8Buffer`] for the full call, and must not own a live buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_select_at_json(
    handle: AfSessionHandle,
    x: f64,
    y: f64,
    tolerance: f64,
    out_selection: *mut AfUtf8Buffer,
) -> AfStatus {
    guard_session(handle, || {
        if out_selection.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: The caller provides exclusive writable output storage;
        // null was checked above.
        unsafe { out_selection.write(EMPTY_BUFFER) };

        if let Err(status) = session_access(handle) {
            return status;
        }
        if !x.is_finite() || !y.is_finite() || !tolerance.is_finite() || tolerance <= 0.0 {
            return AF_STATUS_INVALID_ARGUMENT;
        }

        let result = with_reserved_buffer_owner(&NEXT_BUFFER_OWNER, |owner| {
            let json = select_at_session(handle, x, y, tolerance);
            register_utf8_buffer(owner, json)
        });
        match result {
            Ok(buffer) => {
                // SAFETY: The validated output remains exclusively writable.
                unsafe { out_selection.write(buffer) };
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

/// Returns one atomic render delta: JSON control plus owned interleaved floats.
///
/// # Safety
///
/// Both outputs must be non-null, aligned, mutually disjoint and valid for
/// exclusive writes for the full call. Neither may own a live buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_render_delta(
    handle: AfSessionHandle,
    out_control: *mut AfUtf8Buffer,
    out_vertices: *mut AfF32Buffer,
) -> AfStatus {
    guard_session(handle, || {
        if !out_control.is_null() {
            // SAFETY: A non-null output satisfies the caller's writable-storage
            // precondition and is initialized before any checked work.
            unsafe { out_control.write(EMPTY_BUFFER) };
        }
        if !out_vertices.is_null() {
            // SAFETY: Same contract as the control output.
            unsafe { out_vertices.write(EMPTY_F32_BUFFER) };
        }
        if out_control.is_null() || out_vertices.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        if let Err(status) = session_access(handle) {
            return status;
        }

        let result =
            with_reserved_buffer_owners(&NEXT_BUFFER_OWNER, |control_owner, vertices_owner| {
                let (control, vertices) = render_delta_session(handle);
                register_render_delta_buffers(control_owner, control, vertices_owner, vertices)
            });
        match result {
            Ok((control, vertices)) => {
                // SAFETY: Both validated outputs remain exclusively writable
                // and ownership is published only after dual registration.
                unsafe {
                    out_control.write(control);
                    out_vertices.write(vertices);
                }
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

// ponytail: Full JSON is only the headless correctness bridge for the first
// LINE; move to an owned f32 buffer before a frame loop or a 10k+ corpus.
/// Returns [`ApiSession::render_full`] as plain JSON in an owned UTF-8 buffer.
/// Calling this marks the current render model as seen.
///
/// # Safety
///
/// `out_result` must be non-null, aligned and valid for an exclusive write of
/// one [`AfUtf8Buffer`] for the full call. It must not currently own a live
/// buffer. Its prior value is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_render_full_json(
    handle: AfSessionHandle,
    out_result: *mut AfUtf8Buffer,
) -> AfStatus {
    guard_session(handle, || {
        if out_result.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: The caller provides exclusive writable storage for one
        // output value; null was checked above.
        unsafe { out_result.write(EMPTY_BUFFER) };

        if let Err(status) = session_access(handle) {
            return status;
        }

        let result = with_reserved_buffer_owner(&NEXT_BUFFER_OWNER, |owner| {
            let json = render_full_session(handle)?;
            register_utf8_buffer(owner, json)
        });
        match result {
            Ok(buffer) => {
                // SAFETY: The validated output storage remains exclusively
                // writable for the duration of the call.
                unsafe { out_result.write(buffer) };
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

/// Returns [`ApiSession::layers`] as plain JSON in an owned UTF-8 buffer.
///
/// # Safety
///
/// `out_result` must be non-null, aligned and valid for an exclusive write of
/// one [`AfUtf8Buffer`] for the full call. It must not own a live buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_layers_json(
    handle: AfSessionHandle,
    out_result: *mut AfUtf8Buffer,
) -> AfStatus {
    guard_session(handle, || {
        if out_result.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: The caller provides exclusive writable output storage;
        // null was checked above.
        unsafe { out_result.write(EMPTY_BUFFER) };

        if let Err(status) = session_access(handle) {
            return status;
        }

        let result = with_reserved_buffer_owner(&NEXT_BUFFER_OWNER, |owner| {
            register_utf8_buffer(owner, layers_session(handle)?)
        });
        match result {
            Ok(buffer) => {
                // SAFETY: The validated output remains exclusively writable.
                unsafe { out_result.write(buffer) };
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

// ponytail: This owned f32 path proves one visible LINE; it is not a frame
// transport and must stop before batches, deltas or a 10k+ corpus.
/// Returns [`ApiSession::render_vertices`] as owned interleaved `x, y` floats.
/// This read does not advance transactions or the session's render baseline.
///
/// # Safety
///
/// `out_vertices` must be non-null, aligned and valid for an exclusive write of
/// one [`AfF32Buffer`] for the full call. It must not currently own a live
/// buffer. Its prior value is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_session_render_vertices(
    handle: AfSessionHandle,
    out_vertices: *mut AfF32Buffer,
) -> AfStatus {
    guard_session(handle, || {
        if out_vertices.is_null() {
            return AF_STATUS_INVALID_ARGUMENT;
        }
        // SAFETY: The caller provides exclusive writable storage for one
        // output value; null was checked above.
        unsafe { out_vertices.write(EMPTY_F32_BUFFER) };

        if let Err(status) = session_access(handle) {
            return status;
        }

        let result = with_reserved_buffer_owner(&NEXT_BUFFER_OWNER, |owner| {
            let vertices = render_vertices_session(handle)?;
            register_f32_buffer(owner, vertices)
        });
        match result {
            Ok(buffer) => {
                // SAFETY: The validated output storage remains exclusively
                // writable for the duration of the call.
                unsafe { out_vertices.write(buffer) };
                AF_STATUS_OK
            }
            Err(status) => status,
        }
    })
}

/// Releases an owned UTF-8 result and resets its struct to the canonical empty
/// value. Buffers are not thread-affine, but access must be externally serial.
///
/// # Safety
///
/// `buffer` may be null, or point to one initialized, live, aligned
/// [`AfUtf8Buffer`] with exclusive read/write access for the full call. The
/// payload must not be read concurrently. Copies carrying the same owner must
/// not be freed concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_utf8_buffer_free(buffer: *mut AfUtf8Buffer) -> AfStatus {
    guard(|| {
        if buffer.is_null() {
            return AF_STATUS_OK;
        }
        // SAFETY: The caller guarantees initialized, aligned storage with
        // exclusive read/write access; null was checked above.
        free_utf8_buffer(unsafe { &mut *buffer })
    })
}

/// Releases an owned byte result and resets its struct to the canonical empty
/// value. Buffers are not thread-affine, but access must be externally serial.
///
/// # Safety
///
/// `buffer` may be null, or point to initialized, aligned
/// [`AfByteBuffer`] storage with exclusive read/write access for the full call.
/// The payload must not be read concurrently. Copies carrying the same owner
/// must not be freed concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_byte_buffer_free(buffer: *mut AfByteBuffer) -> AfStatus {
    guard(|| {
        if buffer.is_null() {
            return AF_STATUS_OK;
        }
        // SAFETY: The caller guarantees initialized, aligned storage with
        // exclusive read/write access; null was checked above.
        free_byte_buffer(unsafe { &mut *buffer })
    })
}

/// Releases an owned f32 result and resets it to the canonical empty value.
/// Buffers are not thread-affine, but access must be externally serial.
///
/// # Safety
///
/// `buffer` may be null, or point to initialized, aligned
/// [`AfF32Buffer`] storage with exclusive read/write access for the full call.
/// The payload must not be read concurrently. Copies carrying the same owner
/// must not be freed concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn af_f32_buffer_free(buffer: *mut AfF32Buffer) -> AfStatus {
    guard(|| {
        if buffer.is_null() {
            return AF_STATUS_OK;
        }
        // SAFETY: The caller guarantees initialized, aligned storage with
        // exclusive read/write access; null was checked above.
        free_f32_buffer(unsafe { &mut *buffer })
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn allocator_stops_before_wrap() {
        let counter = AtomicUsize::new(usize::MAX - 1);

        assert_eq!(allocate_handle(&counter), Ok(usize::MAX - 1));
        assert_eq!(counter.load(Ordering::Relaxed), usize::MAX);
        assert_eq!(allocate_handle(&counter), Err(AF_STATUS_INTERNAL));
        assert_eq!(counter.load(Ordering::Relaxed), usize::MAX);
    }

    #[test]
    fn exhausted_buffer_owner_does_not_run_the_mutator() {
        let exhausted = AtomicUsize::new(usize::MAX);
        let called = Cell::new(false);
        let mut session = ApiSession::new(Units::default());

        let result: Result<(), AfStatus> = with_reserved_buffer_owner(&exhausted, |_| {
            called.set(true);
            let _ = session.execute_json("LINE", r#"{"p1":[0,0],"p2":[1,1]}"#);
            Ok(())
        });
        assert_eq!(result, Err(AF_STATUS_INTERNAL));
        assert!(!called.get());
        assert_eq!(session.doc_info().entity_count, 0);

        let available = AtomicUsize::new(1);
        let result = with_reserved_buffer_owner(&available, |_| {
            Ok(session.execute_json("LINE", r#"{"p1":[0,0],"p2":[1,1]}"#))
        })
        .expect("available owner should run the operation");
        assert!(result.contains("\"ok\""));
        assert_eq!(session.doc_info().entity_count, 1);
    }

    #[test]
    fn select_at_updates_session_selection_without_render_change() {
        let mut session = ApiSession::new(Units::default());
        let line = session.execute_json("LINE", r#"{"p1":[0,0],"p2":[10,0]}"#);
        assert!(line.contains("\"txSeq\":0"));
        let id = session.pick(5.0, 0.0, 0.25)[0].id;
        assert!(!session.render_delta().upserts.is_empty());

        assert_eq!(
            select_at_json(&mut session, 5.0, 0.0, 0.25),
            format!("[{id}]")
        );
        assert_eq!(session.selection(), [id]);
        let selection_delta = session.render_delta();
        assert!(selection_delta.upserts.is_empty());
        assert!(selection_delta.removes.is_empty());

        assert_eq!(select_at_json(&mut session, 1000.0, 1000.0, 0.25), "[]");
        assert!(session.selection().is_empty());
    }

    #[test]
    fn exhausted_buffer_owner_does_not_run_selection_mutator() {
        let exhausted = AtomicUsize::new(usize::MAX);
        let called = Cell::new(false);
        let mut session = ApiSession::new(Units::default());
        let line = session.execute_json("LINE", r#"{"p1":[0,0],"p2":[10,0]}"#);
        assert!(line.contains("\"ok\""));
        let id = session.pick(5.0, 0.0, 0.25)[0].id;
        session.set_selection(&[id]);
        let before = session.selection();

        let result: Result<(), AfStatus> = with_reserved_buffer_owner(&exhausted, |_| {
            called.set(true);
            let _ = select_at_json(&mut session, 1000.0, 1000.0, 0.25);
            Ok(())
        });
        assert_eq!(result, Err(AF_STATUS_INTERNAL));
        assert!(!called.get());
        assert_eq!(session.selection(), before);
    }

    #[test]
    fn exhausted_buffer_owner_does_not_consume_pending_render_change() {
        let exhausted = AtomicUsize::new(usize::MAX);
        let called = Cell::new(false);
        let mut session = ApiSession::new(Units::default());
        let result = session.execute_json("LINE", r#"{"p1":[0,0],"p2":[1,1]}"#);
        assert!(result.contains("\"ok\""));

        let render: Result<(), AfStatus> = with_reserved_buffer_owner(&exhausted, |_| {
            called.set(true);
            let _ = session.render_full();
            Ok(())
        });
        assert_eq!(render, Err(AF_STATUS_INTERNAL));
        assert!(!called.get());
        assert!(!session.render_delta().upserts.is_empty());
    }

    #[test]
    fn exhausted_buffer_owner_does_not_invoke_vertices_producer() {
        let exhausted = AtomicUsize::new(usize::MAX);
        let called = Cell::new(false);
        let session = ApiSession::new(Units::default());

        let result: Result<(), AfStatus> = with_reserved_buffer_owner(&exhausted, |_| {
            called.set(true);
            let _ = session.render_vertices();
            Ok(())
        });
        assert_eq!(result, Err(AF_STATUS_INTERNAL));
        assert!(!called.get());
    }

    #[test]
    fn parse_result_envelope_has_stable_goldens() {
        assert_eq!(
            parse_result_json(Ok(ParsedPoint {
                point: [12.0, 34.0]
            })),
            r#"{"ok":{"point":[12.0,34.0]}}"#
        );
        assert_eq!(
            parse_result_json(Err(ApiError::new("parse_error", "bad point")
                .with_detail(serde_json::json!({ "pos": 0 })))),
            r#"{"error":{"code":"parse_error","detail":{"pos":0},"message":"bad point"}}"#
        );
        assert_eq!(
            parse_result_json(Err(ApiError::new("not_a_point", "not a point"))),
            r#"{"error":{"code":"not_a_point","message":"not a point"}}"#
        );
    }

    #[test]
    fn either_exhausted_dual_owner_does_not_consume_pending_render_change() {
        for initial in [usize::MAX, usize::MAX - 1] {
            let exhausted = AtomicUsize::new(initial);
            let called = Cell::new(false);
            let mut session = ApiSession::new(Units::default());
            let result = session.execute_json("LINE", r#"{"p1":[0,0],"p2":[1,1]}"#);
            assert!(result.contains("\"ok\""));

            let render = with_reserved_buffer_owners(&exhausted, |_, _| {
                called.set(true);
                session.render_delta()
            });
            assert_eq!(render, Err(AF_STATUS_INTERNAL));
            assert!(!called.get());
            assert!(!session.render_delta().upserts.is_empty());
        }
    }

    #[test]
    fn render_vertices_does_not_consume_pending_render_change() {
        let mut session = ApiSession::new(Units::default());
        let result = session.execute_json("LINE", r#"{"p1":[0,0],"p2":[10,20]}"#);
        assert!(result.contains("\"ok\""));
        assert_eq!(session.render_vertices(), [0.0, 0.0, 10.0, 20.0]);
        assert!(!session.render_delta().upserts.is_empty());
    }

    #[test]
    fn guard_maps_unwinding_panic() {
        assert_eq!(guard(|| panic!("test panic")), AF_STATUS_PANIC);
    }

    #[test]
    fn terminal_guard_contains_invalidation_panic() {
        struct PanicOnDrop;

        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("invalidation drop panic");
            }
        }

        let invalidation_ran = Cell::new(false);
        let value = PanicOnDrop;

        let status = guard_terminal(
            || panic!("operation panic"),
            || {
                invalidation_ran.set(true);
                drop(value);
            },
        );

        assert_eq!(status, AF_STATUS_PANIC);
        assert!(invalidation_ran.get());
    }

    #[test]
    fn panic_after_render_delta_makes_the_session_handle_terminal() {
        let handle = create_session().expect("session should be created");
        let line = execute_session(handle, "LINE", r#"{"p1":[0,0],"p2":[10,20]}"#)
            .expect("LINE should execute");
        assert!(line.contains("\"ok\""));

        let status = guard_session(handle, || {
            let (control, vertices) = render_delta_session(handle);
            assert!(control.contains("\"upserts\":[{"));
            assert_eq!(vertices, [0.0, 0.0, 10.0, 20.0]);
            panic!("injected panic after render_seen was consumed");
        });

        assert_eq!(status, AF_STATUS_PANIC);
        assert_eq!(session_access(handle), Err(AF_STATUS_INVALID_HANDLE));
        assert!(!SESSIONS.with_borrow(|sessions| sessions.sessions.contains_key(&handle)));
        assert_eq!(destroy_session(handle), AF_STATUS_INVALID_HANDLE);
    }

    #[test]
    fn persistence_owner_exhaustion_runs_neither_producer_nor_mutator() {
        let exhausted = AtomicUsize::new(usize::MAX);
        let save_called = Cell::new(false);
        let mut session = ApiSession::new(Units::default());
        let line = session.execute_json("LINE", r#"{"p1":[0,0],"p2":[1,1]}"#);
        assert!(line.contains("\"ok\""));
        let before = session.save().expect("document should save");

        let save_result: Result<(), AfStatus> = with_reserved_buffer_owner(&exhausted, |_| {
            save_called.set(true);
            let _ = session.save();
            Ok(())
        });
        assert_eq!(save_result, Err(AF_STATUS_INTERNAL));
        assert!(!save_called.get());

        let open_called = Cell::new(false);
        let open_result: Result<(), AfStatus> = with_reserved_buffer_owner(&exhausted, |_| {
            open_called.set(true);
            let _ = session.open(b"corrupt");
            Ok(())
        });
        assert_eq!(open_result, Err(AF_STATUS_INTERNAL));
        assert!(!open_called.get());
        assert_eq!(
            session.save().expect("document should remain valid"),
            before
        );
    }

    #[test]
    fn panic_after_open_is_terminal_and_registers_no_partial_owner() {
        let handle = create_session().expect("session should be created");
        let line = execute_session(handle, "LINE", r#"{"p1":[0,0],"p2":[1,1]}"#)
            .expect("LINE should execute");
        assert!(line.contains("\"ok\""));
        let bytes = save_session(handle).expect("document should save");
        let buffer_count = lock_buffers().len();

        let status = guard_session(handle, || {
            let _ = with_reserved_buffer_owner::<()>(&NEXT_BUFFER_OWNER, |_| {
                let json = open_session(handle, &bytes);
                assert!(json.contains("\"ok\""));
                panic!("injected panic after open before buffer registration");
            });
            AF_STATUS_OK
        });

        assert_eq!(status, AF_STATUS_PANIC);
        assert_eq!(session_access(handle), Err(AF_STATUS_INVALID_HANDLE));
        assert_eq!(lock_buffers().len(), buffer_count);
        assert_eq!(destroy_session(handle), AF_STATUS_INVALID_HANDLE);
    }
}
