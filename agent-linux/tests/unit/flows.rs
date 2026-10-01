use super::*;

// what the lab's kernel really printed (shortened): loadgen asks a Service address, a pod answers
const SERVICE_CALL: &str = "ipv4     2 tcp      6 94 TIME_WAIT src=10.244.228.88 dst=10.107.91.120 sport=43032 dport=80 packets=151 bytes=7941 src=10.244.228.87 dst=10.244.228.88 sport=80 dport=43032 packets=177 bytes=5252347 [ASSURED] mark=0 zone=0 use=2";
const DNS: &str = "ipv4     2 udp      17 4 src=10.244.228.88 dst=10.96.0.10 sport=50564 dport=53 packets=2 bytes=144 src=10.244.46.32 dst=10.244.228.88 sport=53 dport=50564 packets=2 bytes=279 mark=0 zone=0 use=2";
const LOOPBACK: &str = "ipv4     2 tcp      6 107 TIME_WAIT src=127.0.0.1 dst=127.0.0.1 sport=38758 dport=9098 packets=6 bytes=429 src=127.0.0.1 dst=127.0.0.1 sport=9098 dport=38758 packets=4 bytes=1117 [ASSURED] mark=0 zone=0 use=2";
const NO_BYTES: &str = "ipv4     2 udp      17 29 src=192.168.150.22 dst=192.168.150.21 sport=58449 dport=4789 [UNREPLIED] src=192.168.150.21 dst=192.168.150.22 sport=4789 dport=58449 mark=0 zone=0 use=2";

fn conn(src: &str, dst: &str, sport: u16, out: u64, reply_src: &str, back: u64) -> Conn {
    Conn {
        proto: "tcp".into(),
        src: src.into(),
        dst: dst.into(),
        sport,
        dport: 80,
        bytes_out: out,
        reply_src: reply_src.into(),
        bytes_in: back,
        counted: true,
    }
}

#[test]
fn a_line_says_who_asked_for_what_who_answered_and_how_much_moved() {
    let c = &parse(SERVICE_CALL)[0];
    assert_eq!(
        (
            c.proto.as_str(),
            c.src.as_str(),
            c.dst.as_str(),
            c.reply_src.as_str()
        ),
        ("tcp", "10.244.228.88", "10.107.91.120", "10.244.228.87")
    );
    assert_eq!(
        (c.sport, c.dport, c.bytes_out, c.bytes_in, c.counted),
        (43032, 80, 7941, 5_252_347, true)
    );
    let d = &parse(DNS)[0];
    assert_eq!(
        (d.proto.as_str(), d.dport, d.bytes_out, d.bytes_in),
        ("udp", 53, 144, 279)
    );
}

#[test]
fn a_line_without_byte_counters_is_still_a_connection_but_not_a_counted_one() {
    let c = &parse(NO_BYTES)[0];
    assert!(!c.counted);
    assert_eq!((c.bytes_out, c.bytes_in), (0, 0));
}

#[test]
fn other_protocols_and_garbage_are_left_out() {
    let text = format!(
        "ipv4 2 icmp 1 29 src=10.0.0.1 dst=10.0.0.2 type=8 code=0 id=1 packets=1 bytes=84 src=10.0.0.2 dst=10.0.0.1 type=0 code=0 id=1 packets=1 bytes=84\nnot a line\n\n{SERVICE_CALL}\nipv4 2 tcp 6 5 src=1.1.1.1\n"
    );
    assert_eq!(parse(&text).len(), 1);
}

#[test]
fn ipv6_lines_are_read_too() {
    let line = "ipv6     10 tcp      6 100 ESTABLISHED src=fd00::1 dst=fd00::2 sport=40000 dport=443 packets=3 bytes=300 src=fd00::2 dst=fd00::1 sport=443 dport=40000 packets=4 bytes=4000 [ASSURED] mark=0 use=1";
    let c = &parse(line)[0];
    assert_eq!(
        (c.src.as_str(), c.dst.as_str(), c.bytes_in),
        ("fd00::1", "fd00::2", 4000)
    );
}

#[test]
fn the_first_look_only_sets_the_baseline_and_after_that_only_growth_counts() {
    let mut t = Tracker::default();
    let start = conn(
        "10.0.0.1",
        "10.96.0.9",
        1000,
        1_000_000,
        "10.0.0.5",
        9_000_000,
    );
    assert!(
        t.step(std::slice::from_ref(&start), 5.0).is_empty(),
        "what it had moved before we looked is not this interval's"
    );
    let grown = conn(
        "10.0.0.1",
        "10.96.0.9",
        1000,
        1_625_000,
        "10.0.0.5",
        9_625_000,
    );
    let flows = t.step(&[grown], 5.0);
    assert_eq!(flows.len(), 1);
    let f = &flows[0];
    assert_eq!(
        (
            f.src.as_str(),
            f.dst.as_str(),
            f.served_by.as_str(),
            f.port,
            f.proto.as_str()
        ),
        ("10.0.0.1", "10.96.0.9", "10.0.0.5", 80, "tcp")
    );
    assert!(
        (f.out_mbps - 1.0).abs() < 1e-9 && (f.in_mbps - 1.0).abs() < 1e-9,
        "625 kB in 5 s is 1 Mb/s: {f:?}"
    );
}

#[test]
fn a_connection_that_appears_after_the_first_look_counts_in_full() {
    let mut t = Tracker::default();
    t.step(&[], 5.0);
    let short = conn("10.0.0.1", "10.96.0.9", 2000, 625_000, "10.0.0.5", 625_000); // opened and closed inside one interval
    let f = &t.step(&[short], 5.0)[0];
    assert!((f.out_mbps - 1.0).abs() < 1e-9);
    assert!(t.step(&[], 5.0).is_empty(), "and it is gone the next time");
}

#[test]
fn connections_between_the_same_addresses_add_up_and_a_service_that_answers_as_itself_has_no_served_by()
 {
    let mut t = Tracker::default();
    t.step(&[], 5.0);
    let flows = t.step(
        &[
            conn("10.0.0.1", "10.96.0.9", 1, 625_000, "10.0.0.5", 0),
            conn("10.0.0.1", "10.96.0.9", 2, 625_000, "10.0.0.6", 0),
            conn("10.0.0.1", "10.0.0.7", 3, 625_000, "10.0.0.7", 0),
        ],
        5.0,
    );
    assert_eq!(flows.len(), 3, "the answering pod is part of the pair");
    let direct = flows.iter().find(|f| f.dst == "10.0.0.7").unwrap();
    assert_eq!(direct.served_by, "");
}

#[test]
fn a_counter_that_went_down_is_a_connection_that_began_again() {
    let mut t = Tracker::default();
    t.step(
        &[conn("10.0.0.1", "10.0.0.2", 5, 5_000_000, "10.0.0.2", 0)],
        5.0,
    );
    let f = &t.step(
        &[conn("10.0.0.1", "10.0.0.2", 5, 625_000, "10.0.0.2", 0)],
        5.0,
    )[0];
    assert!((f.out_mbps - 1.0).abs() < 1e-9, "{f:?}");
}

#[test]
fn loopback_connections_and_uncounted_ones_say_nothing() {
    let mut t = Tracker::default();
    let mut all = parse(&format!("{LOOPBACK}\n{NO_BYTES}\n{DNS}"));
    t.step(&all, 5.0);
    for c in &mut all {
        c.bytes_out += 625_000;
    }
    let flows = t.step(&all, 5.0);
    assert_eq!(
        flows.iter().map(|f| f.dst.as_str()).collect::<Vec<_>>(),
        ["10.96.0.10"]
    );
}
