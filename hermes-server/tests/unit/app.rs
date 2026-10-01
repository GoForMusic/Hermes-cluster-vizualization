use super::*;

#[test]
fn the_listen_address_reads_like_the_go_hubs() {
    assert_eq!(parse_addr(":8765").unwrap().to_string(), "0.0.0.0:8765");
    assert_eq!(
        parse_addr("127.0.0.1:9").unwrap().to_string(),
        "127.0.0.1:9"
    );
    assert!(parse_addr("nonsense").is_err());
}

#[test]
fn incidents_are_kept_thirty_days_unless_the_admin_says_otherwise() {
    assert_eq!(incident_days(None), 30);
    assert_eq!(incident_days(Some("{}")), 30);
    assert_eq!(incident_days(Some("not json")), 30);
    assert_eq!(incident_days(Some(r#"{"incidentDays": 7}"#)), 7);
    assert_eq!(
        incident_days(Some(r#"{"incidentDays": 0}"#)),
        1,
        "never less than a day: 0 would forget everything at once"
    );
    assert_eq!(incident_days(Some(r#"{"incidentDays": 999999}"#)), 3650);
}
