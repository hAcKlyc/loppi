// Resolve the same semantic colors as the web UI. Invalid or cyclic tokens must
// fail the build rather than silently give native chrome a second palette.
pub fn colorref(css: &str, token: &str) -> u32 {
    fn resolve(css: &str, token: &str, depth: u8) -> u32 {
        assert!(depth < 16, "cyclic native theme token: {token}");
        let prefix = format!("{token}:");
        let value = css
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix(&prefix)
                    .map(|v| v.trim().trim_end_matches(';'))
            })
            .unwrap_or_else(|| panic!("missing native theme token: {token}"));
        if let Some(alias) = value.strip_prefix("var(").and_then(|v| v.strip_suffix(')')) {
            return resolve(css, alias, depth + 1);
        }
        let hex = value.strip_prefix('#').expect("native color must be hex");
        assert_eq!(hex.len(), 6, "native color must be opaque RGB");
        let rgb = u32::from_str_radix(hex, 16).expect("invalid native RGB");
        (rgb & 0xff) << 16 | (rgb & 0xff00) | (rgb >> 16)
    }
    resolve(css, token, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_colors_follow_semantic_aliases() {
        let css = include_str!("../src/tokens.css");
        assert_eq!(colorref(css, "--color-canvas"), 0xf1f5f6);
        assert_eq!(colorref(css, "--color-text"), 0x282c29);
        assert_eq!(colorref("--a: var(--b);\n--b: #123456;", "--a"), 0x563412);
    }
    #[test]
    fn malformed_or_cyclic_theme_is_rejected() {
        for css in ["--a: var(--b);", "--a: var(--a);", "--a: #12345678;"] {
            assert!(std::panic::catch_unwind(|| colorref(css, "--a")).is_err());
        }
    }
}
