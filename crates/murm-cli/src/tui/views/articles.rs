use crate::ArticleSummary;
use crate::tui::sanitize;
pub fn label(article: &ArticleSummary) -> String {
    format!(
        "{} · rev {} · {:?}{}\n{} · {}",
        sanitize(&article.title),
        article.revision,
        article.status,
        if article.conflicts {
            " · CONFLITO"
        } else {
            ""
        },
        article.signer,
        sanitize(&article.publication)
    )
}
