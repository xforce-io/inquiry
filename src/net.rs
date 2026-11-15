//! Public-page acquisition. DNS answers are checked before every redirect and
//! curl connects to that exact IP, including through a configured proxy.
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    process::Stdio,
    time::Duration,
};
use taskloop::ToolContext;
use tokio::{io::AsyncReadExt, process::Command};
use url::Url;

type HttpResponse = (u16, BTreeMap<String, String>, Vec<u8>);

pub(crate) const BODY_LIMIT: usize = 1024 * 1024;
const TOTAL_LIMIT: usize = BODY_LIMIT + 64 * 1024;
#[derive(Debug, Clone)]
pub struct Page {
    pub requested_url: String,
    pub final_url: String,
    pub status: u16,
    pub content_type: String,
    pub retry_after: Option<String>,
    pub body: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct FetchError {
    pub denied: bool,
    pub reason: String,
}
fn denied(reason: &str) -> FetchError {
    FetchError {
        denied: true,
        reason: reason.into(),
    }
}
fn unavailable(reason: impl Into<String>) -> FetchError {
    FetchError {
        denied: false,
        reason: reason.into(),
    }
}

pub fn normalize_url(raw: &str) -> Result<Url, FetchError> {
    if raw.len() > 4096 || raw.trim() != raw || raw.chars().any(char::is_control) {
        return Err(denied("网址格式不允许"));
    }
    let mut u = Url::parse(raw).map_err(|_| denied("网址格式不允许"))?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || !matches!(u.port_or_known_default(), Some(80 | 443))
    {
        return Err(denied("仅支持无凭据的公开 HTTP(S) 网页，端口为80或443"));
    }
    u.set_fragment(None);
    Ok(u)
}
/// Conservative public-address policy, based on the IANA special-purpose registries.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let value = u32::from(ip);
            for (base, bits) in [
                ([0, 0, 0, 0], 8),
                ([10, 0, 0, 0], 8),
                ([100, 64, 0, 0], 10),
                ([127, 0, 0, 0], 8),
                ([169, 254, 0, 0], 16),
                ([172, 16, 0, 0], 12),
                ([192, 0, 0, 0], 24),
                ([192, 0, 2, 0], 24),
                ([192, 88, 99, 0], 24),
                ([192, 168, 0, 0], 16),
                ([198, 18, 0, 0], 15),
                ([198, 51, 100, 0], 24),
                ([203, 0, 113, 0], 24),
                ([224, 0, 0, 0], 4),
                ([240, 0, 0, 0], 4),
            ] {
                let mask = u32::MAX << (32 - bits);
                if value & mask == u32::from(Ipv4Addr::from(base)) & mask {
                    return false;
                }
            }
            true
        }
        IpAddr::V6(ip) => {
            if ip.segments()[0] & 0xe000 != 0x2000 {
                return false;
            }
            let value = u128::from(ip);
            for (base, bits) in [
                ("2001::", 23),
                ("2001:db8::", 32),
                ("2002::", 16),
                ("3fff::", 20),
            ] {
                let mask = u128::MAX << (128 - bits);
                if value & mask
                    == u128::from(base.parse::<Ipv6Addr>().expect("constant IPv6 prefix")) & mask
                {
                    return false;
                }
            }
            true
        }
    }
}
pub async fn checked_addresses(url: &Url) -> Result<Vec<IpAddr>, FetchError> {
    let host = url
        .host_str()
        .ok_or_else(|| denied("缺少主机名"))?
        .trim_matches(['[', ']']);
    let mut ips = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![ip]
    } else if dns_mode()? == "cloudflare-doh" {
        // Host explicitly chooses the resolver. Never fall back after denied answers.
        let (a, aaaa) = tokio::try_join!(doh(host, 1), doh(host, 28))?;
        a.into_iter().chain(aaaa).collect()
    } else {
        let port = url
            .port_or_known_default()
            .ok_or_else(|| denied("端口不允许"))?;
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| unavailable("DNS查询超时"))?
        .map_err(|_| unavailable("DNS查询失败"))?
        .map(|a| a.ip())
        .collect()
    };
    ips.sort();
    ips.dedup();
    if ips.is_empty() {
        return Err(unavailable("域名没有可用地址"));
    }
    if ips.iter().any(|ip| !public_ip(*ip)) {
        return Err(denied("目标包含非公开网络地址，未发起获取"));
    }
    Ok(ips)
}
/// Explicit host network setting; default stays on the system resolver.
pub fn dns_mode() -> Result<String, FetchError> {
    match std::env::var("INQUIRY_DNS_MODE") {
        Err(std::env::VarError::NotPresent) => Ok("system".into()),
        Ok(mode) if matches!(mode.as_str(), "system" | "cloudflare-doh") => Ok(mode),
        _ => Err(denied("INQUIRY_DNS_MODE只接受system或cloudflare-doh")),
    }
}
fn doh_answers(bytes: &[u8], host: &str, kind: u64) -> Result<Vec<IpAddr>, FetchError> {
    let value = taskloop::parse_unique(bytes).map_err(|_| unavailable("DoH响应不是有效JSON"))?;
    if value["Status"] != 0
        || value["TC"] != false
        || value["Question"][0]["type"] != kind
        || value["Question"][0]["name"]
            .as_str()
            .is_none_or(|s| !s.trim_end_matches('.').eq_ignore_ascii_case(host))
    {
        return Err(unavailable("DoH响应状态、完整性或查询身份不匹配"));
    }
    let Some(records) = value.get("Answer") else {
        return Ok(vec![]);
    };
    let records = records
        .as_array()
        .ok_or_else(|| unavailable("DoH答案不是数组"))?;
    let mut out = Vec::new();
    for record in records {
        if let Some(record_kind @ (1 | 28)) = record["type"].as_u64() {
            let ip = record["data"]
                .as_str()
                .and_then(|s| s.parse::<IpAddr>().ok())
                .ok_or_else(|| unavailable("DoH地址格式无效"))?;
            if (record_kind == 1) != ip.is_ipv4() {
                return Err(unavailable("DoH地址类型不匹配"));
            }
            out.push(ip);
        }
    }
    Ok(out)
}
async fn doh(host: &str, kind: u64) -> Result<Vec<IpAddr>, FetchError> {
    let mut url = Url::parse("https://cloudflare-dns.com/dns-query").expect("constant DoH URL");
    url.query_pairs_mut()
        .append_pair("name", host)
        .append_pair("type", &kind.to_string());
    let mut cmd = Command::new("curl");
    // Pin the fixed resolver bootstrap, retain TLS hostname verification, no redirects.
    cmd.args([
        "--disable",
        "--silent",
        "--show-error",
        "--fail",
        "--max-time",
        "5",
        "--connect-timeout",
        "3",
        "--proto",
        "=https",
        "--proxytunnel",
        "--connect-to",
        "cloudflare-dns.com:443:1.1.1.1:443",
        "--header",
        "accept: application/dns-json",
        url.as_str(),
    ]);
    if let Some(proxy) = system_proxy() {
        cmd.env("HTTPS_PROXY", &proxy).env("HTTP_PROXY", proxy);
    }
    cmd.kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|_| unavailable("DoH进程无法启动"))?;
    let mut bytes = Vec::new();
    let stdout = child.stdout.take().expect("piped DoH stdout");
    let status = tokio::time::timeout(Duration::from_secs(6), async {
        stdout.take(65_537).read_to_end(&mut bytes).await?;
        child.wait().await
    })
    .await
    .map_err(|_| unavailable("DoH查询超时"))?
    .map_err(|_| unavailable("DoH查询无法完成"))?;
    if !status.success() || bytes.len() > 65_536 {
        return Err(unavailable("DoH查询失败或响应超过边界"));
    }
    doh_answers(&bytes, host, kind)
}
fn system_proxy() -> Option<String> {
    if ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .any(|k| std::env::var_os(k).is_some())
    {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("scutil")
            .arg("--proxy")
            .output()
            .ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        let values: BTreeMap<_, _> = text
            .lines()
            .filter_map(|line| line.trim().split_once(" : "))
            .collect();
        if values.get("HTTPSEnable") == Some(&"1") {
            return Some(format!(
                "http://{}:{}",
                values.get("HTTPSProxy")?,
                values.get("HTTPSPort")?
            ));
        }
    }
    None
}
fn connect_rule(url: &Url, ip: IpAddr) -> String {
    let host = url.host_str().expect("validated host");
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let target = match ip {
        IpAddr::V4(v) => v.to_string(),
        IpAddr::V6(v) => format!("[{v}]"),
    };
    let port = url.port_or_known_default().expect("validated port");
    format!("{host}:{port}:{target}:{port}")
}
async fn response(
    url: &Url,
    ip: IpAddr,
    c: &ToolContext,
    deadline: tokio::time::Instant,
) -> Result<HttpResponse, FetchError> {
    let read_budget = Duration::from_secs(20)
        .min(deadline.saturating_duration_since(tokio::time::Instant::now()));
    if read_budget.is_zero() {
        return Err(unavailable("获取总时长达到边界"));
    }
    let mut cmd = Command::new("curl");
    // --disable must be first: never inherit .curlrc, netrc or user headers.
    cmd.args([
        "--disable",
        "--silent",
        "--show-error",
        "--include",
        "--suppress-connect-headers",
        "--proxytunnel",
        "--connect-timeout",
        "6",
        "--max-time",
        "18",
        "--proto",
        "=http,https",
        "--header",
        "Accept-Encoding: identity",
        "--user-agent",
        "Inquiry/0.1 public-page-research",
        "--connect-to",
        &connect_rule(url, ip),
        url.as_str(),
    ]);
    if let Some(proxy) = system_proxy() {
        cmd.env("HTTPS_PROXY", &proxy).env("HTTP_PROXY", proxy);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|_| unavailable("无法启动公开网页获取器"))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut bytes = Vec::new();
    let result = {
        let mut reader = stdout.take((TOTAL_LIMIT + 1) as u64);
        tokio::select! {
            result=tokio::time::timeout(read_budget,reader.read_to_end(&mut bytes))=>match result {Ok(Ok(_))=>Ok(()),_=>Err(unavailable("网页获取超时或读取失败"))},
            _=async{while !c.is_cancelled(){tokio::time::sleep(Duration::from_millis(20)).await;}}=>Err(unavailable("获取已停止")),
        }
    };
    if result.is_err() || bytes.len() > TOTAL_LIMIT {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return result.and(Err(unavailable("响应超过大小边界")));
    }
    let status = match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
        Ok(Ok(status)) => status,
        _ => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(unavailable("获取器未及时结束"));
        }
    };
    if !status.success() {
        return Err(unavailable(format!(
            "网络或TLS获取失败（{}）",
            status.code().unwrap_or(-1)
        )));
    }
    parse_response(&bytes)
}
fn parse_response(bytes: &[u8]) -> Result<HttpResponse, FetchError> {
    let mut start = 0;
    loop {
        let end = bytes[start..]
            .windows(4)
            .position(|p| p == b"\r\n\r\n")
            .map(|n| start + n)
            .ok_or_else(|| unavailable("HTTP响应头不完整"))?;
        if end - start > 64 * 1024 {
            return Err(unavailable("HTTP响应头超限"));
        }
        let header = std::str::from_utf8(&bytes[start..end])
            .map_err(|_| unavailable("HTTP响应头编码异常"))?;
        let mut lines = header.split("\r\n");
        let first = lines.next().unwrap_or("");
        if !first.starts_with("HTTP/") {
            return Err(unavailable("HTTP响应协议异常"));
        }
        let status: u16 = first
            .split_whitespace()
            .nth(1)
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| unavailable("HTTP状态异常"))?;
        start = end + 4;
        if (100..200).contains(&status) {
            continue;
        }
        let mut headers = BTreeMap::new();
        for line in lines {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.to_ascii_lowercase();
                if matches!(key.as_str(), "location" | "content-encoding")
                    && headers.contains_key(&key)
                {
                    return Err(unavailable("HTTP响应头重复，无法安全解释"));
                }
                headers.insert(key, value.trim().to_string());
            }
        }
        let body = bytes[start..].to_vec();
        if body.len() > BODY_LIMIT {
            return Err(unavailable("网页正文超过1MiB，未采纳为完整快照"));
        }
        if headers
            .get("content-encoding")
            .is_some_and(|s| s != "identity")
        {
            return Err(unavailable("来源未提供支持的正文编码"));
        }
        return Ok((status, headers, body));
    }
}
pub async fn fetch(raw: &str, c: &ToolContext) -> Result<Page, FetchError> {
    let initial = normalize_url(raw)?;
    let mut url = initial.clone();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(32);
    for redirect in 0..=5 {
        if c.is_cancelled() {
            return Err(unavailable("获取已停止"));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(unavailable("获取总时长达到边界"));
        }
        let ips = checked_addresses(&url).await?;
        let (status, headers, body) = response(&url, ips[0], c, deadline).await?;
        if (300..400).contains(&status) {
            if redirect == 5 {
                return Err(unavailable("重定向超过5次"));
            }
            let next = url
                .join(
                    headers
                        .get("location")
                        .ok_or_else(|| unavailable("重定向缺少目标"))?,
                )
                .map_err(|_| denied("重定向地址无效"))?;
            let next = normalize_url(next.as_str())?;
            if url.scheme() == "https" && next.scheme() != "https" {
                return Err(denied("不跟随HTTPS降级重定向"));
            }
            url = next;
            continue;
        }
        return Ok(Page {
            requested_url: initial.to_string(),
            final_url: url.to_string(),
            status,
            content_type: headers.get("content-type").cloned().unwrap_or_default(),
            retry_after: headers.get("retry-after").cloned(),
            body,
        });
    }
    Err(unavailable("重定向未收敛"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doh_response_identity_errors_and_mixed_addresses_cannot_be_hidden() {
        use serde_json::json;
        let base = json!({"Status":0,"TC":false,"Question":[{"name":"public.example.","type":1}],"Answer":[{"type":1,"data":"8.8.8.8"},{"type":1,"data":"127.0.0.1"}]});
        let ips = doh_answers(&serde_json::to_vec(&base).unwrap(), "public.example", 1).unwrap();
        assert_eq!(ips.len(), 2);
        assert!(
            ips.iter().any(|ip| !public_ip(*ip)),
            "must retain denied answers for the shared boundary"
        );
        for (pointer, value) in [
            ("/Status", json!(3)),
            ("/TC", json!(true)),
            ("/Question/0/name", json!("other.example")),
            ("/Question/0/type", json!(28)),
            ("/Answer/0/data", json!("::1")),
        ] {
            let mut bad = base.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(
                doh_answers(&serde_json::to_vec(&bad).unwrap(), "public.example", 1).is_err(),
                "{pointer}"
            );
        }
        assert!(doh_answers(br#"{"Status":0,"Status":3}"#, "public.example", 1).is_err());
    }
    #[test]
    fn private_special_and_documentation_addresses_are_denied() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "100.64.0.1",
            "169.254.169.254",
            "192.0.2.3",
            "198.18.1.1",
            "224.0.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "3fff::1",
            "2002:7f00:1::",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "185.199.108.153", "2001:4860:4860::8888"] {
            assert!(public_ip(ip.parse().unwrap()), "{ip}");
        }
    }
    #[test]
    fn unsupported_url_credentials_ports_and_protocols_are_denied() {
        for u in [
            "file:///etc/passwd",
            "https://user:password@example.com/",
            "http://example.com:6379/",
            "https://example.com/\n",
        ] {
            assert!(normalize_url(u).is_err());
        }
        assert_eq!(
            normalize_url("https://example.com/path#fragment")
                .unwrap()
                .as_str(),
            "https://example.com/path"
        );
    }
    #[test]
    fn bounded_http_parser_handles_informational_and_binary_bodies() {
        let (status,_,body)=parse_response(b"HTTP/1.1 103 Early Hints\r\nLink: x\r\n\r\nHTTP/2 200 OK\r\nContent-Type: text/plain\r\n\r\nhello\0").unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"hello\0");
        assert!(
            parse_response(b"HTTP/1.1 302 Found\r\nLocation: a\r\nLocation: b\r\n\r\n").is_err()
        );
    }
}
