/// Subsequence fuzzy match. Higher is better; None if `query` isn't a subsequence of `text`.
pub fn fuzzy(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let orig: Vec<char> = text.chars().collect();
    let (mut qi, mut score, mut last, mut run) = (0usize, 0i32, usize::MAX, 0i32);
    for (ti, &c) in t.iter().enumerate() {
        if qi < q.len() && c == q[qi] {
            let boundary = ti == 0 || !t[ti - 1].is_alphanumeric() || (orig.get(ti).is_some_and(|c| c.is_uppercase()) && orig.get(ti - 1).is_some_and(|c| c.is_lowercase()));
            run = if last != usize::MAX && last + 1 == ti { run + 1 } else { 0 };
            score += 10 + run * 8 + if boundary { 14 } else { 0 } - (ti as i32).min(20) / 4;
            last = ti;
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    // Prefer shorter candidates and exact substring hits.
    if t.iter().collect::<String>().contains(&q.iter().collect::<String>()) {
        score += 40;
    }
    Some(score - (t.len() as i32) / 6)
}
