use crate::DraftSummary;
pub fn label(draft: &DraftSummary) -> String {
    format!(
        "rev {} · {}",
        draft
            .revision
            .map(|n| n.to_string())
            .unwrap_or_else(|| "invalid".into()),
        if draft.queued_event_id.is_some() {
            "already queued"
        } else {
            "local draft"
        }
    )
}
