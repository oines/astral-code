#[derive(Debug, Clone)]
pub(crate) enum StatusAccountDisplay {
    ApiKey,
    Chatgpt { email: Option<String> },
}
