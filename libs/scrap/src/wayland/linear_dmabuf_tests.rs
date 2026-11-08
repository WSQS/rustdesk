use super::*;
use std::cell::RefCell;

thread_local! {
    static STATE: RefCell<(u8, Vec<bool>)> = RefCell::new((0, Vec::new()));
}

fn checked_sync(_fd: libc::c_int, end: bool) -> Result<(), &'static str> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.1.push(end);
        match (state.0, end) {
            (1, false) => Err("dmabuf-sync-start-failed"),
            (2, true) => Err("dmabuf-sync-end-failed"),
            _ => Ok(()),
        }
    })
}
fn reset(mode: u8) { STATE.with(|state| *state.borrow_mut() = (mode, vec![])); }
fn calls() -> Vec<bool> { STATE.with(|state| state.borrow().1.clone()) }

#[test]
fn checked_start_and_end_balance_on_action_error() {
    reset(0);
    let result: Result<(), _> = with_read_access(-1, checked_sync, || Err("dmabuf-map-failed"));
    assert_eq!(result, Err("dmabuf-map-failed"));
    assert_eq!(calls(), vec![false, true]);
}

#[test]
fn failed_start_does_not_read_or_end() {
    reset(1);
    let mut copied = false;
    let result = with_read_access(-1, checked_sync, || { copied = true; Ok(()) });
    assert_eq!(result, Err("dmabuf-sync-start-failed"));
    assert!(!copied);
    assert_eq!(calls(), vec![false]);
}

#[test]
fn failed_end_never_returns_the_copied_frame() {
    reset(2);
    let result = with_read_access(-1, checked_sync, || Ok(vec![1u8, 2, 3, 4]));
    assert_eq!(result, Err("dmabuf-sync-end-failed"));
    assert_eq!(calls(), vec![false, true]);
}

#[test]
fn unwind_after_start_still_ends_once() {
    reset(0);
    let result = std::panic::catch_unwind(|| {
        let _: Result<(), _> = with_read_access(-1, checked_sync, || panic!("synthetic unwind"));
    });
    assert!(result.is_err());
    assert_eq!(calls(), vec![false, true]);
}
