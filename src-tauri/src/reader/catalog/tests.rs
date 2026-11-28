use super::*;
use serde_json::{json, Value};

const DETAIL: &str = "https://flingtrainer.com/trainer/black-myth-wukong-trainer/";
const TICKET: &str = "https://flingtrainer.com/downloads/J0ZX9_hiI3QYHi9EG5A7vw,,";

fn query(mode: &str, query: &str, page: u32) -> Value {
    json!({"mode":mode,"query":query,"page":page})
}

#[test]
fn requests_only_call_public_posts_with_fixed_order_filters_and_encoded_search() {
    let make = |mode: &str, q: &str, page| serde_json::from_value::<ListInput>(query(mode, q, page)).unwrap().validate();
    for (mode, orderby, order) in [("latest", "modified", "desc"), ("all", "title", "asc"), ("search", "relevance", "desc")] {
        let url = make(mode, "  黑神话 &page=99#test  ", 2).unwrap().url();
        assert_eq!(url.host_str(), Some("flingtrainer.com"));
        assert_eq!(url.path(), API_PATH);
        assert!(url.fragment().is_none());
        let map: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(map["categories"], "5");
        assert_eq!(map["page"], "2");
        assert_eq!(map["per_page"], "24");
        assert_eq!(map["orderby"], orderby);
        assert_eq!(map["order"], order);
        assert_eq!(map.get("search").map(|value| value.as_ref()), (mode == "search").then_some("黑神话 &page=99#test"));
    }
    assert!(make("search", "  ", 1).is_err());
    assert!(make("search", "Game\nname", 1).is_err());
    assert!(make("search", &"测".repeat(171), 1).is_err());
    assert!(make("latest", "", 0).is_err());
    assert!(make("all", "", MAX_PAGE + 1).is_err());
}

#[test]
fn list_reads_actual_wordpress_shape_rejects_invalid_results_and_handles_missing_totals() {
    let valid = json!({
        "link": DETAIL,
        "title":{"rendered":"Black Myth &amp; Wukong <script>ignore me</script>"},
        "excerpt":{"rendered":"<p>One&nbsp;line.</p><p hidden>Nope</p><p>Another.</p>","protected":false},
        "date_gmt":"2024-08-20T02:16:38",
        "modified_gmt":"2025-10-17T10:39:23-05:00"
    });
    let mut unrelated = valid.clone();
    unrelated["link"] = json!("https://example.test/trainer/black-myth-wukong-trainer/");
    let input = serde_json::to_vec(&json!([valid.clone(), unrelated.clone(), valid])).unwrap();
    let page = parse_list(&input, 2, Some(7)).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].title, "Black Myth & Wukong");
    assert_eq!(page.items[0].summary, "One line. Another.");
    let wire = serde_json::to_value(page).unwrap();
    assert_eq!(wire["items"][0]["publishedAt"], "2024-08-20T02:16:38Z");
    assert_eq!(wire["items"][0]["updatedAt"], "2025-10-17T15:39:23Z");
    assert_eq!(wire["page"], 2);
    assert_eq!(wire["hasNextPage"], true);
    assert_eq!(wire["totalPages"], 7);
    assert!(parse_list(&input, 2, Some(1)).unwrap().total_pages.is_none());
    assert!(!parse_list(&input, 2, None).unwrap().has_next_page);
    assert!(!parse_list(b"[]", 1, Some(0)).unwrap().has_next_page);
    assert_eq!(parse_list(b"[]", 1, Some(0)).unwrap().total_pages, Some(0));
    assert!(parse_list(&serde_json::to_vec(&json!([unrelated])).unwrap(), 1, None).is_err());
    assert!(parse_list(br#"{"code":"error"}"#, 1, None).is_err());
    assert_eq!(date(Some("0000-00-00T00:00:00")), None);

    let full_page: Vec<_> = (0..PAGE_SIZE).map(|i| json!({
        "link":format!("https://flingtrainer.com/trainer/game-{i}/"), "title":{"rendered":"Trainer"}
    })).collect();
    let bytes = serde_json::to_vec(&full_page).unwrap();
    assert!(parse_list(&bytes, 1, None).unwrap().has_next_page);
    assert!(!parse_list(&bytes, 1, Some(1)).unwrap().has_next_page);
    assert!(parse_list(&serde_json::to_vec(&vec![full_page[0].clone(); PAGE_SIZE + 1]).unwrap(), 1, None).is_err());
}

#[test]
fn detail_selects_only_article_text_and_verified_attachment_table() {
    let doc = format!(r#"<!doctype html><html><head>
      <link rel="canonical" href="{DETAIL}">
      <meta property="article:published_time" content="2024-08-20T02:16:38+00:00">
      <meta property="article:modified_time" content="2025-10-17T15:39:23+00:00">
      <base href="https://external.test/">
      </head><body><article class="post type-post category-trainer">
      <h1 class="post-title">Black Myth: Wukong &amp; Co. &#8238;</h1>
      <div class="entry"><div class="fling-entity-placement">ad content<script src="https://external.test/ad.js"></script></div>
      44 Options · Game Version: v1.0+
      <p>First &ndash; line<script>untrusted scripts</script><br>Second line</p>
      <p aria-hidden="true">not public text</p>
      <iframe src="https://external.test/">media</iframe>
      <div class="download-attachments style-table"><table class="da-attachments-table"><tbody>
      <tr class="zip"><td class="attachment-title"><a class="attachment-link" href="/downloads/J0ZX9_hiI3QYHi9EG5A7vw,,">Official &amp; latest</a></td></tr>
      <tr><td><a href="{TICKET}">duplicate</a></td></tr>
      <tr><td><a href="/wp-content/uploads/2019/05/Trainer.zip">Older archive</a></td></tr>
      <tr><td><a href="https://external.test/other.zip">external</a></td></tr>
      <tr><td><a href="/wp-content/uploads/2025/11/picture.png">image</a></td></tr>
      <tr><td><a href="/downloads/XS-QygeD_jCoAS3SgNRuWA,,#fragment">not an attachment</a></td></tr>
      <tr><td><a href="javascript:ignored()">script link</a></td></tr>
      <tr><td><a hidden href="/downloads/2fa-LhXjYs0EYWpGztmtJw,,">hidden attachment</a></td></tr>
      </tbody></table></div>
      <a href="/downloads/XS-QygeD_jCoAS3SgNRuWA,,">unrelated inline link</a>
      </div></article><aside>sidebar</aside><div id="comments">comments</div></body></html>"#);
    let base = detail_address(DETAIL).unwrap();
    let detail = parse_detail(&doc, &base).unwrap();
    assert_eq!(detail.entry.title, "Black Myth: Wukong & Co.");
    assert_eq!(detail.entry.published_at.as_deref(), Some("2024-08-20T02:16:38Z"));
    assert_eq!(detail.entry.summary, "44 Options · Game Version: v1.0+");
    assert!(detail.content.contains("First – line\nSecond line"));
    for removed in ["Official", "ad content", "untrusted scripts", "not public text", "sidebar", "comments", "external.test"] {
        assert!(!detail.content.contains(removed), "{removed}");
    }
    assert_eq!(detail.attachments.len(), 2);
    assert_eq!(detail.attachments[0].label, "Official & latest");
    assert!(contains_attachment(&detail, &Url::parse(TICKET).unwrap()));
    let other = Url::parse("https://flingtrainer.com/downloads/XS-QygeD_jCoAS3SgNRuWA,,").unwrap();
    assert!(trainer::is_attachment(&other));
    assert!(!contains_attachment(&detail, &other));
    assert!(parse_detail(&doc, &detail_address("https://flingtrainer.com/trainer/different-trainer/").unwrap()).is_err());
    assert!(parse_detail("<h1>Challenge page</h1>", &base).is_err());
    let no_attachments = parse_detail("<article class='category-trainer'><h1 class='post-title'>Game</h1><div class='entry'><p>Official description</p></div></article>", &base).unwrap();
    assert!(no_attachments.attachments.is_empty());
}

#[test]
fn detail_only_allows_specific_official_paths_and_reveal_only_matches_literal_path() {
    assert_eq!(detail_address(DETAIL.trim_end_matches('/')).unwrap().as_str(), DETAIL);
    assert!(detail_address("https://flingtrainer.com/trainer/%e6%b5%8b%e8%af%95-trainer/").is_some());
    for url in [
        trainer::HOME, "https://www.flingtrainer.com/trainer/example/", "http://flingtrainer.com/trainer/example/",
        "https://flingtrainer.com:443/trainer/example/", "https://guest@flingtrainer.com/trainer/example/",
        "https://flingtrainer.com.evil.test/trainer/example/", "https://flingtrainer.com/trainer/%2e%2e/",
        "https://flingtrainer.com/trainer/name%2fother/", "https://flingtrainer.com/trainer/name?redirect=1",
        "https://flingtrainer.com/trainer/name#comments", "https://flingtrainer.com/trainer/name/foo/", TICKET,
    ] { assert!(detail_address(url).is_none(), "{url}"); }
    let saved = Path::new(r"C:\Games\Trainer.zip");
    assert_eq!(matching_saved_path(Some(saved), r"C:\Games\Trainer.zip"), Some(saved));
    assert_eq!(matching_saved_path(Some(saved), r"C:\Games\trainer.zip"), None);
    assert_eq!(matching_saved_path(Some(saved), r"C:\Games\.\Trainer.zip"), None);
    assert_eq!(matching_saved_path(None, r"C:\Games\Trainer.zip"), None);
    assert_eq!(serde_json::to_value(TrainerSaveResult::Cancelled).unwrap(), json!({"status":"cancelled"}));
}

#[tokio::test]
async fn network_limits_reject_wrong_media_partial_responses_and_large_chunks() {
    use axum::http::Response as HttpResponse;
    let make = |content_type, body| HttpResponse::builder().header(header::CONTENT_TYPE, content_type).body(body).unwrap().into();
    assert_eq!(response_body(make("application/json; charset=UTF-8", "[]"), "application/json", 2).await.unwrap(), b"[]");
    assert_eq!(response_body(make("text/html", "[]"), "application/json", 2).await.unwrap_err(), FORMAT_ERROR);
    assert_eq!(response_body(make("application/json", "[   ]"), "application/json", 2).await.unwrap_err(), TOO_LARGE);
    let mixed = HttpResponse::builder().header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_TYPE, "text/html").body("[]").unwrap().into();
    assert_eq!(response_body(mixed, "application/json", 2).await.unwrap_err(), FORMAT_ERROR);
    let encoded = HttpResponse::builder().header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_ENCODING, "gzip").body("[]").unwrap().into();
    assert_eq!(response_body(encoded, "application/json", 2).await.unwrap_err(), FORMAT_ERROR);
    let partial = HttpResponse::builder().header(header::CONTENT_TYPE, "text/html")
        .header(header::CONTENT_RANGE, "bytes 0-1/10").body("[]").unwrap().into();
    assert_eq!(response_body(partial, "text/html", 2).await.unwrap_err(), FORMAT_ERROR);
    let body = reqwest::Body::wrap_stream(futures_util::stream::iter([Ok::<&[u8], std::io::Error>(b"1234"), Ok(b"5678")]));
    let large = HttpResponse::builder().header(header::CONTENT_TYPE, "text/html").body(body).unwrap().into();
    assert_eq!(response_body(large, "text/html", 7).await.unwrap_err(), TOO_LARGE);
    let mut headers = HeaderMap::new();
    headers.append("x-wp-totalpages", "32".parse().unwrap());
    assert_eq!(pagination_header(&headers), Some(32));
    headers.append("x-wp-totalpages", "0".parse().unwrap());
    assert_eq!(pagination_header(&headers), None);
    assert!(status_error(StatusCode::TEMPORARY_REDIRECT).contains("跳转"));
    assert!(status_error(StatusCode::TOO_MANY_REQUESTS).contains("限制"));
}

#[tokio::test]
async fn force_refresh_bypasses_cache_and_failed_refresh_keeps_prior_results() {
    let proxy = ProxyState::new(crate::proxy::ProxySettings { mode: "direct".into(), ..Default::default() }, None).unwrap();
    let catalog = Catalog::default();
    let page_input = query("latest", "", 1);
    let key = serde_json::from_value::<ListInput>(page_input.clone()).unwrap().validate().unwrap();
    catalog.pages.put(key, parse_list(b"[]", 1, Some(0)).unwrap());
    let url = detail_address(DETAIL).unwrap();
    catalog.details.put(url.clone(), parse_detail("<article class='category-trainer'><h1 class='post-title'>Cached trainer</h1><div class='entry'>Cached description</div></article>", &url).unwrap());

    // Occupy all network slots so a refresh fails deterministically without leaving the test process.
    let _slots = catalog.reads.try_acquire_many(4).unwrap();
    let busy = "正在查询官网，请稍后重试。";
    assert_eq!(catalog.list(&proxy, json!({"mode":"latest","query":"","page":1,"forceRefresh":true})).await.unwrap_err(), busy);
    assert_eq!(catalog.detail(&proxy, json!({"url":DETAIL,"forceRefresh":true})).await.unwrap_err(), busy);
    assert!(catalog.list(&proxy, json!({"mode":"latest","query":"","page":1,"forceRefresh":"true"})).await.is_err());
    assert_eq!(catalog.list(&proxy, page_input).await.unwrap().total_pages, Some(0));
    assert_eq!(catalog.detail(&proxy, json!({"url":DETAIL,"forceRefresh":false})).await.unwrap().entry.title, "Cached trainer");
}

#[tokio::test]
#[ignore = "requires the public FLiNG site; only reads JSON and detail HTML"]
async fn live_catalog_reads_latest_all_search_and_official_attachments() {
    let proxy = ProxyState::new(crate::proxy::ProxySettings { mode: "direct".into(), ..Default::default() }, None).unwrap();
    let catalog = Catalog::default();
    let latest = catalog.list(&proxy, json!({"mode":"latest","query":"","page":1,"forceRefresh":true})).await.unwrap();
    assert!(!latest.items.is_empty());
    assert!(latest.items.windows(2).all(|pair| pair[0].updated_at >= pair[1].updated_at));
    let alphabetical = catalog.list(&proxy, query("all", "", 1)).await.unwrap();
    let total = alphabetical.total_pages.expect("WordPress total pages");
    assert!(total > 1 && alphabetical.has_next_page);
    let last = catalog.list(&proxy, query("all", "", total)).await.unwrap();
    assert!(!last.items.is_empty() && !last.has_next_page);
    let search = catalog.list(&proxy, query("search", "Black Myth", 1)).await.unwrap();
    let found = search.items.iter().find(|item| item.title.contains("Wukong")).expect("WordPress search result");
    let detail = catalog.detail(&proxy, json!({"url":found.url,"forceRefresh":true})).await.unwrap();
    assert!(!detail.content.is_empty());
    assert!(!detail.attachments.is_empty());
    let empty = catalog.list(&proxy, query("search", "ReleaseHubNoMatchingGame48869", 1)).await.unwrap();
    assert!(empty.items.is_empty() && !empty.has_next_page);
    let out_of_range = catalog.list(&proxy, query("all", "", total + 1)).await.unwrap_err();
    assert!(out_of_range.contains("超出官网目录范围"));
    println!("FLiNG: latest first = {}, A-Z pages = {total}, last page = {}, Black Myth attachments = {}", latest.items[0].title, last.items.len(), detail.attachments.len());
}

#[tokio::test]
#[ignore = "requires the public FLiNG site; reads only trainer HTML and attachment response headers"]
async fn live_handoff_resolves_listed_official_attachment_without_downloading() {
    let proxy = ProxyState::new(crate::proxy::ProxySettings { mode: "direct".into(), ..Default::default() }, None).unwrap();
    let catalog = Catalog::default();
    let detail = catalog.detail(&proxy, json!({"url":DETAIL})).await.unwrap();
    let row = detail.attachments.iter().find(|row| row.url.starts_with("https://flingtrainer.com/downloads/")).expect("official download ticket");
    let (entry, attachment) = catalog.verified_attachment(&proxy, DETAIL, &row.url).await.unwrap();
    assert_eq!(entry.url, DETAIL);
    let (url, name) = trainer::resolve_for_external(&proxy, attachment).await.unwrap();
    assert!(trainer::is_resolved_attachment(&url, &name));
    assert!(name.contains("Wukong"));
    let unlisted = "https://flingtrainer.com/wp-content/uploads/2025/07/releasehub-unlisted-fixture.zip";
    let error = catalog.verified_attachment(&proxy, DETAIL, unlisted).await.unwrap_err();
    assert!(error.contains("没有列出"));
    println!("FLiNG: official attachment HEAD verified ({name}); unlisted file refused");
}
