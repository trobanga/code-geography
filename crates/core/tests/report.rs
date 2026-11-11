use cg_core::CallReport;

#[test]
fn to_value_adds_up_and_is_zero_when_a_report_without_it_is_read() {
    let a = CallReport {
        to_value: 2,
        to_function: 1,
        ..CallReport::default()
    };
    let b = CallReport {
        to_value: 3,
        external: 4,
        ..CallReport::default()
    };

    assert_eq!((a + b).to_value, 5);

    let old = r#"{"to_function":1,"to_type":2,"external":3,"unresolved":4}"#;
    let read: CallReport = serde_json::from_str(old).unwrap();
    assert_eq!(read.to_value, 0);
    assert_eq!(read.unresolved, 4);
}
