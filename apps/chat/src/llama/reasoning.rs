//! Levels accepted by a loaded model's template, minus user-hidden levels.

pub const CANDIDATES: &[(&str, &str)] = &[
    ("Minimal", "minimal"),
    ("Low", "low"),
    ("Medium", "medium"),
    ("High", "high"),
    ("XHigh", "xhigh"),
    ("Max", "max"),
];

pub fn options(levels: &[String], hidden: &[String]) -> Vec<(&'static str, &'static str)> {
    CANDIDATES.iter().copied().filter(|(_, value)| levels.iter().any(|level| level == value)
        && !hidden.iter().any(|level| level == value)).collect()
}

pub fn effective<'a>(options: &[(&str, &'a str)], selected: Option<&str>) -> Option<&'a str> {
    selected.and_then(|level| options.iter().find(|(_, value)| *value == level).map(|(_, value)| *value))
        .or_else(|| selected.and_then(|_| options.first().map(|(_, value)| *value)))
}

pub fn label(level: &str) -> Option<&'static str> {
    CANDIDATES.iter().find(|(_, value)| *value == level).map(|(label, _)| *label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_levels_are_filtered_from_display_and_requests() {
        let levels = vec!["low".into(), "medium".into(), "xhigh".into()];
        let hidden = vec!["low".into()];
        let choices = options(&levels, &hidden);
        assert_eq!(choices, [("Medium", "medium"), ("XHigh", "xhigh")]);
        assert_eq!(effective(&choices, Some("low")), Some("medium"));
        assert_eq!(effective(&choices, Some("xhigh")), Some("xhigh"));
        assert_eq!(effective(&choices, None), None);
    }
}
