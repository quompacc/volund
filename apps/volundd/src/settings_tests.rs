use super::*;

#[test]
fn registry_rejects_unknown_shapes_and_values() {
    assert!(validate_instance_name(&json!("My VÖLUND")).is_ok());
    assert!(validate_instance_name(&json!(" VÖLUND")).is_err());
    assert!(validate_locale(&json!("de-DE")).is_ok());
    assert!(validate_locale(&json!("unknown")).is_err());
    assert!(validate_time_zone(&json!("Europe/Berlin")).is_ok());
    assert!(validate_time_zone(&json!("Mars/Olympus")).is_err());
    assert!(validate_session_idle(&json!(30)).is_ok());
    assert!(validate_session_idle(&json!("30")).is_err());
    assert!(validate_catalog_view(&json!(42)).is_err());
    assert!(validate_socket_address(&json!("127.0.0.1:8080")).is_ok());
    assert!(validate_socket_address(&json!("all interfaces")).is_err());
    assert_eq!(
        parse_environment_value("boolean", "true").expect("valid operator boolean"),
        json!(true)
    );
    assert!(parse_environment_value("boolean", "yes").is_err());
    let database = definitions()
        .into_iter()
        .find(|definition| definition.key == "database.connectionOverride")
        .expect("database diagnostic definition");
    let diagnostic = operator_view(database, Some("postgresql://user:secret@host/database"))
        .expect("masked database diagnostic");
    assert_eq!(diagnostic.value, Value::Null);
    assert_eq!(diagnostic.configured, Some(true));
    assert_eq!(diagnostic.origin, "environment");
    assert!(!diagnostic.editable);
}
