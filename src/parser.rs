use aho_corasick::AhoCorasick;
use fancy_regex::Regex;
use rayon::prelude::*;

use crate::error::Result;
use crate::literal::extract_literals;

/// A compiled entry: one regex rule plus its associated data.
pub(crate) struct CompiledEntry<T> {
    pub regex: Regex,
    pub data: T,
}

/// Result of a successful match.
pub(crate) struct MatchResult<'a, T> {
    pub data: &'a T,
    pub captures: fancy_regex::Captures<'a>,
}

// ---------------------------------------------------------------------------
// CompiledParser — flat list matching (bots, OS, clients, engines)
// ---------------------------------------------------------------------------

/// Core matching engine: Aho-Corasick pre-filter + fancy-regex matching.
///
/// `T` is the associated data for each entry (e.g. bot name, OS name, etc.).
pub(crate) struct CompiledParser<T> {
    ac: Option<AhoCorasick>,
    /// For each AC pattern, the list of entry indices it maps to.
    ac_pattern_to_entries: Vec<Vec<usize>>,
    /// Entries with no extractable literal — must always be tried.
    always_candidates: Vec<usize>,
    entries: Vec<CompiledEntry<T>>,
}

impl<T> CompiledParser<T> {
    /// Build a CompiledParser from an iterator of (regex_pattern, data) pairs.
    ///
    /// Regex compilation is parallelized across all available cores via rayon.
    pub fn build(items: impl IntoIterator<Item = (String, T)>) -> Result<Self>
    where
        T: Send,
    {
        let items: Vec<(String, T)> = items.into_iter().collect();

        // Phase 1: compile all regexes + extract literals in parallel.
        let compiled: Vec<(Regex, T, Vec<String>)> = items
            .into_par_iter()
            .map(|(pattern, data)| {
                let regex = compile_regex(&pattern)?;
                let literals = extract_literals(&pattern, 3);
                Ok((regex, data, literals))
            })
            .collect::<Result<Vec<_>>>()?;

        // Phase 2: build Aho-Corasick index sequentially (fast).
        let mut entries = Vec::with_capacity(compiled.len());
        let mut ac_patterns: Vec<String> = Vec::new();
        let mut ac_pattern_to_entries: Vec<Vec<usize>> = Vec::new();
        let mut always_candidates: Vec<usize> = Vec::new();
        let mut literal_index: std::collections::HashMap<String, usize> = Default::default();

        for (regex, data, literals) in compiled {
            let idx = entries.len();
            entries.push(CompiledEntry { regex, data });

            if literals.is_empty() {
                always_candidates.push(idx);
                continue;
            }

            for lit in literals {
                if let Some(&ac_idx) = literal_index.get(&lit) {
                    ac_pattern_to_entries[ac_idx].push(idx);
                } else {
                    let ac_idx = ac_patterns.len();
                    literal_index.insert(lit.clone(), ac_idx);
                    ac_patterns.push(lit);
                    ac_pattern_to_entries.push(vec![idx]);
                }
            }
        }

        let ac = if ac_patterns.is_empty() {
            None
        } else {
            Some(
                AhoCorasick::builder()
                    .ascii_case_insensitive(true)
                    .build(&ac_patterns)?,
            )
        };

        Ok(Self {
            ac,
            ac_pattern_to_entries,
            always_candidates,
            entries,
        })
    }

    /// Find the first matching entry (preserving original order).
    pub fn match_first<'a>(&'a self, ua: &'a str) -> Option<MatchResult<'a, T>> {
        // Collect candidate entry indices from AC matches.
        let mut candidates: Vec<usize> = Vec::new();

        if let Some(ac) = &self.ac {
            for mat in ac.find_overlapping_iter(ua) {
                for &entry_idx in &self.ac_pattern_to_entries[mat.pattern().as_usize()] {
                    candidates.push(entry_idx);
                }
            }
        }

        // Add always-candidates.
        candidates.extend_from_slice(&self.always_candidates);

        // Deduplicate and sort by original index (first-match-wins order).
        candidates.sort_unstable();
        candidates.dedup();

        // Try each candidate in order.
        for idx in candidates {
            let entry = &self.entries[idx];
            match entry.regex.captures(ua) {
                Ok(Some(caps)) => {
                    return Some(MatchResult {
                        data: &entry.data,
                        captures: caps,
                    });
                }
                _ => continue,
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// DeviceBrandParser — two-level brand-gate + model matching (device files)
// ---------------------------------------------------------------------------

/// A compiled brand with its gate regex and optional model sub-regexes.
pub(crate) struct CompiledBrand<B, M> {
    pub regex: Regex,
    pub data: B,
    pub models: Vec<CompiledEntry<M>>,
}

/// Result of a device brand match.
pub(crate) struct BrandMatchResult<'a, B, M> {
    pub brand_data: &'a B,
    /// Captures from the brand regex (used if no model matches).
    pub brand_captures: fancy_regex::Captures<'a>,
    /// If a model regex matched, its data and captures.
    pub model_match: Option<MatchResult<'a, M>>,
}

/// Two-level matching engine for device brand/model detection.
///
/// Matches the Matomo PHP logic: iterate brands in order, try the brand regex
/// as a gate, and only if it matches, try model regexes within that brand.
pub(crate) struct DeviceBrandParser<B, M> {
    brands: Vec<CompiledBrand<B, M>>,
}

impl<B, M> DeviceBrandParser<B, M> {
    pub fn new(brands: Vec<CompiledBrand<B, M>>) -> Self {
        Self { brands }
    }

    /// Find the first matching brand, then try model regexes within it.
    pub fn match_first<'a>(&'a self, ua: &'a str) -> Option<BrandMatchResult<'a, B, M>> {
        for brand in &self.brands {
            let brand_caps = match brand.regex.captures(ua) {
                Ok(Some(caps)) => caps,
                _ => continue,
            };

            // Brand matched — now try model regexes.
            let model_match =
                brand
                    .models
                    .iter()
                    .find_map(|model| match model.regex.captures(ua) {
                        Ok(Some(caps)) => Some(MatchResult {
                            data: &model.data,
                            captures: caps,
                        }),
                        _ => None,
                    });

            return Some(BrandMatchResult {
                brand_data: &brand.data,
                brand_captures: brand_caps,
                model_match,
            });
        }

        None
    }
}

/// Matomo's word-boundary-like prefix applied to all regexes.
/// Matches: start of string, or a non-alphanumeric boundary, or special prefixes.
const MATOMO_BOUNDARY_PREFIX: &str = r"(?:^|[^A-Z0-9_\-]|[^A-Z0-9\-]_|sprd\-|MZ\-)";

/// Helper: compile a regex with Matomo's boundary prefix and case-insensitive flag.
pub(crate) fn compile_regex(pattern: &str) -> Result<Regex> {
    let full = format!("(?i){}(?:{})", MATOMO_BOUNDARY_PREFIX, pattern);
    Ok(Regex::new(&full)?)
}
