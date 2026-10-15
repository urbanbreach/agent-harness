//! Provider rejections: token-context overflow versus request size or media limits.
use regex::Regex;
use std::sync::LazyLock;

type Pattern = LazyLock<Result<Regex, regex::Error>>;

/// Explicit token-context evidence; it also vetoes a textual payload classification.
static TOKEN_CONTEXT: Pattern = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)prompt is too long|input is too long for requested model|exceeds the context window",
        r"|input token count.*exceeds the maximum|maximum prompt length is \d+",
        r"|reduce the length of the messages|maximum context length is \d+ tokens",
        r"|exceeds the available context size",
        r"|\bprompt\s*\(\s*\d+\s+tokens\s*\)\s*\+\s*max\s+tokens\s*\(\s*\d+\s*\)\s+exceeds\s+the\s+context\s*\(\s*\d+\s*\)",
        r"|\bprompt\s*\(\s*\d+\s+tokens\s*\)\s+leaves\s+no\s+room\s+to\s+answer\s+in\s+the\s+context\s*\(\s*\d+\s*\)",
        r"|requested tokens?.*exceed.*context (window|length|size)",
        r"|context (window|length|size).*(exceeded|overflow|too small)",
        r"|(prompt|input).*(too long|too large).*(context|n_ctx)",
        r"|requested tokens?.*(exceeds?|greater than).*(n_ctx|context)",
        r"|greater than the context length|context window exceeds limit|exceeded model token limit",
        r"|context[_ ]length[_ ]exceeded|too many tokens|token limit exceeded",
        r"|request_too_large[^\n]*\btokens?\b|\btokens?\b[^\n]*request_too_large",
        r"|model_context_window_exceeded|prompt filled the context window",
        r"|exceeds the limit of \d+ tokens?\b|chat history exceeds the \d+-message limit",
    ))
});

/// Generic overflow forms that count as overflow but do not veto a payload rejection.
static GENERIC_OVERFLOW: Pattern = LazyLock::new(|| {
    Regex::new(r"(?i)exceeds the limit of \d+|\b4(00|13)\s*(status code)?\s*\(no body\)")
});

static PAYLOAD: Pattern = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b413\s*(?:status code\s*)?\(no body\)",
        r"|\b413\b[^.\n]{0,120}\b(?:request|payload|entity|body)\b[^.\n]{0,60}\b(?:exceed|too large|limit)",
        r"|request_too_large|(?:payload|entity) too large",
        r"|request exceeds the maximum (?:size|number of bytes)",
    ))
});

static MEDIA: Pattern = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\btoo many (?:images?|frames?|pixels?)\b",
        r"|\b(?:images?|frames?|pixels?)\s*(?:count|limit)\b",
        r"|\blimit of \d+\s*(?:images?|frames?|pixels?)\b",
        r"|\bmaximum(?: of \d+)? (?:images?|frames?|pixels?)\b",
        r"|\b(?:number|count) of (?:images?|frames?|pixels?)\b",
        r"|\b(?:images?|frames?|pixels?)\b.{0,20}\bexceeds?\b.{0,20}\bmaximum\b",
        r"|\b(?:images?|frames?) (?:is |are )?too large\b",
        r"|\b(?:images?|frames?) dimensions?\b.{0,30}\bexceeds?\b.{0,30}\b(?:pixels?|\d+)\b",
    ))
});

fn matches(pattern: &Pattern, text: &str) -> bool {
    pattern.as_ref().is_ok_and(|regex| regex.is_match(text))
}

/// The error text proves a token-context overflow.
pub fn is_token_overflow(text: &str) -> bool {
    matches(&TOKEN_CONTEXT, text) || matches(&GENERIC_OVERFLOW, text)
}

/// The error text is a request size rejection without explicit token-context evidence.
pub fn is_payload_rejection(text: &str) -> bool {
    matches(&PAYLOAD, text) && !matches(&TOKEN_CONTEXT, text)
}

/// The error names an image, frame or pixel limit, not merely an image type.
pub fn has_media_limit_evidence(text: &str) -> bool {
    matches(&MEDIA, text)
}

/// `1234567` as `1,234,567`, the formatting user-facing token counts use.
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

const SIZE_REMEDY: &str = "this is NOT necessarily a token-context problem. Reduce or remove oversized media from the live request, shrink older history, or raise the provider/proxy body limit.";

/// Warning for a size rejection; `headroom` is `None` when the window is unknown.
pub fn byte_rejection_warning(headroom: Option<u64>) -> String {
    match headroom {
        Some(headroom) => format!(
            "The provider rejected the request size or media budget (HTTP 413), but ~{} tokens of headroom remain locally — {SIZE_REMEDY}",
            thousands(headroom)
        ),
        None => format!(
            "The provider rejected the request size or media budget (HTTP 413), and this model has no known context window to compare against — {SIZE_REMEDY}"
        ),
    }
}

/// Warning when reported usage proves an overflow that no compaction method can shrink.
pub fn usage_dead_end_warning(reported: u64, window: Option<u64>) -> String {
    let window = window.map_or_else(|| "unknown".to_owned(), thousands);
    format!(
        "The provider reports ~{} input tokens against a {window}-token context window — this IS a token-context problem, but automatic compaction is unavailable to shrink it. Enable a compaction method (compaction.enabled with a configured methodOrder), reduce the conversation's token usage, or switch to a larger-context model.",
        thousands(reported)
    )
}

static BODY_READ_TIMEOUT: Pattern = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\brequest (?:body )?read timeout\b",
        r"|\b(?:request )?body\b[^.\n]{0,40}\b(?:timed? ?out|timeout)\b",
        r"|\b(?:timed? ?out|timeout)\b[^.\n]{0,40}\bread(?:ing)?\b[^.\n]{0,20}\bbody\b",
    ))
});

/// The provider gave up waiting for the request body: HTTP 408 or an explicit body-read
/// timeout text. Shrinking the request can help; a token window problem it is not.
pub fn is_body_read_timeout(status: Option<u16>, text: &str) -> bool {
    status == Some(408) || matches(&BODY_READ_TIMEOUT, text)
}

/// Usage the provider reported for the failed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportedUsage {
    /// Input plus cache reads plus cache writes.
    pub raw_input: u64,
    /// Provider-reported context occupancy, when it states one.
    pub context_tokens: Option<u64>,
}

impl ReportedUsage {
    /// What the usage-backed overflow test compares: the context value when present.
    fn backed(self) -> u64 {
        self.context_tokens.unwrap_or(self.raw_input)
    }
}

/// A rejected request as the provider error describes it.
#[derive(Debug, Clone, Copy)]
pub struct Rejection<'a> {
    pub message: &'a str,
    /// HTTP status the failure carried, when it had one.
    pub status: Option<u16>,
    /// The error already says payload rejection; this is authoritative.
    pub payload_flag: bool,
    /// The error's category is a context-window overflow.
    pub overflow_flag: bool,
    pub usage: Option<ReportedUsage>,
}

impl Rejection<'_> {
    /// The rejection is a request size problem. HTTP 413 sets the flag first; final text
    /// with explicit token-context evidence then removes a flag that only the status set.
    pub fn is_payload(&self) -> bool {
        if self.payload_flag {
            return true;
        }
        let text = is_payload_rejection(self.message);
        let from_status = self.status == Some(413) && !matches(&TOKEN_CONTEXT, self.message);
        text || from_status
    }

    /// The error alone proves a token-context overflow (flag or text).
    pub fn is_overflow(&self) -> bool {
        self.overflow_flag || is_token_overflow(self.message)
    }
}

/// Everything the guard knows about one payload rejection before asking the context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub payload: bool,
    /// Payload and also flagged as a context overflow.
    pub ambiguous: bool,
    /// Explicit image, frame or pixel limit evidence.
    pub media: bool,
    /// Reported usage exceeds the known window.
    pub usage_overflow: bool,
    /// Known window, reported raw input within it, local estimate under 90% of it.
    pub trusted: bool,
    /// Window the catalog states; `None` when unknown.
    pub window: Option<u64>,
    /// Local stored estimate of the context.
    pub local: u64,
    /// Tokens the usage-backed test compared (zero without usage).
    pub reported: u64,
}

impl Evidence {
    pub fn assess(rejection: &Rejection<'_>, window: Option<u64>, local: u64) -> Self {
        let window = window.filter(|window| *window > 0);
        let payload = rejection.is_payload();
        let reported = rejection.usage.map_or(0, ReportedUsage::backed);
        let raw = rejection.usage.map_or(0, |usage| usage.raw_input);
        Self {
            payload,
            ambiguous: payload && rejection.overflow_flag,
            media: has_media_limit_evidence(rejection.message),
            usage_overflow: window
                .is_some_and(|window| rejection.usage.is_some() && reported > window),
            trusted: window.is_some_and(|window| raw <= window && local * 10 < window * 9),
            window,
            local,
            reported,
        }
    }

    /// Image-producing methods may not run: a byte or media limit is not helped by frames
    /// unless usage proves a token overflow and the error names no media limit.
    pub fn exclude_archive(&self) -> bool {
        self.payload && (!self.usage_overflow || self.media)
    }

    /// The warning shape for this rejection: tokens when usage proved an overflow, else
    /// bytes with the local headroom (the stored estimate against the known window).
    pub fn dead_end(&self) -> DeadEnd {
        if self.usage_overflow {
            DeadEnd::Tokens {
                reported: self.reported,
                window: self.window,
            }
        } else {
            DeadEnd::Bytes {
                headroom: self.window.map(|window| window.saturating_sub(self.local)),
            }
        }
    }
}

/// What the context says about shrinking, computed by the caller after `Evidence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Compaction is enabled and some method can run for this overflow, given the archive
    /// exclusion `Evidence::exclude_archive` reports.
    pub usable: bool,
    /// An older non-error assistant on the summarized side exists that a usable non-media
    /// method would prepare; a cut only inside the failing turn does not count.
    pub reclaimable: bool,
}

/// Why a payload rejection ends without a summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadEnd {
    /// Request size or media budget; `headroom` is `None` for an unknown window.
    Bytes { headroom: Option<u64> },
    /// Reported usage proves a token overflow that nothing can shrink.
    Tokens { reported: u64, window: Option<u64> },
}

impl DeadEnd {
    pub fn warning(self) -> String {
        match self {
            Self::Bytes { headroom } => byte_rejection_warning(headroom),
            Self::Tokens { reported, window } => usage_dead_end_warning(reported, window),
        }
    }
}

/// The guard's decision for a rejected request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Not a payload rejection; generic overflow routing applies.
    NotPayload,
    /// Warn and stop: no summary, no continuation.
    Terminal(DeadEnd),
    /// Try the normal recovery path. `exclude_media` bars image-producing methods and
    /// `promote` says whether a larger-context model may be tried first.
    Shrink { exclude_media: bool, promote: bool },
}

impl Evidence {
    pub fn verdict(&self, capabilities: Capabilities) -> Verdict {
        if !self.payload {
            return Verdict::NotPayload;
        }
        // Step 6 also ends here when nothing usable can shrink (which covers an unknown
        // window without usable compaction).
        let terminal = !capabilities.usable
            || (self.trusted && !capabilities.reclaimable)
            || (self.media && !self.usage_overflow);
        if terminal {
            return Verdict::Terminal(self.dead_end());
        }
        Verdict::Shrink {
            exclude_media: self.exclude_archive(),
            promote: !self.trusted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_rejections_are_told_apart_from_token_overflow() {
        for (text, token, payload, media) in [
            ("HTTP 413 (no body)", true, true, false),
            (
                "413 Request Entity Too Large: request body exceeds limit",
                false,
                true,
                false,
            ),
            (
                "request_too_large: prompt has too many tokens",
                true,
                false,
                false,
            ),
            (
                "prompt is too long: 210000 tokens > 200000 maximum",
                true,
                false,
                false,
            ),
            (
                "payload too large: too many images (limit of 20 images)",
                false,
                true,
                true,
            ),
            ("unsupported content type image/png", false, false, false),
            ("image dimensions exceed 8000 pixels", false, false, true),
        ] {
            assert_eq!(is_token_overflow(text), token, "{text}");
            assert_eq!(is_payload_rejection(text), payload, "{text}");
            assert_eq!(has_media_limit_evidence(text), media, "{text}");
        }
    }

    #[test]
    fn warnings_format_counts_for_en_us() {
        assert!(byte_rejection_warning(Some(1_234_567)).contains("~1,234,567 tokens of headroom"));
        assert!(usage_dead_end_warning(210_000, None).contains("against a unknown-token"));
        assert!(usage_dead_end_warning(210_000, Some(200_000))
            .contains("~210,000 input tokens against a 200,000-token"));
    }
}

#[cfg(test)]
mod guard_tests;
