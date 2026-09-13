//! `tui::fuzzy` — tiny fuzzy-subsequence scorer for slash-command filter.
//!
//! Not a full fuzzy library — just enough to rank commands by "how well
//! does the query subsequence match this candidate." Query chars must
//! appear in order in the candidate (case-insensitive). Runs of adjacent
//! matches score higher than scattered ones so `/mcp` beats `/model` when
//! the query is `mc`.

/// Score a candidate against a query. Returns `None` when the query is
/// not a subsequence of the candidate; higher scores mean a better match.
#[must_use]
pub fn score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let mut points: i32 = 0;
    let mut streak: i32 = 0;
    let mut qi = 0;
    let mut last_matched_idx: Option<usize> = None;
    for (ci, cch) in candidate.chars().enumerate() {
        if qi >= q.len() {
            break;
        }
        let cch_lower = cch.to_ascii_lowercase();
        if cch_lower == q[qi] {
            // Bonus for consecutive matches (adjacent to previous match).
            if last_matched_idx == Some(ci.wrapping_sub(1)) {
                streak += 1;
                points += 3 + streak;
            } else {
                streak = 0;
                points += 1;
            }
            // Bonus if matched at candidate start.
            if ci == 0 {
                points += 5;
            }
            last_matched_idx = Some(ci);
            qi += 1;
        } else {
            streak = 0;
        }
    }
    if qi == q.len() {
        // Penalize longer candidates so shorter/tighter matches sort first.
        Some(points - i32::try_from(candidate.len().min(64)).unwrap_or(64))
    } else {
        None
    }
}

/// Returns matched candidates sorted by descending score (best first).
/// Ties broken by candidate order (stable).
#[must_use]
pub fn rank<'a, I>(query: &str, candidates: I) -> Vec<(usize, i32)>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut hits: Vec<(usize, i32)> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(idx, name)| score(query, name).map(|s| (idx, s)))
        .collect();
    hits.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything_with_zero_score() {
        assert_eq!(score("", "help"), Some(0));
        assert_eq!(score("", "status"), Some(0));
    }

    #[test]
    fn subsequence_matches_score() {
        assert!(score("hlp", "help").is_some());
        assert!(score("mdl", "model").is_some());
        assert!(score("mc", "mcp").is_some());
    }

    #[test]
    fn non_subsequence_returns_none() {
        assert!(score("xyz", "help").is_none());
        assert!(score("zmodel", "model").is_none());
    }

    #[test]
    fn prefix_matches_beat_scattered() {
        let prefix = score("mo", "model").unwrap();
        let scattered = score("mo", "memory-object").unwrap();
        assert!(
            prefix > scattered,
            "prefix {prefix} should beat scattered {scattered}"
        );
    }

    #[test]
    fn adjacent_matches_beat_split() {
        let adj = score("st", "status").unwrap();
        let split = score("st", "sandbox-test").unwrap();
        assert!(adj > split, "adjacent {adj} should beat split {split}");
    }

    #[test]
    fn rank_orders_best_first() {
        let cands = ["help", "status", "model", "memory", "mcp"];
        let ranking = rank("m", cands);
        // At least 3 hits: model, memory, mcp. All start with `m`.
        assert!(ranking.len() >= 3);
        // Best match should be a name starting with `m`.
        let (best_idx, _) = ranking[0];
        assert!(
            cands[best_idx].starts_with('m'),
            "top hit {:?} does not start with m",
            cands[best_idx]
        );
    }

    #[test]
    fn case_insensitive() {
        assert!(score("HLP", "help").is_some());
        assert!(score("mdl", "MODEL").is_some());
    }
}
