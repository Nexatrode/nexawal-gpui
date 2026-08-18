//! Create-mode seed backup gate: wrote-it-down plus three random word checks.

pub fn pick_indices(word_count: usize) -> Vec<usize> {
    if word_count < 3 {
        return Vec::new();
    }
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        .max(1);
    let mut indices = Vec::new();
    let mut guard = 0;
    while indices.len() < 3 && guard < 64 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let i = (seed as usize) % word_count;
        if !indices.contains(&i) {
            indices.push(i);
        }
        guard += 1;
    }
    indices.sort_unstable();
    indices
}

pub fn answers_match(mnemonic: &str, indices: &[usize], answers: &[String]) -> bool {
    let words: Vec<&str> = mnemonic.split_whitespace().collect();
    if indices.len() != 3 || answers.len() != 3 || words.len() < 3 {
        return false;
    }
    indices.iter().zip(answers.iter()).all(|(idx, answer)| {
        let expected = words.get(*idx).copied().unwrap_or("");
        !answer.trim().is_empty() && expected.eq_ignore_ascii_case(answer.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches() {
        let mnemonic = "alpha bravo charlie delta echo";
        assert!(answers_match(
            mnemonic,
            &[0, 2, 4],
            &["alpha".into(), "CHARLIE".into(), "echo".into()]
        ));
        assert!(!answers_match(
            mnemonic,
            &[0, 2, 4],
            &["alpha".into(), "nope".into(), "echo".into()]
        ));
    }
}
