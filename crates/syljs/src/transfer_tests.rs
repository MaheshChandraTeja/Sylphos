use std::rc::Rc;

use crate::{install_transfer_globals, ResearchTransferHost, ScheduledVm, TransferHost};

#[test]
fn array_buffer_and_uint8_array_read_write_work() {
    let transfer = Rc::new(ResearchTransferHost::default());
    let mut scheduled = ScheduledVm::default();

    install_transfer_globals(&mut scheduled.vm, transfer.clone());

    scheduled
        .execute_script(
            r#"
            const buffer = new ArrayBuffer(4);
            const bytes = new Uint8Array(buffer);
            bytes[0] = 10;
            bytes[1] = 20;
            console.log(buffer.byteLength, bytes.length, bytes[0], bytes[1]);
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["4 4 10 20"]);
    assert_eq!(transfer.metrics().buffers_created, 1);
    assert_eq!(transfer.metrics().byte_writes, 2);
}

#[test]
fn typed_array_slice_and_subarray_work() {
    let transfer = Rc::new(ResearchTransferHost::default());
    let mut scheduled = ScheduledVm::default();

    install_transfer_globals(&mut scheduled.vm, transfer.clone());

    scheduled
        .execute_script(
            r#"
            const buffer = new ArrayBuffer(4);
            const bytes = new Uint8Array(buffer);
            bytes[0] = 7;
            bytes[1] = 8;
            bytes[2] = 9;
            const view = bytes.subarray(1, 3);
            const copy = bytes.slice(0, 2);
            console.log(view.length, view[0], view[1]);
            console.log(copy.length, copy[0], copy[1]);
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["2 8 9", "2 7 8"]);
    assert!(transfer.metrics().buffers_created >= 2);
}

#[test]
fn structured_clone_without_transfer_clones_array_buffer() {
    let transfer = Rc::new(ResearchTransferHost::default());
    let mut scheduled = ScheduledVm::default();

    install_transfer_globals(&mut scheduled.vm, transfer.clone());

    scheduled
        .execute_script(
            r#"
            const buffer = new ArrayBuffer(2);
            const original = new Uint8Array(buffer);
            original[0] = 44;

            const cloned = structuredClone(buffer);
            const cloneBytes = new Uint8Array(cloned);
            console.log(buffer.byteLength, cloned.byteLength, cloneBytes[0]);
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["2 2 44"]);
    assert_eq!(transfer.metrics().buffers_cloned, 1);
}

#[test]
fn direct_host_transfer_detaches_original() {
    let transfer = ResearchTransferHost::default();
    let id = transfer.create_buffer(8);
    transfer.write_byte(id, 0, 123);

    let new_id = transfer.transfer_buffer(id).expect("transfer");
    let old_snapshot = transfer.buffer_snapshot(id).expect("old");
    let new_snapshot = transfer.buffer_snapshot(new_id).expect("new");

    assert!(old_snapshot.detached);
    assert_eq!(old_snapshot.byte_length, 0);
    assert_eq!(new_snapshot.byte_length, 8);
    assert_eq!(transfer.read_byte(new_id, 0), Some(123));
    assert_eq!(transfer.metrics().buffers_transferred, 1);
    assert_eq!(transfer.metrics().buffers_detached, 1);
}

#[test]
fn int32_and_float64_arrays_store_numbers() {
    let transfer = Rc::new(ResearchTransferHost::default());
    let mut scheduled = ScheduledVm::default();

    install_transfer_globals(&mut scheduled.vm, transfer.clone());

    scheduled
        .execute_script(
            r#"
            const ints = new Int32Array(2);
            ints[0] = 123456;
            const floats = new Float64Array(1);
            floats[0] = 3.5;
            console.log(ints[0], floats[0]);
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["123456 3.5"]);
    assert_eq!(transfer.metrics().buffers_created, 2);
}

#[test]
fn array_buffer_slice_copies_range() {
    let transfer = Rc::new(ResearchTransferHost::default());
    let mut scheduled = ScheduledVm::default();

    install_transfer_globals(&mut scheduled.vm, transfer.clone());

    scheduled
        .execute_script(
            r#"
            const buffer = new ArrayBuffer(4);
            const bytes = new Uint8Array(buffer);
            bytes[0] = 1;
            bytes[1] = 2;
            bytes[2] = 3;
            const sliced = buffer.slice(1, 3);
            const view = new Uint8Array(sliced);
            console.log(sliced.byteLength, view[0], view[1]);
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["2 2 3"]);
}
