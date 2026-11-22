use crate::Draft;
use crate::tui::sanitize;
pub fn label(draft: &Draft) -> String {
    format!(
        "{} · rev {} · {}\n{}",
        sanitize(
            draft
                .header
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Sem título")
        ),
        draft
            .header
            .get("revision")
            .map(|v| v.to_string())
            .unwrap_or_default(),
        if draft.queued_event_id.is_some() {
            "já enfileirado"
        } else {
            "rascunho local"
        },
        draft.id
    )
}
