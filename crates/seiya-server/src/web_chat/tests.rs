use super::*;
mod flow_cases;

#[test]
fn search_parser_unwraps_only_provider_links_and_filters_unsafe_results() {
    let html=br#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Farticle">An &amp; <b>article</b></a><p class="result__snippet">Public snippet.</p><a class="result__a" href="https://127.0.0.1/private">bad</a><p class="result__snippet">Must not contaminate previous result</p><script><a class="result__a" href="https://example.org/script">ignored</a></script>"#;
    let found = search::parse(html).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].url, "https://example.org/article");
    assert_eq!(found[0].title, "An & article");
    assert_eq!(found[0].snippet, "Public snippet.");
    assert!(search::parse(b"<html>Anti-bot challenge</html>").is_err());
    assert!(search::parse(&vec![b'x'; 1024 * 1024 + 1]).is_err());
}
#[test]
fn run_registry_is_bounded_tokens_isolated_and_preupload_cancel_is_terminal() {
    struct Search;
    impl search::Search for Search {
        fn search(&self, _: String, _: Instant) -> search::SearchWork<'_> {
            panic!("no network during control operations")
        }
    }
    struct Net;
    impl crate::reader::transport::Network for Net {
        fn resolve(
            &self,
            _: String,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<Vec<std::net::IpAddr>, crate::reader::Error>,
                    > + Send
                    + '_,
            >,
        > {
            panic!("no DNS")
        }
        fn connect(
            &self,
            _: String,
            _: std::net::IpAddr,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            Box<dyn crate::reader::transport::Stream>,
                            crate::reader::Error,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            panic!("no sockets")
        }
    }
    let web = WebChat::with_io(
        Arc::new(Search),
        Arc::new(Reader::new(Arc::new(Net))),
        32768,
        256,
    )
    .unwrap();
    let first = web.create().unwrap();
    let mut forged = first.clone();
    forged.token = "0".repeat(64);
    assert!(web.status(&forged).is_err());
    assert_eq!(web.cancel(&first).unwrap()["phase"], "cancelled");
    assert_eq!(web.status(&first).unwrap()["consumed"], true);
    for _ in 1..128 {
        web.create().unwrap();
    }
    assert!(web.create().is_err());
}
