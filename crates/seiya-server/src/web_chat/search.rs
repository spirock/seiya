//! Fixed-provider search; public HTTPS reader transport, no browser/curl fallback.
use crate::reader::{self, Reader};
use html5ever::tokenizer::{
    BufferQueue, CharacterTokens, EndTag, StartTag, TagToken, Token, TokenSink, TokenSinkResult,
    Tokenizer, TokenizerOpts,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, future::Future, pin::Pin, sync::Arc};
use tokio::time::Instant;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}
pub type SearchWork<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Hit>, reader::Error>> + Send + 'a>>;
pub trait Search: Send + Sync {
    fn search(&self, query: String, deadline: Instant) -> SearchWork<'_>;
    fn quarantined(&self) -> bool {
        false
    }
}
pub struct PublicSearch {
    reader: Arc<Reader>,
}
impl PublicSearch {
    pub fn new(reader: Arc<Reader>) -> Self {
        Self { reader }
    }
}
impl Search for PublicSearch {
    fn search(&self, query: String, deadline: Instant) -> SearchWork<'_> {
        Box::pin(async move {
            if query.trim().is_empty()
                || query.chars().count() > 300
                || query.chars().any(char::is_control)
            {
                return Err(reader::Error::Policy);
            }
            let mut url = reqwest::Url::parse("https://html.duckduckgo.com/html/").unwrap();
            url.query_pairs_mut().append_pair("q", query.trim());
            let report = self
                .reader
                .fetch_report_with_redirect_limit(url.as_str(), deadline, 0)
                .await?;
            let page = report.result?;
            if page.mime != "text/html" {
                return Err(reader::Error::Format);
            }
            parse(&page.body)
        })
    }
    fn quarantined(&self) -> bool {
        self.reader.quarantined()
    }
}
#[derive(Default)]
struct State {
    hits: Vec<Hit>,
    active: Option<(String, usize)>,
    depth: usize,
    tokens: usize,
    failed: bool,
    hidden: usize,
    last_valid: bool,
}
#[derive(Default)]
struct Sink(RefCell<State>);
fn normalized_url(raw: &str) -> Option<String> {
    let input = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.to_owned()
    };
    let parsed = reqwest::Url::parse(&input).ok()?;
    let target = if matches!(
        parsed.host_str(),
        Some("duckduckgo.com" | "html.duckduckgo.com")
    ) {
        let items: Vec<_> = parsed
            .query_pairs()
            .filter(|(k, _)| k == "uddg")
            .map(|(_, v)| v.into_owned())
            .collect();
        if items.len() != 1 {
            return None;
        }
        items[0].clone()
    } else {
        input
    };
    reader::policy::url(&target).ok().map(|v| v.to_string())
}
impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut s = self.0.borrow_mut();
        s.tokens += 1;
        if s.tokens > 100000 {
            s.failed = true;
        }
        if s.failed {
            return TokenSinkResult::Continue;
        }
        match token {
            TagToken(t) => {
                let name = t.name.to_string();
                if t.kind == StartTag {
                    if t.attrs.len() > 256 || s.depth >= 256 {
                        s.failed = true;
                        return TokenSinkResult::Continue;
                    }
                    s.depth += 1;
                    if matches!(name.as_str(), "script" | "style" | "template") {
                        s.hidden += 1;
                    }
                    let class = t
                        .attrs
                        .iter()
                        .find(|a| a.name.local.as_ref() == "class")
                        .map(|a| a.value.to_string())
                        .unwrap_or_default();
                    if s.hidden == 0
                        && name == "a"
                        && class.split_whitespace().any(|c| c == "result__a")
                    {
                        s.active = None;
                        s.last_valid = false;
                        if s.hits.len() < 5 {
                            let href = t
                                .attrs
                                .iter()
                                .find(|a| a.name.local.as_ref() == "href")
                                .map(|a| a.value.to_string())
                                .unwrap_or_default();
                            if let Some(url) = normalized_url(&href) {
                                s.hits.push(Hit {
                                    title: String::new(),
                                    url,
                                    snippet: String::new(),
                                });
                                s.last_valid = true;
                                s.active = Some(("title".into(), s.depth));
                            }
                        }
                    } else if s.hidden == 0
                        && s.last_valid
                        && class.split_whitespace().any(|c| c == "result__snippet")
                        && !s.hits.is_empty()
                    {
                        s.active = Some(("snippet".into(), s.depth));
                    }
                    if t.self_closing
                        || matches!(
                            name.as_str(),
                            "br" | "img"
                                | "input"
                                | "hr"
                                | "meta"
                                | "link"
                                | "wbr"
                                | "area"
                                | "base"
                                | "source"
                        )
                    {
                        s.depth = s.depth.saturating_sub(1);
                    }
                } else if t.kind == EndTag {
                    if s.active.as_ref().is_some_and(|(_, d)| *d >= s.depth) {
                        s.active = None;
                    }
                    s.depth = s.depth.saturating_sub(1);
                    if matches!(name.as_str(), "script" | "style" | "template") {
                        s.hidden = s.hidden.saturating_sub(1);
                    }
                }
            }
            CharacterTokens(text) if s.hidden == 0 => {
                if let Some((field, _)) = &s.active {
                    let field = field.clone();
                    if let Some(hit) = s.hits.last_mut() {
                        let target = if field == "title" {
                            &mut hit.title
                        } else {
                            &mut hit.snippet
                        };
                        let left = 1200usize.saturating_sub(target.chars().count());
                        target.extend(
                            text.chars()
                                .take(left)
                                .filter(|c| !c.is_control() || c.is_whitespace()),
                        );
                    }
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}
pub fn parse(bytes: &[u8]) -> Result<Vec<Hit>, reader::Error> {
    if bytes.len() > 1024 * 1024 {
        return Err(reader::Error::Limit);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| reader::Error::Format)?;
    let sink = Sink::default();
    let input = BufferQueue::default();
    input.push_back(text.into());
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    if state.failed {
        return Err(reader::Error::Limit);
    }
    let mut seen = std::collections::HashSet::new();
    let hits = state
        .hits
        .into_iter()
        .filter_map(|mut hit| {
            hit.title = hit.title.split_whitespace().collect::<Vec<_>>().join(" ");
            hit.snippet = hit.snippet.split_whitespace().collect::<Vec<_>>().join(" ");
            (!hit.title.is_empty() && seen.insert(hit.url.clone())).then_some(hit)
        })
        .collect::<Vec<_>>();
    if hits.is_empty() {
        Err(reader::Error::NoText)
    } else {
        Ok(hits)
    }
}
