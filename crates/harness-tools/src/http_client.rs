pub(crate) fn default_client(expect_message: &'static str) -> reqwest::Client {
    reqwest::Client::builder().build().expect(expect_message)
}

pub(crate) fn default_client_or_fallback() -> reqwest::Client {
    reqwest::Client::builder()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub(crate) fn redirect_limited_client(
    limit: usize,
    expect_message: &'static str,
) -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(limit))
        .build()
        .expect(expect_message)
}
