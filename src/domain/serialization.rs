pub(crate) fn is_default<Value: Default + PartialEq>(value: &Value) -> bool {
    value == &Value::default()
}
