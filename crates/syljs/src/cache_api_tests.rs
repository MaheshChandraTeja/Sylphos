use super::cache_api::{
    analyze_service_worker_script, CacheApiHost, CacheApiRequest, CacheApiResponse,
    ResearchCacheStorage, ResearchServiceWorkerHost, ServiceWorkerHost,
};
use std::rc::Rc;

#[test]
fn cache_storage_put_match_delete_roundtrip() {
    let cache = ResearchCacheStorage::new();
    cache.open_cache("static-v1").unwrap();
    cache
        .put(
            "static-v1",
            CacheApiRequest::get("/app.js"),
            CacheApiResponse::text("/app.js", "console.log('cached')"),
        )
        .unwrap();

    let hit = cache
        .match_in_cache("static-v1", &CacheApiRequest::get("/app.js"))
        .unwrap();
    assert_eq!(hit.body, "console.log('cached')");
    assert_eq!(cache.keys("static-v1").len(), 1);
    assert!(cache.delete_entry("static-v1", &CacheApiRequest::get("/app.js")));
    assert!(cache
        .match_in_cache("static-v1", &CacheApiRequest::get("/app.js"))
        .is_none());
}

#[test]
fn service_worker_analysis_detects_cache_first_precache() {
    let source = r#"
        self.addEventListener('install', event => {
            self.skipWaiting();
            event.waitUntil(caches.open('shell-v1').then(cache => cache.addAll([
                '/', '/app.css', '/app.js'
            ])));
        });
        self.addEventListener('activate', event => clients.claim());
        self.addEventListener('fetch', event => {
            event.respondWith(caches.match(event.request).then(hit => hit || fetch(event.request)));
        });
    "#;

    let analysis = analyze_service_worker_script(source);
    assert!(analysis.has_install_listener);
    assert!(analysis.has_activate_listener);
    assert!(analysis.has_fetch_listener);
    assert!(analysis.skip_waiting);
    assert!(analysis.clients_claim);
    assert!(analysis.cache_first_fetch);
    assert_eq!(analysis.cache_names, vec!["shell-v1".to_owned()]);
    assert!(analysis.precache_urls.contains(&"/app.js".to_owned()));
}

#[test]
fn registered_service_worker_precaches_and_intercepts_fetch() {
    let cache = Rc::new(ResearchCacheStorage::new());
    let host = ResearchServiceWorkerHost::new(cache.clone());
    host.register_script(
        "/sw.js",
        r#"
            self.addEventListener('install', event => {
                event.waitUntil(caches.open('shell').then(cache => cache.addAll(['/offline.html'])));
            });
            self.addEventListener('fetch', event => {
                event.respondWith(caches.match(event.request).then(hit => hit || fetch(event.request)));
            });
        "#,
    );

    let registration = host
        .register_service_worker("/sw.js".to_owned(), Some("/".to_owned()))
        .unwrap();
    assert_eq!(registration.scope, "/");
    assert!(registration.controls_clients);

    let response = host
        .intercept_fetch(&CacheApiRequest::get("/offline.html"))
        .unwrap();
    assert_eq!(response.body, "service-worker-precache:/offline.html");

    let metrics = host.metrics();
    assert_eq!(metrics.service_worker_registrations, 1);
    assert_eq!(metrics.fetch_cache_hits, 1);
}
