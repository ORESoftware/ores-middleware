use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub(crate) fn valid_cidr(value: &str) -> bool {
    parse_cidr(value).is_some()
}

pub(crate) fn cidr_contains(value: &str, ip: IpAddr) -> bool {
    let Some((network, prefix)) = parse_cidr(value) else {
        return false;
    };
    match (network, ip) {
        (IpAddr::V4(network), IpAddr::V4(ip)) => {
            masked_v4(network, prefix) == masked_v4(ip, prefix)
        }
        (IpAddr::V6(network), IpAddr::V6(ip)) => {
            masked_v6(network, prefix) == masked_v6(ip, prefix)
        }
        _ => false,
    }
}

fn parse_cidr(value: &str) -> Option<(IpAddr, u8)> {
    let (network, prefix) = value.split_once('/')?;
    let network = network.parse::<IpAddr>().ok()?;
    let prefix = prefix.parse::<u8>().ok()?;
    let valid = match network {
        IpAddr::V4(_) => prefix <= 32,
        IpAddr::V6(_) => prefix <= 128,
    };
    valid.then_some((network, prefix))
}

fn masked_v4(ip: Ipv4Addr, prefix: u8) -> u32 {
    let value = u32::from(ip);
    if prefix == 0 {
        0
    } else {
        value & (u32::MAX << (32 - prefix))
    }
}

fn masked_v6(ip: Ipv6Addr, prefix: u8) -> u128 {
    let value = u128::from(ip);
    if prefix == 0 {
        0
    } else {
        value & (u128::MAX << (128 - prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ipv4_and_ipv6_networks() {
        assert!(cidr_contains(
            "10.0.0.0/8",
            "10.24.3.2".parse().expect("valid test address")
        ));
        assert!(!cidr_contains(
            "10.0.0.0/8",
            "192.0.2.1".parse().expect("valid test address")
        ));
        assert!(cidr_contains(
            "2001:db8::/32",
            "2001:db8::5".parse().expect("valid test address")
        ));
    }

    #[test]
    fn rejects_invalid_prefixes() {
        assert!(!valid_cidr("10.0.0.0/33"));
        assert!(!valid_cidr("2001:db8::/129"));
        assert!(!valid_cidr("not-a-network"));
    }
}
