//! Finding the scene someone meant: `!switch brb` should find "BRB 💤", and
//! `!switch mian` "Main".

/// Lowercase letters and digits only, so spacing, case and emoji don't matter.
fn simplify(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Edits between two texts, a swap of neighbours counting as one (optimal string alignment).
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// The index of the name that best matches `query`: the same name, then one
/// starting with it, then one containing it (shortest first), then one with a
/// small typo. `None` when nothing is close.
pub fn best_match(query: &str, names: &[String]) -> Option<usize> {
    let query = simplify(query);
    if query.is_empty() {
        return None;
    }
    let simplified: Vec<String> = names.iter().map(|name| simplify(name)).collect();
    let shortest = |matches: &dyn Fn(&str) -> bool| {
        simplified
            .iter()
            .enumerate()
            .filter(|(_, name)| matches(name))
            .min_by_key(|(_, name)| name.len())
            .map(|(index, _)| index)
    };

    shortest(&|name| name == query)
        .or_else(|| shortest(&|name| name.starts_with(&query)))
        .or_else(|| shortest(&|name| name.contains(&query)))
        .or_else(|| {
            let allowed = (query.chars().count() / 3).max(1);
            simplified
                .iter()
                .enumerate()
                .map(|(index, name)| (index, distance(&query, name)))
                .filter(|(_, distance)| *distance <= allowed)
                .min_by_key(|(_, distance)| *distance)
                .map(|(index, _)| index)
        })
}

#[cfg(test)]
mod tests {
    use super::best_match;

    fn names() -> Vec<String> {
        [
            "Main",
            "Main (backup)",
            "BRB 💤",
            "Starting soon",
            "Just Chatting",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn exact_then_prefix_then_contains() {
        assert_eq!(best_match("main", &names()), Some(0));
        assert_eq!(best_match("brb", &names()), Some(2));
        assert_eq!(best_match("start", &names()), Some(3));
        assert_eq!(best_match("chat", &names()), Some(4));
        assert_eq!(best_match("backup", &names()), Some(1));
    }

    #[test]
    fn small_typos_are_forgiven_but_not_nonsense() {
        assert_eq!(best_match("mian", &names()), Some(0));
        assert_eq!(best_match("strating soon", &names()), Some(3));
        assert_eq!(best_match("xyz", &names()), None);
        assert_eq!(best_match("💤", &names()), None);
    }
}
