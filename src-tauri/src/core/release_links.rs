//! Resolve a clicked public GitHub attachment using the release that owns it.
use super::{models::DownloadRecord, network, Core};
use serde_json::{json, Value};
use std::{collections::HashSet, sync::Arc};
use url::Url;

#[derive(Debug, PartialEq, Eq)]
struct ReleaseLink {
    owner: String,
    repo: String,
    tag: Option<String>, // None represents GitHub's /releases/latest/download/<file> alias.
    file: String,
}

pub(crate) fn credential_free_https(url: &Url, host: &str) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some(host)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && !url[..url::Position::BeforePath].contains('@')
}

pub(crate) fn decode_path_segment(part: &str) -> Option<String> {
    fn hex(value: u8) -> Option<u8> { (value as char).to_digit(16).map(|digit| digit as u8) }
    let mut bytes = part.bytes();
    let mut decoded = Vec::new();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' { hex(bytes.next()?)? * 16 + hex(bytes.next()?)? } else { byte });
    }
    let decoded = String::from_utf8(decoded).ok()?;
    (!decoded.is_empty() && !decoded.contains('\\') && !decoded.chars().any(char::is_control)).then_some(decoded)
}

impl ReleaseLink {
    fn from_url(url: &Url) -> Option<Self> {
        if !credential_free_https(url, "github.com") || url.query().is_some() || url.fragment().is_some() || url.as_str().len() > 4096 {
            return None;
        }
        let parts: Vec<_> = url.path_segments()?.collect();
        let (owner, repo, location, file) = match parts.as_slice() {
            [owner, repo, "releases", location @ .., file] => (owner, repo, location, file),
            _ => return None,
        };
        let (parsed_owner, parsed_repo) = network::repository(&format!("{owner}/{repo}")).ok()?;
        if parsed_owner != *owner || parsed_repo != *repo { return None; }
        let tag = match location {
            ["latest", "download"] => None,
            ["download", parts @ ..] if !parts.is_empty() => {
                let tag = parts.iter().map(|part| decode_path_segment(part)).collect::<Option<Vec<_>>>()?.join("/");
                if tag.len() > 200 || tag.split('/').any(|part| matches!(part, "" | "." | "..")) { return None; }
                Some(tag)
            }
            _ => return None,
        };
        let file = decode_path_segment(file)?;
        if file.contains('/') || matches!(file.as_str(), "." | "..") || file.len() > 512 { return None; }
        Some(Self { owner: parsed_owner, repo: parsed_repo, tag, file })
    }

    fn from_asset(asset: &Value) -> Option<Self> {
        let address = asset["browser_download_url"].as_str()?;
        if address.contains('\\') || address.chars().any(char::is_control) { return None; }
        Self::from_url(&Url::parse(address).ok()?)
    }

    fn names_release(&self, owner: &str, repo: &str, tag: &str) -> bool {
        self.owner.eq_ignore_ascii_case(owner) && self.repo.eq_ignore_ascii_case(repo)
            && self.tag.as_deref() == Some(tag)
    }

    fn matching_asset<'a>(&self, release: &'a Value) -> Result<(&'a str, &'a Value), String> {
        let tag = published_tag(release)?;
        if self.tag.as_deref().is_some_and(|clicked| clicked != tag) {
            return Err("未找到链接对应的 GitHub 发行版。".into());
        }
        let asset = release["assets"].as_array().and_then(|assets| assets.iter().find(|asset| {
            Self::from_asset(asset).is_some_and(|candidate| candidate.names_release(&self.owner, &self.repo, tag) && candidate.file == self.file)
        })).ok_or("这个附件不在对应的 GitHub 发行版中，可能已被移除。")?;
        Ok((tag, asset))
    }
}

fn published_tag(release: &Value) -> Result<&str, String> {
    if release["draft"].as_bool() != Some(false) || release["published_at"].as_str().is_none() {
        return Err("该发行版尚未公开发布。".into());
    }
    release["tag_name"].as_str().filter(|tag| !tag.is_empty() && tag.len() <= 200 && !tag.chars().any(char::is_control)
        && tag.split('/').all(|part| !matches!(part, "" | "." | ".."))).ok_or_else(|| "GitHub 未提供有效的发行版标签。".into())
}

fn asset_id(asset: &Value) -> Option<u64> {
    asset["id"].as_u64().filter(|id| (1..=9_007_199_254_740_991).contains(id))
}

fn release_listing(release: &Value, owner: &str, repo: &str) -> Result<Value, String> {
    let tag = published_tag(release)?;
    let mut url = Url::parse("https://github.com/").expect("static GitHub origin");
    url.path_segments_mut().expect("hierarchical GitHub origin").pop_if_empty().extend([owner, repo, "releases", "tag", tag]);
    let mut seen = HashSet::new();
    let assets: Vec<_> = release["assets"].as_array().ok_or("GitHub 附件列表格式无效。")?.iter().filter_map(|asset| {
        if !ReleaseLink::from_asset(asset)?.names_release(owner, repo, tag) { return None; }
        let id = asset_id(asset)?;
        let size = asset["size"].as_u64().filter(|size| (1..=network::MAX_DOWNLOAD_BYTES).contains(size))?;
        let name = network::field(asset, "name", 512).ok()?;
        let url = network::field(asset, "browser_download_url", 4096).ok()?;
        if !seen.insert(id) { return None; }
        Some(json!({"id": id, "name": name, "url": url, "size": size}))
    }).collect();
    Ok(json!({"tag": tag, "url": url.as_str(), "assets": assets}))
}

pub(crate) fn is_release_link(url: &Url) -> bool { ReleaseLink::from_url(url).is_some() }

impl Core {
    async fn require_public_release_repository(&self, owner: &str, repo: &str) -> Result<(), String> {
        // A connected account may read private releases; neither attachment action permits them.
        let repository = self.github.api(owner, repo, &[], None).await?;
        if repository["private"].as_bool() != Some(false) { return Err("这里只能下载公开仓库的发行附件。".into()); }
        Ok(())
    }

    pub(crate) async fn public_release_assets(&self, repository: &str) -> Result<Value, String> {
        let (owner, repo) = network::repository(repository)?;
        self.require_public_release_repository(&owner, &repo).await?;
        match self.github.api_optional(&owner, &repo, &["releases", "latest"], None).await? {
            Some(release) => release_listing(&release, &owner, &repo),
            None => Ok(Value::Null),
        }
    }

    pub(crate) async fn queue_release_link(self: &Arc<Self>, address: &str) -> Result<DownloadRecord, String> {
        let bad_link = "这里只能下载 GitHub 公开发行版的附件。";
        if address.contains('\\') || address.chars().any(char::is_control)
            || address.split_once("://").is_none_or(|(_, tail)| tail.split('/').next().is_some_and(|host| host.contains('@'))) {
            return Err(bad_link.into());
        }
        let url = Url::parse(address).map_err(|_| bad_link.to_owned())?;
        let link = ReleaseLink::from_url(&url).ok_or(bad_link)?;
        self.require_public_release_repository(&link.owner, &link.repo).await?;
        let release = if let Some(tag) = link.tag.as_deref() {
            self.github.api(&link.owner, &link.repo, &["releases", "tags", tag], None).await?
        } else {
            self.github.api(&link.owner, &link.repo, &["releases", "latest"], None).await?
        };
        let (tag, asset) = link.matching_asset(&release)?;
        let id = asset_id(asset).ok_or("GitHub 附件编号无效。")?;
        let size = asset["size"].as_u64().ok_or("GitHub 附件大小无效。")?;
        let name = network::field(asset, "name", 512)?;
        let url = network::field(asset, "browser_download_url", 4096)?;
        let digest = asset["digest"].as_str();
        let repository = format!("{}/{}", link.owner, link.repo);
        self.enqueue_release_file(&repository, tag, id, &name, &url, size, digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(s: &str) -> Option<ReleaseLink> { ReleaseLink::from_url(&Url::parse(s).unwrap()) }

    #[test]
    fn release_click_only_accepts_github_download_routes_and_decodes_tags() {
        let link = parse("https://github.com/Owner/tool/releases/download/release%2Fv1/my%20file.zip").unwrap();
        assert_eq!(link.tag.as_deref(), Some("release/v1"));
        assert_eq!(link.file, "my file.zip");
        assert_eq!(parse("https://github.com/Owner/tool/releases/download/release/v1/my%20file.zip"), Some(link));
        assert!(parse("https://github.com/owner/tool/releases/latest/download/app.exe").unwrap().tag.is_none());
        for bad in ["https://github.com.evil.test/owner/tool/releases/download/v1/a.zip", "https://user@github.com/owner/tool/releases/download/v1/a.zip",
            "https://github.com:444/owner/tool/releases/download/v1/a.zip", "https://github.com/owner/tool/releases/download/v1/a.zip?next=any",
            "https://github.com/owner/tool/releases/download/v1/a%2Fz.zip", "https://github.com/owner/tool/releases/download/v1/a%5Cz.zip",
            "https://github.com/owner/tool/releases/download/v1/%ff.zip", "https://github.com/owner/tool/releases/tag/v1", "https://github.com/owner/tool/archive/v1.zip",
            "https://release-assets.githubusercontent.com/github-production-release-asset/signed"] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn old_version_and_latest_alias_match_only_canonical_release_assets() {
        let mut release = json!({"draft":false,"tag_name":"v1","published_at":"2024-01-01T00:00:00Z", "assets":[
            {"id":1,"browser_download_url":"https://github.com/other/tool/releases/download/v1/file.zip"},
            {"id":2,"browser_download_url":"https://github.com/owner/tool/releases/download/v2/file.zip"},
            {"id":3,"browser_download_url":"https://github.com/owner/tool/releases/download/v1/file.zip"}
        ]});
        for path in ["download/v1/file.zip", "latest/download/file.zip"] {
            let link = parse(&format!("https://github.com/OWNER/tool/releases/{path}")).unwrap();
            assert_eq!(link.matching_asset(&release).unwrap().1["id"], 3);
        }
        let link = parse("https://github.com/owner/tool/releases/download/v2/file.zip").unwrap();
        assert!(link.matching_asset(&release).is_err());
        release["draft"] = json!(true);
        assert!(parse("https://github.com/owner/tool/releases/download/v1/file.zip").unwrap().matching_asset(&release).is_err());
    }

    #[test]
    fn latest_release_listing_exposes_only_matching_downloadable_assets() {
        let mut release = json!({"draft":false,"tag_name":"stable/v3","published_at":"2026-01-01T00:00:00Z", "assets":[
            {"id":4,"name":"200 MB.zip","size":209715200,"browser_download_url":"https://github.com/Owner/Tool/releases/download/stable%2Fv3/200%20MB.zip"},
            {"id":4,"name":"duplicate.zip","size":1,"browser_download_url":"https://github.com/owner/tool/releases/download/stable%2Fv3/duplicate.zip"},
            {"id":5,"name":"foreign.zip","size":1,"browser_download_url":"https://github.com/other/tool/releases/download/stable%2Fv3/foreign.zip"},
            {"id":6,"name":"stale.zip","size":1,"browser_download_url":"https://github.com/owner/tool/releases/download/v2/stale.zip"},
            {"id":7,"name":"empty.zip","size":0,"browser_download_url":"https://github.com/owner/tool/releases/download/stable%2Fv3/empty.zip"}
        ]});
        let listing = release_listing(&release, "owner", "tool").unwrap();
        assert_eq!(listing["tag"], "stable/v3");
        assert_eq!(listing["url"], "https://github.com/owner/tool/releases/tag/stable%2Fv3");
        assert_eq!(listing["assets"], json!([{"id":4,"name":"200 MB.zip","size":209715200,"url":"https://github.com/Owner/Tool/releases/download/stable%2Fv3/200%20MB.zip"}]));
        release["assets"] = json!([]);
        assert_eq!(release_listing(&release, "owner", "tool").unwrap()["assets"], json!([]));
        release["draft"] = json!(true);
        assert!(release_listing(&release, "owner", "tool").is_err());
    }
}
