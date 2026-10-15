//! Error taxonomy (ITEM-057).
//!
//! "Errors" used to be one bucket (`status_code >= 400`). This classifies a
//! failed or aborted request by cause, so reliability is legible: an upstream
//! 429 is not the tenant's 400, and a transport cut is not an upstream outage.
//! The class is chosen at the failure site — never by matching an error string.

/// Why a request failed or was cut short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// The upstream provider answered 429.
    Upstream429,
    /// The upstream provider answered 5xx.
    Upstream5xx,
    /// The upstream provider answered another 4xx (bad request, auth, ...).
    Client4xx,
    /// Inferon itself refused the request (budget cap, no provider, disabled
    /// model, bad input) — a typed 4xx.
    Inferon4xx,
    /// Inferon itself failed (serialization/internal) — a 5xx.
    Inferon5xx,
    /// The request to the upstream failed at the transport level.
    Transport,
    /// The upstream call timed out (connect timeout).
    Timeout,
    /// The schema guard could not produce a valid response.
    Guard,
    /// The client disconnected before the stream finished.
    ClientAbort,
}

impl ErrorClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorClass::Upstream429 => "upstream_429",
            ErrorClass::Upstream5xx => "upstream_5xx",
            ErrorClass::Client4xx => "client_4xx",
            ErrorClass::Inferon4xx => "inferon_4xx",
            ErrorClass::Inferon5xx => "inferon_5xx",
            ErrorClass::Transport => "transport",
            ErrorClass::Timeout => "timeout",
            ErrorClass::Guard => "guard",
            ErrorClass::ClientAbort => "client_abort",
        }
    }

    /// Parse a persisted class; unknown or absent → `None`.
    pub fn from_db(value: Option<&str>) -> Option<Self> {
        match value? {
            "upstream_429" => Some(ErrorClass::Upstream429),
            "upstream_5xx" => Some(ErrorClass::Upstream5xx),
            "client_4xx" => Some(ErrorClass::Client4xx),
            "inferon_4xx" => Some(ErrorClass::Inferon4xx),
            "inferon_5xx" => Some(ErrorClass::Inferon5xx),
            "transport" => Some(ErrorClass::Transport),
            "timeout" => Some(ErrorClass::Timeout),
            "guard" => Some(ErrorClass::Guard),
            "client_abort" => Some(ErrorClass::ClientAbort),
            _ => None,
        }
    }

    /// Class for a non-2xx status returned by an upstream provider.
    pub fn from_upstream_status(status: u16) -> Self {
        match status {
            429 => ErrorClass::Upstream429,
            s if s >= 500 => ErrorClass::Upstream5xx,
            _ => ErrorClass::Client4xx,
        }
    }

    /// Class for a failed upstream call: a connect/request timeout is its own
    /// class, everything else is a transport failure.
    pub fn from_transport(error: &reqwest::Error) -> Self {
        if error.is_timeout() {
            ErrorClass::Timeout
        } else {
            ErrorClass::Transport
        }
    }
}

#[cfg(test)]
mod tests;
