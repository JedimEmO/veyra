//! The page's query string as a command line (ticket 0013): `?at=camp&fresh`
//! is `game --at camp --fresh`, so the web build parses the same `Cli` as a
//! native run and no flag needs a second spelling.

/// The arguments a query string stands for, program name first. A key alone is
/// a switch, `key=value` is an option; keys of anything but letters, digits,
/// `-` and `_` are dropped, and `%XX` and `+` in a value are decoded.
pub fn args_from_query(query: &str) -> Vec<String> {
    let mut args = vec!["game".to_owned()];
    for pair in query.trim_start_matches('?').split('&') {
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, Some(value)),
            None => (pair, None),
        };
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            continue;
        }
        args.push(format!("--{key}"));
        if let Some(value) = value {
            args.push(decode(value));
        }
    }
    args
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => out.push(b' '),
            b'%' if at + 2 < bytes.len()
                && hex(bytes[at + 1]).is_some()
                && hex(bytes[at + 2]).is_some() =>
            {
                out.push(hex(bytes[at + 1]).unwrap_or(0) << 4 | hex(bytes[at + 2]).unwrap_or(0));
                at += 2;
            }
            byte => out.push(byte),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_is_a_command_line() {
        assert_eq!(
            args_from_query("?at=camp&fresh"),
            ["game", "--at", "camp", "--fresh"]
        );
        assert_eq!(args_from_query(""), ["game"]);
        assert_eq!(args_from_query("?"), ["game"]);
    }

    #[test]
    fn values_are_decoded_and_odd_keys_dropped() {
        assert_eq!(
            args_from_query("hostiles=bulwark%4015,marksman&a b=1&x+y&time=0.5"),
            ["game", "--hostiles", "bulwark@15,marksman", "--time", "0.5"]
        );
        assert_eq!(args_from_query("at=100%"), ["game", "--at", "100%"]);
    }

    #[test]
    fn the_flags_it_makes_parse() {
        use clap::Parser;
        let cli = crate::Cli::try_parse_from(args_from_query("?at=camp&fresh&frames=3"));
        assert!(cli.is_ok(), "{:?}", cli.err());
    }
}
