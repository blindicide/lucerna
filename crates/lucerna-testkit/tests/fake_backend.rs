//! The scriptable backend double the daemon will be tested against (directive §48).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::mpsc;

use lucerna_core::backend::{
    BackendError, BackendEvent, OutputId, OutputInfo, Rect, Rotation, SurfaceId, WallpaperBackend,
};
use lucerna_core::testing::{FakeBackend, FakeCall};

fn output(id: &str, x: i32) -> OutputInfo {
    OutputInfo {
        id: OutputId::new(id),
        connector: id.trim_start_matches("conn:").to_owned(),
        geometry: Rect::new(x, 0, 1920, 1080),
        primary: x == 0,
        rotation: Rotation::Normal,
        refresh_mhz: None,
        edid: None,
    }
}

#[test]
fn outputs_surfaces_and_calls_are_scriptable_and_recorded() {
    let handle = FakeBackend::new();
    let mut backend: Box<dyn WallpaperBackend> = Box::new(handle.clone());

    handle.set_outputs(vec![output("conn:A", 0), output("conn:B", 1920)]);
    let outputs = backend.enumerate_outputs().unwrap();
    assert_eq!(outputs.len(), 2);

    let s1 = backend.create_surface(&outputs[0]).unwrap();
    let s2 = backend.create_surface(&outputs[1]).unwrap();
    assert_ne!(s1.id, s2.id);
    assert_eq!(handle.live_surfaces().len(), 2);

    backend
        .resize_surface(s1.id, Rect::new(0, 0, 800, 600))
        .unwrap();
    assert!(matches!(
        backend.resize_surface(SurfaceId(99), Rect::new(0, 0, 1, 1)),
        Err(BackendError::UnknownSurface(_))
    ));
    backend.destroy_surface(s1.id).unwrap();
    backend.destroy_surface(s1.id).unwrap(); // idempotent
    assert_eq!(handle.live_surfaces().len(), 1);

    backend.shutdown().unwrap();
    assert!(handle.live_surfaces().is_empty());
    assert_eq!(handle.calls().first(), Some(&FakeCall::Enumerate));
    assert_eq!(handle.calls().last(), Some(&FakeCall::Shutdown));
}

#[test]
fn events_reach_the_subscribed_sink() {
    let handle = FakeBackend::new();
    let mut backend: Box<dyn WallpaperBackend> = Box::new(handle.clone());
    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    backend
        .subscribe(Box::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        }))
        .unwrap();

    handle.emit(BackendEvent::OutputsChanged);
    handle.emit(BackendEvent::FullscreenChanged(vec![Rect::new(
        0, 0, 10, 10,
    )]));
    assert!(matches!(rx.recv().unwrap(), BackendEvent::OutputsChanged));
    assert!(matches!(rx.recv().unwrap(), BackendEvent::FullscreenChanged(v) if v.len() == 1));

    backend.shutdown().unwrap();
    handle.emit(BackendEvent::StackingDisturbed); // after shutdown nothing is delivered
    assert!(rx.try_recv().is_err());
}

#[test]
fn failures_can_be_injected_once() {
    let handle = FakeBackend::new();
    let mut backend: Box<dyn WallpaperBackend> = Box::new(handle.clone());
    handle.set_outputs(vec![output("conn:A", 0)]);
    let outputs = backend.enumerate_outputs().unwrap();

    handle.fail_next("create_surface");
    assert!(backend.create_surface(&outputs[0]).is_err());
    assert!(
        backend.create_surface(&outputs[0]).is_ok(),
        "only the next call fails"
    );

    handle.fail_next("enumerate_outputs");
    assert!(backend.enumerate_outputs().is_err());
    assert!(backend.enumerate_outputs().is_ok());
}
