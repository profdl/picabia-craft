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

/// An expression text: a string (blank = none) or a number (taken as its text); `null` = none.
pub(crate) fn expr<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        Value::String(s) if s.len() > crate::expr::MAX_EXPR_LEN => {
            Err(D::Error::custom(format!("expressions are at most {} characters", crate::expr::MAX_EXPR_LEN)))
        }
        Value::String(s) => Ok(Some(s)),
        Value::Number(n) => Ok(Some(n.to_string())),
        v => Err(D::Error::custom(format!("expected an expression text, got {v}"))),
    }
}

/// A layer id as text: a string (trimmed) or a whole number (`12` reads as `"12"`); `null` = none.
pub(crate) fn target<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(String::new()),
        Value::String(s) if s.len() > 64 => Err(D::Error::custom("a target id is at most 64 characters")),
        Value::String(s) => Ok(s.trim().to_string()),
        Value::Number(n) if n.is_u64() => Ok(n.to_string()),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f < 9.0e15 => Ok(format!("{}", f as u64)),
            _ => Err(D::Error::custom(format!("expected a layer id, got {n}"))),
        },
        v => Err(D::Error::custom(format!("expected a layer id, got {v}"))),
    }
}
