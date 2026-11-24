use crate::{DestinationState, DestinationStatus};
pub fn destination_label(destination: &DestinationStatus) -> String {
    let status = match destination.state {
        DestinationState::Pending => "pending".to_owned(),
        DestinationState::Stored => format!(
            "stored at relay · retain_until={}",
            destination
                .retain_until
                .map(|n| n.to_string())
                .unwrap_or_else(|| "no minimum retention promise".into())
        ),
        DestinationState::Refused => format!(
            "refused: {}",
            destination.last_error.as_deref().unwrap_or("")
        ),
        DestinationState::RetryAt => format!(
            "retry after {}: {}",
            destination.retry_at.unwrap_or(0),
            destination.last_error.as_deref().unwrap_or("")
        ),
    };
    format!(
        "{} · {}",
        crate::tui::sanitize(&destination.relay),
        crate::tui::sanitize(&status)
    )
}
pub fn entry_details(entry: &crate::OutboxEntry) -> String {
    format!(
        "EVENT\n{}\n\n{}\n\nStored means storage at that relay, not delivery or guaranteed permanent availability.",
        entry.event_id,
        entry
            .destinations
            .iter()
            .map(|d| format!(
                "{}\nAttempts: {}\nRetry at: {}",
                destination_label(d),
                d.attempts,
                d.retry_at
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "none".into())
            ))
            .collect::<Vec<_>>()
            .join("\n\n")
    )
}
