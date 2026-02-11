use fancy_regex::Captures;
use std::borrow::Cow;

/// Simple semver-ish comparison: is `a < b`?  Compares dot-separated numeric
/// components left to right (missing components treated as 0).
pub(crate) fn version_lt(a: &str, b: &str) -> bool {
    let mut ai = a.split('.');
    let mut bi = b.split('.');
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return false,
            (None, Some(bv)) => return bv.parse::<u32>().unwrap_or(0) > 0,
            (Some(_), None) => return false,
            (Some(av), Some(bv)) => {
                let an = av.parse::<u32>().unwrap_or(0);
                let bn = bv.parse::<u32>().unwrap_or(0);
                if an < bn {
                    return true;
                }
                if an > bn {
                    return false;
                }
            }
        }
    }
}

/// Simple semver-ish comparison: is `a >= b`?
pub(crate) fn version_ge(a: &str, b: &str) -> bool {
    !version_lt(a, b)
}

pub(crate) fn capture_or_empty<'a>(captures: &Captures<'a>, group: usize) -> Cow<'a, str> {
    captures
        .get(group)
        .map(|m| Cow::Borrowed(m.as_str()))
        .unwrap_or(Cow::Borrowed(""))
}

/// Quick regex match against a UA, using Matomo's boundary prefix + case-insensitive.
pub(crate) fn ua_matches(ua: &str, pattern: &str) -> bool {
    // This allocates a Regex per call.  For the handful of heuristic checks in
    // parse() the cost is negligible, and it keeps the code straightforward.
    let full = format!(
        "(?i)(?:^|[^A-Z0-9_\\-]|[^A-Z0-9\\-]_|sprd\\-|MZ\\-)(?:{})",
        pattern
    );
    fancy_regex::Regex::new(&full)
        .ok()
        .and_then(|re| re.is_match(ua).ok())
        .unwrap_or(false)
}
