use super::{ChainInput, PreparedRule, RuleInput, chain_commands, rule_commands};
use crate::api::Command;
use crate::facts::iptables::IpVersion;

fn descriptions(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Shell(command) => Some(command.raw_value()),
            _ => None,
        })
        .collect()
}

#[test]
fn chain_creation_and_policy_changes_are_independently_convergent() {
    let mut input = ChainInput::new("MYCHAIN");
    input.policy = Some("DROP".to_owned());
    assert_eq!(
        descriptions(&chain_commands(&input, None)),
        ["iptables -t filter -N MYCHAIN", "iptables -t filter -P MYCHAIN DROP"]
    );
    assert_eq!(descriptions(&chain_commands(&input, Some("ACCEPT"))), ["iptables -t filter -P MYCHAIN DROP"]);
    input.chain = "x; reboot".to_owned();
    assert_eq!(
        descriptions(&chain_commands(&input, None)),
        ["iptables -t filter -N 'x; reboot'", "iptables -t filter -P 'x; reboot' DROP"]
    );
    input.chain = "MYCHAIN".to_owned();
    input.present = false;
    assert_eq!(descriptions(&chain_commands(&input, Some("ACCEPT"))), ["iptables -t filter -X MYCHAIN"]);
    assert!(chain_commands(&input, None).is_empty());
}

#[test]
fn rule_builds_safe_ipv6_nat_and_comment_arguments_and_noops_when_present() {
    let mut input = RuleInput::new("PREROUTING", "DNAT");
    input.version = IpVersion::V6;
    input.table = "nat".to_owned();
    input.protocol = Some("TCP".to_owned());
    input.source = Some("2001:db8::/32".to_owned());
    input.destination_port = Some("443; bad".to_owned());
    input.comment = Some("web app".to_owned());
    input.to_destination = Some("[2001:db8::2]:8443".to_owned());
    let prepared = PreparedRule::new(&input).unwrap();
    assert_eq!(
        descriptions(&rule_commands(&input, &prepared, false)),
        [
            "ip6tables -t nat -A PREROUTING -p tcp -s 2001:db8::/32 --dport '443; bad' -m comment --comment 'web app' -j DNAT --to-destination '[2001:db8::2]:8443'"
        ]
    );
    assert!(rule_commands(&input, &prepared, true).is_empty());
}

#[test]
fn ports_without_protocol_are_rejected() {
    let mut input = RuleInput::new("INPUT", "DROP");
    input.destination_port = Some("22".to_owned());
    assert!(PreparedRule::new(&input).is_err());
}

#[test]
fn rules_match_filter_nat_negation_comment_space_and_ipv6_fixtures() {
    let mut input = RuleInput::new("PREROUTING", "OUTPUT");
    input.table = "nat".to_owned();
    input.destination_port = Some("8080".to_owned());
    input.source_port = Some("80".to_owned());
    input.protocol = Some("TCP".to_owned());
    input.source = Some("0.0.0.0".to_owned());
    input.destination = Some("1.2.3.4".to_owned());
    input.in_interface = Some("eth0".to_owned());
    input.out_interface = Some("eth1".to_owned());
    input.not_protocol = Some("UDP".to_owned());
    input.not_source = Some("192.168.1.0/24".to_owned());
    input.not_destination = Some("1.2.3.4".to_owned());
    input.not_in_interface = Some("eth10".to_owned());
    input.not_out_interface = Some("eth10".to_owned());
    let prepared = PreparedRule::new(&input).unwrap();
    assert_eq!(
        descriptions(&rule_commands(&input, &prepared, false)),
        [
            "iptables -t nat -A PREROUTING -p tcp -s 0.0.0.0 -d 1.2.3.4 -i eth0 -o eth1 ! -p udp ! -s 192.168.1.0/24 ! -d 1.2.3.4 ! -i eth10 ! -o eth10 --dport 8080 --sport 80 -j OUTPUT"
        ]
    );

    input = RuleInput::new("INPUT", "ACCEPT");
    input.protocol = Some("tcp".to_owned());
    input.destination_port = Some("22; reboot #".to_owned());
    input.source_port = Some("80 && reboot".to_owned());
    let prepared = PreparedRule::new(&input).unwrap();
    assert_eq!(
        descriptions(&rule_commands(&input, &prepared, false)),
        ["iptables -t filter -A INPUT -p tcp --dport '22; reboot #' --sport '80 && reboot' -j ACCEPT"]
    );

    input = RuleInput::new("INPUT", "ACCEPT");
    input.protocol = Some("TCP".to_owned());
    input.comment = Some("This rule is amazing!".to_owned());
    let prepared = PreparedRule::new(&input).unwrap();
    assert_eq!(
        descriptions(&rule_commands(&input, &prepared, false)),
        ["iptables -t filter -A INPUT -p tcp -m comment --comment 'This rule is amazing!' -j ACCEPT"]
    );
    assert!(rule_commands(&input, &prepared, true).is_empty());

    input.protocol = None;
    input.comment = None;
    input.extras = vec!["--another".to_owned(), "argument with spaces".to_owned()];
    let prepared = PreparedRule::new(&input).unwrap();
    assert_eq!(
        descriptions(&rule_commands(&input, &prepared, false)),
        ["iptables -t filter -A INPUT --another 'argument with spaces' -j ACCEPT"]
    );

    input = RuleInput::new("PREROUTING", "OUTPUT");
    input.version = IpVersion::V6;
    input.table = "nat".to_owned();
    input.protocol = Some("TCP".to_owned());
    input.source = Some("0.0.0.0".to_owned());
    let prepared = PreparedRule::new(&input).unwrap();
    assert_eq!(
        descriptions(&rule_commands(&input, &prepared, false)),
        ["ip6tables -t nat -A PREROUTING -p tcp -s 0.0.0.0 -j OUTPUT"]
    );
}

#[test]
fn nat_log_add_delete_noop_and_validation_match_reference_fixtures() {
    let mut log = RuleInput::new("INPUT", "LOG");
    log.log_prefix = Some("log-prefix".to_owned());
    let prepared = PreparedRule::new(&log).unwrap();
    assert_eq!(
        descriptions(&rule_commands(&log, &prepared, false)),
        ["iptables -t filter -A INPUT -j LOG --log-prefix log-prefix"]
    );

    let mut redirect = RuleInput::new("PREROUTING", "REDIRECT");
    redirect.table = "nat".to_owned();
    redirect.to_ports = Some("8000".to_owned());
    let prepared = PreparedRule::new(&redirect).unwrap();
    assert_eq!(
        descriptions(&rule_commands(&redirect, &prepared, false)),
        ["iptables -t nat -A PREROUTING -j REDIRECT --to-ports 8000"]
    );

    let mut source = RuleInput::new("PREROUTING", "SNAT");
    source.table = "nat".to_owned();
    source.to_source = Some("10.0.0.0:8000".to_owned());
    let prepared = PreparedRule::new(&source).unwrap();
    assert_eq!(
        descriptions(&rule_commands(&source, &prepared, false)),
        ["iptables -t nat -A PREROUTING -j SNAT --to-source 10.0.0.0:8000"]
    );

    let mut destination = RuleInput::new("PREROUTING", "DNAT");
    destination.table = "nat".to_owned();
    destination.to_destination = Some("10.10.10.10".to_owned());
    let prepared = PreparedRule::new(&destination).unwrap();
    assert!(rule_commands(&destination, &prepared, true).is_empty());
    destination.present = false;
    assert!(rule_commands(&destination, &prepared, false).is_empty());
    assert_eq!(
        descriptions(&rule_commands(&destination, &prepared, true)),
        ["iptables -t nat -D PREROUTING -j DNAT --to-destination 10.10.10.10"]
    );

    let mut invalid = RuleInput::new("PREROUTING", "ACCEPT");
    invalid.destination_port = Some("8080".to_owned());
    assert_eq!(
        PreparedRule::new(&invalid).err().unwrap().to_string(),
        "iptables cannot filter by destination_port/source_port without a protocol"
    );
    for (jump, field, expected) in [
        (
            "DNAT",
            "destination",
            "iptables only supports to_destination on the nat table and the required jump (table=filter, jump=DNAT)",
        ),
        (
            "SNAT",
            "source",
            "iptables only supports to_source on the nat table and the required jump (table=filter, jump=SNAT)",
        ),
        (
            "REDIRECT",
            "ports",
            "iptables only supports to_ports on the nat table and the required jump (table=filter, jump=REDIRECT)",
        ),
    ] {
        invalid = RuleInput::new("PREROUTING", jump);
        match field {
            "destination" => invalid.to_destination = Some("8.8.8.8".to_owned()),
            "source" => invalid.to_source = Some("8.8.8.8".to_owned()),
            _ => invalid.to_ports = Some("8.8.8.8".to_owned()),
        }
        assert_eq!(PreparedRule::new(&invalid).err().unwrap().to_string(), expected);
    }
    invalid = RuleInput::new("INPUT", "NOT_LOG");
    invalid.log_prefix = Some("log-prefix".to_owned());
    assert_eq!(
        PreparedRule::new(&invalid).err().unwrap().to_string(),
        "iptables only supports log_prefix with the LOG jump (jump=NOT_LOG)"
    );
}
