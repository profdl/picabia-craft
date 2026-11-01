//! PICABIA: the scheduler for computed modifier results: stencil sheets ([`crate::picabia_stencil`])
//! and generator results ([`crate::picabia_generate`]).
//!
//! A **job** brings one layer up to date: its generator result, then its stencil sheet, each when
//! stale. Each compute first brings the generated layers it reads up to date, synchronously in the
//! same job (a pathfinder over a traced child, a stencil over generated layers or over its own
//! generator's result): see [`crate::picabia_generate::prepare`].
//! - [`refresh`] (the canvas, every frame): run the jobs of stale layers off the UI thread (native:
//!   one worker thread per layer, at most [`MAX_JOBS`] at once; wasm: on the UI thread once the
//!   layer has stopped changing for [`SETTLE`]), drawing the latest results meanwhile. [`busy`]
//!   says a result is still coming.
//! - [`ensure`]: run every stale layer's job now (exports, headless renders, commands).

use std::sync::{Arc, Mutex, MutexGuard};

use figcraft_doc::{Document, NodeId};
use figcraft_render::picabia::{generated, stencil};

/// Background computes running at once (native).
#[cfg(not(target_arch = "wasm32"))]
const MAX_JOBS: usize = 2;
/// wasm computes on the UI thread: only once a layer has stopped changing for this long.
#[cfg(target_arch = "wasm32")]
const SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

/// The layers of `doc` with a computed result: generated layers and stencil layers.
pub fn layers(doc: &Document) -> Vec<NodeId> {
    doc.nodes().filter(|n| generated::is_generated(n) || stencil::is_stencil(n)).map(|n| n.id).collect()
}

/// Whether layer `id`'s generator result or stencil sheet is out of date (or missing).
pub fn is_stale(doc: &Document, id: NodeId) -> bool {
    let Some(n) = doc.node(id) else { return false };
    (generated::is_generated(n) && !generated::is_fresh(doc, n)) || (stencil::is_stencil(n) && !stencil::is_fresh(doc, n))
}

/// Layer `id`'s job: its generator result, then its stencil sheet, each when stale.
pub fn run(doc: &Document, id: NodeId) {
    let Some(n) = doc.node(id) else { return };
    if generated::is_generated(n) && !generated::is_fresh(doc, n) {
        crate::picabia_generate::compute_now(doc, id);
    }
    if stencil::is_stencil(n) && !stencil::is_fresh(doc, n) {
        crate::picabia_stencil::compute_now(doc, id);
    }
}

/// Run every stale layer's job of `doc` now.
pub fn ensure(doc: &Document) {
    for id in layers(doc) {
        if is_stale(doc, id) {
            run(doc, id);
        }
    }
}

#[derive(Default)]
struct Sched {
    /// Layers being computed in the background.
    running: Vec<NodeId>,
    /// wasm: stale layers waiting to settle (the document they were seen in, and since when).
    #[cfg(target_arch = "wasm32")]
    waiting: Vec<(NodeId, usize, web_time::Instant)>,
}

static SCHED: Mutex<Option<Sched>> = Mutex::new(None);

fn sched() -> MutexGuard<'static, Option<Sched>> {
    SCHED.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether a result is still coming (the canvas keeps repainting until it lands).
pub fn busy() -> bool {
    let s = sched();
    let Some(s) = s.as_ref() else { return false };
    #[cfg(target_arch = "wasm32")]
    if !s.waiting.is_empty() {
        return true;
    }
    !s.running.is_empty()
}

/// Bring `doc`'s results up to date without blocking the caller long: stale layers are recomputed
/// in the background (native) or once they've stopped changing (wasm). Call it every frame.
pub fn refresh(doc: &Arc<Document>) {
    let mut stale = Vec::new();
    for id in layers(doc) {
        if !is_stale(doc, id) {
            continue;
        }
        stale.push(id);
        schedule(doc, id);
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = sched().as_mut() {
        s.waiting.retain(|w| stale.contains(&w.0));
    }
    let _ = stale;
}

#[cfg(not(target_arch = "wasm32"))]
fn schedule(doc: &Arc<Document>, id: NodeId) {
    {
        let mut g = sched();
        let s = g.get_or_insert_with(Sched::default);
        if s.running.contains(&id) || s.running.len() >= MAX_JOBS {
            return;
        }
        s.running.push(id);
    }
    struct Done(NodeId);
    impl Drop for Done {
        fn drop(&mut self) {
            if let Some(s) = sched().as_mut() {
                s.running.retain(|r| *r != self.0);
            }
        }
    }
    let doc2 = doc.clone();
    let spawned = std::thread::Builder::new().name("picabia-compute".into()).spawn(move || {
        let _done = Done(id);
        run(&doc2, id);
    });
    if spawned.is_err() {
        let _done = Done(id);
        run(doc, id);
    }
}

#[cfg(target_arch = "wasm32")]
fn schedule(doc: &Arc<Document>, id: NodeId) {
    let ptr = Arc::as_ptr(doc) as usize;
    let now = web_time::Instant::now();
    // A layer with nothing to draw yet computes at once.
    let first = doc.node(id).is_some_and(|n| {
        (generated::is_generated(n) && generated::lookup(doc, n).is_none()) || (stencil::is_stencil(n) && stencil::lookup(doc, n).is_none())
    });
    let ready = {
        let mut g = sched();
        let s = g.get_or_insert_with(Sched::default);
        match s.waiting.iter_mut().find(|w| w.0 == id) {
            Some(w) if w.1 == ptr => first || now.duration_since(w.2) >= SETTLE,
            Some(w) => {
                *w = (id, ptr, now);
                first
            }
            None => {
                s.waiting.push((id, ptr, now));
                first
            }
        }
    };
    if ready {
        run(doc, id);
        if let Some(s) = sched().as_mut() {
            s.waiting.retain(|w| w.0 != id);
        }
    }
}
