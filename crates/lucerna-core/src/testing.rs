//! Test doubles, available with the `test-support` feature only.
//!
//! [`FakeBackend`] lets the daemon's policy be tested with no X server at all (directive §48).

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::backend::{
    BackendCapabilities, BackendError, BackendEvent, BackendKind, BackendProbe, EmbedTarget,
    EventSink, OutputInfo, Rect, SurfaceHandle, SurfaceId, WallpaperBackend,
};

/// Everything the fake was asked to do, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FakeCall {
    Probe,
    Enumerate,
    Create(String),
    Resize(SurfaceId, Rect),
    Destroy(SurfaceId),
    Refresh,
    Subscribe,
    Shutdown,
}

struct State {
    kind: BackendKind,
    outputs: Vec<OutputInfo>,
    next_surface: u64,
    surfaces: BTreeMap<SurfaceId, (String, Rect)>,
    calls: Vec<FakeCall>,
    sink: Option<Arc<EventSink>>,
    fail_next: HashSet<&'static str>,
    capabilities: BackendCapabilities,
}

/// A scriptable backend. Clone the handle before boxing it as `dyn WallpaperBackend` to keep
/// control of the outputs and events from the test.
#[derive(Clone)]
pub struct FakeBackend {
    state: Arc<Mutex<State>>,
}

impl Default for FakeBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeBackend {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                kind: BackendKind::CinnamonX11,
                outputs: Vec::new(),
                next_surface: 1,
                surfaces: BTreeMap::new(),
                calls: Vec::new(),
                sink: None,
                fail_next: HashSet::new(),
                capabilities: BackendCapabilities {
                    fullscreen_detection: true,
                    input_passthrough: true,
                    hotplug_events: true,
                    stable_monitor_identity: false,
                },
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_outputs(&self, outputs: Vec<OutputInfo>) {
        self.lock().outputs = outputs;
    }

    pub fn set_capabilities(&self, capabilities: BackendCapabilities) {
        self.lock().capabilities = capabilities;
    }

    /// Deliver an event to the subscribed sink (if any), as the backend thread would.
    pub fn emit(&self, event: BackendEvent) {
        let sink = self.lock().sink.clone();
        if let Some(sink) = sink {
            sink(event);
        }
    }

    /// Make the next call to the named method (`"create_surface"`, `"enumerate_outputs"`, ...)
    /// fail with a protocol error.
    pub fn fail_next(&self, method: &'static str) {
        self.lock().fail_next.insert(method);
    }

    pub fn calls(&self) -> Vec<FakeCall> {
        self.lock().calls.clone()
    }

    /// `(output id, geometry)` of every surface currently alive.
    pub fn live_surfaces(&self) -> Vec<(String, Rect)> {
        self.lock().surfaces.values().cloned().collect()
    }

    fn check_failure(state: &mut State, method: &'static str) -> Result<(), BackendError> {
        if state.fail_next.remove(method) {
            Err(BackendError::Protocol(format!(
                "injected failure in {method}"
            )))
        } else {
            Ok(())
        }
    }
}

impl WallpaperBackend for FakeBackend {
    fn kind(&self) -> BackendKind {
        self.lock().kind
    }

    fn probe(&mut self) -> Result<BackendProbe, BackendError> {
        let mut state = self.lock();
        state.calls.push(FakeCall::Probe);
        Self::check_failure(&mut state, "probe")?;
        Ok(BackendProbe {
            kind: state.kind,
            capabilities: state.capabilities.clone(),
            facts: serde_json::json!({"fake": true}),
        })
    }

    fn enumerate_outputs(&mut self) -> Result<Vec<OutputInfo>, BackendError> {
        let mut state = self.lock();
        state.calls.push(FakeCall::Enumerate);
        Self::check_failure(&mut state, "enumerate_outputs")?;
        Ok(state.outputs.clone())
    }

    fn create_surface(&mut self, output: &OutputInfo) -> Result<SurfaceHandle, BackendError> {
        let mut state = self.lock();
        state
            .calls
            .push(FakeCall::Create(output.id.as_str().to_owned()));
        Self::check_failure(&mut state, "create_surface")?;
        let id = SurfaceId(state.next_surface);
        state.next_surface += 1;
        state
            .surfaces
            .insert(id, (output.id.as_str().to_owned(), output.geometry));
        Ok(SurfaceHandle {
            id,
            output: output.id.clone(),
            geometry: output.geometry,
            embed: EmbedTarget::X11Window(0x0100_0000 + u32::try_from(id.0).unwrap_or(0)),
        })
    }

    fn resize_surface(&mut self, surface: SurfaceId, geometry: Rect) -> Result<(), BackendError> {
        let mut state = self.lock();
        state.calls.push(FakeCall::Resize(surface, geometry));
        Self::check_failure(&mut state, "resize_surface")?;
        match state.surfaces.get_mut(&surface) {
            Some(entry) => {
                entry.1 = geometry;
                Ok(())
            }
            None => Err(BackendError::UnknownSurface(surface)),
        }
    }

    fn destroy_surface(&mut self, surface: SurfaceId) -> Result<(), BackendError> {
        let mut state = self.lock();
        state.calls.push(FakeCall::Destroy(surface));
        Self::check_failure(&mut state, "destroy_surface")?;
        state.surfaces.remove(&surface);
        Ok(())
    }

    fn refresh(&mut self) -> Result<(), BackendError> {
        let mut state = self.lock();
        state.calls.push(FakeCall::Refresh);
        Self::check_failure(&mut state, "refresh")
    }

    fn subscribe(&mut self, sink: EventSink) -> Result<(), BackendError> {
        let mut state = self.lock();
        state.calls.push(FakeCall::Subscribe);
        Self::check_failure(&mut state, "subscribe")?;
        state.sink = Some(Arc::new(sink));
        Ok(())
    }

    fn diagnostics(&mut self) -> serde_json::Value {
        let state = self.lock();
        serde_json::json!({"fake": true, "surfaces": state.surfaces.len()})
    }

    fn shutdown(&mut self) -> Result<(), BackendError> {
        let mut state = self.lock();
        state.calls.push(FakeCall::Shutdown);
        state.surfaces.clear();
        state.sink = None;
        Ok(())
    }
}
