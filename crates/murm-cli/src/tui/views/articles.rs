use crate::ArticleSummary;
use crate::tui::sanitize;
pub fn label(article: &ArticleSummary) -> String {
    format!(
        "rev {} · {:?}{} · {}",
        article.revision,
        article.status,
        if article.conflicts {
            " · CONFLICT"
        } else {
            ""
        },
        sanitize(
            &article
                .signer
                .to_string()
                .chars()
                .take(12)
                .collect::<String>()
        )
    )
}
