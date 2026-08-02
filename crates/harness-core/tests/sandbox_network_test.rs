use std::collections::BTreeSet;

use harness_core::sandbox::{
    evaluate_network_confinement_with_landlock, LandlockSupport, NetworkConfinementStatus,
    SandboxNetworkPolicy, SandboxPlatform,
};

#[test]
fn network_policy_parses_deny_unrestricted_and_allowed_tcp_ports() {
    // arrange
    let allowed_ports = BTreeSet::from([443, 8443]);

    // act
    let deny_all = SandboxNetworkPolicy::parse("deny");
    let unrestricted = SandboxNetworkPolicy::parse("unrestricted");
    let allow_tcp = SandboxNetworkPolicy::parse("tcp:8443,443");

    // assert
    assert_eq!(deny_all, Ok(SandboxNetworkPolicy::DenyAll));
    assert_eq!(unrestricted, Ok(SandboxNetworkPolicy::Unrestricted));
    assert_eq!(
        allow_tcp,
        Ok(SandboxNetworkPolicy::AllowTcpPorts { allowed_ports })
    );
}

#[test]
fn network_policy_rejects_empty_zero_and_unknown_endpoint_declarations() {
    // arrange
    let invalid_policies = ["", "tcp:", "tcp:0", "tcp:443,abc", "localhost:443"];

    // act
    let results: Vec<_> = invalid_policies
        .iter()
        .map(|value| (value, SandboxNetworkPolicy::parse(value)))
        .collect();

    // assert
    for (value, parsed) in results {
        assert!(parsed.is_err(), "network policy {value:?} must fail closed");
    }
}

#[test]
fn requested_network_confinement_reports_unavailable_without_landlock() {
    // arrange
    let support = LandlockSupport::Unavailable {
        reason: "test kernel has no Landlock".to_string(),
    };

    // act
    let status = evaluate_network_confinement_with_landlock(
        &SandboxNetworkPolicy::DenyAll,
        SandboxPlatform::Linux,
        &support,
    );

    // assert
    assert!(matches!(
        status,
        NetworkConfinementStatus::Unavailable { reason, .. }
            if reason.contains("test kernel has no Landlock")
    ));
}
