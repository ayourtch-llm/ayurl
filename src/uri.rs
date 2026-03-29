//! Lightweight URI parser that handles all ayurl schemes consistently,
//! including IPv6 addresses and non-standard schemes like `scp://` and `sftp://`.

use crate::error::{AyurlError, Result};

/// A parsed URI with all components extracted.
#[derive(Debug, Clone)]
pub struct ParsedUri {
    scheme: String,
    username: Option<String>,
    password: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

impl ParsedUri {
    /// Parse a URI string into its components.
    pub fn parse(input: &str) -> Result<Self> {
        if input.is_empty() {
            return Err(AyurlError::InvalidUri("empty URI".into()));
        }

        // --- Scheme ---
        let colon_pos = input
            .find("://")
            .ok_or_else(|| AyurlError::InvalidUri(format!("missing scheme in: {input}")))?;

        let scheme = &input[..colon_pos];
        if scheme.is_empty()
            || !scheme
                .chars()
                .next()
                .unwrap()
                .is_ascii_alphabetic()
        {
            return Err(AyurlError::InvalidUri(format!(
                "invalid scheme: {scheme}"
            )));
        }

        let rest = &input[colon_pos + 3..]; // after "://"

        // --- file:// special case (RFC 8089) ---
        // file:///tmp/foo       → path = "/tmp/foo"
        // file:///C:/Users      → path = "/C:/Users"
        // file://localhost/tmp  → path = "/tmp" (localhost = local machine)
        // file://./rel/path     → path = "{cwd}/rel/path" (non-standard, browser compat)
        // file:///a%20b         → path = "/a b" (percent-decoded)
        // file:///p?q#f         → path = "/p", query = "q", fragment = "f"
        if scheme == "file" {
            let rest = if rest.starts_with('/') {
                rest.to_string()
            } else {
                // There's an authority component before the path.
                // Split at the first '/' to separate authority from path.
                let (authority, path_rest) = match rest.find('/') {
                    Some(pos) => (&rest[..pos], &rest[pos..]),
                    None => (rest, ""),
                };
                if authority.eq_ignore_ascii_case("localhost") {
                    // RFC 8089: localhost means local machine — strip it
                    path_rest.to_string()
                } else if authority == "." {
                    // Non-standard: file://./relative/path — resolve from cwd
                    let cwd = std::env::current_dir().unwrap_or_default();
                    format!("{}{}", cwd.display(), path_rest)
                } else {
                    // Unknown authority — treat as part of path for best-effort
                    format!("/{authority}{path_rest}")
                }
            };

            // Parse query and fragment (RFC 3986)
            let (path_str, query, fragment) = parse_path_query_fragment(&rest);

            // Percent-decode the path
            let path = percent_decode(&path_str);

            return Ok(Self {
                scheme: scheme.to_string(),
                username: None,
                password: None,
                host: None,
                port: None,
                path,
                query,
                fragment,
            });
        }

        // --- Split authority from path/query/fragment ---
        // authority ends at first '/' or end of string
        let (authority, remainder) = split_authority(rest);

        // --- Parse authority: [userinfo@]host[:port] ---
        let (userinfo, hostport) = match authority.rfind('@') {
            Some(pos) => (Some(&authority[..pos]), &authority[pos + 1..]),
            None => (None, authority),
        };

        // --- Username / Password ---
        let (username, password) = match userinfo {
            Some(info) => match info.find(':') {
                Some(pos) => (
                    Some(percent_decode(&info[..pos])),
                    Some(percent_decode(&info[pos + 1..])),
                ),
                None => (Some(percent_decode(info)), None),
            },
            None => (None, None),
        };

        // --- Host / Port (IPv6 aware) ---
        let (host, port) = parse_host_port(hostport)?;

        // --- Path / Query / Fragment ---
        let (path, query, fragment) = parse_path_query_fragment(remainder);

        Ok(Self {
            scheme: scheme.to_lowercase(),
            username,
            password,
            host,
            port,
            path,
            query,
            fragment,
        })
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub fn fragment(&self) -> Option<&str> {
        self.fragment.as_deref()
    }
}

/// Split `rest` (after `://`) into authority and remainder (starting with `/`).
fn split_authority(rest: &str) -> (&str, &str) {
    // Find the first `/` that's not inside `[...]` (IPv6)
    let mut in_bracket = false;
    for (i, ch) in rest.char_indices() {
        match ch {
            '[' => in_bracket = true,
            ']' => in_bracket = false,
            '/' if !in_bracket => return (&rest[..i], &rest[i..]),
            _ => {}
        }
    }
    // No path — authority is the whole string
    (rest, "")
}

/// Parse `host[:port]` with IPv6 bracket awareness.
fn parse_host_port(hostport: &str) -> Result<(Option<String>, Option<u16>)> {
    if hostport.is_empty() {
        return Ok((None, None));
    }

    if hostport.starts_with('[') {
        // IPv6: [addr] or [addr]:port
        let bracket_end = hostport
            .find(']')
            .ok_or_else(|| AyurlError::InvalidUri("unterminated IPv6 bracket".into()))?;

        let addr = &hostport[1..bracket_end]; // strip brackets
        let after_bracket = &hostport[bracket_end + 1..];

        let port = if let Some(port_str) = after_bracket.strip_prefix(':') {
            if port_str.is_empty() {
                None
            } else {
                Some(port_str.parse::<u16>().map_err(|_| {
                    AyurlError::InvalidUri(format!("invalid port: {port_str}"))
                })?)
            }
        } else {
            None
        };

        Ok((Some(addr.to_string()), port))
    } else {
        // IPv4 or hostname: host or host:port
        // Find the *last* colon — but only if it's not part of the host
        match hostport.rfind(':') {
            Some(pos) => {
                let host = &hostport[..pos];
                let port_str = &hostport[pos + 1..];
                match port_str.parse::<u16>() {
                    Ok(port) => Ok((Some(host.to_string()), Some(port))),
                    Err(_) => {
                        // Not a valid port — treat entire string as host
                        Ok((Some(hostport.to_string()), None))
                    }
                }
            }
            None => Ok((Some(hostport.to_string()), None)),
        }
    }
}

/// Parse path, query, and fragment from the remainder after authority.
fn parse_path_query_fragment(remainder: &str) -> (String, Option<String>, Option<String>) {
    if remainder.is_empty() {
        return ("/".to_string(), None, None);
    }

    // Split fragment first (after #)
    let (before_frag, fragment) = match remainder.find('#') {
        Some(pos) => (&remainder[..pos], Some(remainder[pos + 1..].to_string())),
        None => (remainder, None),
    };

    // Split query (after ?)
    let (path, query) = match before_frag.find('?') {
        Some(pos) => (
            &before_frag[..pos],
            Some(before_frag[pos + 1..].to_string()),
        ),
        None => (before_frag, None),
    };

    let path = if path.is_empty() { "/" } else { path };
    (path.to_string(), query, fragment)
}

/// Decode percent-encoded characters in a string.
fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(byte as char);
                    continue;
                }
            }
            // Invalid percent encoding — keep as-is
            result.push('%');
            result.push_str(&hex);
        } else {
            result.push(ch);
        }
    }

    result
}

impl std::fmt::Display for ParsedUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}://", self.scheme)?;

        if let Some(ref user) = self.username {
            write!(f, "{user}")?;
            if let Some(ref pass) = self.password {
                write!(f, ":{pass}")?;
            }
            write!(f, "@")?;
        }

        if let Some(ref host) = self.host {
            if host.contains(':') {
                write!(f, "[{host}]")?; // IPv6
            } else {
                write!(f, "{host}")?;
            }
        }

        if let Some(port) = self.port {
            write!(f, ":{port}")?;
        }

        write!(f, "{}", self.path)?;

        if let Some(ref q) = self.query {
            write!(f, "?{q}")?;
        }

        if let Some(ref frag) = self.fragment {
            write!(f, "#{frag}")?;
        }

        Ok(())
    }
}
