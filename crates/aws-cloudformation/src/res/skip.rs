//! The identity of a skipped resource: a physical id shaped like the real one (`vpc-…`, `sg-…`, or the name the template gives), and
//! attributes worked out on demand. An ARN is ARN-shaped; a host name is under the reserved `.invalid` top level, so a connection to
//! it fails loudly instead of reaching something real.

use serde_json::Value;

use super::{Rc, text};
use crate::tmpl::eval::Pseudo;

/// The physical id a skipped resource gets.
pub(crate) fn physical(rc: &Rc, p: &Value) -> String {
    let hex = || {
        crate::model::suffix(17, true)
            .chars()
            .map(|c| {
                if c.is_ascii_digit() || ('a'..='f').contains(&c) {
                    c
                } else {
                    'a'
                }
            })
            .collect::<String>()
    };
    let prefix = match rc.ty.as_str() {
        "AWS::EC2::VPC" => Some("vpc"),
        "AWS::EC2::Subnet" => Some("subnet"),
        "AWS::EC2::SecurityGroup" => Some("sg"),
        "AWS::EC2::InternetGateway" => Some("igw"),
        "AWS::EC2::EgressOnlyInternetGateway" => Some("eigw"),
        "AWS::EC2::RouteTable" => Some("rtb"),
        "AWS::EC2::NatGateway" => Some("nat"),
        "AWS::EC2::NetworkAcl" => Some("acl"),
        "AWS::EC2::VPCEndpoint" => Some("vpce"),
        "AWS::EC2::NetworkInterface" => Some("eni"),
        "AWS::EC2::TransitGateway" => Some("tgw"),
        "AWS::EC2::LaunchTemplate" => Some("lt"),
        "AWS::EC2::PrefixList" => Some("pl"),
        "AWS::EC2::DHCPOptions" => Some("dopt"),
        _ => None,
    };
    if let Some(prefix) = prefix {
        return format!("{prefix}-{}", hex());
    }
    if rc.ty == "AWS::EC2::EIP" {
        // TEST-NET-3 (RFC 5737): an address that routes nowhere.
        return format!("203.0.113.{}", aws_seal::random_below(250).unwrap_or(0) + 1);
    }
    // A name the template gives (`RepositoryName`, `DBSubnetGroupName`, `Name`...) is the id, as it is on AWS for most types.
    if let Some(Value::Object(o)) = Some(p)
        && let Some((_, v)) = o
            .iter()
            .find(|(k, v)| (k.ends_with("Name") && k.as_str() != "KeyName") && v.is_string())
    {
        return v.as_str().unwrap_or_default().to_owned();
    }
    let _ = text;
    super::generated_name(rc, 128, false)
}

/// An attribute of a skipped resource.
pub(crate) fn attribute(
    ty: &str,
    logical: &str,
    physical: &str,
    attr: &str,
    pseudo: &Pseudo,
) -> Value {
    let service = super::family(ty).to_ascii_lowercase();
    let host = || {
        format!(
            "{}.{}.invalid",
            logical.to_ascii_lowercase(),
            pseudo.stack_name.to_ascii_lowercase()
        )
    };
    if attr == "Arn" || attr.ends_with("Arn") || attr.ends_with("ARN") {
        return Value::String(format!(
            "arn:{}:{service}:{}:{}:{}/{physical}",
            pseudo.partition(),
            pseudo.region,
            pseudo.account,
            logical.to_ascii_lowercase()
        ));
    }
    if ["Url", "URL", "Uri"].iter().any(|w| attr.ends_with(w)) {
        return Value::String(format!("https://{}", host()));
    }
    if [
        "Address",
        "DNSName",
        "DomainName",
        "Endpoint",
        "Hostname",
        "HostName",
    ]
    .iter()
    .any(|w| attr.contains(w))
    {
        return Value::String(host());
    }
    if attr.ends_with("Port") {
        return Value::String("0".into());
    }
    if matches!(
        attr,
        "CidrBlockAssociations"
            | "Ipv6CidrBlocks"
            | "NetworkInterfaceIds"
            | "SecondaryPrivateIpAddresses"
            | "DnsEntries"
    ) {
        return Value::Array(vec![Value::String(physical.to_owned())]);
    }
    if attr == "CidrBlock" {
        return Value::String("10.0.0.0/16".into());
    }
    Value::String(physical.to_owned())
}
