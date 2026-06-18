use super::WebFetchFormat;
use super::clean_html_before_conversion;
use super::post_process_text;
use super::readable_content;
use super::validate_fetch_url;

#[test]
fn rejects_local_and_private_urls() {
    assert!(validate_fetch_url("file:///tmp/page.html").is_err());
    assert!(validate_fetch_url("http://localhost:8080").is_err());
    assert!(validate_fetch_url("http://127.0.0.1:8080").is_err());
    assert!(validate_fetch_url("http://10.0.0.1").is_err());
    assert!(validate_fetch_url("https://example.com/page").is_ok());
}

#[test]
fn removes_base64_data_uris_before_html_conversion() {
    let base64 = "a".repeat(120);
    let html = format!(
        r#"<main><h1>Hello</h1><img src="data:image/png;base64,{base64}"><p>Body</p></main>"#
    );

    let cleaned = clean_html_before_conversion(&html);

    assert!(!cleaned.contains("data:image"));
    assert!(!cleaned.contains(&base64));
    assert!(cleaned.contains("Hello"));
}

#[test]
fn removes_script_style_and_navigation_noise() {
    let html = r#"
        <html>
            <script>window.x = "noise";</script>
            <style>body { color: red; }</style>
            <nav>Menu</nav>
            <main><h1>Useful</h1><p>Actual body.</p></main>
        </html>
    "#;

    let (content, truncated) = readable_content(html, "text/html", WebFetchFormat::Text);

    assert!(!truncated);
    assert!(content.contains("Useful"));
    assert!(content.contains("Actual body."));
    assert!(!content.contains("window.x"));
    assert!(!content.contains("color: red"));
    assert!(!content.contains("Menu"));
}

#[test]
fn replaces_long_encoded_tokens() {
    let noisy = format!("prefix {} suffix", "A".repeat(800));

    let cleaned = post_process_text(&noisy);

    assert_eq!(cleaned, "prefix [removed long encoded data] suffix");
}

#[test]
fn truncates_large_output() {
    let text = "word ".repeat(20_000);

    let (content, truncated) = readable_content(&text, "text/plain", WebFetchFormat::Text);

    assert!(truncated);
    assert!(content.contains("[Content truncated after 40000 characters.]"));
}
