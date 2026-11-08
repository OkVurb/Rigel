//! MCP transport hardening (roadmap 69).
//!
//! Four rules, each with a refusal that names the rule that fired:
//!
//! 1. **environment** — a stdio server inherits only a small allowlist, never the app's
//!    provider keys or any variable that looks like a secret;
//! 2. **authentication** — an HTTP server must present a token on every request;
//! 3. **private-network** — an HTTP server may not point at RFC 1918, link-local or
//!    cloud-metadata addresses (loopback is allowed for a local server);
//! 4. **message-size** — one JSON-RPC frame above the cap is refused, not buffered.

use std::collections::BTreeMap;

/// A single frame may not exceed four mebibytes. A tool result larger than that belongs
/// in a file, not in a JSON-RPC line.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// The variables a stdio server may inherit. `PATH` is needed to resolve the command; the
/// Windows variables are needed by the loader; the rest make a Unix child behave.
pub const ENVIRONMENT_ALLOWLIST: &[&str] = &[
    "PATH",
    "Path",
    "SystemRoot",
    "SYSTEMROOT",
    "COMSPEC",
    "PATHEXT",
    "TEMP",
    "TMP",
    "HOME",
    "USERPROFILE",
    "LANG",
    "LC_ALL",
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("rule environment: {name} would leak into a stdio MCP server")]
    EnvironmentLeak { name: String },

    #[error("rule authentication: an HTTP MCP server requires a bearer token")]
    MissingAuth,

    #[error("rule private-network: {url} points at private address {host}")]
    PrivateNetwork { url: String, host: String },

    #[error("rule message-size: {actual} bytes exceeds the {limit} byte cap")]
    MessageTooLarge { limit: usize, actual: usize },

    #[error("rule url: {0} is not an absolute http(s) URL with a host")]
    InvalidUrl(String),
}

impl TransportError {
    pub fn rule(&self) -> &'static str {
        match self {
            TransportError::EnvironmentLeak { .. } => "environment",
            TransportError::MissingAuth => "authentication",
            TransportError::PrivateNetwork { .. } => "private-network",
            TransportError::MessageTooLarge { .. } => "message-size",
            TransportError::InvalidUrl(_) => "url",
        }
    }
}

/// The environment a stdio server actually receives. Everything not on the allowlist is
/// dropped, including `OPENAI_API_KEY`, `AWS_SECRET_ACCESS_KEY` and `RIGEL_*` values.
pub fn scrub_environment(
    inherited: impl IntoIterator<Item = (String, String)>,
) -> BTreeMap<String, String> {
    let mut environment = BTreeMap::new();
    for (name, value) in inherited {
        if ENVIRONMENT_ALLOWLIST.contains(&name.as_str()) {
            environment.insert(name, value);
        }
    }
    environment
}

/// Validate that a prepared environment has no leak. The scrubber already enforces this;
/// the check exists so a caller that builds an environment by hand is caught.
pub fn check_stdio_environment(
    environment: &BTreeMap<String, String>,
) -> Result<(), TransportError> {
    for name in environment.keys() {
        if !ENVIRONMENT_ALLOWLIST.contains(&name.as_str()) {
            return Err(TransportError::EnvironmentLeak { name: name.clone() });
        }
    }
    Ok(())
}

/// Validate an HTTP MCP endpoint. A non-loopback scheme must be https, a token is
/// mandatory, and a private address is refused even over https.
pub fn check_http(url: &str, has_auth: bool) -> Result<(), TransportError> {
    let (scheme, host) = split(url)?;
    if !has_auth {
        return Err(TransportError::MissingAuth);
    }
    let loopback = is_loopback(host);
    if scheme != "https" && !(scheme == "http" && loopback) {
        return Err(TransportError::InvalidUrl(format!(
            "{url} must use https unless it is loopback"
        )));
    }
    if !loopback && is_private(host) {
        return Err(TransportError::PrivateNetwork {
            url: url.to_string(),
            host: host.to_string(),
        });
    }
    Ok(())
}

/// Refuse a frame above the cap before it reaches a parser.
pub fn check_message(bytes: &[u8]) -> Result<(), TransportError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(TransportError::MessageTooLarge {
            limit: MAX_MESSAGE_BYTES,
            actual: bytes.len(),
        });
    }
    Ok(())
}

fn split(url: &str) -> Result<(String, &str), TransportError> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| TransportError::InvalidUrl(url.to_string()))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(TransportError::InvalidUrl(url.to_string()));
    }
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if host.is_empty() {
        return Err(TransportError::InvalidUrl(url.to_string()));
    }
    Ok((scheme, host))
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host == "::1"
        || host == "0:0:0:0:0:0:0:1"
        || host
            .parse::<std::net::Ipv4Addr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn is_private(host: &str) -> bool {
    match host.parse::<std::net::Ipv4Addr>() {
        Ok(address) => {
            address.is_private()
                || address.is_link_local()
                || address.is_broadcast()
                || address.is_documentation()
                || address.is_unspecified()
                || address.octets()[0] == 100 && (64..=127).contains(&address.octets()[1])
        }
        // A hostname cannot be classified without resolving it; the caller resolves once
        // and re-checks the address, which is the DNS-pinning rule.
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("SystemRoot".to_string(), "C:\\Windows".to_string()),
            ("OPENAI_API_KEY".to_string(), "sk-live".to_string()),
            ("AWS_SECRET_ACCESS_KEY".to_string(), "secret".to_string()),
            ("RIGEL_CUSTOM_DOOR".to_string(), "{}".to_string()),
        ])
    }

    #[test]
    fn the_stdio_environment_is_cleared_to_the_allowlist() {
        let scrubbed = scrub_environment(environment());
        assert_eq!(scrubbed.len(), 2);
        assert!(scrubbed.contains_key("PATH"));
        assert!(scrubbed.contains_key("SystemRoot"));
        assert!(!scrubbed.contains_key("OPENAI_API_KEY"));
        assert!(!scrubbed.contains_key("AWS_SECRET_ACCESS_KEY"));
        check_stdio_environment(&scrubbed).unwrap();
    }

    #[test]
    fn a_hand_built_environment_with_a_key_is_refused_by_name() {
        let mut prepared = scrub_environment(environment());
        prepared.insert("GITHUB_TOKEN".to_string(), "ghp_x".to_string());
        assert_eq!(
            check_stdio_environment(&prepared),
            Err(TransportError::EnvironmentLeak {
                name: "GITHUB_TOKEN".to_string()
            })
        );
        assert_eq!(
            check_stdio_environment(&prepared).unwrap_err().rule(),
            "environment"
        );
    }

    #[test]
    fn an_http_server_without_a_token_is_refused() {
        assert_eq!(
            check_http("https://mcp.example.test/rpc", false),
            Err(TransportError::MissingAuth)
        );
        check_http("https://mcp.example.test/rpc", true).unwrap();
    }

    #[test]
    fn a_private_network_url_is_refused_even_over_https() {
        let error = check_http("https://10.0.0.5/rpc", true).unwrap_err();
        assert!(matches!(error, TransportError::PrivateNetwork { .. }));
        assert!(error.to_string().contains("rule private-network"));
        for url in [
            "https://192.168.1.9/mcp",
            "https://172.16.0.4/mcp",
            "http://169.254.169.254/latest",
        ] {
            assert!(check_http(url, true).is_err(), "{url} was allowed");
        }
        // A local server remains usable over loopback.
        check_http("http://127.0.0.1:8765/mcp", true).unwrap();
        check_http("http://localhost:8765/mcp", true).unwrap();
    }

    #[test]
    fn a_frame_above_the_cap_is_refused_and_names_the_limit() {
        assert!(check_message(&vec![0u8; MAX_MESSAGE_BYTES]).is_ok());
        let error = check_message(&vec![0u8; MAX_MESSAGE_BYTES + 1]).unwrap_err();
        assert!(matches!(
            error,
            TransportError::MessageTooLarge {
                limit: MAX_MESSAGE_BYTES,
                ..
            }
        ));
        assert!(error.to_string().contains("4194304"));
    }
}
