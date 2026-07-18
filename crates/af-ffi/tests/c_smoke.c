#include "arccad.h"

#include <assert.h>
#include <float.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static int contains(const AfUtf8Buffer *buffer, const char *needle) {
    const size_t needle_len = strlen(needle);
    if (needle_len == 0) {
        return 1;
    }
    if (buffer->len < needle_len) {
        return 0;
    }
    for (size_t i = 0; i <= buffer->len - needle_len; ++i) {
        if (memcmp(buffer->data + i, needle, needle_len) == 0) {
            return 1;
        }
    }
    return 0;
}

static int is_single_id_array(const AfUtf8Buffer *buffer) {
    if (buffer->len < 3 || buffer->data[0] != '[' ||
        buffer->data[buffer->len - 1] != ']' || buffer->data[1] < '1' ||
        buffer->data[1] > '9') {
        return 0;
    }
    for (size_t i = 1; i + 1 < buffer->len; ++i) {
        if (buffer->data[i] < '0' || buffer->data[i] > '9') {
            return 0;
        }
    }
    return 1;
}

static AfUtf8Buffer execute(
    AfSessionHandle session,
    const char *command,
    const char *args) {
    AfUtf8Buffer result = {0};
    assert(af_session_execute_json(
               session,
               (const uint8_t *)command,
               strlen(command),
               (const uint8_t *)args,
               strlen(args),
               &result) == AF_STATUS_OK);
    assert(result.data != NULL);
    assert(result.len > 0);
    assert(result.capacity >= result.len);
    assert(result.owner != 0);
    return result;
}

static AfByteBuffer save_arcf(AfSessionHandle session) {
    AfByteBuffer result = {0};
    assert(af_session_save_arcf(session, &result) == AF_STATUS_OK);
    assert(result.data != NULL);
    assert(result.len > 0);
    assert(result.capacity >= result.len);
    assert(result.owner != 0);
    return result;
}

static AfUtf8Buffer open_arcf(
    AfSessionHandle session,
    const AfByteBuffer *bytes) {
    AfUtf8Buffer result = {0};
    assert(af_session_open_arcf_json(
               session,
               bytes->data,
               bytes->len,
               &result) == AF_STATUS_OK);
    assert(result.data != NULL);
    assert(result.len > 0);
    assert(result.capacity >= result.len);
    assert(result.owner != 0);
    return result;
}

static AfUtf8Buffer parse_point(
    AfSessionHandle session,
    const char *input,
    uint8_t has_base,
    double base_x,
    double base_y) {
    AfUtf8Buffer result = {0};
    assert(af_session_parse_input_json(
               session,
               (const uint8_t *)input,
               strlen(input),
               has_base,
               base_x,
               base_y,
               &result) == AF_STATUS_OK);
    assert(result.data != NULL);
    assert(result.len > 0);
    assert(result.capacity >= result.len);
    assert(result.owner != 0);
    return result;
}

static AfUtf8Buffer snap_point(
    AfSessionHandle session,
    double x,
    double y,
    double radius) {
    AfUtf8Buffer result = {0};
    assert(af_session_snap_json(session, x, y, radius, &result) == AF_STATUS_OK);
    assert(result.data != NULL);
    assert(result.len > 0);
    assert(result.capacity >= result.len);
    assert(result.owner != 0);
    return result;
}

static AfUtf8Buffer select_at(
    AfSessionHandle session,
    double x,
    double y,
    double tolerance) {
    AfUtf8Buffer result = {0};
    assert(af_session_select_at_json(session, x, y, tolerance, &result) ==
           AF_STATUS_OK);
    assert(result.data != NULL);
    assert(result.len > 0);
    assert(result.capacity >= result.len);
    assert(result.owner != 0);
    return result;
}

static void render_delta(
    AfSessionHandle session,
    AfUtf8Buffer *control,
    AfF32Buffer *vertices) {
    assert(af_session_render_delta(session, control, vertices) == AF_STATUS_OK);
    assert(control->data != NULL);
    assert(control->len > 0);
    assert(control->capacity >= control->len);
    assert(control->owner != 0);
    assert(vertices->data != NULL);
    assert(vertices->len > 0);
    assert(vertices->capacity >= vertices->len);
    assert(vertices->owner != 0);
    assert(control->owner != vertices->owner);
}

static AfUtf8Buffer render_full(AfSessionHandle session) {
    AfUtf8Buffer result = {0};
    assert(af_session_render_full_json(session, &result) == AF_STATUS_OK);
    assert(result.data != NULL);
    assert(result.len > 0);
    assert(result.capacity >= result.len);
    assert(result.owner != 0);
    return result;
}

static AfF32Buffer render_vertices(AfSessionHandle session) {
    AfF32Buffer result = {0};
    assert(af_session_render_vertices(session, &result) == AF_STATUS_OK);
    assert(result.data != NULL);
    assert(result.len > 0);
    assert(result.capacity >= result.len);
    assert(result.owner != 0);
    return result;
}

int main(void) {
    assert(sizeof(AfStatus) == sizeof(uint32_t));
    assert(sizeof(AfSessionHandle) == sizeof(uintptr_t));
    assert(sizeof(float) == 4 && FLT_RADIX == 2 && FLT_MANT_DIG == 24 && FLT_MAX_EXP == 128);
    assert(sizeof(AfUtf8Buffer) == 4 * sizeof(uintptr_t));
    assert(offsetof(AfUtf8Buffer, data) == 0);
    assert(offsetof(AfUtf8Buffer, len) == sizeof(uintptr_t));
    assert(offsetof(AfUtf8Buffer, capacity) == 2 * sizeof(uintptr_t));
    assert(offsetof(AfUtf8Buffer, owner) == 3 * sizeof(uintptr_t));
    assert(sizeof(AfByteBuffer) == 4 * sizeof(uintptr_t));
    assert(offsetof(AfByteBuffer, data) == 0);
    assert(offsetof(AfByteBuffer, len) == sizeof(uintptr_t));
    assert(offsetof(AfByteBuffer, capacity) == 2 * sizeof(uintptr_t));
    assert(offsetof(AfByteBuffer, owner) == 3 * sizeof(uintptr_t));
    assert(sizeof(AfF32Buffer) == 4 * sizeof(uintptr_t));
    assert(offsetof(AfF32Buffer, data) == 0);
    assert(offsetof(AfF32Buffer, len) == sizeof(uintptr_t));
    assert(offsetof(AfF32Buffer, capacity) == 2 * sizeof(uintptr_t));
    assert(offsetof(AfF32Buffer, owner) == 3 * sizeof(uintptr_t));

    AfVersion version = {0};
    assert(af_abi_version(&version) == AF_STATUS_OK);
    assert(version.major == AF_ABI_VERSION_MAJOR &&
           version.minor == AF_ABI_VERSION_MINOR &&
           version.patch == AF_ABI_VERSION_PATCH);
    assert(af_abi_version_matches(version));

    AfSessionHandle session = 0;
    AfVersion mismatch = version;
    mismatch.patch = (uint16_t)(mismatch.patch + UINT16_C(1));
    assert(!af_abi_version_matches(mismatch));
    assert(session == 0); /* A mismatched client stops before create. */

    const AfStatus statuses[] = {
        AF_STATUS_OK,
        AF_STATUS_INVALID_ARGUMENT,
        AF_STATUS_INVALID_HANDLE,
        AF_STATUS_WRONG_THREAD,
        AF_STATUS_INTERNAL,
        AF_STATUS_INVALID_UTF8,
        AF_STATUS_PANIC,
        UINT32_MAX,
    };
    const char *messages[] = {
        "ok",
        "invalid argument",
        "invalid handle",
        "wrong thread",
        "internal error",
        "invalid UTF-8",
        "panic",
        "unknown status",
    };
    for (size_t i = 0; i < sizeof(statuses) / sizeof(statuses[0]); ++i) {
        assert(strcmp(af_status_message(statuses[i]), messages[i]) == 0);
    }

    assert(af_utf8_buffer_free(NULL) == AF_STATUS_OK);
    assert(af_byte_buffer_free(NULL) == AF_STATUS_OK);
    assert(af_f32_buffer_free(NULL) == AF_STATUS_OK);

    assert(af_session_create(&session) == AF_STATUS_OK);
    assert(session != 0);

    AfUtf8Buffer baseline = render_full(session);
    assert(contains(&baseline, "\"batches\":[]"));
    assert(af_utf8_buffer_free(&baseline) == AF_STATUS_OK);

    AfUtf8Buffer parsed = parse_point(session, "0,0", UINT8_C(0), 0.0, 0.0);
    assert(contains(&parsed, "\"ok\":{\"point\":[0.0,0.0]}"));
    assert(af_utf8_buffer_free(&parsed) == AF_STATUS_OK);

    AfUtf8Buffer line = execute(
        session,
        "LINE",
        "{\"p1\":[0,0],\"p2\":[10,20]}");
    assert(contains(&line, "\"ok\""));
    assert(contains(&line, "\"txSeq\":0"));
    assert(contains(&line, "\"created\":["));

    AfUtf8Buffer snapped = snap_point(session, 0.1, 0.0, 1.0);
    assert(contains(&snapped, "\"kind\":\"endpoint\""));
    assert(contains(&snapped, "\"point\":[0.0,0.0]"));
    assert(contains(&snapped, "\"entity\":"));

    /* The verified snapped endpoint above is the start of the second LINE. */
    AfUtf8Buffer second = execute(
        session,
        "LINE",
        "{\"p1\":[0,0],\"p2\":[5,5]}");
    assert(contains(&second, "\"txSeq\":1"));

    AfUtf8Buffer selected = select_at(session, 10.0, 20.0, 0.1);
    assert(is_single_id_array(&selected));

    AfUtf8Buffer delta_control = {0};
    AfF32Buffer delta_vertices = {0};
    render_delta(session, &delta_control, &delta_vertices);
    assert(contains(&delta_control, "\"upserts\":["));
    assert(contains(&delta_control, "\"removes\":[]"));
    assert(contains(&delta_control, "\"vertices\":[]"));
    assert(contains(&delta_control, "\"ltscale\":1.0"));
    assert(delta_vertices.len == 8);
    assert(delta_vertices.data[0] == 0.0f);
    assert(delta_vertices.data[1] == 0.0f);
    assert(delta_vertices.data[2] == 10.0f);
    assert(delta_vertices.data[3] == 20.0f);
    assert(delta_vertices.data[4] == 0.0f);
    assert(delta_vertices.data[5] == 0.0f);
    assert(delta_vertices.data[6] == 5.0f);
    assert(delta_vertices.data[7] == 5.0f);
    assert(af_utf8_buffer_free(&delta_control) == AF_STATUS_OK);
    assert(af_f32_buffer_free(&delta_vertices) == AF_STATUS_OK);

    AfUtf8Buffer undo = execute(session, "UNDO", "null");
    assert(contains(&undo, "\"ok\""));
    assert(contains(&undo, "\"txSeq\":null"));
    assert(contains(&undo, "\"created\":[]"));
    assert(af_utf8_buffer_free(&undo) == AF_STATUS_OK);

    AfUtf8Buffer undo_control = {0};
    AfF32Buffer undo_vertices = {0};
    render_delta(session, &undo_control, &undo_vertices);
    assert(undo_vertices.len == 4);
    assert(undo_vertices.data[0] == 0.0f);
    assert(undo_vertices.data[1] == 0.0f);
    assert(undo_vertices.data[2] == 10.0f);
    assert(undo_vertices.data[3] == 20.0f);
    assert(af_utf8_buffer_free(&undo_control) == AF_STATUS_OK);
    assert(af_f32_buffer_free(&undo_vertices) == AF_STATUS_OK);

    AfUtf8Buffer redo = execute(session, "REDO", "null");
    assert(contains(&redo, "\"ok\""));
    assert(contains(&redo, "\"txSeq\":null"));
    assert(contains(&redo, "\"created\":[]"));
    assert(af_utf8_buffer_free(&redo) == AF_STATUS_OK);

    AfUtf8Buffer redo_control = {0};
    AfF32Buffer redo_vertices = {0};
    render_delta(session, &redo_control, &redo_vertices);
    assert(redo_vertices.len == 8);
    assert(af_utf8_buffer_free(&redo_control) == AF_STATUS_OK);
    assert(af_f32_buffer_free(&redo_vertices) == AF_STATUS_OK);

    AfByteBuffer saved = save_arcf(session);
    AfSessionHandle reopened = 0;
    assert(af_session_create(&reopened) == AF_STATUS_OK);
    AfUtf8Buffer opened = open_arcf(reopened, &saved);
    assert(contains(&opened, "\"ok\":[]"));

    AfUtf8Buffer reopen_control = {0};
    AfF32Buffer reopen_vertices = {0};
    render_delta(reopened, &reopen_control, &reopen_vertices);
    assert(reopen_vertices.len == 8);
    assert(reopen_vertices.data[0] == 0.0f);
    assert(reopen_vertices.data[1] == 0.0f);
    assert(reopen_vertices.data[2] == 10.0f);
    assert(reopen_vertices.data[3] == 20.0f);
    assert(af_utf8_buffer_free(&reopen_control) == AF_STATUS_OK);
    assert(af_f32_buffer_free(&reopen_vertices) == AF_STATUS_OK);

    AfUtf8Buffer continued = execute(
        reopened,
        "LINE",
        "{\"p1\":[20,20],\"p2\":[30,30]}");
    assert(contains(&continued, "\"txSeq\":0"));
    assert(af_utf8_buffer_free(&continued) == AF_STATUS_OK);
    assert(af_utf8_buffer_free(&opened) == AF_STATUS_OK);
    assert(af_byte_buffer_free(&saved) == AF_STATUS_OK);
    assert(saved.data == NULL && saved.len == 0 && saved.capacity == 0 && saved.owner == 0);
    assert(af_byte_buffer_free(&saved) == AF_STATUS_OK);
    assert(af_session_destroy(reopened) == AF_STATUS_OK);

    assert(af_utf8_buffer_free(&snapped) == AF_STATUS_OK);
    assert(af_utf8_buffer_free(&selected) == AF_STATUS_OK);
    assert(af_utf8_buffer_free(&second) == AF_STATUS_OK);

    AfF32Buffer vertices = render_vertices(session);
    assert(vertices.owner != line.owner);
    assert(vertices.len == 8);
    assert(vertices.data[0] == 0.0f);
    assert(vertices.data[1] == 0.0f);
    assert(vertices.data[2] == 10.0f);
    assert(vertices.data[3] == 20.0f);

    AfUtf8Buffer render = render_full(session);
    assert(render.owner != line.owner);
    assert(render.owner != vertices.owner);
    assert(contains(&render, "\"batches\":"));
    assert(contains(&render, "\"entity\":"));
    assert(contains(&render, "\"vertices\":"));
    assert(af_utf8_buffer_free(&render) == AF_STATUS_OK);
    assert(render.data == NULL && render.len == 0 && render.capacity == 0 && render.owner == 0);

    assert(af_f32_buffer_free(&vertices) == AF_STATUS_OK);
    assert(vertices.data == NULL && vertices.len == 0 && vertices.capacity == 0 && vertices.owner == 0);
    assert(af_f32_buffer_free(&vertices) == AF_STATUS_OK);

    assert(af_utf8_buffer_free(&line) == AF_STATUS_OK);
    assert(line.data == NULL && line.len == 0 && line.capacity == 0 && line.owner == 0);
    assert(af_utf8_buffer_free(&line) == AF_STATUS_OK);

    AfUtf8Buffer unknown = execute(session, "NOSUCHCOMMAND", "null");
    assert(contains(&unknown, "\"code\":\"unknown_command\""));
    assert(af_utf8_buffer_free(&unknown) == AF_STATUS_OK);

    AfUtf8Buffer malformed = execute(session, "LINE", "{not-json");
    assert(contains(&malformed, "\"code\":\"malformed_json\""));
    assert(af_utf8_buffer_free(&malformed) == AF_STATUS_OK);

    const uint8_t invalid_utf8[] = {UINT8_C(0xff)};
    AfUtf8Buffer invalid = {(const uint8_t *)1, 1, 1, 1};
    assert(af_session_execute_json(
               session,
               invalid_utf8,
               sizeof(invalid_utf8),
               NULL,
               0,
               &invalid) == AF_STATUS_INVALID_UTF8);
    assert(invalid.data == NULL && invalid.len == 0 && invalid.capacity == 0 && invalid.owner == 0);

    AfUtf8Buffer invalid_parse = {(const uint8_t *)1, 1, 1, 1};
    assert(af_session_parse_input_json(
               session,
               invalid_utf8,
               sizeof(invalid_utf8),
               UINT8_C(0),
               0.0,
               0.0,
               &invalid_parse) == AF_STATUS_INVALID_UTF8);
    assert(invalid_parse.data == NULL && invalid_parse.len == 0 &&
           invalid_parse.capacity == 0 && invalid_parse.owner == 0);

    AfUtf8Buffer invalid_snap = {(const uint8_t *)1, 1, 1, 1};
    assert(af_session_snap_json(session, 0.0, 0.0, 0.0, &invalid_snap) ==
           AF_STATUS_INVALID_ARGUMENT);
    assert(invalid_snap.data == NULL && invalid_snap.len == 0 &&
           invalid_snap.capacity == 0 && invalid_snap.owner == 0);

    assert(af_session_destroy(session) == AF_STATUS_OK);
    assert(af_session_destroy(session) == AF_STATUS_INVALID_HANDLE);
    puts("PASS af-ffi C smoke ABI=0.7.0 negotiate+status+parse+snap+select+2xLINE+undo+redo+save+open+delta+free");
    return 0;
}
