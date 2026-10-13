//! The Ollama library at ollama.com. There's no public API, so this reads the
//! search and tags pages and keeps only what it can find reliably.

use anyhow::Result;
use scraper::{ElementRef, Html, Selector};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub name: String,
    pub description: String,
    /// vision, tools, thinking, embedding, …
    pub capabilities: Vec<String>,
    /// Parameter sizes offered: 8b, 70b, …
    pub sizes: Vec<String>,
    pub pulls: String,
    pub updated: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tag {
    pub name: String,
    pub size: String,
    pub context: String,
}

fn text(e: ElementRef) -> String {
    e.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("valid selector")
}

pub fn parse_search(html: &str) -> Vec<Entry> {
    let doc = Html::parse_document(html);
    let (li, link, title, para, span) = (sel("li"), sel("a[href^='/library/']"), sel("h2"), sel("p"), sel("span"));
    let mut out = Vec::new();
    for item in doc.select(&li) {
        let Some(a) = item.select(&link).next() else { continue };
        let name = a.value().attr("href").unwrap_or("").trim_start_matches("/library/").to_string();
        if name.is_empty() || name.contains(':') {
            continue;
        }
        let title = item.select(&title).next().map(text).unwrap_or_else(|| name.clone());
        let description = item.select(&para).next().map(text).unwrap_or_default();
        let mut entry = Entry { name: title, description, ..Default::default() };
        for s in item.select(&span) {
            let class = s.value().attr("class").unwrap_or("");
            let t = text(s);
            if class.contains("rounded-md") {
                if class.contains("indigo") {
                    entry.capabilities.push(t);
                } else if !t.is_empty() {
                    entry.sizes.push(t);
                }
            }
        }
        // Stats: "2.9M Pulls", "12 Tags", "Updated 5 days ago".
        if let Some(stats) = item.select(&para).nth(1) {
            let groups: Vec<String> = stats.child_elements().map(text).collect();
            for g in groups {
                if let Some(p) = g.strip_suffix(" Pulls") {
                    entry.pulls = p.to_string();
                } else if let Some(u) = g.strip_prefix("Updated ") {
                    entry.updated = u.to_string();
                }
            }
        }
        out.push(entry);
    }
    out
}

pub fn parse_tags(html: &str, model: &str) -> Vec<Tag> {
    let doc = Html::parse_document(html);
    let prefix = format!("/library/{model}:");
    let link = sel("a");
    let mut out: Vec<Tag> = Vec::new();
    for a in doc.select(&link) {
        let href = a.value().attr("href").unwrap_or("");
        let Some(tag) = href.strip_prefix(&prefix) else { continue };
        if out.iter().any(|t| t.name == tag) {
            continue;
        }
        // "500a1f067a9f • 5.2GB • 40K context window • Text input • 1 year ago"
        let t = text(a);
        let parts: Vec<&str> = t.split('•').map(str::trim).collect();
        let size = parts.iter().find(|p| p.ends_with("GB") || p.ends_with("MB") || p.ends_with("TB")).unwrap_or(&"").to_string();
        let context = parts.iter().find(|p| p.contains("context")).map(|p| p.replace(" context window", "")).unwrap_or_default();
        if size.is_empty() {
            continue;
        }
        out.push(Tag { name: tag.to_string(), size, context });
    }
    out
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("nixe-chat/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

pub async fn search(query: &str) -> Result<Vec<Entry>> {
    let url = format!("https://ollama.com/search?q={}", crate::util::encode_query(query));
    let html = client().get(url).send().await?.error_for_status()?.text().await?;
    Ok(parse_search(&html))
}

pub async fn tags(model: &str) -> Result<Vec<Tag>> {
    let url = format!("https://ollama.com/library/{model}/tags");
    let html = client().get(url).send().await?.error_for_status()?.text().await?;
    Ok(parse_tags(&html, model))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = r#"<ul><li class="flex">
      <a href="/library/qwen3" class="group w-full">
        <div title="qwen3"><h2><span>qwen3</span></h2>
          <p class="max-w-lg">Qwen3 is the latest generation.</p></div>
        <div class="flex flex-col"><div class="flex flex-wrap">
          <span class="inline-flex rounded-md bg-indigo-50 text-indigo-600">tools</span>
          <span class="inline-flex rounded-md bg-indigo-50 text-indigo-600">thinking</span>
          <span class="inline-flex rounded-md bg-[#ddf4ff] text-blue-600">8b</span>
          <span class="inline-flex rounded-md bg-[#ddf4ff] text-blue-600">32b</span>
        </div>
        <p class="my-1 flex">
          <span class="flex items-center"><svg></svg><span>2.9M</span><span class="hidden sm:flex">&nbsp;Pulls</span></span>
          <span class="flex items-center"><span>12</span><span>&nbsp;Tags</span></span>
          <span class="flex items-center" title="Sep 25"><span>Updated&nbsp;</span><span>5 days ago</span></span>
        </p></div></a></li></ul>"#;

    #[test]
    fn parses_search_results() {
        let r = parse_search(SEARCH);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "qwen3");
        assert_eq!(r[0].capabilities, vec!["tools", "thinking"]);
        assert_eq!(r[0].sizes, vec!["8b", "32b"]);
        assert_eq!(r[0].pulls, "2.9M");
        assert_eq!(r[0].updated, "5 days ago");
    }

    #[test]
    fn parses_tags() {
        let html = r#"<a href="/library/qwen3:8b" class="md:hidden"><span>qwen3:8b</span>
            <span class="font-mono">500a1f067a9f</span> • 5.2GB • 40K context window • Text input • 1 year ago</a>
            <a href="/library/qwen3:8b" class="g">qwen3:8b</a>"#;
        let t = parse_tags(html, "qwen3");
        assert_eq!(t, vec![Tag { name: "8b".into(), size: "5.2GB".into(), context: "40K".into() }]);
    }
}

#[cfg(test)]
mod network_tests {
    #[test]
    #[ignore]
    fn searches_the_live_library() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let r = super::search("qwen").await.unwrap();
            assert!(!r.is_empty());
            let tags = super::tags(&r[0].name).await.unwrap();
            println!("{} {:?} {:?}", r[0].name, r[0].sizes, tags.iter().take(3).collect::<Vec<_>>());
            assert!(!tags.is_empty());
        });
    }
}
