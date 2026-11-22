use crate::{DestinationState, DestinationStatus};
pub fn destination_label(destination: &DestinationStatus) -> String {
    let status = match destination.state {
        DestinationState::Pending => "pendente".to_owned(),
        DestinationState::Stored => format!(
            "armazenado no relay · retain_until={}",
            destination
                .retain_until
                .map(|n| n.to_string())
                .unwrap_or_else(|| "sem promessa mínima".into())
        ),
        DestinationState::Refused => format!(
            "recusado: {}",
            destination.last_error.as_deref().unwrap_or("")
        ),
        DestinationState::RetryAt => format!(
            "nova tentativa após {}: {}",
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
