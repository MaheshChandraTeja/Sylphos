#![deny(unsafe_code)]
#![allow(clippy::too_many_lines)]

use std::{io, pin::Pin, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use bytes::Bytes;
use futures_core::Stream;
use futures_util::{StreamExt, TryStreamExt};
use http::{header, HeaderMap, HeaderName, HeaderValue};
use http_body_util::Empty;
use hyper::body::Incoming;
use hyper::{Method, Request, StatusCode, Uri};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use once_cell::sync::OnceCell;
use tokio::io::{AsyncBufRead, BufReader};
use tokio_util::io::{ReaderStream, StreamReader};
use url::Url;

const DEFAULT_MAX_REDIRECTS: usize = 10;
const DEFAULT_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_HEADER_BYTES: usize = 64 * 1024;
const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_REQ_TIMEOUT_MS: u64 = 20_000;
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Sylphos/1.0 Safari/537.36";

/// Fetch client configuration.
#[derive(Debug, Clone)]
pub struct FetchConfig {
    pub max_redirects: usize,
    pub max_body_bytes: usize,
    pub max_header_bytes: usize,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            max_redirects: DEFAULT_MAX_REDIRECTS,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            connect_timeout: Duration::from_millis(DEFAULT_CONNECT_TIMEOUT_MS),
            request_timeout: Duration::from_millis(DEFAULT_REQ_TIMEOUT_MS),
        }
    }
}

/// Redirect behavior for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectPolicy {
    Follow,
    Error,
    Manual,
}

impl Default for RedirectPolicy {
    fn default() -> Self {
        Self::Follow
    }
}

/// High-level HTTP request options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestOptions {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub max_body_bytes: Option<usize>,
    pub redirect_policy: RedirectPolicy,
}

impl RequestOptions {
    /// Creates a GET request.
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET".to_owned(),
            url: url.into(),
            headers: Vec::new(),
            max_body_bytes: None,
            redirect_policy: RedirectPolicy::Follow,
        }
    }

    /// Creates a request with explicit method.
    #[must_use]
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: Vec::new(),
            max_body_bytes: None,
            redirect_policy: RedirectPolicy::Follow,
        }
    }

    /// Adds a header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Adds multiple headers.
    #[must_use]
    pub fn headers<I, K, V>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.headers.extend(
            headers
                .into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        );
        self
    }

    /// Sets body byte limit.
    #[must_use]
    pub fn max_body_bytes(mut self, value: usize) -> Self {
        self.max_body_bytes = Some(value);
        self
    }

    /// Sets redirect behavior.
    #[must_use]
    pub fn redirect_policy(mut self, value: RedirectPolicy) -> Self {
        self.redirect_policy = value;
        self
    }
}

/// One followed redirect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectRecord {
    pub from_url: String,
    pub to_url: String,
    pub status: u16,
}

fn make_client() -> Client<hyper_rustls::HttpsConnector<HttpConnector>, Empty<Bytes>> {
    let mut http = HttpConnector::new();
    http.enforce_http(false);

    let https = HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(http);

    Client::builder(TokioExecutor::new())
        .pool_max_idle_per_host(8)
        .pool_idle_timeout(Duration::from_secs(90))
        .build(https)
}

static CLIENT: OnceCell<FetchClient> = OnceCell::new();

/// Initializes the global fetch client. Later calls are ignored.
pub fn init(config: FetchConfig) {
    let _ = CLIENT.set(FetchClient::new(config));
}

/// Executes a GET request with default semantics.
pub async fn get(url: &str) -> Result<Response> {
    request(RequestOptions::get(url)).await
}

/// Executes a request through the global client.
pub async fn request(options: RequestOptions) -> Result<Response> {
    let client = CLIENT.get_or_init(|| FetchClient::new(FetchConfig::default()));
    client.request(options).await
}

pub type ResponseBody = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>;

/// Streaming HTTP response with final URL, headers, and redirect diagnostics.
pub struct Response {
    final_url: String,
    status: StatusCode,
    headers: HeaderMap,
    body: ResponseBody,
    limit: usize,
    redirect_chain: Vec<RedirectRecord>,
}

impl Response {
    /// Final URL after redirects.
    #[must_use]
    pub fn final_url(&self) -> &str {
        &self.final_url
    }

    /// Status code.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Status code as u16.
    #[must_use]
    pub fn status_u16(&self) -> u16 {
        self.status.as_u16()
    }

    /// Cloned raw header map.
    #[must_use]
    pub fn headers(&self) -> HeaderMap {
        self.headers.clone()
    }

    /// Header pairs converted to UTF-8 strings.
    #[must_use]
    pub fn header_pairs(&self) -> Vec<(String, String)> {
        self.headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect()
    }

    /// Redirect chain.
    #[must_use]
    pub fn redirect_chain(&self) -> &[RedirectRecord] {
        &self.redirect_chain
    }

    /// Mutable response body stream.
    pub fn body_mut(&mut self) -> &mut ResponseBody {
        &mut self.body
    }

    /// Reads full body bytes while enforcing the configured limit.
    pub async fn get_bytes(mut self) -> Result<Vec<u8>> {
        let mut total = 0usize;
        let mut buf = Vec::new();
        while let Some(chunk) = self.body.next().await {
            let chunk = chunk?;
            total = total.saturating_add(chunk.len());
            if total > self.limit {
                bail!("response body exceeds limit ({} bytes)", self.limit);
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(buf)
    }

    /// Reads full body as UTF-8 text.
    pub async fn get_text(self) -> Result<String> {
        let bytes = self.get_bytes().await?;
        String::from_utf8(bytes).context("response not valid utf-8")
    }
}

/// Hyper/rustls fetch client.
pub struct FetchClient {
    cfg: FetchConfig,
    inner: Client<hyper_rustls::HttpsConnector<HttpConnector>, Empty<Bytes>>,
}

impl FetchClient {
    /// Creates a fetch client.
    #[must_use]
    pub fn new(cfg: FetchConfig) -> Self {
        let inner = make_client();
        Self { cfg, inner }
    }

    /// Executes GET request.
    pub async fn get(&self, url: &str) -> Result<Response> {
        self.request(RequestOptions::get(url)).await
    }

    /// Executes request with method, headers, redirect mode, and byte limit.
    pub async fn request(&self, options: RequestOptions) -> Result<Response> {
        let method = parse_method(&options.method)?;
        let limit = options.max_body_bytes.unwrap_or(self.cfg.max_body_bytes);
        self.request_follow(method, options, limit).await
    }

    async fn request_follow(
        &self,
        mut method: Method,
        options: RequestOptions,
        limit: usize,
    ) -> Result<Response> {
        let mut current = Url::parse(&options.url).context("invalid URL")?;
        validate_scheme(&current)?;
        let mut redirects_left = self.cfg.max_redirects;
        let mut redirect_chain = Vec::new();

        loop {
            let uri: Uri = current.as_str().parse().context("uri parse")?;
            let mut builder = Request::builder().method(method.clone()).uri(uri);

            let headers = builder
                .headers_mut()
                .context("request headers unavailable")?;
            install_default_headers(headers);
            install_user_headers(headers, &options.headers)?;

            let req = builder.body(Empty::<Bytes>::new())?;

            let resp = tokio::time::timeout(self.cfg.request_timeout, self.inner.request(req))
                .await
                .context("request timed out")??;

            self.enforce_header_limit(resp.headers())?;

            if is_redirect(resp.status()) {
                let status = resp.status().as_u16();
                let loc = resp
                    .headers()
                    .get(header::LOCATION)
                    .cloned()
                    .ok_or_else(|| anyhow!("redirect without Location header"))?;
                let new_url = resolve_location(&current, &loc)
                    .context("failed to resolve redirect Location")?;
                validate_scheme(&new_url)?;

                match options.redirect_policy {
                    RedirectPolicy::Error => bail!("redirect blocked by request redirect policy"),
                    RedirectPolicy::Manual => {
                        let status = resp.status();
                        let headers = resp.headers().clone();
                        let body = limit_body_stream(
                            decode_body_stream(&headers, hyper_body_stream(resp.into_body())),
                            limit,
                        );
                        return Ok(Response {
                            final_url: current.to_string(),
                            status,
                            headers,
                            body,
                            limit,
                            redirect_chain,
                        });
                    }
                    RedirectPolicy::Follow => {}
                }

                if redirects_left == 0 {
                    bail!("too many redirects");
                }
                redirects_left = redirects_left.saturating_sub(1);

                let from_url = current.to_string();
                let to_url = new_url.to_string();
                redirect_chain.push(RedirectRecord {
                    from_url,
                    to_url: to_url.clone(),
                    status,
                });
                if status == 303 && method != Method::GET && method != Method::HEAD {
                    method = Method::GET;
                }
                current = new_url;
                drop(resp);
                continue;
            }

            let status = resp.status();
            let headers = resp.headers().clone();
            let raw_body = hyper_body_stream(resp.into_body());
            let decoded = decode_body_stream(&headers, raw_body);
            let limited = limit_body_stream(decoded, limit);

            return Ok(Response {
                final_url: current.to_string(),
                status,
                headers,
                body: limited,
                limit,
                redirect_chain,
            });
        }
    }

    fn enforce_header_limit(&self, headers: &HeaderMap) -> Result<()> {
        let mut total = 0usize;
        for (k, v) in headers.iter() {
            total = total.saturating_add(k.as_str().len());
            total = total.saturating_add(v.as_bytes().len());
            if total > self.cfg.max_header_bytes {
                bail!(
                    "headers exceed configured size limit ({} bytes)",
                    self.cfg.max_header_bytes
                );
            }
        }
        Ok(())
    }
}

fn install_default_headers(headers: &mut HeaderMap) {
    headers.insert(header::ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(
        header::ACCEPT_ENCODING,
        HeaderValue::from_static("gzip, deflate"),
    );
    headers.insert(
        header::ACCEPT_LANGUAGE,
        HeaderValue::from_static("en-US,en;q=0.9"),
    );
    headers.insert(
        header::USER_AGENT,
        HeaderValue::from_static(DEFAULT_USER_AGENT),
    );
}

fn install_user_headers(headers: &mut HeaderMap, pairs: &[(String, String)]) -> Result<()> {
    for (name, value) in pairs {
        let name = HeaderName::from_bytes(name.trim().as_bytes())
            .with_context(|| format!("invalid request header name `{name}`"))?;
        let value = HeaderValue::from_str(value.trim())
            .with_context(|| format!("invalid request header value for `{name}`"))?;
        headers.insert(name, value);
    }
    Ok(())
}

fn parse_method(value: &str) -> Result<Method> {
    match value.trim().to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "HEAD" => Ok(Method::HEAD),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "PATCH" => Ok(Method::PATCH),
        "DELETE" => Ok(Method::DELETE),
        "OPTIONS" => Ok(Method::OPTIONS),
        other => bail!("unsupported HTTP method `{other}`"),
    }
}

fn validate_scheme(url: &Url) -> Result<()> {
    match url.scheme() {
        "http" | "https" => Ok(()),
        scheme => bail!("unsupported URL scheme `{scheme}`"),
    }
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

fn resolve_location(base: &Url, loc: &HeaderValue) -> Result<Url> {
    let value = loc.to_str().context("Location not valid UTF-8")?;
    base.join(value)
        .or_else(|_| Url::parse(value))
        .context("invalid Location URL")
}

fn hyper_body_stream(body: Incoming) -> ResponseBody {
    let stream = http_body_util::BodyExt::into_data_stream(body).map_err(anyhow::Error::from);
    Box::pin(stream)
}

fn decode_body_stream(headers: &HeaderMap, body: ResponseBody) -> ResponseBody {
    let encoding = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    if encoding.contains("gzip") {
        let reader = stream_reader(body);
        let decoded = async_compression::tokio::bufread::GzipDecoder::new(reader);
        let stream = ReaderStream::new(decoded).map_err(anyhow::Error::from);
        Box::pin(stream)
    } else if encoding.contains("deflate") {
        let reader = stream_reader(body);
        let decoded = async_compression::tokio::bufread::ZlibDecoder::new(reader);
        let stream = ReaderStream::new(decoded).map_err(anyhow::Error::from);
        Box::pin(stream)
    } else {
        body
    }
}

fn stream_reader(stream: ResponseBody) -> impl AsyncBufRead {
    let byte_stream = stream.map_err(io::Error::other);
    let reader = StreamReader::new(byte_stream);
    BufReader::new(reader)
}

fn limit_body_stream(body: ResponseBody, max: usize) -> ResponseBody {
    struct LimitStream {
        inner: ResponseBody,
        total: usize,
        max: usize,
    }

    impl Stream for LimitStream {
        type Item = Result<Bytes>;

        fn poll_next(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            match self.inner.as_mut().poll_next(cx) {
                std::task::Poll::Ready(Some(Ok(bytes))) => {
                    self.total = self.total.saturating_add(bytes.len());
                    if self.total > self.max {
                        return std::task::Poll::Ready(Some(Err(anyhow!(
                            "response body exceeds limit"
                        ))));
                    }
                    std::task::Poll::Ready(Some(Ok(bytes)))
                }
                other => other,
            }
        }
    }

    Box::pin(LimitStream {
        inner: body,
        total: 0,
        max,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{stream, TryStreamExt};

    #[tokio::test]
    async fn header_limit_ok() {
        let cfg = FetchConfig {
            max_header_bytes: 10_000,
            ..Default::default()
        };
        let client = FetchClient::new(cfg);
        let mut headers = HeaderMap::new();
        headers.insert("a", HeaderValue::from_static("b"));
        assert!(client.enforce_header_limit(&headers).is_ok());
    }

    #[tokio::test]
    async fn decode_passthrough() {
        let stream = Box::pin(stream::iter(vec![Ok(Bytes::from_static(b"hi"))])) as ResponseBody;
        let out = decode_body_stream(&HeaderMap::new(), stream);
        let value = out
            .map_ok(|bytes| bytes.to_vec())
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        assert_eq!(value.concat(), b"hi");
    }

    #[test]
    fn request_builder_sets_headers() {
        let request = RequestOptions::get("https://example.com/")
            .header("Accept", "text/html")
            .max_body_bytes(1024)
            .redirect_policy(RedirectPolicy::Error);
        assert_eq!(request.headers.len(), 1);
        assert_eq!(request.max_body_bytes, Some(1024));
        assert_eq!(request.redirect_policy, RedirectPolicy::Error);
    }

    #[ignore]
    #[tokio::test]
    async fn live_get_example() {
        init(Default::default());
        let response = get("https://example.com").await.unwrap();
        assert!(response.status().is_success());
        let text = response.get_text().await.unwrap();
        assert!(text.to_lowercase().contains("example"));
    }
}
