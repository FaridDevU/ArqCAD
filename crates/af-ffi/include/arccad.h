#ifndef ARCCAD_FFI_H
#define ARCCAD_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef uint32_t AfStatus;
typedef uintptr_t AfSessionHandle;

typedef struct AfVersion {
    uint16_t major;
    uint16_t minor;
    uint16_t patch;
} AfVersion;

/* Read-only UTF-8 bytes, not NUL-terminated. All fields are opaque to callers. */
typedef struct AfUtf8Buffer {
    const uint8_t *data;
    size_t len;
    size_t capacity;
    uintptr_t owner;
} AfUtf8Buffer;

/* Read-only bytes owned by the Rust buffer registry. */
typedef struct AfByteBuffer {
    const uint8_t *data;
    size_t len;
    size_t capacity;
    uintptr_t owner;
} AfByteBuffer;

/* Read-only IEEE-754 binary32 elements owned by the Rust buffer registry. */
typedef struct AfF32Buffer {
    const float *data;
    size_t len;
    size_t capacity;
    uintptr_t owner;
} AfF32Buffer;

#define AF_STATUS_OK UINT32_C(0)
#define AF_STATUS_INVALID_ARGUMENT UINT32_C(1)
#define AF_STATUS_INVALID_HANDLE UINT32_C(2)
#define AF_STATUS_WRONG_THREAD UINT32_C(3)
#define AF_STATUS_INTERNAL UINT32_C(4)
#define AF_STATUS_INVALID_UTF8 UINT32_C(5)
#define AF_STATUS_PANIC UINT32_C(255)

#define AF_ABI_VERSION_MAJOR UINT16_C(0)
#define AF_ABI_VERSION_MINOR UINT16_C(7)
#define AF_ABI_VERSION_PATCH UINT16_C(0)

/* Exact client-side compatibility check; call after af_abi_version and before create. */
static inline int af_abi_version_matches(AfVersion version) {
    return version.major == AF_ABI_VERSION_MAJOR &&
           version.minor == AF_ABI_VERSION_MINOR &&
           version.patch == AF_ABI_VERSION_PATCH;
}

/* Stable NUL-terminated UTF-8 text with no allocation or mutable global state. */
static inline const char *af_status_message(AfStatus status) {
    switch (status) {
    case AF_STATUS_OK:
        return "ok";
    case AF_STATUS_INVALID_ARGUMENT:
        return "invalid argument";
    case AF_STATUS_INVALID_HANDLE:
        return "invalid handle";
    case AF_STATUS_WRONG_THREAD:
        return "wrong thread";
    case AF_STATUS_INTERNAL:
        return "internal error";
    case AF_STATUS_INVALID_UTF8:
        return "invalid UTF-8";
    case AF_STATUS_PANIC:
        return "panic";
    default:
        return "unknown status";
    }
}

/*
 * out_version must be non-null, correctly aligned and writable for one
 * AfVersion with exclusive access for the full call. Only a null pointer maps
 * to INVALID_ARGUMENT; violating the remaining pointer preconditions is undefined behavior.
 */
AfStatus af_abi_version(AfVersion *out_version);

/*
 * out_handle must be non-null, correctly aligned and writable for one
 * AfSessionHandle with exclusive access for the full call. Only a null pointer
 * maps to INVALID_ARGUMENT; violating the remaining pointer preconditions is
 * undefined behavior. On any reported failure, the output is zero when
 * writable.
 */
AfStatus af_session_create(AfSessionHandle *out_handle);

/*
 * Every operation below must be called on the creating thread. Zero and stale
 * handles are invalid. AF_STATUS_PANIC terminally invalidates the handle; no
 * later operation, including destroy, may reuse it.
 */
AfStatus af_session_destroy(AfSessionHandle handle);

/*
 * Serializes through ApiSession::save on the creating thread. On success,
 * out_bytes owns non-empty native .arcf bytes. out_bytes must be non-null,
 * correctly aligned and exclusively writable for one AfByteBuffer, and must
 * not own a live buffer. It is initialized empty before checked work.
 * AF_STATUS_PANIC terminally invalidates the session and publishes no owner.
 */
AfStatus af_session_save_arcf(
    AfSessionHandle handle,
    AfByteBuffer *out_bytes);

/*
 * Opens native .arcf bytes through ApiSession::open and returns exactly one
 * {"ok":[warnings]} or {"error":ApiError} UTF-8 envelope. A document error is
 * returned with AF_STATUS_OK and does not invalidate or replace the session.
 *
 * bytes follows the strict pointer/length contract of execute_json: NULL is
 * accepted only with length zero and len must be <= ISIZE_MAX. The handle and
 * owner thread are checked before reading input. out_result must be non-null,
 * correctly aligned, disjoint from input and exclusively writable for one
 * AfUtf8Buffer, and must not own a live buffer. It is initialized empty before
 * checked work. AF_STATUS_PANIC terminally invalidates the session and
 * publishes no owner.
 */
AfStatus af_session_open_arcf_json(
    AfSessionHandle handle,
    const uint8_t *bytes,
    size_t len,
    AfUtf8Buffer *out_result);

/*
 * Executes through ApiSession::execute_json on the session's creating thread.
 * Domain failures are returned as an {"error": ...} JSON value with status OK.
 *
 * out_result must be non-null, correctly aligned, exclusively writable for one
 * AfUtf8Buffer, disjoint from both input ranges, and must not currently own a
 * live buffer. It is initialized to {NULL,0,0,0} before other checked work.
 *
 * Each input is pointer plus byte length, not NUL-terminated. NULL is accepted
 * only with length zero. A non-empty range must be immutable and readable for
 * the full call, belong to one allocation, have length <= ISIZE_MAX, and not
 * wrap its address space. Violating pointer validity, overlap or concurrency
 * preconditions is undefined behavior. Checked invalid UTF-8 returns
 * AF_STATUS_INVALID_UTF8 and leaves out_result empty.
 */
AfStatus af_session_execute_json(
    AfSessionHandle handle,
    const uint8_t *command_ptr,
    size_t command_len,
    const uint8_t *args_json_ptr,
    size_t args_json_len,
    AfUtf8Buffer *out_result);

/*
 * Parses one command-line point through ApiSession::parse_input. Successful
 * and domain-error results use the stable {"ok":...}/{"error":...} envelope.
 * has_base must be exactly 0 or 1; when it is 1 both base values must be finite.
 * When it is 0 the base values are ignored.
 *
 * input follows the strict UTF-8 pointer/length contract of execute_json.
 * out_result must be non-null, correctly aligned, exclusively writable for one
 * AfUtf8Buffer, disjoint from input and must not own a live buffer. It is
 * initialized empty before checked work.
 */
AfStatus af_session_parse_input_json(
    AfSessionHandle handle,
    const uint8_t *input,
    size_t input_len,
    uint8_t has_base,
    double base_x,
    double base_y,
    AfUtf8Buffer *out_result);

/*
 * Returns ApiSession::snap with default options as a ranked plain JSON array.
 * x, y and radius are world units, must be finite, and radius must be > 0.
 * out_result must be non-null, correctly aligned, exclusively writable for one
 * AfUtf8Buffer and must not own a live buffer. It is initialized empty before
 * checked work.
 */
AfStatus af_session_snap_json(
    AfSessionHandle handle,
    double x,
    double y,
    double radius,
    AfUtf8Buffer *out_result);

/*
 * Replaces the runtime selection with the best ranked ApiSession::pick hit, or
 * clears it when there is no hit, and returns ApiSession::selection() as a
 * plain JSON u64 array. x, y and tolerance are world units, must be finite,
 * and tolerance must be > 0. Selection does not execute a command, consume a
 * transaction sequence or change the render delta.
 *
 * out_selection must be non-null, correctly aligned and exclusively writable
 * for one AfUtf8Buffer, and must not own a live buffer. It is initialized empty
 * before checked work. AF_STATUS_PANIC terminally invalidates the session and
 * publishes no buffer owner.
 */
AfStatus af_session_select_at_json(
    AfSessionHandle handle,
    double x,
    double y,
    double tolerance,
    AfUtf8Buffer *out_selection);

/*
 * Returns one ApiSession::render_delta read as an atomic pair: JSON control in
 * out_control and interleaved x,y geometry in out_vertices. The control keeps
 * upserts, removes, ltscale and an empty vertices array; offsets/counts index
 * the companion f32 buffer. Both outputs must be non-null, correctly aligned,
 * mutually disjoint and exclusively writable for one AfUtf8Buffer and one
 * AfF32Buffer respectively, and must not own live buffers. Every non-null
 * output is initialized empty before checked work; a non-OK status publishes
 * no owner.
 *
 * AF_STATUS_PANIC terminally invalidates and removes the session. The handle
 * must not be reused; a later af_session_destroy call is safe and returns
 * AF_STATUS_INVALID_HANDLE.
 */
AfStatus af_session_render_delta(
    AfSessionHandle handle,
    AfUtf8Buffer *out_control,
    AfF32Buffer *out_vertices);

/*
 * Returns the session's current ApiSession::render_full() view as plain JSON
 * on the creating thread. The result has batches, vertices and ltscale fields.
 * Calling this marks the current render model as seen.
 *
 * out_result must be non-null, correctly aligned and exclusively writable for
 * one AfUtf8Buffer, and must not currently own a live buffer. It is initialized
 * to {NULL,0,0,0} without reading its prior value before other checked work.
 * Violating pointer validity or concurrency preconditions is undefined behavior.
 */
AfStatus af_session_render_full_json(
    AfSessionHandle handle,
    AfUtf8Buffer *out_result);

/*
 * Returns ApiSession::layers() as a plain JSON array. Each layer includes its
 * stable id, name, style defaults, off/frozen/locked/plot flags and current.
 * This read does not execute a command or advance transaction history.
 *
 * out_result must be non-null, correctly aligned and exclusively writable for
 * one AfUtf8Buffer, and must not own a live buffer. It is initialized empty
 * before checked work.
 */
AfStatus af_session_layers_json(
    AfSessionHandle handle,
    AfUtf8Buffer *out_result);

/*
 * Returns ApiSession::render_vertices() as interleaved x,y float elements on
 * the session's creating thread. len and capacity count floats, not bytes.
 *
 * out_vertices must be non-null, correctly aligned and exclusively writable
 * for one AfF32Buffer, and must not currently own a live buffer. It is
 * initialized to {NULL,0,0,0} without reading its prior value before other
 * checked work. An empty session succeeds with that canonical empty value.
 * Violating pointer validity or concurrency preconditions is undefined behavior.
 */
AfStatus af_session_render_vertices(
    AfSessionHandle handle,
    AfF32Buffer *out_vertices);

/*
 * Releases a live buffer and resets it to {NULL,0,0,0}. The canonical empty
 * value may be released repeatedly. A stale nonzero owner returns
 * INVALID_HANDLE; altered live metadata or owner zero with noncanonical fields
 * returns INVALID_ARGUMENT. Every failure preserves the struct and allocation.
 *
 * buffer must be NULL or point to one initialized, live, correctly aligned
 * AfUtf8Buffer with exclusive read/write access for the full call. Payload
 * reads and frees of copies with the same owner must be externally serialized.
 * Passing NULL is idempotent and returns OK.
 * A non-null pointer violating these preconditions causes undefined behavior.
 * The buffer is not session-thread-affine and may be freed on any one thread.
 */
AfStatus af_utf8_buffer_free(AfUtf8Buffer *buffer);

/*
 * Releases a live byte buffer and resets it to {NULL,0,0,0}. The canonical
 * empty value may be released repeatedly. A stale nonzero owner returns
 * INVALID_HANDLE; altered metadata or an owner registered for another buffer
 * type returns INVALID_ARGUMENT. Every failure preserves struct/allocation.
 *
 * buffer may be NULL or point to initialized, correctly aligned AfByteBuffer
 * storage with exclusive read/write access for the full call. Payload reads
 * and frees of copies with the same owner must be externally serialized.
 * Passing NULL is idempotent and returns OK; violating other non-null pointer
 * preconditions is undefined behavior.
 */
AfStatus af_byte_buffer_free(AfByteBuffer *buffer);

/*
 * Releases a live f32 buffer and resets it to {NULL,0,0,0}. The canonical
 * empty value may be released repeatedly. A stale nonzero owner returns
 * INVALID_HANDLE; altered metadata or an owner registered for another buffer
 * type returns INVALID_ARGUMENT. Every failure preserves the struct and allocation.
 *
 * buffer may be NULL or point to initialized, correctly aligned AfF32Buffer
 * storage with exclusive read/write access for the full call. Payload reads
 * and frees of copies with the same owner must be externally serialized.
 * Passing NULL is idempotent and returns OK; violating other non-null pointer
 * preconditions is undefined behavior. The buffer is not session-thread-affine
 * and may be freed on any one thread.
 */
AfStatus af_f32_buffer_free(AfF32Buffer *buffer);

#ifdef __cplusplus
}
#endif

#endif
