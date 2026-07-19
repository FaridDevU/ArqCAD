use std::mem::{MaybeUninit, align_of, offset_of, size_of};
use std::ptr;
use std::slice;
use std::thread;

use af_ffi::{
    AF_STATUS_INVALID_ARGUMENT, AF_STATUS_INVALID_HANDLE, AF_STATUS_INVALID_UTF8, AF_STATUS_OK,
    AF_STATUS_WRONG_THREAD, AfByteBuffer, AfF32Buffer, AfSessionHandle, AfUtf8Buffer,
    af_byte_buffer_free, af_f32_buffer_free, af_session_create, af_session_destroy,
    af_session_execute_json, af_session_open_arcf_json, af_session_parse_input_json,
    af_session_render_delta, af_session_render_full_json, af_session_render_vertices,
    af_session_save_arcf, af_session_select_at_json, af_session_snap_json, af_utf8_buffer_free,
};
use serde_json::{Value, json};

const LINE: &[u8] = b"LINE";
const LINE_ARGS: &[u8] = br#"{"p1":[0,0],"p2":[1,1]}"#;

fn create_session() -> AfSessionHandle {
    let mut handle = 0;
    // SAFETY: `handle` is aligned writable storage for one handle.
    assert_eq!(unsafe { af_session_create(&mut handle) }, AF_STATUS_OK);
    assert_ne!(handle, 0);
    handle
}

fn execute(handle: AfSessionHandle, command: &[u8], args: &[u8]) -> (u32, AfUtf8Buffer) {
    let mut output = MaybeUninit::<AfUtf8Buffer>::uninit();
    // SAFETY: both slices remain readable and immutable for the call, and
    // `output` is disjoint aligned writable storage for one buffer.
    let status = unsafe {
        af_session_execute_json(
            handle,
            command.as_ptr(),
            command.len(),
            args.as_ptr(),
            args.len(),
            output.as_mut_ptr(),
        )
    };
    // SAFETY: every call with a non-null output initializes it to the canonical
    // empty value before any fallible work.
    (status, unsafe { output.assume_init() })
}

fn save(handle: AfSessionHandle) -> (u32, AfByteBuffer) {
    let mut output = MaybeUninit::<AfByteBuffer>::uninit();
    // SAFETY: `output` is aligned writable storage for one byte buffer.
    let status = unsafe { af_session_save_arcf(handle, output.as_mut_ptr()) };
    // SAFETY: every call with a non-null output initializes it first.
    (status, unsafe { output.assume_init() })
}

fn open(handle: AfSessionHandle, bytes: &[u8]) -> (u32, AfUtf8Buffer) {
    let mut output = MaybeUninit::<AfUtf8Buffer>::uninit();
    // SAFETY: input remains immutable and readable for the call; output is
    // disjoint aligned writable storage for one UTF-8 buffer.
    let status = unsafe {
        af_session_open_arcf_json(handle, bytes.as_ptr(), bytes.len(), output.as_mut_ptr())
    };
    // SAFETY: every call with a non-null output initializes it first.
    (status, unsafe { output.assume_init() })
}

fn parse_point(
    handle: AfSessionHandle,
    input: &[u8],
    has_base: u8,
    base_x: f64,
    base_y: f64,
) -> (u32, AfUtf8Buffer) {
    let mut output = MaybeUninit::<AfUtf8Buffer>::uninit();
    // SAFETY: input is readable and immutable for the call; output is disjoint,
    // aligned writable storage for one buffer.
    let status = unsafe {
        af_session_parse_input_json(
            handle,
            input.as_ptr(),
            input.len(),
            has_base,
            base_x,
            base_y,
            output.as_mut_ptr(),
        )
    };
    // SAFETY: every call with non-null output initializes it first.
    (status, unsafe { output.assume_init() })
}

fn snap(handle: AfSessionHandle, x: f64, y: f64, radius: f64) -> (u32, AfUtf8Buffer) {
    let mut output = MaybeUninit::<AfUtf8Buffer>::uninit();
    // SAFETY: output is aligned writable storage for one buffer.
    let status = unsafe { af_session_snap_json(handle, x, y, radius, output.as_mut_ptr()) };
    // SAFETY: every call with non-null output initializes it first.
    (status, unsafe { output.assume_init() })
}

fn select_at(handle: AfSessionHandle, x: f64, y: f64, tolerance: f64) -> (u32, AfUtf8Buffer) {
    let mut output = MaybeUninit::<AfUtf8Buffer>::uninit();
    // SAFETY: output is aligned writable storage for one buffer.
    let status = unsafe { af_session_select_at_json(handle, x, y, tolerance, output.as_mut_ptr()) };
    // SAFETY: every call with non-null output initializes it first.
    (status, unsafe { output.assume_init() })
}

fn render_delta(handle: AfSessionHandle) -> (u32, AfUtf8Buffer, AfF32Buffer) {
    let mut control = MaybeUninit::<AfUtf8Buffer>::uninit();
    let mut vertices = MaybeUninit::<AfF32Buffer>::uninit();
    // SAFETY: outputs are disjoint aligned writable storage for their values.
    let status =
        unsafe { af_session_render_delta(handle, control.as_mut_ptr(), vertices.as_mut_ptr()) };
    // SAFETY: every call with non-null outputs initializes both first.
    (status, unsafe { control.assume_init() }, unsafe {
        vertices.assume_init()
    })
}

fn render(handle: AfSessionHandle) -> (u32, AfUtf8Buffer) {
    let mut output = MaybeUninit::<AfUtf8Buffer>::uninit();
    // SAFETY: `output` is aligned writable storage for one buffer.
    let status = unsafe { af_session_render_full_json(handle, output.as_mut_ptr()) };
    // SAFETY: every call with a non-null output initializes it before fallible work.
    (status, unsafe { output.assume_init() })
}

fn render_vertices(handle: AfSessionHandle) -> (u32, AfF32Buffer) {
    let mut output = MaybeUninit::<AfF32Buffer>::uninit();
    // SAFETY: `output` is aligned writable storage for one f32 buffer.
    let status = unsafe { af_session_render_vertices(handle, output.as_mut_ptr()) };
    // SAFETY: every call with a non-null output initializes it before fallible work.
    (status, unsafe { output.assume_init() })
}

fn text(buffer: &AfUtf8Buffer) -> &str {
    assert!(!buffer.data.is_null());
    assert!(buffer.capacity >= buffer.len);
    // SAFETY: a live returned buffer owns at least `len` initialized bytes and
    // remains immutable until its matching free.
    let bytes = unsafe { slice::from_raw_parts(buffer.data, buffer.len) };
    std::str::from_utf8(bytes).expect("ABI result must be valid UTF-8")
}

fn byte_values(buffer: &AfByteBuffer) -> &[u8] {
    assert!(!buffer.data.is_null());
    assert_ne!(buffer.owner, 0);
    assert!(buffer.capacity >= buffer.len);
    // SAFETY: a live returned buffer owns at least `len` initialized bytes and
    // remains immutable until its matching free.
    unsafe { slice::from_raw_parts(buffer.data, buffer.len) }
}

fn float_values(buffer: &AfF32Buffer) -> &[f32] {
    assert!(!buffer.data.is_null());
    assert_ne!(buffer.owner, 0);
    assert!(buffer.capacity >= buffer.len);
    assert_eq!((buffer.data as usize) % align_of::<f32>(), 0);
    // SAFETY: a live returned buffer owns at least `len` initialized f32 values
    // and remains immutable until its matching free.
    unsafe { slice::from_raw_parts(buffer.data, buffer.len) }
}

fn empty_buffer() -> AfUtf8Buffer {
    AfUtf8Buffer {
        data: ptr::null(),
        len: 0,
        capacity: 0,
        owner: 0,
    }
}

fn empty_byte_buffer() -> AfByteBuffer {
    AfByteBuffer {
        data: ptr::null(),
        len: 0,
        capacity: 0,
        owner: 0,
    }
}

fn copy_buffer(buffer: &AfUtf8Buffer) -> AfUtf8Buffer {
    AfUtf8Buffer {
        data: buffer.data,
        len: buffer.len,
        capacity: buffer.capacity,
        owner: buffer.owner,
    }
}

fn copy_byte_buffer(buffer: &AfByteBuffer) -> AfByteBuffer {
    AfByteBuffer {
        data: buffer.data,
        len: buffer.len,
        capacity: buffer.capacity,
        owner: buffer.owner,
    }
}

fn copy_f32_buffer(buffer: &AfF32Buffer) -> AfF32Buffer {
    AfF32Buffer {
        data: buffer.data,
        len: buffer.len,
        capacity: buffer.capacity,
        owner: buffer.owner,
    }
}

fn assert_empty(buffer: &AfUtf8Buffer) {
    assert!(buffer.data.is_null());
    assert_eq!(buffer.len, 0);
    assert_eq!(buffer.capacity, 0);
    assert_eq!(buffer.owner, 0);
}

fn assert_byte_empty(buffer: &AfByteBuffer) {
    assert!(buffer.data.is_null());
    assert_eq!(buffer.len, 0);
    assert_eq!(buffer.capacity, 0);
    assert_eq!(buffer.owner, 0);
}

fn assert_f32_empty(buffer: &AfF32Buffer) {
    assert!(buffer.data.is_null());
    assert_eq!(buffer.len, 0);
    assert_eq!(buffer.capacity, 0);
    assert_eq!(buffer.owner, 0);
}

fn created_count(json: &str) -> usize {
    let marker = "\"created\":[";
    let start = json.find(marker).expect("created array") + marker.len();
    let body = &json[start..json[start..].find(']').expect("created array end") + start];
    if body.is_empty() {
        0
    } else {
        body.split(',').count()
    }
}

fn parse_json(buffer: &AfUtf8Buffer) -> Value {
    serde_json::from_str(text(buffer)).expect("ABI result must be valid JSON")
}

#[test]
fn buffer_has_the_declared_four_word_c_layout() {
    assert_eq!(size_of::<AfUtf8Buffer>(), 4 * size_of::<usize>());
    assert_eq!(align_of::<AfUtf8Buffer>(), align_of::<usize>());
    assert_eq!(offset_of!(AfUtf8Buffer, data), 0);
    assert_eq!(offset_of!(AfUtf8Buffer, len), size_of::<usize>());
    assert_eq!(offset_of!(AfUtf8Buffer, capacity), 2 * size_of::<usize>());
    assert_eq!(offset_of!(AfUtf8Buffer, owner), 3 * size_of::<usize>());

    assert_eq!(size_of::<AfByteBuffer>(), 4 * size_of::<usize>());
    assert_eq!(align_of::<AfByteBuffer>(), align_of::<usize>());
    assert_eq!(offset_of!(AfByteBuffer, data), 0);
    assert_eq!(offset_of!(AfByteBuffer, len), size_of::<usize>());
    assert_eq!(offset_of!(AfByteBuffer, capacity), 2 * size_of::<usize>());
    assert_eq!(offset_of!(AfByteBuffer, owner), 3 * size_of::<usize>());

    assert_eq!(size_of::<AfF32Buffer>(), 4 * size_of::<usize>());
    assert_eq!(align_of::<AfF32Buffer>(), align_of::<usize>());
    assert_eq!(offset_of!(AfF32Buffer, data), 0);
    assert_eq!(offset_of!(AfF32Buffer, len), size_of::<usize>());
    assert_eq!(offset_of!(AfF32Buffer, capacity), 2 * size_of::<usize>());
    assert_eq!(offset_of!(AfF32Buffer, owner), 3 * size_of::<usize>());
}

#[test]
fn save_open_roundtrip_preserves_lines_history_sequence_and_next_id() {
    let source = create_session();
    let lines: [&[u8]; 4] = [
        br#"{"p1":[0,0],"p2":[10,0]}"#,
        br#"{"p1":[10,0],"p2":[10,5]}"#,
        br#"{"p1":[10,5],"p2":[0,5]}"#,
        br#"{"p1":[0,5],"p2":[0,0]}"#,
    ];
    let mut ids = Vec::new();
    for (tx_seq, args) in lines.into_iter().enumerate() {
        let (status, mut line) = execute(source, LINE, args);
        assert_eq!(status, AF_STATUS_OK);
        let result = parse_json(&line);
        assert_eq!(result["ok"]["txSeq"], tx_seq as u64);
        ids.push(
            result["ok"]["created"][0]
                .as_u64()
                .expect("LINE ID must be u64"),
        );
        // SAFETY: exact live metadata returned above.
        assert_eq!(unsafe { af_utf8_buffer_free(&mut line) }, AF_STATUS_OK);
    }
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));

    let (status, mut saved) = save(source);
    assert_eq!(status, AF_STATUS_OK);
    assert!(!byte_values(&saved).is_empty());

    let target = create_session();
    let (status, mut discarded) = execute(target, LINE, br#"{"p1":[99,99],"p2":[100,100]}"#);
    assert_eq!(status, AF_STATUS_OK);
    // SAFETY: exact live metadata returned above.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut discarded) }, AF_STATUS_OK);
    let (status, mut discarded_control, mut discarded_vertices) = render_delta(target);
    assert_eq!(status, AF_STATUS_OK);
    // SAFETY: exact live metadata returned above.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut discarded_control) },
        AF_STATUS_OK
    );
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut discarded_vertices) },
        AF_STATUS_OK
    );

    // The native save allocation remains live and is consumed directly as the
    // open input; no host-side reserialization or UTF-8 conversion is involved.
    let (status, mut opened) = open(target, byte_values(&saved));
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&opened), json!({"ok": []}));

    let (status, mut control, mut vertices) = render_delta(target);
    assert_eq!(status, AF_STATUS_OK);
    let delta = parse_json(&control);
    let mut reopened_ids = Vec::new();
    for batch in delta["upserts"].as_array().expect("upserts array") {
        for strip in batch["strips"].as_array().expect("strips array") {
            reopened_ids.push(strip["entity"].as_u64().expect("strip entity ID"));
            assert_eq!(strip["count"], 2);
        }
    }
    assert_eq!(reopened_ids, ids, "persistent ID/order must roundtrip");
    assert_eq!(
        float_values(&vertices),
        [
            0.0, 0.0, 10.0, 0.0, 10.0, 0.0, 10.0, 5.0, 10.0, 5.0, 0.0, 5.0, 0.0, 5.0, 0.0, 0.0,
        ]
    );

    for command in [b"UNDO".as_slice(), b"REDO".as_slice()] {
        let (status, mut result) = execute(target, command, b"null");
        assert_eq!(status, AF_STATUS_OK);
        let result_json = parse_json(&result);
        assert!(result_json["ok"]["txSeq"].is_null());
        // SAFETY: exact live metadata returned above.
        assert_eq!(unsafe { af_utf8_buffer_free(&mut result) }, AF_STATUS_OK);

        let (status, mut empty_control, mut empty_vertices) = render_delta(target);
        assert_eq!(status, AF_STATUS_OK);
        let empty_delta = parse_json(&empty_control);
        assert_eq!(empty_delta["upserts"], json!([]));
        assert_eq!(empty_delta["removes"], json!([]));
        assert_f32_empty(&empty_vertices);
        // SAFETY: exact live metadata, including canonical empty vertices.
        assert_eq!(
            unsafe { af_utf8_buffer_free(&mut empty_control) },
            AF_STATUS_OK
        );
        assert_eq!(
            unsafe { af_f32_buffer_free(&mut empty_vertices) },
            AF_STATUS_OK
        );
    }

    let (status, mut next) = execute(target, LINE, br#"{"p1":[20,20],"p2":[30,30]}"#);
    assert_eq!(status, AF_STATUS_OK);
    let next_json = parse_json(&next);
    assert_eq!(next_json["ok"]["txSeq"], 0);
    let next_id = next_json["ok"]["created"][0]
        .as_u64()
        .expect("continued LINE ID must be u64");
    assert_eq!(Some(next_id), ids.iter().copied().max().map(|id| id + 1));

    let (status, mut before_corrupt) = save(target);
    assert_eq!(status, AF_STATUS_OK);
    let before_bytes = byte_values(&before_corrupt).to_vec();
    let (status, mut corrupt) = open(target, b"not an arcf document");
    assert_eq!(status, AF_STATUS_OK);
    let corrupt_json = parse_json(&corrupt);
    let corrupt_root = corrupt_json.as_object().expect("open envelope object");
    assert_eq!(corrupt_root.len(), 1);
    let error = corrupt_root["error"].as_object().expect("ApiError object");
    assert!(error["code"].as_str().is_some_and(|code| !code.is_empty()));
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
    );
    assert!(
        error
            .keys()
            .all(|key| matches!(key.as_str(), "code" | "message" | "detail"))
    );
    let (status, mut after_corrupt) = save(target);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(byte_values(&after_corrupt), before_bytes);

    let (status, mut pending_control, mut pending_vertices) = render_delta(target);
    assert_eq!(status, AF_STATUS_OK);
    let pending_delta = parse_json(&pending_control);
    let mut pending_ids = Vec::new();
    for batch in pending_delta["upserts"].as_array().expect("upserts array") {
        for strip in batch["strips"].as_array().expect("strips array") {
            pending_ids.push(strip["entity"].as_u64().expect("strip entity ID"));
        }
    }
    let mut expected_ids = ids.clone();
    expected_ids.push(next_id);
    assert_eq!(pending_ids, expected_ids);
    assert_eq!(
        &float_values(&pending_vertices)[float_values(&pending_vertices).len() - 4..],
        [20.0, 20.0, 30.0, 30.0]
    );

    let (status, mut undo) = execute(target, b"UNDO", b"null");
    assert_eq!(status, AF_STATUS_OK);
    assert!(parse_json(&undo)["ok"]["txSeq"].is_null());
    let (status, mut after_undo) = render_vertices(target);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(float_values(&after_undo), float_values(&vertices));

    let (status, mut redo) = execute(target, b"REDO", b"null");
    assert_eq!(status, AF_STATUS_OK);
    assert!(parse_json(&redo)["ok"]["txSeq"].is_null());
    let (status, mut after_redo) = render_vertices(target);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(float_values(&after_redo), float_values(&pending_vertices));

    let (status, mut continued) = execute(target, LINE, br#"{"p1":[40,40],"p2":[50,50]}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&continued)["ok"]["txSeq"], 1);

    // SAFETY: every buffer retains exact live metadata.
    assert_eq!(unsafe { af_byte_buffer_free(&mut saved) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_utf8_buffer_free(&mut opened) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_utf8_buffer_free(&mut control) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_f32_buffer_free(&mut vertices) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_utf8_buffer_free(&mut next) }, AF_STATUS_OK);
    assert_eq!(
        unsafe { af_byte_buffer_free(&mut before_corrupt) },
        AF_STATUS_OK
    );
    assert_eq!(unsafe { af_utf8_buffer_free(&mut corrupt) }, AF_STATUS_OK);
    assert_eq!(
        unsafe { af_byte_buffer_free(&mut after_corrupt) },
        AF_STATUS_OK
    );
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut pending_control) },
        AF_STATUS_OK
    );
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut pending_vertices) },
        AF_STATUS_OK
    );
    assert_eq!(unsafe { af_utf8_buffer_free(&mut undo) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_f32_buffer_free(&mut after_undo) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_utf8_buffer_free(&mut redo) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_f32_buffer_free(&mut after_redo) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_utf8_buffer_free(&mut continued) }, AF_STATUS_OK);
    assert_eq!(af_session_destroy(source), AF_STATUS_OK);
    assert_eq!(af_session_destroy(target), AF_STATUS_OK);
}

#[test]
fn save_destroy_open_roundtrips_each_history_checkpoint() {
    let cases: [(&str, &[&[u8]], usize); 3] = [
        ("commit", &[], 2),
        ("undo", &[b"UNDO".as_slice()], 1),
        ("redo", &[b"UNDO".as_slice(), b"REDO".as_slice()], 2),
    ];

    for (label, checkpoint_commands, expected_lines) in cases {
        let source = create_session();
        let mut ids = Vec::new();
        for args in [
            br#"{"p1":[0,0],"p2":[1,1]}"#.as_slice(),
            br#"{"p1":[2,2],"p2":[3,3]}"#.as_slice(),
        ] {
            let (status, mut line) = execute(source, LINE, args);
            assert_eq!(status, AF_STATUS_OK, "{label}");
            ids.push(parse_json(&line)["ok"]["created"][0].as_u64().unwrap());
            assert_eq!(unsafe { af_utf8_buffer_free(&mut line) }, AF_STATUS_OK);
        }
        for command in checkpoint_commands {
            let (status, mut outcome) = execute(source, command, b"null");
            assert_eq!(status, AF_STATUS_OK, "{label}");
            assert!(parse_json(&outcome)["ok"]["txSeq"].is_null());
            assert_eq!(unsafe { af_utf8_buffer_free(&mut outcome) }, AF_STATUS_OK);
        }

        let (status, mut before) = render(source);
        assert_eq!(status, AF_STATUS_OK, "{label}");
        let expected = parse_json(&before);
        assert_eq!(
            expected["vertices"].as_array().unwrap().len(),
            expected_lines * 4,
            "{label}"
        );
        assert_eq!(unsafe { af_utf8_buffer_free(&mut before) }, AF_STATUS_OK);

        let (status, mut saved) = save(source);
        assert_eq!(status, AF_STATUS_OK, "{label}");
        assert_eq!(af_session_destroy(source), AF_STATUS_OK);

        let target = create_session();
        let (status, mut opened) = open(target, byte_values(&saved));
        assert_eq!(status, AF_STATUS_OK, "{label}");
        assert_eq!(parse_json(&opened), json!({"ok": []}), "{label}");
        assert_eq!(unsafe { af_utf8_buffer_free(&mut opened) }, AF_STATUS_OK);
        assert_eq!(unsafe { af_byte_buffer_free(&mut saved) }, AF_STATUS_OK);

        let (status, mut after) = render(target);
        assert_eq!(status, AF_STATUS_OK, "{label}");
        assert_eq!(parse_json(&after), expected, "{label}");
        assert_eq!(unsafe { af_utf8_buffer_free(&mut after) }, AF_STATUS_OK);

        for command in [b"REDO".as_slice(), b"UNDO".as_slice()] {
            let (status, mut empty_history) = execute(target, command, b"null");
            assert_eq!(status, AF_STATUS_OK, "{label}");
            assert!(parse_json(&empty_history)["error"].is_object(), "{label}");
            assert_eq!(
                unsafe { af_utf8_buffer_free(&mut empty_history) },
                AF_STATUS_OK
            );
        }

        let (status, mut unchanged) = render(target);
        assert_eq!(status, AF_STATUS_OK, "{label}");
        assert_eq!(parse_json(&unchanged), expected, "{label}");
        assert_eq!(unsafe { af_utf8_buffer_free(&mut unchanged) }, AF_STATUS_OK);

        let (status, mut first) = execute(target, LINE, br#"{"p1":[9,9],"p2":[10,10]}"#);
        assert_eq!(status, AF_STATUS_OK, "{label}");
        let first_json = parse_json(&first);
        assert_eq!(first_json["ok"]["txSeq"], 0, "{label}");
        assert_eq!(
            first_json["ok"]["created"][0].as_u64().unwrap(),
            ids.iter().copied().max().unwrap() + 1,
            "{label} nextObjectId"
        );
        assert_eq!(unsafe { af_utf8_buffer_free(&mut first) }, AF_STATUS_OK);
        assert_eq!(af_session_destroy(target), AF_STATUS_OK);
    }
}

#[test]
fn persistence_validates_precedence_thread_and_typed_byte_ownership() {
    let handle = create_session();

    // SAFETY: null outputs are explicitly checked error cases.
    assert_eq!(
        unsafe { af_session_save_arcf(handle, ptr::null_mut()) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { af_session_open_arcf_json(handle, ptr::null(), 0, ptr::null_mut()) },
        AF_STATUS_INVALID_ARGUMENT
    );

    let mut bytes = AfByteBuffer {
        data: ptr::dangling(),
        len: 7,
        capacity: 9,
        owner: 0,
    };
    // SAFETY: output is valid exclusive storage; invalid handle is checked.
    assert_eq!(
        unsafe { af_session_save_arcf(0, &mut bytes) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_byte_empty(&bytes);

    let mut result = AfUtf8Buffer {
        data: ptr::dangling(),
        len: 7,
        capacity: 9,
        owner: 0,
    };
    // SAFETY: invalid handle has precedence over the invalid input shape.
    assert_eq!(
        unsafe { af_session_open_arcf_json(0, ptr::null(), 1, &mut result) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_empty(&result);
    // SAFETY: null with positive length is an explicitly checked shape error.
    assert_eq!(
        unsafe { af_session_open_arcf_json(handle, ptr::null(), 1, &mut result) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_empty(&result);
    // SAFETY: oversized length is rejected before the dangling pointer is read.
    assert_eq!(
        unsafe {
            af_session_open_arcf_json(
                handle,
                ptr::dangling(),
                isize::MAX as usize + 1,
                &mut result,
            )
        },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_empty(&result);

    // Null plus zero length is a valid byte range and yields a product error.
    let (status, mut empty_document) = open(handle, &[]);
    assert_eq!(status, AF_STATUS_OK);
    assert!(parse_json(&empty_document).get("error").is_some());
    // SAFETY: exact live metadata returned above.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut empty_document) },
        AF_STATUS_OK
    );

    let (save_status, save_empty, open_status, open_empty) = thread::spawn(move || {
        let (save_status, saved) = save(handle);
        let mut opened = MaybeUninit::<AfUtf8Buffer>::uninit();
        // SAFETY: output is valid storage. Wrong thread precedes invalid input.
        let open_status =
            unsafe { af_session_open_arcf_json(handle, ptr::null(), 1, opened.as_mut_ptr()) };
        // SAFETY: the non-null output was initialized before thread validation.
        let opened = unsafe { opened.assume_init() };
        (
            save_status,
            saved.owner == 0 && saved.data.is_null(),
            open_status,
            opened.owner == 0 && opened.data.is_null(),
        )
    })
    .join()
    .expect("persistence thread should not panic");
    assert_eq!(save_status, AF_STATUS_WRONG_THREAD);
    assert!(save_empty);
    assert_eq!(open_status, AF_STATUS_WRONG_THREAD);
    assert!(open_empty);

    let (status, mut saved) = save(handle);
    assert_eq!(status, AF_STATUS_OK);
    let (status, mut opened) = open(handle, byte_values(&saved));
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&opened), json!({"ok": []}));

    let mut bytes_as_utf8 = AfUtf8Buffer {
        data: saved.data,
        len: saved.len,
        capacity: saved.capacity,
        owner: saved.owner,
    };
    let mut bytes_as_f32 = AfF32Buffer {
        data: saved.data.cast(),
        len: saved.len,
        capacity: saved.capacity,
        owner: saved.owner,
    };
    let mut utf8_as_bytes = AfByteBuffer {
        data: opened.data,
        len: opened.len,
        capacity: opened.capacity,
        owner: opened.owner,
    };
    // SAFETY: initialized metadata names live owners of the wrong checked type.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut bytes_as_utf8) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut bytes_as_f32) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { af_byte_buffer_free(&mut utf8_as_bytes) },
        AF_STATUS_INVALID_ARGUMENT
    );

    let mut tampered = copy_byte_buffer(&saved);
    tampered.len -= 1;
    let tampered_before = copy_byte_buffer(&tampered);
    // SAFETY: mismatched metadata is checked without releasing the allocation.
    assert_eq!(
        unsafe { af_byte_buffer_free(&mut tampered) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(tampered, tampered_before);

    let mut stale = copy_byte_buffer(&saved);
    let stale_before = copy_byte_buffer(&stale);
    // SAFETY: exact metadata releases the live allocation; stale is then
    // checked without dereferencing its payload.
    assert_eq!(unsafe { af_byte_buffer_free(&mut saved) }, AF_STATUS_OK);
    assert_byte_empty(&saved);
    assert_eq!(
        unsafe { af_byte_buffer_free(&mut stale) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_eq!(stale, stale_before);
    // SAFETY: wrong-type attempts preserved the UTF-8 allocation.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut opened) }, AF_STATUS_OK);

    let mut empty = empty_byte_buffer();
    // SAFETY: canonical empty is initialized exclusive storage.
    assert_eq!(unsafe { af_byte_buffer_free(&mut empty) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_byte_buffer_free(&mut empty) }, AF_STATUS_OK);
    // SAFETY: null is the documented idempotent no-op.
    assert_eq!(
        unsafe { af_byte_buffer_free(ptr::null_mut()) },
        AF_STATUS_OK
    );

    let (status, cross_thread) = save(handle);
    assert_eq!(status, AF_STATUS_OK);
    let raw = (
        cross_thread.data as usize,
        cross_thread.len,
        cross_thread.capacity,
        cross_thread.owner,
    );
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
    let (free_status, was_zeroed) = thread::spawn(move || {
        let mut moved = AfByteBuffer {
            data: raw.0 as *const u8,
            len: raw.1,
            capacity: raw.2,
            owner: raw.3,
        };
        // SAFETY: ownership moved to this thread with exact live metadata.
        let status = unsafe { af_byte_buffer_free(&mut moved) };
        (status, moved.owner == 0 && moved.data.is_null())
    })
    .join()
    .expect("byte free thread should not panic");
    assert_eq!(free_status, AF_STATUS_OK);
    assert!(was_zeroed);
}

#[test]
fn parse_snap_and_select_keep_line_transactions_consecutive() {
    let handle = create_session();

    let (status, mut absolute) = parse_point(handle, b"12,34", 0, f64::NAN, f64::INFINITY);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(text(&absolute), r#"{"ok":{"point":[12.0,34.0]}}"#);

    let (status, mut relative) = parse_point(handle, b"@2,3", 1, 10.0, 20.0);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(
        parse_json(&relative),
        json!({"ok": {"point": [12.0, 23.0]}})
    );

    let (status, mut polar) = parse_point(handle, b"@10<90", 1, 0.0, 0.0);
    assert_eq!(status, AF_STATUS_OK);
    let polar_json = parse_json(&polar);
    let polar_point = polar_json["ok"]["point"]
        .as_array()
        .expect("polar point array");
    assert!(polar_point[0].as_f64().expect("polar x").abs() < 1e-9);
    assert!((polar_point[1].as_f64().expect("polar y") - 10.0).abs() < 1e-9);

    let (status, mut missing_base) = parse_point(handle, b"@2,3", 0, 0.0, 0.0);
    assert_eq!(status, AF_STATUS_OK);
    let error = parse_json(&missing_base);
    assert_eq!(error["error"]["code"], "parse_error");
    assert!(error["error"]["detail"]["pos"].is_u64());

    let (status, mut not_a_point) = parse_point(handle, b"LINE", 0, 0.0, 0.0);
    assert_eq!(status, AF_STATUS_OK);
    let error = parse_json(&not_a_point);
    assert_eq!(error["error"]["code"], "not_a_point");
    assert!(error["error"].get("detail").is_none());

    let (status, mut empty_snaps) = snap(handle, 0.0, 0.0, 1.0);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&empty_snaps), json!([]));

    let (status, mut empty_selection) = select_at(handle, 0.0, 0.0, 1.0);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&empty_selection), json!([]));

    let (status, mut line) = execute(handle, LINE, br#"{"p1":[0,0],"p2":[10,20]}"#);
    assert_eq!(status, AF_STATUS_OK);
    let line_json = parse_json(&line);
    assert_eq!(line_json["ok"]["txSeq"], 0);
    let entity = line_json["ok"]["created"][0].as_u64().expect("LINE entity");

    let (status, mut snaps) = snap(handle, 0.1, 0.0, 1.0);
    assert_eq!(status, AF_STATUS_OK);
    let snaps_json = parse_json(&snaps);
    let first = &snaps_json.as_array().expect("snap array")[0];
    assert_eq!(first["point"], json!([0.0, 0.0]));
    assert_eq!(first["kind"], "endpoint");
    assert_eq!(first["entity"], entity);
    assert_eq!(first["dist"].as_f64(), Some(0.1));

    let (status, mut selected) = select_at(handle, 5.0, 10.0, 0.25);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&selected), json!([entity]));

    let (status, mut cleared) = select_at(handle, 1000.0, 1000.0, 0.25);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&cleared), json!([]));

    let (status, mut next_line) = execute(handle, LINE, br#"{"p1":[1,1],"p2":[2,2]}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&next_line)["ok"]["txSeq"], 1);

    // SAFETY: all buffers are live and retain their exact metadata.
    for buffer in [
        &mut absolute,
        &mut relative,
        &mut polar,
        &mut missing_base,
        &mut not_a_point,
        &mut empty_snaps,
        &mut empty_selection,
        &mut line,
        &mut snaps,
        &mut selected,
        &mut cleared,
        &mut next_line,
    ] {
        assert_eq!(unsafe { af_utf8_buffer_free(buffer) }, AF_STATUS_OK);
    }
    // SAFETY: the first free reset this buffer to the canonical empty value.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut empty_selection) },
        AF_STATUS_OK
    );
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}

#[test]
fn parse_snap_and_select_validate_in_contractual_precedence_and_wrong_thread() {
    let handle = create_session();
    let invalid_utf8 = [0xff];

    // SAFETY: null output is an explicitly checked error case.
    assert_eq!(
        unsafe {
            af_session_parse_input_json(handle, b"1,2".as_ptr(), 3, 0, 0.0, 0.0, ptr::null_mut())
        },
        AF_STATUS_INVALID_ARGUMENT
    );

    let mut output = AfUtf8Buffer {
        data: ptr::dangling(),
        len: 7,
        capacity: 9,
        owner: 0,
    };
    // SAFETY: output is valid exclusive storage. Invalid handle precedes the
    // invalid base flag and UTF-8 payload.
    assert_eq!(
        unsafe {
            af_session_parse_input_json(
                0,
                invalid_utf8.as_ptr(),
                invalid_utf8.len(),
                2,
                f64::NAN,
                f64::NAN,
                &mut output,
            )
        },
        AF_STATUS_INVALID_HANDLE
    );
    assert_empty(&output);

    // SAFETY: all ranges and output storage are valid and disjoint.
    assert_eq!(
        unsafe {
            af_session_parse_input_json(
                handle,
                invalid_utf8.as_ptr(),
                invalid_utf8.len(),
                2,
                0.0,
                0.0,
                &mut output,
            )
        },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_empty(&output);
    assert_eq!(
        unsafe {
            af_session_parse_input_json(
                handle,
                invalid_utf8.as_ptr(),
                invalid_utf8.len(),
                1,
                f64::NAN,
                0.0,
                &mut output,
            )
        },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_empty(&output);
    assert_eq!(
        unsafe {
            af_session_parse_input_json(
                handle,
                invalid_utf8.as_ptr(),
                invalid_utf8.len(),
                0,
                0.0,
                0.0,
                &mut output,
            )
        },
        AF_STATUS_INVALID_UTF8
    );
    assert_empty(&output);
    assert_eq!(
        unsafe { af_session_parse_input_json(handle, ptr::null(), 1, 0, 0.0, 0.0, &mut output) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_empty(&output);

    let (status, mut ignored_nonfinite_base) =
        parse_point(handle, b"1,2", 0, f64::NAN, f64::INFINITY);
    assert_eq!(status, AF_STATUS_OK);
    // SAFETY: exact live metadata returned above.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut ignored_nonfinite_base) },
        AF_STATUS_OK
    );

    for (x, y, radius) in [
        (f64::NAN, 0.0, 1.0),
        (0.0, f64::INFINITY, 1.0),
        (0.0, 0.0, 0.0),
        (0.0, 0.0, -1.0),
        (0.0, 0.0, f64::INFINITY),
    ] {
        let (status, invalid) = snap(handle, x, y, radius);
        assert_eq!(status, AF_STATUS_INVALID_ARGUMENT);
        assert_empty(&invalid);
    }
    let (status, invalid) = snap(0, f64::NAN, 0.0, 0.0);
    assert_eq!(status, AF_STATUS_INVALID_HANDLE);
    assert_empty(&invalid);

    // SAFETY: null output is an explicitly checked error case.
    assert_eq!(
        unsafe { af_session_select_at_json(handle, 0.0, 0.0, 1.0, ptr::null_mut()) },
        AF_STATUS_INVALID_ARGUMENT
    );
    let mut selection = AfUtf8Buffer {
        data: ptr::dangling(),
        len: 7,
        capacity: 9,
        owner: 0,
    };
    // SAFETY: output is valid exclusive storage. Invalid handle precedes the
    // invalid numeric arguments.
    assert_eq!(
        unsafe { af_session_select_at_json(0, f64::NAN, f64::INFINITY, 0.0, &mut selection) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_empty(&selection);
    for (x, y, tolerance) in [
        (f64::NAN, 0.0, 1.0),
        (0.0, f64::INFINITY, 1.0),
        (0.0, 0.0, 0.0),
        (0.0, 0.0, -1.0),
        (0.0, 0.0, f64::INFINITY),
    ] {
        let (status, invalid) = select_at(handle, x, y, tolerance);
        assert_eq!(status, AF_STATUS_INVALID_ARGUMENT);
        assert_empty(&invalid);
    }

    let (parse_status, parse_empty, snap_status, snap_empty, select_status, select_empty) =
        thread::spawn(move || {
            let (parse_status, parse) = parse_point(handle, b"1,2", 0, 0.0, 0.0);
            let (snap_status, snaps) = snap(handle, 0.0, 0.0, 1.0);
            let (select_status, selection) = select_at(handle, 0.0, 0.0, 1.0);
            (
                parse_status,
                parse.owner == 0 && parse.data.is_null(),
                snap_status,
                snaps.owner == 0 && snaps.data.is_null(),
                select_status,
                selection.owner == 0 && selection.data.is_null(),
            )
        })
        .join()
        .expect("query thread should not panic");
    assert_eq!(parse_status, AF_STATUS_WRONG_THREAD);
    assert!(parse_empty);
    assert_eq!(snap_status, AF_STATUS_WRONG_THREAD);
    assert!(snap_empty);
    assert_eq!(select_status, AF_STATUS_WRONG_THREAD);
    assert!(select_empty);
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);

    let (status, stale) = select_at(handle, 0.0, 0.0, 1.0);
    assert_eq!(status, AF_STATUS_INVALID_HANDLE);
    assert_empty(&stale);
}

#[test]
fn render_delta_publishes_control_and_geometry_as_one_owned_pair() {
    let handle = create_session();
    let (status, mut baseline) = render(handle);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&baseline)["batches"], json!([]));

    let (status, mut line) = execute(handle, LINE, br#"{"p1":[0,0],"p2":[10,20]}"#);
    assert_eq!(status, AF_STATUS_OK);
    let line_json = parse_json(&line);
    let entity = line_json["ok"]["created"][0].as_u64().expect("LINE entity");

    let (status, mut full_vertices) = render_vertices(handle);
    assert_eq!(status, AF_STATUS_OK);
    let (status, mut control, mut vertices) = render_delta(handle);
    assert_eq!(status, AF_STATUS_OK);
    for owner in [
        baseline.owner,
        line.owner,
        full_vertices.owner,
        control.owner,
        vertices.owner,
    ] {
        assert_ne!(owner, 0);
    }
    let mut owners = vec![
        baseline.owner,
        line.owner,
        full_vertices.owner,
        control.owner,
        vertices.owner,
    ];
    owners.sort_unstable();
    owners.dedup();
    assert_eq!(owners.len(), 5, "every live owner must be unique");

    let delta = parse_json(&control);
    let root = delta.as_object().expect("delta root object");
    assert_eq!(root.len(), 4);
    assert_eq!(delta["vertices"], json!([]));
    assert_eq!(delta["removes"], json!([]));
    assert_eq!(delta["ltscale"], 1.0);
    let upserts = delta["upserts"].as_array().expect("upserts array");
    assert_eq!(upserts.len(), 1);
    let batch = &upserts[0];
    assert!(batch["layer"].as_u64().is_some());
    assert_eq!(batch["color"], json!([255, 255, 255, 255]));
    assert!(batch["linetype"].as_u64().is_some());
    assert_eq!(batch["markers"], json!([]));
    let strips = batch["strips"].as_array().expect("strips array");
    assert_eq!(strips.len(), 1);
    assert_eq!(strips[0]["entity"], entity);
    assert_eq!(strips[0]["offset"], 0);
    assert_eq!(strips[0]["count"], 2);
    assert_eq!(strips[0]["width"], 0.25);
    assert_eq!(
        float_values(&vertices)
            .iter()
            .copied()
            .map(f32::to_bits)
            .collect::<Vec<_>>(),
        [0.0f32, 0.0, 10.0, 20.0].map(f32::to_bits)
    );
    assert_eq!(float_values(&full_vertices), float_values(&vertices));

    let (status, mut full) = render(handle);
    assert_eq!(status, AF_STATUS_OK);
    let full_json = parse_json(&full);
    assert_eq!(full_json["batches"], delta["upserts"]);
    assert_eq!(full_json["vertices"], json!([0.0, 0.0, 10.0, 20.0]));
    assert_eq!(full_json["ltscale"], delta["ltscale"]);

    let (status, mut next_control, mut next_vertices) = render_delta(handle);
    assert_eq!(status, AF_STATUS_OK);
    assert_ne!(next_control.owner, 0);
    assert_eq!(
        parse_json(&next_control),
        json!({"upserts": [], "removes": [], "vertices": [], "ltscale": 1.0})
    );
    assert_f32_empty(&next_vertices);

    let mut control_as_f32 = AfF32Buffer {
        data: control.data.cast(),
        len: control.len,
        capacity: control.capacity,
        owner: control.owner,
    };
    let mut vertices_as_utf8 = AfUtf8Buffer {
        data: vertices.data.cast(),
        len: vertices.len,
        capacity: vertices.capacity,
        owner: vertices.owner,
    };
    // SAFETY: initialized metadata names live owners of the wrong checked type.
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut control_as_f32) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut vertices_as_utf8) },
        AF_STATUS_INVALID_ARGUMENT
    );

    let mut tampered_control = copy_buffer(&control);
    tampered_control.len -= 1;
    let mut tampered_vertices = copy_f32_buffer(&vertices);
    tampered_vertices.len -= 2;
    // SAFETY: metadata mismatches are checked without releasing either owner.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut tampered_control) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut tampered_vertices) },
        AF_STATUS_INVALID_ARGUMENT
    );

    let mut stale_control = copy_buffer(&control);
    let mut stale_vertices = copy_f32_buffer(&vertices);
    // SAFETY: exact metadata frees vertices then control; stale copies are
    // checked after each owner leaves the registry.
    assert_eq!(unsafe { af_f32_buffer_free(&mut vertices) }, AF_STATUS_OK);
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut stale_vertices) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_eq!(unsafe { af_utf8_buffer_free(&mut control) }, AF_STATUS_OK);
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut stale_control) },
        AF_STATUS_INVALID_HANDLE
    );

    // SAFETY: remaining buffers are live, or canonical empty for next_vertices.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut baseline) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_utf8_buffer_free(&mut line) }, AF_STATUS_OK);
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut full_vertices) },
        AF_STATUS_OK
    );
    assert_eq!(unsafe { af_utf8_buffer_free(&mut full) }, AF_STATUS_OK);
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut next_control) },
        AF_STATUS_OK
    );
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut next_vertices) },
        AF_STATUS_OK
    );
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}

#[test]
fn render_delta_errors_empty_every_output_and_do_not_consume_the_change() {
    let handle = create_session();
    let (status, mut baseline) = render(handle);
    assert_eq!(status, AF_STATUS_OK);
    // SAFETY: baseline is live and unmodified.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut baseline) }, AF_STATUS_OK);
    let (status, mut line) = execute(handle, LINE, br#"{"p1":[0,0],"p2":[10,20]}"#);
    assert_eq!(status, AF_STATUS_OK);
    // SAFETY: LINE result is live and unmodified.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut line) }, AF_STATUS_OK);

    let mut control = AfUtf8Buffer {
        data: ptr::dangling(),
        len: 7,
        capacity: 9,
        owner: 0,
    };
    let mut vertices = AfF32Buffer {
        data: ptr::dangling(),
        len: 2,
        capacity: 2,
        owner: 0,
    };
    // SAFETY: the non-null output is valid exclusive storage; each null output
    // is an explicitly checked error case.
    assert_eq!(
        unsafe { af_session_render_delta(handle, ptr::null_mut(), &mut vertices) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_f32_empty(&vertices);
    assert_eq!(
        unsafe { af_session_render_delta(handle, &mut control, ptr::null_mut()) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_empty(&control);

    control.len = 1;
    vertices.len = 2;
    // SAFETY: both outputs are initialized exclusive storage with owner zero;
    // invalid handle is an explicitly checked case.
    assert_eq!(
        unsafe { af_session_render_delta(0, &mut control, &mut vertices) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_empty(&control);
    assert_f32_empty(&vertices);

    let (wrong_status, both_empty) = thread::spawn(move || {
        let (status, control, vertices) = render_delta(handle);
        (
            status,
            control.owner == 0
                && control.data.is_null()
                && vertices.owner == 0
                && vertices.data.is_null(),
        )
    })
    .join()
    .expect("render delta thread should not panic");
    assert_eq!(wrong_status, AF_STATUS_WRONG_THREAD);
    assert!(both_empty);

    let (status, mut pending_control, mut pending_vertices) = render_delta(handle);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(
        parse_json(&pending_control)["upserts"]
            .as_array()
            .expect("pending upserts")
            .len(),
        1
    );
    assert_eq!(float_values(&pending_vertices), [0.0, 0.0, 10.0, 20.0]);
    // SAFETY: exact live metadata for both allocations.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut pending_control) },
        AF_STATUS_OK
    );
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut pending_vertices) },
        AF_STATUS_OK
    );
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);

    let (status, stale_control, stale_vertices) = render_delta(handle);
    assert_eq!(status, AF_STATUS_INVALID_HANDLE);
    assert_empty(&stale_control);
    assert_f32_empty(&stale_vertices);
}

#[test]
fn render_checks_output_handle_and_owner_thread_without_losing_session() {
    let handle = create_session();

    // SAFETY: null output is an explicitly checked error case.
    assert_eq!(
        unsafe { af_session_render_full_json(handle, ptr::null_mut()) },
        AF_STATUS_INVALID_ARGUMENT
    );

    let mut output = AfUtf8Buffer {
        data: ptr::dangling(),
        len: 7,
        capacity: 9,
        owner: 0,
    };
    // SAFETY: `output` is valid exclusive storage and owner zero cannot name a
    // live allocation; invalid handle is an explicitly checked case.
    assert_eq!(
        unsafe { af_session_render_full_json(0, &mut output) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_empty(&output);

    let (status, was_empty) = thread::spawn(move || {
        let (status, output) = render(handle);
        (status, output.owner == 0 && output.data.is_null())
    })
    .join()
    .expect("render thread should not panic");
    assert_eq!(status, AF_STATUS_WRONG_THREAD);
    assert!(was_empty);

    let (status, mut snapshot) = render(handle);
    assert_eq!(status, AF_STATUS_OK);
    // SAFETY: `snapshot` is the live initialized buffer returned above.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut snapshot) }, AF_STATUS_OK);
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);

    output.len = 1;
    output.capacity = 1;
    // SAFETY: `output` is initialized exclusive storage with owner zero, and
    // the stale handle is an explicitly checked case.
    assert_eq!(
        unsafe { af_session_render_full_json(handle, &mut output) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_empty(&output);
}

#[test]
fn render_vertices_checks_output_handle_and_owner_thread_without_losing_session() {
    let handle = create_session();

    // SAFETY: null output is an explicitly checked error case.
    assert_eq!(
        unsafe { af_session_render_vertices(handle, ptr::null_mut()) },
        AF_STATUS_INVALID_ARGUMENT
    );

    let mut output = AfF32Buffer {
        data: ptr::dangling(),
        len: 2,
        capacity: 2,
        owner: 0,
    };
    // SAFETY: `output` is valid exclusive storage with no live owner, and the
    // invalid handle is an explicitly checked case.
    assert_eq!(
        unsafe { af_session_render_vertices(0, &mut output) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_f32_empty(&output);

    let (status, was_empty) = thread::spawn(move || {
        let (status, output) = render_vertices(handle);
        (status, output.owner == 0 && output.data.is_null())
    })
    .join()
    .expect("render vertices thread should not panic");
    assert_eq!(status, AF_STATUS_WRONG_THREAD);
    assert!(was_empty);

    let (status, mut vertices) = render_vertices(handle);
    assert_eq!(status, AF_STATUS_OK);
    assert_f32_empty(&vertices);
    // SAFETY: canonical empty is initialized exclusive storage.
    assert_eq!(unsafe { af_f32_buffer_free(&mut vertices) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_f32_buffer_free(&mut vertices) }, AF_STATUS_OK);
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);

    output.len = 2;
    output.capacity = 2;
    // SAFETY: `output` has no live owner and the stale handle is checked.
    assert_eq!(
        unsafe { af_session_render_vertices(handle, &mut output) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_f32_empty(&output);
}

#[test]
fn render_empty_snapshot_has_the_plain_render_view_schema() {
    let handle = create_session();
    let (status, mut snapshot) = render(handle);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(
        parse_json(&snapshot),
        json!({"batches": [], "vertices": [], "ltscale": 1.0})
    );
    // SAFETY: `snapshot` is live and unmodified.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut snapshot) }, AF_STATUS_OK);
    assert_empty(&snapshot);
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}

#[test]
fn render_line_preserves_entity_vertices_and_transaction_sequence() {
    let handle = create_session();
    let (line_status, mut line) = execute(handle, LINE, br#"{"p1":[0,0],"p2":[10,20]}"#);
    assert_eq!(line_status, AF_STATUS_OK);
    let line_json = parse_json(&line);
    let transaction = line_json["ok"]["txSeq"]
        .as_u64()
        .expect("LINE transaction must be u64");
    let entity = line_json["ok"]["created"][0]
        .as_u64()
        .expect("LINE entity must be u64");

    let (vertices_status, mut vertices) = render_vertices(handle);
    assert_eq!(vertices_status, AF_STATUS_OK);
    assert_ne!(line.owner, vertices.owner);
    let values = float_values(&vertices);
    assert_eq!(values.len(), 4);
    assert_eq!(
        values.iter().copied().map(f32::to_bits).collect::<Vec<_>>(),
        [0.0f32, 0.0, 10.0, 20.0].map(f32::to_bits)
    );

    let (render_status, mut snapshot) = render(handle);
    assert_eq!(render_status, AF_STATUS_OK);
    assert_ne!(line.owner, snapshot.owner);
    assert_ne!(vertices.owner, snapshot.owner);
    let view = parse_json(&snapshot);
    let root = view.as_object().expect("RenderView root must be an object");
    assert_eq!(root.len(), 3);
    assert!(root.contains_key("batches"));
    assert!(root.contains_key("vertices"));
    assert!(root.contains_key("ltscale"));
    assert_eq!(view["vertices"], json!([0.0, 0.0, 10.0, 20.0]));
    assert_eq!(view["ltscale"].as_f64(), Some(1.0));

    let batches = view["batches"]
        .as_array()
        .expect("batches must be an array");
    assert_eq!(batches.len(), 1);
    let batch = &batches[0];
    assert!(batch["layer"].as_u64().is_some());
    assert_eq!(batch["color"], json!([255, 255, 255, 255]));
    assert!(batch["linetype"].as_u64().is_some());
    assert_eq!(batch["markers"], json!([]));
    let strips = batch["strips"].as_array().expect("strips must be an array");
    assert_eq!(strips.len(), 1);
    assert_eq!(strips[0]["entity"].as_u64(), Some(entity));
    assert_eq!(strips[0]["offset"].as_u64(), Some(0));
    assert_eq!(strips[0]["count"].as_u64(), Some(2));
    assert_eq!(strips[0]["width"].as_f64(), Some(0.25));

    let (next_status, mut next) = execute(handle, LINE, br#"{"p1":[1,1],"p2":[2,2]}"#);
    assert_eq!(next_status, AF_STATUS_OK);
    assert_eq!(
        parse_json(&next)["ok"]["txSeq"].as_u64(),
        Some(
            transaction
                .checked_add(1)
                .expect("transaction must advance")
        )
    );

    // SAFETY: all four buffers are live and retain their exact metadata.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut line) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_f32_buffer_free(&mut vertices) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_utf8_buffer_free(&mut snapshot) }, AF_STATUS_OK);
    assert_eq!(unsafe { af_utf8_buffer_free(&mut next) }, AF_STATUS_OK);
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}

#[test]
fn execute_rejects_null_output_bad_pointer_shapes_and_invalid_utf8() {
    let handle = create_session();

    // SAFETY: null output is an explicitly checked error case; inputs are valid.
    assert_eq!(
        unsafe {
            af_session_execute_json(
                handle,
                LINE.as_ptr(),
                LINE.len(),
                LINE_ARGS.as_ptr(),
                LINE_ARGS.len(),
                ptr::null_mut(),
            )
        },
        AF_STATUS_INVALID_ARGUMENT
    );

    let sentinel = ptr::dangling::<u8>();
    let mut output = AfUtf8Buffer {
        data: sentinel,
        len: 7,
        capacity: 9,
        owner: 0,
    };
    // SAFETY: null with positive length is an explicitly checked error case;
    // `output` is valid and disjoint.
    assert_eq!(
        unsafe {
            af_session_execute_json(
                handle,
                ptr::null(),
                1,
                LINE_ARGS.as_ptr(),
                LINE_ARGS.len(),
                &mut output,
            )
        },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_empty(&output);

    // SAFETY: command is valid; null args with positive length is an explicitly
    // checked shape error and output is valid disjoint storage.
    assert_eq!(
        unsafe {
            af_session_execute_json(
                handle,
                LINE.as_ptr(),
                LINE.len(),
                ptr::null(),
                1,
                &mut output,
            )
        },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_empty(&output);

    let invalid_utf8 = [0xff];
    // SAFETY: both input ranges and the output storage are valid and disjoint.
    assert_eq!(
        unsafe {
            af_session_execute_json(
                handle,
                invalid_utf8.as_ptr(),
                invalid_utf8.len(),
                ptr::null(),
                0,
                &mut output,
            )
        },
        AF_STATUS_INVALID_UTF8
    );
    assert_empty(&output);

    // SAFETY: the invalid args bytes are a valid immutable range and all other
    // pointers are valid and disjoint.
    assert_eq!(
        unsafe {
            af_session_execute_json(
                handle,
                LINE.as_ptr(),
                LINE.len(),
                invalid_utf8.as_ptr(),
                invalid_utf8.len(),
                &mut output,
            )
        },
        AF_STATUS_INVALID_UTF8
    );
    assert_empty(&output);

    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}

#[test]
fn handle_is_checked_before_inputs_and_wrong_thread_preserves_session() {
    let mut output = empty_buffer();
    // SAFETY: null with positive length would be a checked shape error, but the
    // invalid handle has contractual precedence; output storage is valid.
    assert_eq!(
        unsafe { af_session_execute_json(0, ptr::null(), 1, ptr::null(), 1, &mut output) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_empty(&output);

    let handle = create_session();
    let (status, was_empty) = thread::spawn(move || {
        let (status, output) = execute(handle, LINE, LINE_ARGS);
        (status, output.owner == 0 && output.data.is_null())
    })
    .join()
    .expect("execute thread should not panic");
    assert_eq!(status, AF_STATUS_WRONG_THREAD);
    assert!(was_empty);

    let (status, mut result) = execute(handle, LINE, LINE_ARGS);
    assert_eq!(status, AF_STATUS_OK);
    assert!(text(&result).contains("\"ok\""));
    // SAFETY: `result` is the live initialized buffer returned above.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut result) }, AF_STATUS_OK);
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}

#[test]
fn line_executes_real_core_and_domain_errors_remain_json_values() {
    let handle = create_session();

    let (status, mut line) = execute(handle, LINE, LINE_ARGS);
    assert_eq!(status, AF_STATUS_OK);
    let line_json = text(&line);
    assert!(
        line_json.contains("\"ok\""),
        "unexpected result: {line_json}"
    );
    assert!(
        line_json.contains("\"txSeq\":"),
        "missing transaction: {line_json}"
    );
    assert!(
        line_json.contains("\"created\":["),
        "missing created id: {line_json}"
    );
    assert_eq!(created_count(line_json), 1, "LINE must create one id");
    assert!(!line_json.contains("\"txSeq\":null"));
    assert!(!line_json.contains("\"error\""));
    // SAFETY: `line` is live and unmodified.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut line) }, AF_STATUS_OK);

    let (status, mut unknown) = execute(handle, b"NOSUCHCOMMAND", b"null");
    assert_eq!(status, AF_STATUS_OK);
    assert!(text(&unknown).contains("\"code\":\"unknown_command\""));
    // SAFETY: `unknown` is live and unmodified.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut unknown) }, AF_STATUS_OK);

    let (status, mut malformed) = execute(handle, LINE, b"{not-json");
    assert_eq!(status, AF_STATUS_OK);
    assert!(text(&malformed).contains("\"code\":\"malformed_json\""));
    // SAFETY: `malformed` is live and unmodified.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut malformed) }, AF_STATUS_OK);

    let (status, mut unicode) = execute(handle, "LÍNEA_INEXISTENTE".as_bytes(), b"null");
    assert_eq!(status, AF_STATUS_OK);
    let unicode_text = text(&unicode);
    assert!(unicode_text.contains("\"code\":\"unknown_command\""));
    assert!(unicode_text.as_bytes().iter().any(|byte| *byte >= 0x80));
    // SAFETY: `unicode` is live and unmodified.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut unicode) }, AF_STATUS_OK);

    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}

#[test]
fn owners_are_unique_and_free_is_checked_and_idempotent_for_empty() {
    let handle = create_session();
    let (status_a, mut a) = execute(handle, b"A", b"null");
    let (status_b, mut b) = render(handle);
    assert_eq!((status_a, status_b), (AF_STATUS_OK, AF_STATUS_OK));
    assert_ne!(a.owner, 0);
    assert_ne!(b.owner, 0);
    assert_ne!(a.owner, b.owner);

    let mut stale = copy_buffer(&a);
    let stale_before = copy_buffer(&stale);
    // SAFETY: `a` is the live exact metadata for its allocation.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut a) }, AF_STATUS_OK);
    assert_empty(&a);
    // SAFETY: `a` is now the initialized canonical empty value.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut a) }, AF_STATUS_OK);
    // SAFETY: `stale` is readable metadata in separate storage; its owner is no
    // longer registered, so no payload dereference is required.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut stale) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_eq!(stale, stale_before);

    let mut tampered = copy_buffer(&b);
    tampered.len = tampered.len.saturating_sub(1);
    let tampered_before = copy_buffer(&tampered);
    // SAFETY: `tampered` is initialized readable/writable storage. Its metadata
    // mismatch is an explicitly checked case and leaves the live allocation intact.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut tampered) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(tampered, tampered_before);
    // SAFETY: the canonical metadata in `b` still owns the allocation.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut b) }, AF_STATUS_OK);

    let mut unknown = AfUtf8Buffer {
        data: ptr::null(),
        len: 0,
        capacity: 0,
        owner: usize::MAX,
    };
    let unknown_before = copy_buffer(&unknown);
    // SAFETY: `unknown` is initialized readable/writable storage with an unknown
    // token; the implementation does not dereference its data pointer.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut unknown) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_eq!(unknown, unknown_before);

    let mut noncanonical_zero = AfUtf8Buffer {
        data: ptr::null(),
        len: 1,
        capacity: 1,
        owner: 0,
    };
    let noncanonical_before = copy_buffer(&noncanonical_zero);
    // SAFETY: initialized storage with owner zero but noncanonical metadata is
    // an explicitly checked error case.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut noncanonical_zero) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(noncanonical_zero, noncanonical_before);
    // SAFETY: null is the documented idempotent no-op.
    assert_eq!(
        unsafe { af_utf8_buffer_free(ptr::null_mut()) },
        AF_STATUS_OK
    );

    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}

#[test]
fn f32_free_rejects_wrong_type_tampering_and_stale_copies() {
    let handle = create_session();
    let (line_status, mut line) = execute(handle, LINE, LINE_ARGS);
    let (vertices_status, mut vertices) = render_vertices(handle);
    assert_eq!((line_status, vertices_status), (AF_STATUS_OK, AF_STATUS_OK));
    assert_eq!(float_values(&vertices), [0.0, 0.0, 1.0, 1.0]);

    let mut utf8_as_f32 = AfF32Buffer {
        data: line.data.cast(),
        len: line.len,
        capacity: line.capacity,
        owner: line.owner,
    };
    let utf8_as_f32_before = copy_f32_buffer(&utf8_as_f32);
    // SAFETY: initialized metadata names a live owner of the wrong checked type.
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut utf8_as_f32) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(utf8_as_f32, utf8_as_f32_before);

    let mut f32_as_utf8 = AfUtf8Buffer {
        data: vertices.data.cast(),
        len: vertices.len,
        capacity: vertices.capacity,
        owner: vertices.owner,
    };
    let f32_as_utf8_before = copy_buffer(&f32_as_utf8);
    // SAFETY: initialized metadata names a live owner of the wrong checked type.
    assert_eq!(
        unsafe { af_utf8_buffer_free(&mut f32_as_utf8) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(f32_as_utf8, f32_as_utf8_before);

    let mut tampered = copy_f32_buffer(&vertices);
    tampered.len -= 1;
    let tampered_before = copy_f32_buffer(&tampered);
    // SAFETY: initialized metadata mismatch is an explicitly checked case.
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut tampered) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(tampered, tampered_before);

    let mut stale = copy_f32_buffer(&vertices);
    let stale_before = copy_f32_buffer(&stale);
    // SAFETY: exact live metadata releases the allocation.
    assert_eq!(unsafe { af_f32_buffer_free(&mut vertices) }, AF_STATUS_OK);
    assert_f32_empty(&vertices);
    // SAFETY: canonical empty is idempotently releasable.
    assert_eq!(unsafe { af_f32_buffer_free(&mut vertices) }, AF_STATUS_OK);
    // SAFETY: stale owner is checked without dereferencing its payload.
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut stale) },
        AF_STATUS_INVALID_HANDLE
    );
    assert_eq!(stale, stale_before);

    // SAFETY: wrong-type attempts preserved the original UTF-8 allocation.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut line) }, AF_STATUS_OK);

    let mut noncanonical_empty = AfF32Buffer {
        data: ptr::null(),
        len: 2,
        capacity: 2,
        owner: 0,
    };
    let noncanonical_before = copy_f32_buffer(&noncanonical_empty);
    // SAFETY: initialized owner-zero metadata is an explicitly checked case.
    assert_eq!(
        unsafe { af_f32_buffer_free(&mut noncanonical_empty) },
        AF_STATUS_INVALID_ARGUMENT
    );
    assert_eq!(noncanonical_empty, noncanonical_before);
    // SAFETY: null is the documented idempotent no-op.
    assert_eq!(unsafe { af_f32_buffer_free(ptr::null_mut()) }, AF_STATUS_OK);

    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}

#[test]
fn buffer_can_outlive_session_and_be_freed_on_another_thread() {
    let handle = create_session();
    let (line_status, mut line) = execute(handle, LINE, LINE_ARGS);
    assert_eq!(line_status, AF_STATUS_OK);
    // SAFETY: `line` is live and unmodified.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut line) }, AF_STATUS_OK);

    let (status, result) = render(handle);
    assert_eq!(status, AF_STATUS_OK);
    assert!(text(&result).contains("\"batches\""));
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);

    let raw = (
        result.data as usize,
        result.len,
        result.capacity,
        result.owner,
    );
    let (free_status, was_zeroed) = thread::spawn(move || {
        let mut moved = AfUtf8Buffer {
            data: raw.0 as *const u8,
            len: raw.1,
            capacity: raw.2,
            owner: raw.3,
        };
        // SAFETY: ownership was moved to this thread and `moved` contains the
        // exact live metadata with exclusive access.
        let status = unsafe { af_utf8_buffer_free(&mut moved) };
        (status, moved.owner == 0 && moved.data.is_null())
    })
    .join()
    .expect("free thread should not panic");
    assert_eq!(free_status, AF_STATUS_OK);
    assert!(was_zeroed);
}

#[test]
fn f32_buffer_can_outlive_session_and_be_freed_on_another_thread() {
    let handle = create_session();
    let (line_status, mut line) = execute(handle, LINE, LINE_ARGS);
    assert_eq!(line_status, AF_STATUS_OK);
    // SAFETY: `line` is live and unmodified.
    assert_eq!(unsafe { af_utf8_buffer_free(&mut line) }, AF_STATUS_OK);

    let (status, result) = render_vertices(handle);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(float_values(&result), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);

    let raw = (
        result.data as usize,
        result.len,
        result.capacity,
        result.owner,
    );
    let (free_status, was_zeroed) = thread::spawn(move || {
        let mut moved = AfF32Buffer {
            data: raw.0 as *const f32,
            len: raw.1,
            capacity: raw.2,
            owner: raw.3,
        };
        // SAFETY: ownership moved to this thread with exact live metadata.
        let status = unsafe { af_f32_buffer_free(&mut moved) };
        (status, moved.owner == 0 && moved.data.is_null())
    })
    .join()
    .expect("f32 free thread should not panic");
    assert_eq!(free_status, AF_STATUS_OK);
    assert!(was_zeroed);
}

#[test]
fn pgp_reinit_resolve_invoke_fail_closed_and_two_sessions_use_the_same_ffi_gateway() {
    const REINIT: &[u8] = b"__ARCFORGE_PGP_REINIT";
    const RESOLVE: &[u8] = b"__ARCFORGE_PGP_RESOLVE";

    let first = create_session();
    let second = create_session();
    let (status, mut layered) = execute(
        first,
        REINIT,
        br#"{"system":"DRAW,*LINE\nC,*MOVE","user":"DRAW,*CIRCLE","project":"DRAW,*COPY","session":"DRAW,*LINE"}"#,
    );
    assert_eq!(status, AF_STATUS_OK);
    let layered_json = parse_json(&layered);
    assert!(layered_json["ok"]["txSeq"].is_null());
    assert!(layered_json["ok"]["created"].as_array().unwrap().is_empty());
    assert!(
        layered_json["ok"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("reemplaza capa project"))
    );

    let (status, mut legacy) = execute(second, REINIT, br#"{"pgp":"DRAW,*MOVE"}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert!(
        parse_json(&legacy)["ok"]["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("PGP: 1 alias(es)"))
    );

    let (status, mut first_resolve) = execute(first, RESOLVE, br#"{"token":"draw"}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&first_resolve)["ok"]["message"], "LINE");
    let (status, mut second_resolve) = execute(second, RESOLVE, br#"{"token":"draw"}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&second_resolve)["ok"]["message"], "MOVE");
    let (status, mut canonical) = execute(first, RESOLVE, br#"{"token":"CIRCLE"}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&canonical)["ok"]["message"], "CIRCLE");

    let (status, mut typed_error) = execute(first, b"DRAW", br#"{"p1":[0,0]}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&typed_error)["error"]["code"], "missing_param");
    let (status, mut first_line) = execute(first, b"DRAW", br#"{"p1":[0,0],"p2":[1,1]}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&first_line)["ok"]["txSeq"], 0);

    let (status, mut invalid_pgp) = execute(
        first,
        REINIT,
        br#"{"system":"","user":"NEW,*NOPE","project":"","session":""}"#,
    );
    assert_eq!(status, AF_STATUS_OK);
    let invalid_json = parse_json(&invalid_pgp);
    assert_eq!(invalid_json["error"]["code"], "invalid_pgp");
    assert!(
        invalid_json["error"]["message"]
            .as_str()
            .is_some_and(
                |message| message.contains("PGP user linea 1") && message.contains("desconocido")
            )
    );

    let (status, mut retained) = execute(first, RESOLVE, br#"{"token":"DRAW"}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&retained)["ok"]["message"], "LINE");
    let (status, mut second_line) = execute(first, b"DRAW", br#"{"p1":[2,2],"p2":[3,3]}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&second_line)["ok"]["txSeq"], 1);

    let (status, mut nested) = execute(
        first,
        REINIT,
        br#"{"pgp":{"system":"","user":"","project":"","session":""}}"#,
    );
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(parse_json(&nested)["error"]["code"], "invalid_args");

    for buffer in [
        &mut layered,
        &mut legacy,
        &mut first_resolve,
        &mut second_resolve,
        &mut canonical,
        &mut typed_error,
        &mut first_line,
        &mut invalid_pgp,
        &mut retained,
        &mut second_line,
        &mut nested,
    ] {
        // SAFETY: every buffer retains the exact live metadata returned above.
        assert_eq!(unsafe { af_utf8_buffer_free(buffer) }, AF_STATUS_OK);
    }
    assert_eq!(af_session_destroy(first), AF_STATUS_OK);
    assert_eq!(af_session_destroy(second), AF_STATUS_OK);
}
