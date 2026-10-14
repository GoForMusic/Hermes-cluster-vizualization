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

// ---- the master <-> member contract -----------------------------------------------------------------------------

mod federation {
    use hermes_proto::v1::{MasterMessage, MemberMessage, master_message, member_message};
    use prost::Message;

    /// The first version of the link, written field by field: the numbers are the contract, and a hub of another version reads them.
    #[derive(Clone, PartialEq, Message)]
    struct OldMasterHello {
        #[prost(uint32, tag = "1")]
        protocol: u32,
        #[prost(string, tag = "2")]
        datacenter: String,
        #[prost(string, tag = "3")]
        version: String,
    }

    #[derive(Clone, PartialEq, Message)]
    struct OldMasterMessage {
        #[prost(oneof = "old_master::Message", tags = "1")]
        message: Option<old_master::Message>,
    }

    mod old_master {
        use prost::Oneof;

        #[derive(Clone, PartialEq, Oneof)]
        pub enum Message {
            #[prost(message, tag = "1")]
            Hello(super::OldMasterHello),
        }
    }

    #[derive(Clone, PartialEq, Message)]
    struct OldMemberHello {
        #[prost(uint32, tag = "1")]
        protocol: u32,
        #[prost(string, tag = "2")]
        version: String,
        #[prost(string, tag = "3")]
        agent_address: String,
    }

    #[derive(Clone, PartialEq, Message)]
    struct OldMemberMessage {
        #[prost(oneof = "old_member::Message", tags = "1")]
        message: Option<old_member::Message>,
    }

    mod old_member {
        use prost::Oneof;

        #[derive(Clone, PartialEq, Oneof)]
        pub enum Message {
            #[prost(message, tag = "1")]
            Hello(super::OldMemberHello),
        }
    }

    #[test]
    fn the_hellos_of_the_first_version_of_the_link_are_still_read() {
        let old = OldMasterMessage {
            message: Some(old_master::Message::Hello(OldMasterHello {
                protocol: 1,
                datacenter: "dc2".into(),
                version: "1.0.0".into(),
            })),
        };
        let new = MasterMessage::decode(old.encode_to_vec().as_slice()).unwrap();
        let Some(master_message::Message::Hello(h)) = new.message else {
            panic!("not a hello")
        };
        assert_eq!(
            (h.protocol, h.datacenter.as_str(), h.version.as_str()),
            (1, "dc2", "1.0.0")
        );

        let old = OldMemberMessage {
            message: Some(old_member::Message::Hello(OldMemberHello {
                protocol: 1,
                version: "1.0.0".into(),
                agent_address: "http://hub:8765".into(),
            })),
        };
        let new = MemberMessage::decode(old.encode_to_vec().as_slice()).unwrap();
        let Some(member_message::Message::Hello(h)) = new.message else {
            panic!("not a hello")
        };
        assert_eq!(h.agent_address, "http://hub:8765");
        assert!(
            h.features.is_empty(),
            "a member that knows no features says none"
        );
    }

    #[test]
    fn a_message_of_a_kind_this_version_does_not_know_is_ignored_not_an_error() {
        // field 99 of a MemberMessage: a case a later version added
        let mut bytes = Vec::new();
        prost::encoding::encode_key(99, prost::encoding::WireType::LengthDelimited, &mut bytes);
        prost::encoding::encode_varint(2, &mut bytes);
        bytes.extend_from_slice(b"xx");
        let m = MemberMessage::decode(bytes.as_slice()).unwrap();
        assert!(m.message.is_none());
    }
}
