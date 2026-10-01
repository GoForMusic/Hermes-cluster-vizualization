use super::*;
use serde_json::json;

#[test]
fn round_trip_keeps_the_shape() {
    let meta = json!({"image": "nginx:1.27", "restarts": 3, "ready": true, "ports": [80, 443], "labels": {"app": "web"}, "gone": null, "ratio": 0.5});
    assert_eq!(json_from_struct(&struct_from_json(meta.clone())), meta);
}

#[test]
fn whole_numbers_stay_integers() {
    assert_eq!(json_from_value(&value_from_json(json!(3))).to_string(), "3");
}

#[test]
fn non_objects_become_empty() {
    assert!(struct_from_json(json!([1, 2])).fields.is_empty());
}
