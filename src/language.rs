use isolang::Language;
use whatlang::{Lang, detect};

pub fn target_language<'a>(
    text: &str,
    user_language: &'a str,
    second_language: &'a str,
) -> &'a str {
    match detect(text) {
        Some(info) if matches_configured_language(info.lang(), user_language) => second_language,
        _ => user_language,
    }
}

fn matches_configured_language(detected: Lang, configured: &str) -> bool {
    let normalized = configured.trim().to_ascii_lowercase();
    let base = normalized.split(['-', '_']).next().unwrap_or(&normalized);
    if matches!(base, "zh" | "zho" | "chi" | "cmn")
        || matches!(
            normalized.as_str(),
            "chinese" | "simplified chinese" | "mandarin"
        )
    {
        return detected == Lang::Cmn;
    }

    let configured = Language::from_639_1(base)
        .or_else(|| Language::from_639_3(base))
        .or_else(|| Language::from_name(configured.trim()));
    let detected = Language::from_639_3(detected.code());
    configured.is_some() && configured == detected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_text_uses_second_language() {
        let target = target_language(
            "这是一段用于测试语言识别功能的简体中文文本。",
            "zh-CN",
            "en",
        );
        assert_eq!(target, "en");
    }

    #[test]
    fn other_language_uses_user_language() {
        let target = target_language(
            "This is a sufficiently long English sentence for language detection.",
            "zh-CN",
            "en",
        );
        assert_eq!(target, "zh-CN");
    }

    #[test]
    fn english_can_be_the_user_language() {
        let target = target_language(
            "This is another sufficiently long English sentence for reliable detection.",
            "English",
            "fr",
        );
        assert_eq!(target, "fr");
    }
}
