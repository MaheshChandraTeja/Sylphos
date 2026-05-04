use crate::{
    compute_freshness, mime_allowed_for_destination, sniff_mime, HttpDestination, HttpHeaderList,
    HttpMethod,
};
use std::time::Duration;

#[test]
fn method_parsing_is_stable() {
    assert_eq!(HttpMethod::parse("get"), Some(HttpMethod::Get));
    assert_eq!(HttpMethod::parse("POST"), Some(HttpMethod::Post));
    assert_eq!(HttpMethod::parse("BREW"), None);
}

#[test]
fn no_store_is_not_storable() {
    let headers = HttpHeaderList::from_pairs([("cache-control", "no-store, max-age=999")]);
    let decision = compute_freshness(200, &headers, Duration::from_secs(60), 1000);
    assert!(!decision.storable);
    assert_eq!(decision.expires_at_ms, 1000);
}

#[test]
fn destination_mime_guard_blocks_script_as_png() {
    let headers = HttpHeaderList::from_pairs([("content-type", "image/png")]);
    let mime = sniff_mime(
        HttpDestination::Script,
        "https://example.com/app.js",
        &headers,
        b"alert(1)",
    );
    assert_eq!(mime.essence, "image/png");
    assert!(!mime_allowed_for_destination(
        HttpDestination::Script,
        &mime
    ));
}

#[test]
fn url_extension_sniffing_works_for_worker() {
    let headers = HttpHeaderList::new();
    let mime = sniff_mime(
        HttpDestination::Worker,
        "https://example.com/worker.mjs",
        &headers,
        b"self.onmessage=function(){};",
    );
    assert_eq!(mime.essence, "text/javascript");
    assert!(mime.sniffed);
}
