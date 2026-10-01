//! The contract changes independently of the agents installed in the field. These are the messages as the FIRST version of the contract
//! wrote them (before agents could be upgraded from the dashboard), kept here word for word: the hub must read what an old agent sends, and
//! an old agent must survive what the new hub sends. proto3 gives this as long as fields are only added, never renumbered or reused; this
//! test is the one that says so when someone forgets.

use hermes_proto::v1::{AgentMessage, Hello, HubMessage, Upgrade, agent_message, hub_message};
use prost::Message;

/// `Hello` before `features` (field 6) and `UpgradeStatus` existed.
#[derive(Clone, PartialEq, Message)]
struct OldHello {
    #[prost(uint32, tag = "1")]
    protocol: u32,
    #[prost(string, tag = "2")]
    agent: String,
    #[prost(string, tag = "3")]
    host: String,
    #[prost(string, tag = "4")]
    version: String,
    #[prost(string, tag = "5")]
    collector: String,
}

#[derive(Clone, PartialEq, Message)]
struct OldAgentMessage {
    #[prost(oneof = "old_agent::Message", tags = "1, 2")]
    message: Option<old_agent::Message>,
}

mod old_agent {
    use prost::Oneof;

    #[derive(Clone, PartialEq, Oneof)]
    pub enum Message {
        #[prost(message, tag = "1")]
        Hello(super::OldHello),
        #[prost(message, tag = "2")]
        Batch(hermes_proto::v1::Batch),
    }
}

/// `HubMessage` before `upgrade` (field 2): only `resync` (field 1).
#[derive(Clone, PartialEq, Message)]
struct OldHubMessage {
    #[prost(oneof = "old_hub::Message", tags = "1")]
    message: Option<old_hub::Message>,
}

mod old_hub {
    use prost::Oneof;

    #[derive(Clone, PartialEq, Oneof)]
    pub enum Message {
        #[prost(message, tag = "1")]
        Resync(hermes_proto::v1::Resync),
    }
}

#[test]
fn the_hub_reads_the_hello_of_an_agent_that_knows_nothing_of_upgrades() {
    let old = OldAgentMessage {
        message: Some(old_agent::Message::Hello(OldHello {
            protocol: 1,
            agent: "pod-1".into(),
            host: "h".into(),
            version: "1.0.0".into(),
            collector: "kubernetes".into(),
        })),
    };
    let new = AgentMessage::decode(old.encode_to_vec().as_slice()).unwrap();
    let Some(agent_message::Message::Hello(h)) = new.message else {
        panic!("not a hello")
    };
    assert_eq!(
        (h.agent.as_str(), h.version.as_str(), h.collector.as_str()),
        ("pod-1", "1.0.0", "kubernetes")
    );
    assert!(
        h.features.is_empty(),
        "an old agent can do nothing beyond reporting, and says so by saying nothing"
    );
}

#[test]
fn an_old_agent_ignores_what_it_does_not_know_and_still_reads_the_rest() {
    let asked = HubMessage {
        message: Some(hub_message::Message::Upgrade(Upgrade {
            version: "1.0.5".into(),
        })),
    };
    let old = OldHubMessage::decode(asked.encode_to_vec().as_slice()).unwrap();
    assert_eq!(
        old.message, None,
        "an upgrade is not something it understands, and it is not an error either"
    );
    let resync = HubMessage {
        message: Some(hub_message::Message::Resync(hermes_proto::v1::Resync {})),
    };
    let old = OldHubMessage::decode(resync.encode_to_vec().as_slice()).unwrap();
    assert!(matches!(old.message, Some(old_hub::Message::Resync(_))));
}

#[test]
fn a_new_hello_is_read_by_an_old_hub() {
    let new = AgentMessage {
        message: Some(agent_message::Message::Hello(Hello {
            protocol: 1,
            agent: "a".into(),
            version: "1.0.5".into(),
            features: vec!["upgrade".into()],
            ..Default::default()
        })),
    };
    let old = OldAgentMessage::decode(new.encode_to_vec().as_slice()).unwrap();
    let Some(old_agent::Message::Hello(h)) = old.message else {
        panic!("not a hello")
    };
    assert_eq!((h.agent.as_str(), h.version.as_str()), ("a", "1.0.5"));
}
