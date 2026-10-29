//! Small helpers shared by the whole application.

use std::cmp::Ordering;
use std::path::Path;

/// Natural ("human") string comparison: "page2" < "page10", case-insensitive.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    bi.next();
                }
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(ca), Some(cb)) => {
                let la = ca.to_lowercase().next().unwrap_or(ca);
                let lb = cb.to_lowercase().next().unwrap_or(cb);
                if la != lb {
                    return la.cmp(&lb);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// Compares two issue numbers the way ComicRack does: numerically when both
/// parse as numbers ("1.5" < "2"), otherwise naturally.
pub fn number_cmp(a: &str, b: &str) -> Ordering {
    match (parse_number(a), parse_number(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Some(_), None) if b.is_empty() => Ordering::Less,
        (None, Some(_)) if a.is_empty() => Ordering::Greater,
        _ => natural_cmp(a, b),
    }
}

/// Parses the leading numeric part of an issue number ("12", "1.5", "½", "3a").
pub fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s == "½" {
        return Some(0.5);
    }
    let end = s
        .char_indices()
        .take_while(|(i, c)| c.is_ascii_digit() || (*c == '.' && *i > 0) || (*c == '-' && *i == 0))
        .map(|(i, c)| i + c.len_utf8())
        .last()?;
    s[..end].parse().ok()
}

/// Size in bytes as a short human-readable text ("1.5 MB").
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} {}", crate::i18n::tr(UNITS[0]))
    } else {
        format!("{v:.1} {}", crate::i18n::tr(UNITS[u]))
    }
}

pub fn extension_lower(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Splits a ComicRack multi-value field ("A, B, C") into its trimmed items.
pub fn split_list(s: &str) -> Vec<String> {
    s.split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Locates an external executable, either from an explicit setting or the PATH.
pub fn find_program(configured: &str, candidates: &[&str]) -> Option<std::path::PathBuf> {
    if !configured.trim().is_empty() {
        let p = std::path::PathBuf::from(configured.trim());
        if p.is_file() {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        vec![".exe".into(), String::new()]
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for c in candidates {
            for e in &exts {
                let p = dir.join(format!("{c}{e}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    if cfg!(windows) {
        for base in ["C:\\Program Files\\WinRAR", "C:\\Program Files\\7-Zip"] {
            for c in candidates {
                let p = Path::new(base).join(format!("{c}.exe"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural() {
        assert_eq!(natural_cmp("p2.jpg", "p10.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("A", "a"), Ordering::Equal);
        assert_eq!(number_cmp("1.5", "2"), Ordering::Less);
        assert_eq!(number_cmp("10", "9"), Ordering::Greater);
        assert_eq!(parse_number("3a"), Some(3.0));
    }
}
