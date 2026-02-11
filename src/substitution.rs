use std::borrow::Cow;

/// Replace `$1`, `$2`, ... in `template` with capture groups from the regex
/// match, then trim trailing whitespace and dots (matching Matomo PHP behaviour).
///
/// Returns borrowed data when the template contains no `$N` placeholders,
/// avoiding allocation entirely in that case.
pub(crate) fn substitute<'a>(template: &'a str, captures: &fancy_regex::Captures) -> Cow<'a, str> {
    // Fast path: no placeholders → borrow directly from the template.
    if !template.contains('$') {
        return Cow::Borrowed(template.trim_end_matches(|c: char| c.is_whitespace() || c == '.'));
    }

    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            if let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    chars.next();
                    let idx = (d as u8 - b'0') as usize;
                    if let Some(m) = captures.get(idx) {
                        result.push_str(m.as_str());
                    }
                    continue;
                }
            }
        }
        result.push(c);
    }

    let trimmed_len = result
        .trim_end_matches(|c: char| c.is_whitespace() || c == '.')
        .len();
    result.truncate(trimmed_len);
    Cow::Owned(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps<'a>(re: &'a fancy_regex::Regex, text: &'a str) -> fancy_regex::Captures<'a> {
        re.captures(text).unwrap().unwrap()
    }

    #[test]
    fn basic_substitution() {
        let re = fancy_regex::Regex::new(r"(Chrome)/(\d+)\.(\d+)").unwrap();
        let c = caps(&re, "Chrome/120.0");
        assert_eq!(substitute("$1 v$2.$3", &c), "Chrome v120.0");
    }

    #[test]
    fn no_placeholders() {
        let re = fancy_regex::Regex::new(r"(Chrome)").unwrap();
        let c = caps(&re, "Chrome");
        assert_eq!(substitute("Safari", &c), "Safari");
    }

    #[test]
    fn missing_group_is_ignored() {
        let re = fancy_regex::Regex::new(r"(Chrome)").unwrap();
        let c = caps(&re, "Chrome");
        assert_eq!(substitute("$1 $2", &c), "Chrome");
    }
}
