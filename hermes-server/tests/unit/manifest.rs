use super::*;

fn params(full: bool) -> AgentParams<'static> {
    AgentParams {
        image: "registry.example.com/infraviz/agent:1.2.3",
        hub_url: "https://hub.example.com:8765",
        source_id: "s1a2b3c4d5e6",
        source_name: "prod cluster",
        token: "0123456789abcdef0123456789abcdef0123456789abcdef",
        windows_image: if full {
            "registry.example.com/infraviz/agent-windows:1.2.3-ltsc2022"
        } else {
            ""
        },
        pull_secret: if full { "regcred" } else { "" },
        pull_secret_data: "",
        flows: false,
        upgrades: false,
    }
}

// The golden files were rendered by the Go hub's own templates (see tests/golden): the Rust hub must produce the same bytes.
#[test]
fn the_manifests_are_the_ones_the_go_hub_produced() {
    for (kind, file) in [
        (TYPE_KUBERNETES_AGENT, "kubernetes"),
        (TYPE_SWARM_AGENT, "swarm"),
    ] {
        for (full, variant) in [(true, "full"), (false, "bare")] {
            let golden =
                std::fs::read_to_string(format!("tests/golden/{file}_{variant}.yaml")).unwrap();
            let (out, _) = render(kind, &params(full)).unwrap();
            assert_eq!(out, golden, "{file} ({variant})");
        }
        let hint = std::fs::read_to_string(format!("tests/golden/{file}.hint")).unwrap();
        assert_eq!(render(kind, &params(true)).unwrap().1, hint);
    }
}

#[test]
fn optional_parts_appear_only_when_asked_for() {
    let (k8s, _) = render(TYPE_KUBERNETES_AGENT, &params(true)).unwrap();
    assert!(k8s.contains("imagePullSecrets") && k8s.contains("- name: regcred"));
    let (k8s, _) = render(TYPE_KUBERNETES_AGENT, &params(false)).unwrap();
    assert!(!k8s.contains("imagePullSecrets"));
    let (swarm, _) = render(TYPE_SWARM_AGENT, &params(false)).unwrap();
    assert!(
        !swarm.contains("ltsc2022") && swarm.contains("{{.Node.ID}}"),
        "Swarm's own placeholder is left for Swarm to fill in"
    );
}

#[test]
fn a_name_with_a_quote_cannot_break_out_of_the_yaml() {
    let p = AgentParams {
        source_name: r#"evil" } ; injected: true #"#,
        ..params(false)
    };
    let (out, _) = render(TYPE_SWARM_AGENT, &p).unwrap();
    assert!(
        out.contains(r#"SOURCE_NAME: "evil\" } ; injected: true #""#),
        "{out}"
    );
}

#[test]
fn an_unknown_type_has_no_installer() {
    assert!(render("Nomad (agent)", &params(false)).is_err());
    assert_eq!(
        agent_types(),
        ["Docker Swarm (agent)", "Kubernetes (agent)"]
    );
}

#[test]
fn who_talks_to_whom_puts_the_node_agent_in_the_node_network_and_only_then() {
    let flows = AgentParams {
        flows: true,
        ..params(false)
    };
    let (on, _) = render(TYPE_KUBERNETES_AGENT, &flows).unwrap();
    let node = &on[on.find("name: infraviz-node").unwrap()..];
    assert!(
        node.contains("hostNetwork: true") && node.contains("ClusterFirstWithHostNet"),
        "the node agent runs in the node's network"
    );
    assert!(node.contains("name: FLOWS, value: \"1\""));
    assert!(
        node.contains("capabilities: { drop: [\"ALL\"] }")
            && !node.contains("add: [")
            && !node.contains("privileged: true"),
        "no capability is added"
    );
    assert_eq!(
        on.matches("kind: DaemonSet").count(),
        1,
        "one agent on each node, not a second one for the traffic"
    );
    let (off, _) = render(TYPE_KUBERNETES_AGENT, &params(false)).unwrap();
    assert!(
        !off.contains("hostNetwork") && !off.contains("FLOWS"),
        "off by default: the manifest is the same as before"
    );
}

#[test]
fn a_registry_login_becomes_a_pull_secret_in_the_kubernetes_manifest() {
    let mut p = params(false);
    p.pull_secret = "infraviz-registry";
    let data = docker_config("git.example.com", "robot", "s3cret");
    p.pull_secret_data = &data;
    let (out, _) = render(TYPE_KUBERNETES_AGENT, &p).unwrap();
    assert!(out.contains("kind: Secret") && out.contains("type: kubernetes.io/dockerconfigjson"));
    assert!(out.contains(&format!(".dockerconfigjson: {data}")));
    assert_eq!(
        out.matches("- name: infraviz-registry").count(),
        2,
        "both the agent and the node agent use it"
    );
    let decoded = String::from_utf8(
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &data).unwrap(),
    )
    .unwrap();
    assert!(decoded.contains("\"git.example.com\"") && decoded.contains("cm9ib3Q6czNjcmV0"));
}

#[test]
fn allowing_upgrades_adds_exactly_two_named_objects_to_the_rights_and_the_switch() {
    let (off, _) = render(TYPE_KUBERNETES_AGENT, &params(false)).unwrap();
    assert!(
        !off.contains("UPGRADES") && !off.contains("kind: Role\n"),
        "read-only unless allowed"
    );
    let (on, _) = render(
        TYPE_KUBERNETES_AGENT,
        &AgentParams {
            upgrades: true,
            ..params(false)
        },
    )
    .unwrap();
    assert!(on.contains("kind: Role\n") && on.contains("kind: RoleBinding"));
    assert!(
        on.contains("resourceNames: [\"infraviz-agent\"]")
            && on.contains("resourceNames: [\"infraviz-node\"]")
    );
    assert_eq!(
        on.matches("verbs: [\"get\", \"patch\"]").count(),
        2,
        "nothing beyond get and patch"
    );
    assert_eq!(
        on.matches("name: UPGRADES, value: \"1\"").count(),
        1,
        "only the cluster reader may do it, not the node agents"
    );
    let (swarm_off, _) = render(TYPE_SWARM_AGENT, &params(false)).unwrap();
    let (swarm_on, _) = render(
        TYPE_SWARM_AGENT,
        &AgentParams {
            upgrades: true,
            ..params(false)
        },
    )
    .unwrap();
    assert!(!swarm_off.contains("UPGRADES") && swarm_on.contains("UPGRADES: \"1\""));
}
