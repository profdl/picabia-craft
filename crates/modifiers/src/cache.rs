//! Process-wide caches: parsed stacks by source text, evaluated outputs by (source, bounds).
//!
//! One `Mutex` (works the same on wasm32, where there is one thread). Both maps are bounded; when
//! one is full it is cleared, which is simple and fine for a cache that refills in microseconds.
//! A poisoned lock is recovered: the caches hold no invariants a panic could break.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard};

use figcraft_geom::Rect;

use crate::eval::{Output, evaluate};
use crate::model::Stack;
use crate::{ModError, parse};

/// The most stacks kept parsed, and the most source bytes they may hold.
const MAX_PARSED: usize = 1024;
const MAX_PARSED_BYTES: usize = 8 * 1024 * 1024;
/// The most outputs kept, and the most instances they may hold in all.
const MAX_OUTPUTS: usize = 1024;
const MAX_OUTPUT_INSTANCES: usize = 200_000;

struct Parsed {
    src: Box<str>,
    stack: Result<Arc<Stack>, ModError>,
}

#[derive(Default)]
struct Caches {
    parsed: HashMap<u64, Parsed>,
    parsed_bytes: usize,
    outputs: HashMap<(u64, usize, [u64; 4]), Arc<Output>>,
    output_instances: usize,
}

static CACHES: Mutex<Option<Caches>> = Mutex::new(None);

fn lock() -> MutexGuard<'static, Option<Caches>> {
    CACHES.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn hash(src: &str) -> u64 {
    let mut h = DefaultHasher::new();
    src.hash(&mut h);
    h.finish()
}

/// [`parse`] once per distinct source.
pub(crate) fn parsed(src: &str) -> Result<Arc<Stack>, ModError> {
    let key = hash(src);
    if let Some(c) = lock().as_ref()
        && let Some(p) = c.parsed.get(&key)
        && &*p.src == src
    {
        return p.stack.clone();
    }
    // Parsed outside the lock: another thread may parse the same source meanwhile (harmless).
    let stack = parse(src).map(Arc::new);
    let mut guard = lock();
    let c = guard.get_or_insert_with(Caches::default);
    if c.parsed.len() >= MAX_PARSED || c.parsed_bytes + src.len() > MAX_PARSED_BYTES {
        c.parsed.clear();
        c.parsed_bytes = 0;
    }
    if src.len() <= MAX_PARSED_BYTES {
        c.parsed_bytes += src.len();
        if let Some(old) = c.parsed.insert(key, Parsed { src: src.into(), stack: stack.clone() }) {
            c.parsed_bytes = c.parsed_bytes.saturating_sub(old.src.len());
        }
    }
    stack
}

/// [`evaluate`] once per distinct (source, bounds). `Err` when the source doesn't parse.
pub(crate) fn evaluated(src: &str, bounds: Rect) -> Result<Arc<Output>, ModError> {
    let stack = parsed(src)?;
    let key = (hash(src), src.len(), [bounds.x0.to_bits(), bounds.y0.to_bits(), bounds.x1.to_bits(), bounds.y1.to_bits()]);
    if let Some(c) = lock().as_ref()
        && let Some(o) = c.outputs.get(&key)
    {
        return Ok(o.clone());
    }
    let out = Arc::new(evaluate(&stack, bounds));
    let mut guard = lock();
    let c = guard.get_or_insert_with(Caches::default);
    let n = out.instances.len();
    if c.outputs.len() >= MAX_OUTPUTS || c.output_instances + n > MAX_OUTPUT_INSTANCES {
        c.outputs.clear();
        c.output_instances = 0;
    }
    if n <= MAX_OUTPUT_INSTANCES {
        c.output_instances += n;
        if let Some(old) = c.outputs.insert(key, out.clone()) {
            c.output_instances = c.output_instances.saturating_sub(old.instances.len());
        }
    }
    Ok(out)
}
