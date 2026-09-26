//! Line-level edits of `config.toml` for the setup wizard: set one key
//! while keeping every comment and every other line exactly as it was
//! (the file is documentation too — config.example.toml's comments are
//! the manual). Pure text in, text out.

/// `text` with `key = <value>` set. `value` is a plain string; it is
/// TOML-encoded here. Replaces the active line if there is one, else
/// uncomments the documented `#key = …` line, else appends.
pub fn set_string(text: &str, key: &str, value: &str) -> String {
    let encoded = toml::Value::String(value.to_string()).to_string();
    set_raw(text, key, &encoded)
}

fn key_of(line: &str) -> Option<(&str, bool)> {
    let t = line.trim_start();
    let (commented, rest) = match t.strip_prefix('#') {
        Some(r) => (true, r.trim_start()),
        None => (false, t),
    };
    let (k, _) = rest.split_once('=')?;
    let k = k.trim();
    (!k.is_empty() && !k.contains(' ')).then_some((k, commented))
}

fn set_raw(text: &str, key: &str, encoded: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let new_line = format!("{key} = {encoded}");
    let position = lines
        .iter()
        .position(|l| key_of(l) == Some((key, false)))
        .or_else(|| lines.iter().position(|l| key_of(l) == Some((key, true))));
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    match position {
        Some(i) => out[i] = new_line,
        None => out.push(new_line),
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

/// Reads one string key without validating the rest (the wizard shows
/// what is there, even in a config the courier would refuse).
pub fn get_string(text: &str, key: &str) -> Option<String> {
    let value: toml::Value = toml::from_str(text).ok()?;
    value.get(key)?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str =
        "# The hub.\nhub_url = \"http://127.0.0.1:8080\"\n\n# Language.\n#language = \"nl\"\n";

    #[test]
    fn an_active_key_is_replaced_in_place_keeping_comments() {
        let out = set_string(EXAMPLE, "hub_url", "http://10.10.10.9:8080");
        assert_eq!(
            out,
            "# The hub.\nhub_url = \"http://10.10.10.9:8080\"\n\n# Language.\n#language = \"nl\"\n"
        );
    }

    #[test]
    fn a_documented_commented_key_is_uncommented_where_it_stands() {
        let out = set_string(EXAMPLE, "language", "en");
        assert!(out.ends_with("# Language.\nlanguage = \"en\"\n"), "{out}");
        assert!(out.contains("# The hub."), "prose comments stay");
    }

    #[test]
    fn an_unknown_key_is_appended_and_values_are_toml_escaped() {
        let out = set_string("a = 1", "token_file", r"C:\Users\Kenny\token");
        let parsed: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(parsed["token_file"].as_str(), Some(r"C:\Users\Kenny\token"));
        assert_eq!(parsed["a"].as_integer(), Some(1));
    }

    #[test]
    fn prose_comments_mentioning_a_key_are_not_mistaken_for_it() {
        let text = "# hub_url is where the hub lives\nhub_url = \"x\"\n";
        let out = set_string(text, "hub_url", "y");
        assert_eq!(out, "# hub_url is where the hub lives\nhub_url = \"y\"\n");
    }

    #[test]
    fn get_reads_a_string_key() {
        assert_eq!(
            get_string(EXAMPLE, "hub_url").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(get_string(EXAMPLE, "language"), None);
        assert_eq!(get_string("not toml [", "hub_url"), None);
    }
}
