use crate::reactor::perf::Scenario;

#[test]
fn checked_in_scenarios_are_valid() {
	for toml in [
		include_str!("baseline.toml"),
		include_str!("animated-high-res.toml"),
	] {
		Scenario::from_toml(toml)
			.expect("checked-in performance scenario must be valid");
	}
}

#[test]
fn scenarios_require_a_final_finish_event() {
	let error = Scenario::from_toml(
		r#"
name = "incomplete"
events = [{ at_ms = 0, command = { type = "next" } }]
"#,
	)
	.expect_err("a scenario without finish must be rejected");
	assert!(error.contains("final event"));
}

#[test]
fn bare_scenario_names_resolve_from_the_performance_directory() {
	let scenario = Scenario::from_reference(std::path::Path::new("baseline"))
		.expect("baseline scenario must resolve by name");
	assert_eq!(scenario.name(), "baseline");
}
