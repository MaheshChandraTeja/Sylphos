#![deny(unsafe_code)]

use std::{io, pin::Pin, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use bytes::Bytes;
use futures_core::Stream;
use futures_util::{StreamExt, TryStreamExt};
use http::{header, HeaderMap, HeaderValue};
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
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Sylphos/0.1 Safari/537.36";

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

pub fn init(config: FetchConfig) {
    let _ = CLIENT.set(FetchClient::new(config));
}

pub async fn get(url: &str) -> Result<Response> {
    let client = CLIENT.get_or_init(|| FetchClient::new(FetchConfig::default()));
    client.get(url).await
}

pub type ResponseBody = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>;

pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: ResponseBody,
    limit: usize,
}

impl Response {
    pub fn status(&self) -> StatusCode {
        self.status
    }
    pub fn headers(&self) -> HeaderMap {
        self.headers.clone()
    }
    pub fn body_mut(&mut self) -> &mut ResponseBody {
        &mut self.body
    }
    pub async fn get_text(mut self) -> Result<String> {
        let mut total = 0usize;
        let mut buf = Vec::new();
        while let Some(chunk) = self.body.next().await {
            let c = chunk?;
            total += c.len();
            if total > self.limit {
                bail!("response body exceeds limit ({} bytes)", self.limit);
            }
            buf.extend_from_slice(&c);
        }
        let s = String::from_utf8(buf).context("response not valid utf-8")?;
        Ok(s)
    }
}

pub struct FetchClient {
    cfg: FetchConfig,
    inner: Client<hyper_rustls::HttpsConnector<HttpConnector>, Empty<Bytes>>,
}

impl FetchClient {
    pub fn new(cfg: FetchConfig) -> Self {
        let inner = make_client();
        Self { cfg, inner }
    }

    pub async fn get(&self, url: &str) -> Result<Response> {
        self.request_follow(Method::GET, url, self.cfg.max_redirects)
            .await
    }

    async fn request_follow(
        &self,
        method: Method,
        url: &str,
        mut redirects_left: usize,
    ) -> Result<Response> {
        let mut current = Url::parse(url).context("invalid URL")?;

        loop {
            let uri: Uri = current.as_str().parse().context("uri parse")?;
            let req = Request::builder()
                .method(method.clone())
                .uri(uri)
                .header(
                    header::ACCEPT,
                    "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
                )
                .header(header::ACCEPT_ENCODING, "gzip, deflate")
                .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
                .header(header::USER_AGENT, DEFAULT_USER_AGENT)
                .body(Empty::<Bytes>::new())?;

            let resp = tokio::time::timeout(self.cfg.request_timeout, self.inner.request(req))
                .await
                .context("request timed out")??;

            self.enforce_header_limit(resp.headers())?;

            if is_redirect(resp.status()) {
                let loc = resp
                    .headers()
                    .get(header::LOCATION)
                    .cloned()
                    .ok_or_else(|| anyhow!("redirect without Location header"))?;
                if redirects_left == 0 {
                    bail!("too many redirects");
                }
                redirects_left -= 1;

                let new_url = resolve_location(&current, &loc)
                    .context("failed to resolve redirect Location")?;
                current = new_url;
                drop(resp);
                continue;
            }

            let status = resp.status();
            let headers = resp.headers().clone();

            let raw_body = hyper_body_stream(resp.into_body());
            let decoded = decode_body_stream(&headers, raw_body);
            let limited = limit_body_stream(decoded, self.cfg.max_body_bytes);

            let response = Response {
                status,
                headers,
                body: limited,
                limit: self.cfg.max_body_bytes,
            };
            return Ok(response);
        }
    }

    fn enforce_header_limit(&self, headers: &HeaderMap) -> Result<()> {
        let mut total = 0usize;
        for (k, v) in headers.iter() {
            total += k.as_str().len();
            total += v.as_bytes().len();
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
    let s = loc.to_str().context("Location not valid UTF-8")?;
    let url = base.join(s).or_else(|_| Url::parse(s))?;
    Ok(url)
}

fn hyper_body_stream(body: Incoming) -> ResponseBody {
    let s = http_body_util::BodyExt::into_data_stream(body).map_err(anyhow::Error::from);
    Box::pin(s)
}

fn decode_body_stream(headers: &HeaderMap, body: ResponseBody) -> ResponseBody {
    let enc = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    if enc.contains("gzip") {
        let reader = stream_reader(body);
        let dec = async_compression::tokio::bufread::GzipDecoder::new(reader);
        let s = ReaderStream::new(dec).map_err(|e| anyhow!(e));
        Box::pin(s)
    } else if enc.contains("deflate") {
        let reader = stream_reader(body);
        let dec = async_compression::tokio::bufread::ZlibDecoder::new(reader);
        let s = ReaderStream::new(dec).map_err(|e| anyhow!(e));
        Box::pin(s)
    } else {
        body
    }
}

fn stream_reader(s: ResponseBody) -> impl AsyncBufRead {
    let byte_stream = s.map_err(io::Error::other);
    let r = StreamReader::new(byte_stream);
    BufReader::new(r)
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
                std::task::Poll::Ready(Some(Ok(b))) => {
                    self.total += b.len();
                    if self.total > self.max {
                        return std::task::Poll::Ready(Some(Err(anyhow!(
                            "response body exceeds limit"
                        ))));
                    }
                    std::task::Poll::Ready(Some(Ok(b)))
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
        let c = FetchClient::new(cfg);
        let mut h = HeaderMap::new();
        h.insert("a", HeaderValue::from_static("b"));
        assert!(c.enforce_header_limit(&h).is_ok());
    }

    #[tokio::test]
    async fn decode_passthrough() {
        let s = Box::pin(stream::iter(vec![Ok(Bytes::from_static(b"hi"))])) as ResponseBody;
        let out = decode_body_stream(&HeaderMap::new(), s);
        let v = out
            .map_ok(|b| b.to_vec())
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        assert_eq!(v.concat(), b"hi");
    }

    #[ignore]
    #[tokio::test]
    async fn live_get_example() {
        init(Default::default());
        let r = get("https://example.com").await.unwrap();
        assert!(r.status().is_success());
        let text = r.get_text().await.unwrap();
        assert!(text.to_lowercase().contains("example"));
    }
}
