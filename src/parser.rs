use rayon::prelude::*;

use crate::error::Result;

/// Matomo's word-boundary-like prefix applied to all regexes.
/// Matches: start of string, or a non-alphanumeric boundary, or special prefixes.
const MATOMO_BOUNDARY_PREFIX: &str = r"(?:^|[^A-Z0-9_\-]|[^A-Z0-9\-]_|sprd\-|MZ\-)";

/// Build the full Matomo-prefixed, case-insensitive regex string.
pub(crate) fn full_pattern(pattern: &str) -> String {
    format!("(?i){}(?:{})", MATOMO_BOUNDARY_PREFIX, pattern)
}

/// Helper: compile a regex with Matomo's boundary prefix and case-insensitive flag
/// using fancy_regex (needed for patterns with PCRE features).
pub(crate) fn compile_regex(pattern: &str) -> Result<fancy_regex::Regex> {
    let full = full_pattern(pattern);
    Ok(fancy_regex::Regex::new(&full)?)
}

// ---------------------------------------------------------------------------
// Captures — unified enum over regex::Captures and fancy_regex::Captures
// ---------------------------------------------------------------------------

/// Lightweight wrapper so callers (substitute, capture_or_empty) don't need
/// to know which regex engine produced the match.
pub(crate) enum Captures<'a> {
    Standard(regex::Captures<'a>),
    Fancy(fancy_regex::Captures<'a>),
}

impl<'a> Captures<'a> {
    /// Get the matched text for capture group `i`, or `None` if the group
    /// didn't participate in the match.
    pub fn get_str(&self, i: usize) -> Option<&'a str> {
        match self {
            Captures::Standard(c) => c.get(i).map(|m| m.as_str()),
            Captures::Fancy(c) => c.get(i).map(|m| m.as_str()),
        }
    }
}

// ---------------------------------------------------------------------------
// Shared result types
// ---------------------------------------------------------------------------

/// A compiled entry: one fancy_regex rule plus its associated data.
/// Used for model sub-regexes within a brand (small count, not on hot path).
pub(crate) struct CompiledEntry<T> {
    pub regex: fancy_regex::Regex,
    pub data: T,
}

/// Result of a successful match.
pub(crate) struct MatchResult<'a, T> {
    pub data: &'a T,
    pub captures: Captures<'a>,
}

// ---------------------------------------------------------------------------
// CompiledParser — flat list matching (bots, OS, clients, engines)
// ---------------------------------------------------------------------------

/// Core matching engine: regex-filtered prefilter + fancy-regex fallback.
///
/// `T` is the associated data for each entry (e.g. bot name, OS name, etc.).
pub(crate) struct CompiledParser<T> {
    /// regex-filtered set built from patterns the `regex` crate can handle.
    filtered: regex_filtered::Regexes,
    /// Maps regex-filtered index → entry index.
    filtered_to_entry: Vec<usize>,
    /// Entries whose patterns require PCRE features (lookahead/lookbehind),
    /// sorted by entry index.
    fancy_entries: Vec<(usize, fancy_regex::Regex)>,
    /// Entry data indexed by entry index.
    data: Vec<T>,
}

impl<T> CompiledParser<T> {
    /// Build a CompiledParser from an iterator of (regex_pattern, data) pairs.
    ///
    /// Patterns that compile with the `regex` crate go through regex-filtered
    /// for fast Thompson-NFA matching; the rest fall back to fancy_regex.
    pub fn build(items: impl IntoIterator<Item = (String, T)>) -> Result<Self>
    where
        T: Send,
    {
        let items: Vec<(String, T)> = items.into_iter().collect();
        let n = items.len();

        // Phase 1: compute full patterns, separate data.
        let mut full_patterns: Vec<String> = Vec::with_capacity(n);
        let mut data: Vec<T> = Vec::with_capacity(n);

        for (pattern, d) in items {
            full_patterns.push(full_pattern(&pattern));
            data.push(d);
        }

        // Phase 2: classify — can the regex crate handle this pattern?
        let is_standard: Vec<bool> = full_patterns
            .par_iter()
            .map(|p| regex::Regex::new(p).is_ok())
            .collect();

        // Phase 3: build regex-filtered set from standard patterns.
        let mut builder = regex_filtered::Builder::new();
        let mut filtered_to_entry: Vec<usize> = Vec::new();

        for (idx, pattern) in full_patterns.iter().enumerate() {
            if is_standard[idx] {
                builder = builder.push(pattern).expect("pre-validated pattern");
                filtered_to_entry.push(idx);
            }
        }

        let filtered = builder.build()?;

        // Phase 4: compile fancy-only patterns in parallel.
        let fancy_indices: Vec<usize> = (0..n).filter(|&i| !is_standard[i]).collect();
        let fancy_regexes: Vec<fancy_regex::Regex> = fancy_indices
            .par_iter()
            .map(|&idx| {
                fancy_regex::Regex::new(&full_patterns[idx]).map_err(crate::error::Error::from)
            })
            .collect::<Result<Vec<_>>>()?;

        let fancy_entries: Vec<(usize, fancy_regex::Regex)> =
            fancy_indices.into_iter().zip(fancy_regexes).collect();

        eprintln!(
            "entries: {:?}/{:?}",
            fancy_entries.len(),
            filtered.regexes().len()
        );

        Ok(Self {
            filtered,
            filtered_to_entry,
            fancy_entries,
            data,
        })
    }

    /// Find the first matching entry (preserving original order).
    pub fn match_first<'a>(&'a self, ua: &'a str) -> Option<MatchResult<'a, T>> {
        // Get the first (lowest entry-index) match from regex-filtered.
        // filtered_to_entry is monotonically increasing, and matching()
        // returns results in ascending filtered-index order, so the first
        // hit corresponds to the lowest entry index among standard patterns.
        let mut best_filtered: Option<(usize, &regex::Regex)> = None;
        for (filtered_idx, re) in self.filtered.matching(ua) {
            let entry_idx = self.filtered_to_entry[filtered_idx];
            best_filtered = Some((entry_idx, re));
            break;
        }

        let cutoff = best_filtered.map(|(idx, _)| idx).unwrap_or(usize::MAX);

        // Try fancy entries whose index is lower than the best filtered match.
        for &(entry_idx, ref re) in &self.fancy_entries {
            if entry_idx >= cutoff {
                break;
            }
            if let Ok(Some(caps)) = re.captures(ua) {
                return Some(MatchResult {
                    data: &self.data[entry_idx],
                    captures: Captures::Fancy(caps),
                });
            }
        }

        // Use the filtered match if it exists.
        if let Some((entry_idx, re)) = best_filtered {
            if let Some(caps) = re.captures(ua) {
                return Some(MatchResult {
                    data: &self.data[entry_idx],
                    captures: Captures::Standard(caps),
                });
            }
        }

        // No filtered match — try remaining fancy entries (beyond cutoff).
        // This only runs when cutoff < usize::MAX but no fancy beat it,
        // and there are fancy entries after the cutoff.
        if cutoff < usize::MAX {
            for &(entry_idx, ref re) in &self.fancy_entries {
                if entry_idx <= cutoff {
                    continue;
                }
                if let Ok(Some(caps)) = re.captures(ua) {
                    return Some(MatchResult {
                        data: &self.data[entry_idx],
                        captures: Captures::Fancy(caps),
                    });
                }
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// DeviceBrandParser — two-level brand-gate + model matching (device files)
// ---------------------------------------------------------------------------

/// Brand entry: data + model sub-regexes (gate regex handled by regex-filtered).
pub(crate) struct BrandEntry<B, M> {
    pub data: B,
    pub models: Vec<CompiledEntry<M>>,
}

/// Result of a device brand match.
pub(crate) struct BrandMatchResult<'a, B, M> {
    pub brand_data: &'a B,
    /// Captures from the brand regex (used if no model matches).
    pub brand_captures: Captures<'a>,
    /// If a model regex matched, its data and captures.
    pub model_match: Option<MatchResult<'a, M>>,
}

/// Two-level matching engine for device brand/model detection.
///
/// Brand gate regexes use regex-filtered for fast prefiltering;
/// model regexes within a matched brand stay as fancy_regex.
pub(crate) struct DeviceBrandParser<B, M> {
    /// regex-filtered set for brand gate regexes.
    filtered: regex_filtered::Regexes,
    /// Maps regex-filtered index → brand index.
    filtered_to_brand: Vec<usize>,
    /// Brands whose gate regex requires PCRE features, sorted by brand index.
    fancy_brands: Vec<(usize, fancy_regex::Regex)>,
    /// Brand data + models, indexed by brand index.
    brands: Vec<BrandEntry<B, M>>,
}

impl<B, M> DeviceBrandParser<B, M> {
    /// Build a `DeviceBrandParser`.
    ///
    /// Each item is `(full_matomo_pattern, brand_data, compiled_model_entries)`.
    /// The full pattern includes the Matomo boundary prefix and `(?i)` flag.
    pub fn build(items: Vec<(String, B, Vec<CompiledEntry<M>>)>) -> Result<Self>
    where
        B: Send,
        M: Send,
    {
        let n = items.len();
        let mut full_patterns: Vec<String> = Vec::with_capacity(n);
        let mut brands: Vec<BrandEntry<B, M>> = Vec::with_capacity(n);

        for (pattern, data, models) in items {
            full_patterns.push(pattern);
            brands.push(BrandEntry { data, models });
        }

        // Classify patterns.
        let is_standard: Vec<bool> = full_patterns
            .par_iter()
            .map(|p| regex::Regex::new(p).is_ok())
            .collect();

        // Build regex-filtered set from standard patterns.
        let mut builder = regex_filtered::Builder::new();
        let mut filtered_to_brand: Vec<usize> = Vec::new();

        for (idx, pattern) in full_patterns.iter().enumerate() {
            if is_standard[idx] {
                builder = builder.push(pattern).expect("pre-validated pattern");
                filtered_to_brand.push(idx);
            }
        }

        let filtered = builder.build()?;

        // Compile fancy-only patterns in parallel.
        let fancy_indices: Vec<usize> = (0..n).filter(|&i| !is_standard[i]).collect();
        let fancy_regexes: Vec<fancy_regex::Regex> = fancy_indices
            .par_iter()
            .map(|&idx| {
                fancy_regex::Regex::new(&full_patterns[idx]).map_err(crate::error::Error::from)
            })
            .collect::<Result<Vec<_>>>()?;

        let fancy_brands: Vec<(usize, fancy_regex::Regex)> =
            fancy_indices.into_iter().zip(fancy_regexes).collect();

        Ok(Self {
            filtered,
            filtered_to_brand,
            fancy_brands,
            brands,
        })
    }

    /// Find the first matching brand, then try model regexes within it.
    pub fn match_first<'a>(&'a self, ua: &'a str) -> Option<BrandMatchResult<'a, B, M>> {
        // Get the first (lowest brand-index) match from regex-filtered.
        let mut best_filtered: Option<(usize, &regex::Regex)> = None;
        for (filtered_idx, re) in self.filtered.matching(ua) {
            let brand_idx = self.filtered_to_brand[filtered_idx];
            best_filtered = Some((brand_idx, re));
            break;
        }

        let cutoff = best_filtered.map(|(idx, _)| idx).unwrap_or(usize::MAX);

        // Try fancy brands with index < cutoff.
        for &(brand_idx, ref re) in &self.fancy_brands {
            if brand_idx >= cutoff {
                break;
            }
            // Check if regex matches first before extracting captures
            if re.is_match(ua).unwrap_or(false) {
                if let Ok(Some(caps)) = re.captures(ua) {
                    let brand = &self.brands[brand_idx];
                    let model_match = match_model(ua, &brand.models);
                    return Some(BrandMatchResult {
                        brand_data: &brand.data,
                        brand_captures: Captures::Fancy(caps),
                        model_match,
                    });
                }
            }
        }

        // Use the filtered match if it exists.
        if let Some((brand_idx, re)) = best_filtered {
            // For standard regex, captures() is more efficient, but still check first
            if let Some(caps) = re.captures(ua) {
                let brand = &self.brands[brand_idx];
                let model_match = match_model(ua, &brand.models);
                return Some(BrandMatchResult {
                    brand_data: &brand.data,
                    brand_captures: Captures::Standard(caps),
                    model_match,
                });
            }
        }

        // No filtered match — try remaining fancy brands.
        if cutoff < usize::MAX {
            for &(brand_idx, ref re) in &self.fancy_brands {
                if brand_idx <= cutoff {
                    continue;
                }
                // Check if regex matches first before extracting captures
                if re.is_match(ua).unwrap_or(false) {
                    if let Ok(Some(caps)) = re.captures(ua) {
                        let brand = &self.brands[brand_idx];
                        let model_match = match_model(ua, &brand.models);
                        return Some(BrandMatchResult {
                            brand_data: &brand.data,
                            brand_captures: Captures::Fancy(caps),
                            model_match,
                        });
                    }
                }
            }
        }

        None
    }
}

/// Try model regexes within a matched brand (stays as fancy_regex).
/// Optimized to check for match first before extracting captures.
fn match_model<'a, M>(ua: &'a str, models: &'a [CompiledEntry<M>]) -> Option<MatchResult<'a, M>> {
    models.iter().find_map(|model| {
        // First check if the regex matches (which is faster than capturing)
        if model.regex.is_match(ua).unwrap_or(false) {
            // Only extract captures if we know there's a match
            match model.regex.captures(ua) {
                Ok(Some(caps)) => Some(MatchResult {
                    data: &model.data,
                    captures: Captures::Fancy(caps),
                }),
                _ => None,
            }
        } else {
            None
        }
    })
}
