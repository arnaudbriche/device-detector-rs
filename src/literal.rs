use regex_syntax::{hir::literal::Extractor, parse};

/// Extract literal substrings from a regex pattern for use as Aho-Corasick
/// pre-filter candidates. Returns literals of at least `min_len` bytes,
/// or an empty vec if none are found (meaning the entry must always be tried).
///
/// Uses `regex_syntax::hir::literal::Extractor` for correct handling of all
/// regex constructs. If the pattern cannot be parsed (e.g. exotic PCRE-isms
/// unsupported by regex_syntax), returns empty → the entry becomes an
/// "always candidate" that is checked on every input.
pub(crate) fn extract_literals(pattern: &str, min_len: usize) -> Vec<String> {
    let hir = match parse(pattern) {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };

    let mut extractor = Extractor::new();
    extractor.kind(regex_syntax::hir::literal::ExtractKind::Prefix);

    let seq = extractor.extract(&hir);
    let literals: Vec<String> = seq
        .literals()
        .into_iter()
        .flatten()
        .filter_map(|lit| {
            let s = std::str::from_utf8(lit.as_bytes()).ok()?;
            if s.len() >= min_len {
                Some(s.to_lowercase())
            } else {
                None
            }
        })
        .collect();

    literals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_literal() {
        let lits = extract_literals("Firefox", 3);
        assert_eq!(lits, vec!["firefox"]);
    }

    #[test]
    fn alternation() {
        let lits = extract_literals("Firefox|Chrome", 3);
        assert!(lits.contains(&"firefox".to_string()));
        assert!(lits.contains(&"chrome".to_string()));
    }

    #[test]
    fn too_short_returns_empty() {
        let lits = extract_literals(r"\d+\.\d+", 3);
        assert!(lits.is_empty());
    }
}
