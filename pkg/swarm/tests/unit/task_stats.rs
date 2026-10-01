use std::collections::HashMap;

use super::*;

fn attached(list: &[(&str, &str)]) -> HashMap<String, ContainerNetwork> {
    list.iter()
        .map(|(name, id)| {
            (
                (*name).to_string(),
                ContainerNetwork {
                    id: (*id).to_string(),
                },
            )
        })
        .collect()
}

fn interfaces(list: &[(&str, u64, u64)]) -> HashMap<String, NetStats> {
    list.iter()
        .map(|(name, rx, tx)| {
            (
                (*name).to_string(),
                NetStats {
                    rx_bytes: *rx,
                    tx_bytes: *tx,
                },
            )
        })
        .collect()
}

#[test]
fn a_task_on_one_network_has_that_network_on_eth0_and_its_way_out_on_eth1() {
    let a = attached(&[("demo_demonet", "net-1")]);
    let i = interfaces(&[("eth0", 100, 200), ("eth1", 5, 6)]);
    let (network, c) = overlay_counters(&a, &i).unwrap();
    assert_eq!(
        (network, c.rx_bytes, c.tx_bytes),
        ("net-1", 100, 200),
        "eth1 is docker_gwbridge: it is not the network's traffic"
    );
}

#[test]
fn the_routing_mesh_is_not_a_network_of_the_task_for_this() {
    let a = attached(&[("ingress", "mesh"), ("demo_demonet", "net-1")]);
    let i = interfaces(&[("eth0", 1, 2), ("eth1", 3, 4), ("eth2", 5, 6)]);
    assert_eq!(overlay_counters(&a, &i).unwrap().0, "net-1");
}

#[test]
fn with_more_than_one_network_no_network_is_claimed_because_the_interfaces_do_not_say_which_is_which()
 {
    let a = attached(&[("front", "net-1"), ("back", "net-2")]);
    assert!(overlay_counters(&a, &interfaces(&[("eth0", 1, 2), ("eth1", 3, 4)])).is_none());
}

#[test]
fn a_task_with_no_network_or_no_interface_says_nothing() {
    assert!(overlay_counters(&attached(&[]), &interfaces(&[("eth0", 1, 2)])).is_none());
    assert!(overlay_counters(&attached(&[("n", "net-1")]), &interfaces(&[])).is_none());
    assert!(
        overlay_counters(&attached(&[("n", "")]), &interfaces(&[("eth0", 1, 2)])).is_none(),
        "a network with no id cannot be named"
    );
}

#[test]
fn a_windows_container_with_one_interface_named_otherwise_is_still_read() {
    let a = attached(&[("nat", "net-1")]);
    let i = interfaces(&[("Ethernet", 7, 8)]);
    assert_eq!(overlay_counters(&a, &i).unwrap().1.rx_bytes, 7);
}
