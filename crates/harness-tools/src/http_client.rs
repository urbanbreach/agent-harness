use crate::UnwrapOrAbort;
pub(crate) fn default_client(_expect_message: &'static str) -> reqwest::Client {
    reqwest::Client::builder().build().unwrap_or_abort()
}

pub(crate) fn default_client_or_fallback() -> reqwest::Client {
    reqwest::Client::builder()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub(crate) fn redirect_limited_client(
    limit: usize,
    _expect_message: &'static str,
) -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(limit))
        .build()
        .unwrap_or_abort()
}
