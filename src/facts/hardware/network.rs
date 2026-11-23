use std::error::Error;

use indexmap::IndexMap;
use serde_json::Value;

use crate::api::StringCommand;
use crate::facts::{Fact, UNKNOWN_STATE};

const COMMAND: &str = "ip -j addr show 2> /dev/null || ip addr show 2> /dev/null || ifconfig -a";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkAddress {
    pub address: String,
    pub broadcast: Option<String>,
    pub mask_bits: Option<u32>,
    pub netmask: Option<String>,
    pub additional_ips: Vec<NetworkAddress>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkDevice {
    pub ether: Option<String>,
    pub mtu: Option<u32>,
    pub state: String,
    pub ipv4: Option<NetworkAddress>,
    pub ipv6: Option<NetworkAddress>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NetworkDevices;

impl Fact for NetworkDevices {
    type Input = ();
    type Output = IndexMap<String, NetworkDevice>;

    fn name(&self) -> &'static str {
        "hardware.NetworkDevices"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new([COMMAND])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let prefix = output.iter().take(2).map(|line| line.trim()).collect::<String>();
        if prefix.starts_with("[{") { parse_json(output) } else { Ok(parse_text(output)) }
    }
}

fn parse_json(output: &[String]) -> Result<IndexMap<String, NetworkDevice>, Box<dyn Error + Send + Sync>> {
    let decoded = serde_json::from_str::<Vec<Value>>(&output.join("\n"))?;
    let mut devices = IndexMap::new();
    for value in decoded {
        let Some(info) = value.as_object() else { continue };
        let Some(name) = info.get("ifname").and_then(Value::as_str) else { continue };
        let mut device = NetworkDevice {
            ether: info.get("address").and_then(Value::as_str).map(str::to_owned),
            mtu: info.get("mtu").and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok()),
            state: info.get("operstate").and_then(Value::as_str).unwrap_or(UNKNOWN_STATE).to_owned(),
            ..NetworkDevice::default()
        };
        for address in info.get("addr_info").and_then(Value::as_array).into_iter().flatten() {
            let Some(address) = address.as_object() else { continue };
            let Some(local) = address.get("local").and_then(Value::as_str) else { continue };
            let family = address.get("family").and_then(Value::as_str);
            let bits = address.get("prefixlen").and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok());
            let item = NetworkAddress {
                address: local.to_owned(),
                broadcast: address.get("broadcast").and_then(Value::as_str).map(str::to_owned),
                mask_bits: bits,
                netmask: bits.filter(|_| family == Some("inet")).map(ipv4_netmask),
                additional_ips: Vec::new(),
            };
            match family {
                Some("inet") => add_address(&mut device.ipv4, item),
                Some("inet6") => add_address(&mut device.ipv6, item),
                _ => {}
            }
        }
        devices.insert(name.to_owned(), device);
    }
    Ok(devices)
}

fn parse_text(output: &[String]) -> IndexMap<String, NetworkDevice> {
    let mut devices = IndexMap::new();
    let mut current = None::<String>;
    for raw in output {
        let line = raw.trim();
        if let Some((name, device)) = parse_header(line) {
            current = Some(name.clone());
            devices.insert(name, device);
            continue;
        }
        let Some(device) = current.as_ref().and_then(|name| devices.get_mut(name)) else { continue };
        let fields = line.split_whitespace().collect::<Vec<_>>();
        match fields.as_slice() {
            ["link/ether", ether, ..] | ["ether", ether, ..] => device.ether = Some((*ether).to_owned()),
            ["inet", value, rest @ ..] => add_address(&mut device.ipv4, parse_ipv4(value, rest)),
            ["inet6", value, rest @ ..] => add_address(&mut device.ipv6, parse_ipv6(value, rest)),
            _ => {}
        }
    }
    devices
}

fn parse_header(line: &str) -> Option<(String, NetworkDevice)> {
    let (prefix, rest) = line.split_once(':')?;
    let (name, details) = if prefix.chars().all(|value| value.is_ascii_digit()) {
        let (name, details) = rest.trim_start().split_once(':')?;
        (name, details)
    } else if rest.contains("mtu ") {
        (prefix, rest)
    } else {
        return None;
    };
    let fields = details.split_whitespace().collect::<Vec<_>>();
    let mtu = fields.windows(2).find(|pair| pair[0] == "mtu").and_then(|pair| pair[1].parse().ok());
    let state = if details.contains("UP") {
        "UP"
    } else if details.contains("DOWN") {
        "DOWN"
    } else {
        UNKNOWN_STATE
    };
    Some((name.to_owned(), NetworkDevice { mtu, state: state.to_owned(), ..NetworkDevice::default() }))
}

fn parse_ipv4(value: &str, rest: &[&str]) -> NetworkAddress {
    let (address, inline_mask) = value.split_once('/').map_or((value, None), |(address, mask)| (address, Some(mask)));
    let mask_value = inline_mask.or_else(|| option_value(rest, "netmask"));
    let (mask_bits, netmask) = mask_value.map_or((None, None), parse_mask);
    NetworkAddress {
        address: address.to_owned(),
        broadcast: option_value(rest, "brd").or_else(|| option_value(rest, "broadcast")).map(str::to_owned),
        mask_bits,
        netmask,
        additional_ips: Vec::new(),
    }
}

fn parse_ipv6(value: &str, rest: &[&str]) -> NetworkAddress {
    let (address, inline_mask) = value.split_once('/').map_or((value, None), |(address, mask)| (address, Some(mask)));
    NetworkAddress {
        address: address.to_owned(),
        mask_bits: inline_mask.or_else(|| option_value(rest, "prefixlen")).and_then(|value| value.parse().ok()),
        ..NetworkAddress::default()
    }
}

fn option_value<'a>(values: &'a [&str], option: &str) -> Option<&'a str> {
    values.windows(2).find(|pair| pair[0] == option).map(|pair| pair[1])
}

fn parse_mask(value: &str) -> (Option<u32>, Option<String>) {
    if let Ok(bits) = value.parse::<u32>() {
        return (Some(bits), Some(ipv4_netmask(bits)));
    }
    if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        if let Ok(mask) = u32::from_str_radix(hex, 16) {
            return (Some(mask.count_ones()), Some(ipv4_netmask(mask.count_ones())));
        }
    }
    let bits = value.split('.').filter_map(|part| part.parse::<u8>().ok()).map(u8::count_ones).sum::<u32>();
    (Some(bits), Some(value.to_owned()))
}

fn ipv4_netmask(bits: u32) -> String {
    let mask = if bits == 0 { 0 } else { u32::MAX.checked_shl(32 - bits.min(32)).unwrap_or_default() };
    mask.to_be_bytes().iter().map(u8::to_string).collect::<Vec<_>>().join(".")
}

fn add_address(slot: &mut Option<NetworkAddress>, address: NetworkAddress) {
    if let Some(first) = slot {
        first.additional_ips.push(address);
    } else {
        *slot = Some(address);
    }
}

fn address_lists(output: &[String], ipv6: bool) -> Result<IndexMap<String, Vec<String>>, Box<dyn Error + Send + Sync>> {
    let devices = NetworkDevices.process(output, &())?;
    Ok(devices
        .into_iter()
        .filter_map(|(name, device)| {
            let address = if ipv6 { device.ipv6 } else { device.ipv4 }?;
            let mut addresses = vec![address.address];
            addresses.extend(address.additional_ips.into_iter().map(|item| item.address));
            Some((name, addresses))
        })
        .collect())
}

macro_rules! address_fact {
    ($name:ident, $fact_name:literal, $ipv6:literal, $multiple:literal, $output:ty, $convert:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = ();
            type Output = IndexMap<String, $output>;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, _: &()) -> StringCommand {
                StringCommand::new([COMMAND])
            }
            fn default(&self) -> Self::Output {
                IndexMap::new()
            }
            fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(address_lists(output, $ipv6)?.into_iter().filter_map($convert).collect())
            }
        }
    };
}

address_fact!(Ipv4Addrs, "hardware.Ipv4Addrs", false, true, Vec<String>, |item| Some(item));
address_fact!(Ipv6Addrs, "hardware.Ipv6Addrs", true, true, Vec<String>, |item| Some(item));
address_fact!(Ipv4Addresses, "hardware.Ipv4Addresses", false, false, String, |(name, values): (
    String,
    Vec<String>
)| values
    .into_iter()
    .next()
    .map(|value| (name, value)));
address_fact!(Ipv6Addresses, "hardware.Ipv6Addresses", true, false, String, |(name, values): (String, Vec<String>)| {
    values.into_iter().next().map(|value| (name, value))
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_ip_addresses_and_derived_address_facts() {
        let output = vec![
            "2: eth0: <BROADCAST,MULTICAST,UP> mtu 1500 state UP".to_owned(),
            "link/ether c6:c5:60:cd:73:f1 brd ff:ff:ff:ff:ff:ff".to_owned(),
            "inet 166.22.203.66/20 brd 165.22.207.255 scope global".to_owned(),
            "inet 10.18.0.10/16 brd 10.18.255.255 scope global".to_owned(),
            "inet6 fe80::1/64 scope link".to_owned(),
        ];
        let devices = NetworkDevices.process(&output, &()).unwrap();
        assert_eq!(devices["eth0"].ether.as_deref(), Some("c6:c5:60:cd:73:f1"));
        assert_eq!(devices["eth0"].ipv4.as_ref().and_then(|value| value.netmask.as_deref()), Some("255.255.240.0"));
        assert_eq!(Ipv4Addrs.process(&output, &()).unwrap()["eth0"], ["166.22.203.66", "10.18.0.10"]);
        assert_eq!(Ipv6Addresses.process(&output, &()).unwrap()["eth0"], "fe80::1");
    }

    #[test]
    fn parses_ifconfig_and_json_forms() {
        let ifconfig = vec![
            "en0: flags=8863<UP,BROADCAST> mtu 1500".to_owned(),
            "inet 1.2.3.4 netmask 0xffffff00 broadcast 255.255.255.255".to_owned(),
            "inet6 2a01::01 prefixlen 64".to_owned(),
        ];
        let devices = NetworkDevices.process(&ifconfig, &()).unwrap();
        assert_eq!(devices["en0"].ipv4.as_ref().and_then(|value| value.mask_bits), Some(24));
        let json = vec![r#"[{"ifname":"eth0","mtu":1500,"operstate":"UP","address":"aa:bb:cc:dd:ee:ff","addr_info":[{"family":"inet","local":"1.2.3.4","prefixlen":24}]}]"#.to_owned()];
        let devices = NetworkDevices.process(&json, &()).unwrap();
        assert_eq!(devices["eth0"].ipv4.as_ref().map(|value| value.address.as_str()), Some("1.2.3.4"));
    }
}
