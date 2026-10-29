//! Variable bindings on modifier params: keys, type checks and writing bound values.

use figcraft_doc::{Node, VariableType, VariableValue};
use serde_json::json;

use crate::{
    LinearArray, Modifier, ModifierKind, PLUGIN_KEY, Stack, apply_bound_values, binding_key, binding_takes, bound_variable, field_type, parse,
    parse_binding_key, stale_binding_keys, to_json,
};

fn stack_src() -> String {
    let mut s = Stack::default();
    s.insert(9, Modifier::new("m1", "linearArray").unwrap()).unwrap();
    s.insert(9, Modifier::new("m2", "mirror").unwrap()).unwrap();
    to_json(&s)
}

fn num(x: f64) -> VariableValue {
    VariableValue::Number(x)
}

#[test]
fn keys() {
    assert_eq!(binding_key("m1", "count"), "picabia.modifiers/m1/count");
    assert_eq!(parse_binding_key("picabia.modifiers/m1/count"), Some(("m1", "count")));
    for k in ["picabia.modifiers/m1", "picabia.modifiers//count", "picabia.modifiers/m1/", "picabia.modifiers/m1/a/b", "width", "fills/0/color"] {
        assert_eq!(parse_binding_key(k), None, "{k}");
    }
}

#[test]
fn field_types() {
    let lin = ModifierKind::default_for("linearArray").unwrap();
    assert_eq!(field_type(&lin, "count"), Some(VariableType::Number));
    assert_eq!(field_type(&lin, "nope"), None);
    let mirror = ModifierKind::default_for("mirror").unwrap();
    assert_eq!(field_type(&mirror, "keepOriginal"), Some(VariableType::Boolean));
    assert_eq!(field_type(&mirror, "axis"), Some(VariableType::String));
}

#[test]
fn takes_checks_modifier_param_and_type() {
    let mut n = Node::new(figcraft_doc::NodeId(1), figcraft_doc::NodeKind::Rectangle, "r", 10.0, 10.0);
    assert_eq!(binding_takes(&n, "width", VariableType::Number), None, "not Picabia's");
    assert_eq!(binding_takes(&n, &binding_key("m1", "count"), VariableType::Number), Some(false), "no stack");
    n.plugin_data.insert(PLUGIN_KEY.into(), stack_src());
    assert_eq!(binding_takes(&n, &binding_key("m1", "count"), VariableType::Number), Some(true));
    assert_eq!(binding_takes(&n, &binding_key("m1", "count"), VariableType::Color), Some(false));
    assert_eq!(binding_takes(&n, &binding_key("m3", "count"), VariableType::Number), Some(false));
    assert_eq!(binding_takes(&n, &binding_key("m2", "keepOriginal"), VariableType::Boolean), Some(true));
    n.bound.insert(binding_key("m1", "count"), "v1".into());
    assert_eq!(bound_variable(&n, "m1", "count"), Some("v1"));
    assert_eq!(bound_variable(&n, "m1", "offsetX"), None);
}

#[test]
fn bound_values_are_written_into_the_stack() {
    let src = stack_src();
    let next = apply_bound_values(&src, &[(binding_key("m1", "count"), num(7.4)), (binding_key("m2", "axis"), VariableValue::String("both".into()))])
        .unwrap();
    let st = parse(&next).unwrap();
    assert_eq!(st.modifiers[0].kind, ModifierKind::LinearArray(LinearArray { count: 7, ..Default::default() }));
    assert_eq!(st.modifiers[1].to_value()["axis"], json!("both"));
    // Already in place: nothing to write.
    assert_eq!(apply_bound_values(&next, &[(binding_key("m1", "count"), num(7.0))]), None);
    // Values that don't fit are skipped: wrong type, missing modifier or param, bad choice,
    // non-finite numbers, damaged data.
    for v in [
        (binding_key("m1", "count"), VariableValue::Boolean(true)),
        (binding_key("m9", "count"), num(2.0)),
        (binding_key("m1", "nope"), num(2.0)),
        (binding_key("m2", "axis"), VariableValue::String("sideways".into())),
        (binding_key("m1", "count"), num(f64::NAN)),
        ("width".to_string(), num(2.0)),
    ] {
        assert_eq!(apply_bound_values(&src, std::slice::from_ref(&v)), None, "{v:?}");
    }
    assert_eq!(apply_bound_values("{damaged", &[(binding_key("m1", "count"), num(2.0))]), None);
}

#[test]
fn stale_keys_are_those_without_their_modifier() {
    let st = parse(&stack_src()).unwrap();
    let bound: std::collections::BTreeMap<String, String> =
        [(binding_key("m1", "count"), "a"), (binding_key("m7", "count"), "b"), ("width".into(), "c")]
            .into_iter()
            .map(|(k, v)| (k, v.to_string()))
            .collect();
    assert_eq!(stale_binding_keys(&bound, Some(&st)), [binding_key("m7", "count")]);
    assert_eq!(stale_binding_keys(&bound, None), [binding_key("m1", "count"), binding_key("m7", "count")]);
}
