use std::mem::MaybeUninit;
use std::ptr;
use std::slice;

use af_ffi::{
    AF_STATUS_INVALID_ARGUMENT, AF_STATUS_OK, AfSessionHandle, AfUtf8Buffer, af_session_create,
    af_session_destroy, af_session_execute_json, af_session_layers_json, af_utf8_buffer_free,
};
use serde_json::Value;

fn create_session() -> AfSessionHandle {
    let mut handle = 0;
    // SAFETY: `handle` is writable storage for one handle.
    assert_eq!(unsafe { af_session_create(&mut handle) }, AF_STATUS_OK);
    handle
}

fn call_json(call: impl FnOnce(*mut AfUtf8Buffer) -> u32) -> (u32, AfUtf8Buffer) {
    let mut output = MaybeUninit::<AfUtf8Buffer>::uninit();
    let status = call(output.as_mut_ptr());
    // SAFETY: every ABI call used here initializes a non-null output first.
    (status, unsafe { output.assume_init() })
}

fn layers(handle: AfSessionHandle) -> (u32, AfUtf8Buffer) {
    call_json(|output| {
        // SAFETY: `output` is aligned writable storage for one buffer.
        unsafe { af_session_layers_json(handle, output) }
    })
}

fn execute(handle: AfSessionHandle, command: &[u8], args: &[u8]) -> (u32, AfUtf8Buffer) {
    call_json(|output| {
        // SAFETY: inputs remain readable for the call and output is disjoint.
        unsafe {
            af_session_execute_json(
                handle,
                command.as_ptr(),
                command.len(),
                args.as_ptr(),
                args.len(),
                output,
            )
        }
    })
}

fn value(buffer: &AfUtf8Buffer) -> Value {
    // SAFETY: a live returned buffer owns at least `len` immutable bytes.
    let bytes = unsafe { slice::from_raw_parts(buffer.data, buffer.len) };
    serde_json::from_slice(bytes).expect("ABI result must be valid JSON")
}

fn free(buffer: &mut AfUtf8Buffer) {
    // SAFETY: `buffer` is writable storage containing one live ABI buffer.
    assert_eq!(unsafe { af_utf8_buffer_free(buffer) }, AF_STATUS_OK);
}

#[test]
fn header_declares_layers_snapshot_export() {
    assert!(include_str!("../include/arccad.h").contains("AfStatus af_session_layers_json("));
}

#[test]
fn layer_snapshot_plot_operation_error_and_undo_are_native() {
    let handle = create_session();

    assert_eq!(
        unsafe { af_session_layers_json(handle, ptr::null_mut()) },
        AF_STATUS_INVALID_ARGUMENT
    );

    let (status, mut initial) = layers(handle);
    assert_eq!(status, AF_STATUS_OK);
    let initial_json = value(&initial);
    assert_eq!(initial_json.as_array().unwrap().len(), 1);
    assert_eq!(initial_json[0]["name"], "0");
    assert_eq!(initial_json[0]["plot"], true);
    assert_eq!(initial_json[0]["current"], true);
    free(&mut initial);

    let (status, mut created) = execute(handle, b"LAYER", br#"{"op":"new","name":"A"}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert!(value(&created).get("ok").is_some());
    free(&mut created);

    let (status, mut snapshot) = layers(handle);
    assert_eq!(status, AF_STATUS_OK);
    let snapshot_json = value(&snapshot);
    let layer = snapshot_json
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["name"] == "A")
        .unwrap();
    let id = layer["id"].as_u64().unwrap();
    assert_eq!(layer["plot"], true);
    free(&mut snapshot);

    let args = format!(r#"{{"op":"no-plot","layer":{id}}}"#);
    let (status, mut toggled) = execute(handle, b"LAYER", args.as_bytes());
    assert_eq!(status, AF_STATUS_OK);
    assert!(value(&toggled).get("ok").is_some());
    free(&mut toggled);

    let (status, mut snapshot) = layers(handle);
    assert_eq!(status, AF_STATUS_OK);
    let no_plot = value(&snapshot);
    assert_eq!(
        no_plot
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == id)
            .unwrap()["plot"],
        false
    );
    free(&mut snapshot);

    let (status, mut failed) = execute(handle, b"LAYER", br#"{"op":"plot","layer":999999}"#);
    assert_eq!(status, AF_STATUS_OK);
    assert!(value(&failed).get("error").is_some());
    free(&mut failed);

    let (status, mut undo) = execute(handle, b"UNDO", b"null");
    assert_eq!(status, AF_STATUS_OK);
    assert!(value(&undo).get("ok").is_some());
    free(&mut undo);

    let (status, mut restored) = layers(handle);
    assert_eq!(status, AF_STATUS_OK);
    assert_eq!(
        value(&restored)
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == id)
            .unwrap()["plot"],
        true
    );
    free(&mut restored);

    assert_eq!(af_session_destroy(handle), AF_STATUS_OK);
}
