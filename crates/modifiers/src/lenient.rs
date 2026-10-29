//! Lenient field deserializers: counts and seeds accept any finite number (rounded, clamped), so a
//! stack written by hand or by an agent (`"count": 3.0`, `"count": -2`) still reads.

use serde::{Deserialize, Deserializer, de::Error};
use serde_json::Value;

/// A finite number from a JSON number or a numeric string (`"12"`, `" 4.5 "`).
pub(crate) fn finite(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

fn whole<'de, D: Deserializer<'de>>(d: D, max: f64) -> Result<u32, D::Error> {
    let v = Value::deserialize(d)?;
    let f = finite(&v).ok_or_else(|| D::Error::custom(format!("expected a whole number, got {v}")))?;
    // Clamped before the cast: `as` saturates anyway, but the range is the point.
    Ok(f.round().clamp(0.0, max) as u32)
}

/// A copy count: rounded and clamped to `0..=MAX_COUNT`.
pub(crate) fn count<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    whole(d, f64::from(crate::MAX_COUNT))
}

/// A seed: rounded and clamped to the `u32` range.
pub(crate) fn seed<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    whole(d, f64::from(u32::MAX))
}
