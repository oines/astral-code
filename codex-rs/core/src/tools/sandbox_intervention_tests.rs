use super::SANDBOX_INTERVENTION_HINT;
use super::append_sandbox_intervention_hint;

#[test]
fn appends_sandbox_intervention_hint_after_existing_output() {
    let mut output = "operation not permitted".to_string();

    append_sandbox_intervention_hint(&mut output);

    assert_eq!(
        output,
        format!("operation not permitted\n\n{SANDBOX_INTERVENTION_HINT}")
    );
}

#[test]
fn appends_sandbox_intervention_hint_to_empty_output() {
    let mut output = String::new();

    append_sandbox_intervention_hint(&mut output);

    assert_eq!(output, SANDBOX_INTERVENTION_HINT);
}
