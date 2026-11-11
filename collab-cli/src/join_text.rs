//! The parts of `collab join` and `collab pair` that only handle text: tidying
//! a typed pairing code, and updating a `.collab.toml` without disturbing
//! what else is in it. No network or files here, so it is easy to test.

/// Digits in a pairing code.
pub const CODE_LEN: usize = 6;

/// Turn what a person typed into a pairing code, or `None` if it is not one.
/// Spaces and dashes are for reading the code aloud ("482 913", "482-913")
/// and are dropped.
pub fn normalize_code(input: &str) -> Option<String> {
    let digits: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if digits.len() == CODE_LEN && digits.bytes().all(|b| b.is_ascii_digit()) {
        Some(digits)
    } else {
        None
    }
}

/// The code as shown to a person: "482 913".
pub fn display_code(code: &str) -> String {
    if code.len() == CODE_LEN {
        format!("{} {}", &code[..3], &code[3..])
    } else {
        code.to_string()
    }
}

/// A server address without trailing slashes, with `http://` assumed when no
/// scheme was typed ("raspi:8000" is what people say).
pub fn normalize_server(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

/// True when the address only means "this machine", so it is no use to
/// another one.
pub fn is_loopback_server(server: &str) -> bool {
    let rest = server.split("://").nth(1).unwrap_or(server);
    let authority = rest.split('/').next().unwrap_or("");
    let host = if let Some(inner) = authority.strip_prefix('[') {
        inner.split(']').next().unwrap_or("")
    } else {
        authority.rsplit_once(':').map_or(authority, |(h, _)| h)
    };
    let host = host.to_ascii_lowercase();
    host == "localhost" || host == "::1" || host == "0.0.0.0" || host.starts_with("127.")
}

/// Write `key = "value"` settings into the text of a `.collab.toml`.
///
/// Existing lines for those keys are replaced where they stand; new keys go
/// at the top. Every other line, comments included, is kept as it was. Only
/// top-level keys are touched: anything after the first `[section]` header
/// belongs to that section and is left alone.
pub fn merge_config(existing: &str, settings: &[(&str, &str)]) -> String {
    let mut pending: Vec<(&str, &str)> = settings.to_vec();
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;

    for line in existing.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_section = true;
        }
        let key = if in_section {
            None
        } else {
            top_level_key(trimmed)
        };
        match key.and_then(|k| pending.iter().position(|(want, _)| *want == k)) {
            Some(i) => {
                let (k, v) = pending.remove(i);
                out.push(setting_line(k, v));
            }
            None => out.push(line.to_string()),
        }
    }

    let mut head: Vec<String> = pending.iter().map(|(k, v)| setting_line(k, v)).collect();
    head.extend(out);
    let mut text = head.join("\n");
    text.push('\n');
    text
}

/// What the desktop app writes to `~/.collab.toml`: the server and team token,
/// with everything else in the file left as it was.
///
/// The app used to replace the whole file, which threw away `instance`,
/// `recipients` and anything else set by hand or by `collab join`.
///
/// When the app created the team it also leaves the admin token as a comment,
/// so the person can see it exists. It is a comment because the CLI takes the
/// admin token from `COLLAB_ADMIN_TOKEN` only. An older note is replaced, and
/// removed when there is no admin token.
pub fn merge_app_config(existing: &str, host: &str, token: &str, admin_token: &str) -> String {
    const NOTE: &str = "# admin_token = ";
    let kept: Vec<&str> = existing
        .lines()
        .filter(|line| !line.starts_with(NOTE))
        .collect();
    let mut text = merge_config(&kept.join("\n"), &[("host", host), ("token", token)]);
    if !admin_token.is_empty() {
        let value = setting_line("admin_token", admin_token);
        text.push_str(&format!(
            "# {value}  # set COLLAB_ADMIN_TOKEN in your shell to use\n"
        ));
    }
    text
}

/// The key of a `key = value` line, if the line is one.
fn top_level_key(line: &str) -> Option<&str> {
    if line.starts_with('#') {
        return None;
    }
    let (key, _) = line.split_once('=')?;
    let key = key.trim();
    let plain = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    plain.then_some(key)
}

/// `key = "value"` with the value written as a TOML basic string.
fn setting_line(key: &str, value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c => escaped.push(c),
        }
    }
    format!("{key} = \"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_six_digits_however_they_are_spaced() {
        assert_eq!(normalize_code("482913").as_deref(), Some("482913"));
        assert_eq!(normalize_code("482 913").as_deref(), Some("482913"));
        assert_eq!(normalize_code(" 482-913 ").as_deref(), Some("482913"));
        assert_eq!(normalize_code("000042").as_deref(), Some("000042"));
    }

    #[test]
    fn anything_else_is_not_a_code() {
        for bad in [
            "",
            "12345",
            "1234567",
            "12a456",
            "four82",
            "１２３４５６",
            "482.913",
        ] {
            assert_eq!(normalize_code(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn codes_are_shown_in_two_groups() {
        assert_eq!(display_code("482913"), "482 913");
    }

    #[test]
    fn a_bare_host_gets_http_and_loses_trailing_slashes() {
        assert_eq!(normalize_server("raspi:8000"), "http://raspi:8000");
        assert_eq!(normalize_server("http://raspi:8000/"), "http://raspi:8000");
        assert_eq!(
            normalize_server(" https://team.example.com// "),
            "https://team.example.com"
        );
    }

    #[test]
    fn loopback_addresses_are_recognised() {
        for yes in [
            "http://localhost:8000",
            "http://LOCALHOST",
            "http://127.0.0.1:8000",
            "http://[::1]:8000",
            "http://0.0.0.0:8000/x",
        ] {
            assert!(is_loopback_server(yes), "{yes}");
        }
        for no in [
            "http://raspi:8000",
            "http://192.168.1.20:8000",
            "https://team.example.com",
            "http://localhost.example.com:8000",
        ] {
            assert!(!is_loopback_server(no), "{no}");
        }
    }

    #[test]
    fn a_new_file_is_just_the_settings() {
        let text = merge_config("", &[("host", "http://raspi:8000"), ("token", "tm_abc")]);
        assert_eq!(text, "host = \"http://raspi:8000\"\ntoken = \"tm_abc\"\n");
    }

    #[test]
    fn existing_settings_are_replaced_in_place_and_the_rest_is_kept() {
        let before = "# my notes\nhost = \"http://old:8000\"\ninstance = \"deck\"\ntoken = \"tm_old\"\nrecipients = [\"a\", \"b\"]\n";
        let after = merge_config(
            before,
            &[("host", "http://raspi:8000"), ("token", "tm_new")],
        );
        assert_eq!(
            after,
            "# my notes\nhost = \"http://raspi:8000\"\ninstance = \"deck\"\ntoken = \"tm_new\"\nrecipients = [\"a\", \"b\"]\n"
        );
    }

    #[test]
    fn missing_settings_are_added_at_the_top() {
        let after = merge_config(
            "instance = \"deck\"\n",
            &[("host", "http://raspi:8000"), ("team", "crabs")],
        );
        assert_eq!(
            after,
            "host = \"http://raspi:8000\"\nteam = \"crabs\"\ninstance = \"deck\"\n"
        );
    }

    #[test]
    fn commented_out_settings_are_not_mistaken_for_real_ones() {
        let before = "# token = \"tm_commented\"\n";
        let after = merge_config(before, &[("token", "tm_new")]);
        assert_eq!(after, "token = \"tm_new\"\n# token = \"tm_commented\"\n");
    }

    #[test]
    fn keys_inside_a_section_belong_to_the_section() {
        let before = "host = \"http://old:8000\"\n\n[profile]\ntoken = \"section-token\"\n";
        let after = merge_config(
            before,
            &[("host", "http://raspi:8000"), ("token", "tm_new")],
        );
        assert_eq!(
            after,
            "token = \"tm_new\"\nhost = \"http://raspi:8000\"\n\n[profile]\ntoken = \"section-token\"\n"
        );
    }

    #[test]
    fn awkward_values_are_escaped() {
        let after = merge_config("", &[("instance", "a\"b\\c")]);
        assert_eq!(after, "instance = \"a\\\"b\\\\c\"\n");
    }

    #[test]
    fn merging_twice_changes_nothing_more() {
        let settings = [("host", "http://raspi:8000"), ("token", "tm_abc")];
        let once = merge_config("instance = \"deck\"\n", &settings);
        assert_eq!(merge_config(&once, &settings), once);
    }

    #[test]
    fn the_app_keeps_what_it_did_not_write() {
        let before = "# mine\nhost = \"http://localhost:8000\"\ninstance = \"deck\"\nrecipients = [\"hermi\"]\n\n[extra]\nkeep = true\n";
        let after = merge_app_config(before, "http://raspi:8000", "tm_new", "");
        assert_eq!(
            after,
            "token = \"tm_new\"\n# mine\nhost = \"http://raspi:8000\"\ninstance = \"deck\"\nrecipients = [\"hermi\"]\n\n[extra]\nkeep = true\n"
        );
    }

    #[test]
    fn the_app_writes_a_new_file_as_before() {
        assert_eq!(
            merge_app_config("", "http://raspi:8000", "tm_new", ""),
            "host = \"http://raspi:8000\"\ntoken = \"tm_new\"\n"
        );
    }

    #[test]
    fn the_admin_token_note_is_replaced_not_stacked() {
        let first = merge_app_config("instance = \"mac\"\n", "http://h:8000", "tm_a", "adm_1");
        assert_eq!(first.matches("# admin_token").count(), 1);
        assert!(first.contains("# admin_token = \"adm_1\""), "{first}");

        let second = merge_app_config(&first, "http://h:8000", "tm_a", "adm_2");
        assert_eq!(second.matches("# admin_token").count(), 1);
        assert!(
            second.contains("adm_2") && !second.contains("adm_1"),
            "{second}"
        );
        assert!(second.contains("instance = \"mac\""), "{second}");

        // Joining a team later has no admin token: the old note must go.
        let third = merge_app_config(&second, "http://h:8000", "tm_a", "");
        assert!(!third.contains("admin_token"), "{third}");
    }

    #[test]
    fn a_real_admin_token_setting_is_left_alone() {
        let before = "admin_token = \"legacy\"\n";
        let after = merge_app_config(before, "http://h:8000", "tm_a", "");
        assert!(after.contains("admin_token = \"legacy\""), "{after}");
    }
}
