use crate::parser::Captures;
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
        .get_str(group)
        .map(Cow::Borrowed)
        .unwrap_or(Cow::Borrowed(""))
}
