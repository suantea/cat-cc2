// 订阅解析：URL 拉取 / base64 节点列表 / share link / Clash YAML
use base64::{
    engine::general_purpose::{STANDARD as B64, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
    Engine as _,
};
use serde::Serialize;
use serde_json::json;
use std::time::Duration;
use url::Url;

#[derive(Serialize, Clone, Default, Debug)]
pub struct Node {
    pub r#type: String,
    pub name: String,
    pub server: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sni: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reality_pbk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reality_sid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reality_fp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_opts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obfs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obfs_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insecure: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub down: Option<u32>,
}

fn decode_b64(s: &str) -> Option<String> {
    let s = s.trim();
    // 1) 标准 base64（vmess JSON、普通订阅）
    if let Ok(v) = B64.decode(s) {
        return String::from_utf8(v).ok();
    }
    // 2) URL-safe（ss:// 的 method:password 常用 - _）
    if let Ok(v) = URL_SAFE.decode(s) {
        return String::from_utf8(v).ok();
    }
    // 3) URL-safe 无 padding
    if let Ok(v) = URL_SAFE_NO_PAD.decode(s) {
        return String::from_utf8(v).ok();
    }
    // 4) 标准无 padding
    if let Ok(v) = STANDARD_NO_PAD.decode(s) {
        return String::from_utf8(v).ok();
    }
    None
}

fn try_b64_decode(text: &str) -> Option<String> {
    let t = text.trim();
    if t.len() < 10
        || !t
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+/=-_".contains(c) || c.is_whitespace())
    {
        return None;
    }
    if let Some(dec) = decode_b64(&t.replace(char::is_whitespace, "")) {
        if dec.contains("://") {
            return Some(dec);
        }
    }
    None
}

fn name_from_hash(u: &Url) -> String {
    u.fragment().map(percent_decode_str).unwrap_or_default()
}

fn percent_decode_str(s: &str) -> String {
    // 简单的 percent-decode（处理 %XX 和 +）
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_share_link(link: &str) -> Option<Node> {
    if let Some(rest) = link.strip_prefix("vmess://") {
        let dec = decode_b64(rest)?;
        let o: serde_json::Value = serde_json::from_str(&dec).ok()?;
        let add = o
            .get("add")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let port = o
            .get("port")
            .and_then(|v| {
                v.as_str()
                    .or_else(|| {
                        v.as_u64()
                            .map(|n| Box::leak(n.to_string().into_boxed_str()) as &str)
                    })
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(0);
        return Some(Node {
            r#type: "vmess".into(),
            name: o
                .get("ps")
                .and_then(|v| v.as_str())
                .unwrap_or(&add)
                .to_string(),
            server: add.clone(),
            port,
            uuid: o.get("id").and_then(|v| v.as_str()).map(String::from),
            aid: o.get("aid").and_then(|v| {
                v.as_str()
                    .or_else(|| {
                        v.as_u64()
                            .map(|n| Box::leak(n.to_string().into_boxed_str()) as &str)
                    })
                    .and_then(|s| s.parse().ok())
            }),
            security: Some(
                o.get("scy")
                    .and_then(|v| v.as_str())
                    .unwrap_or("auto")
                    .to_string(),
            ),
            network: Some(
                o.get("net")
                    .and_then(|v| v.as_str())
                    .unwrap_or("tcp")
                    .to_string(),
            ),
            host: o.get("host").and_then(|v| v.as_str()).map(String::from),
            path: o.get("path").and_then(|v| v.as_str()).map(String::from),
            sni: o.get("host").and_then(|v| v.as_str()).map(String::from),
            tls: Some(
                if o.get("tls").and_then(|v| v.as_str()) == Some("tls")
                    || o.get("security").and_then(|v| v.as_str()) == Some("tls")
                {
                    "tls"
                } else {
                    ""
                }
                .to_string(),
            ),
            ..Default::default()
        });
    }
    if let Some(rest) = link.strip_prefix("ss://") {
        let (body_raw, name) = match rest.find('#') {
            Some(i) => (&rest[..i], percent_decode_str(&rest[i + 1..])),
            None => (rest, String::new()),
        };
        // SIP002：body 可能带 ?plugin=... 查询参数（去掉结尾 '/'）
        let (body, query) = match body_raw.find('?') {
            Some(i) => (body_raw[..i].trim_end_matches('/'), &body_raw[i + 1..]),
            None => (body_raw.trim_end_matches('/'), ""),
        };
        let (plugin, plugin_opts) = parse_ss_plugin(query)
            .map(|(p, o)| (Some(p), Some(o)))
            .unwrap_or((None, None));
        if let Some(at) = body.find('@') {
            if let Some(dec) = decode_b64(&body[..at]) {
                let (method, password) = dec.split_once(':').unwrap_or(("", ""));
                let (server, port_str) = body[at + 1..].split_once(':').unwrap_or(("", ""));
                return Some(Node {
                    r#type: "ss".into(),
                    name,
                    server: server.to_string(),
                    port: port_str.parse().unwrap_or(0),
                    method: Some(method.to_string()),
                    password: Some(password.to_string()),
                    plugin: plugin.clone(),
                    plugin_opts: plugin_opts.clone(),
                    ..Default::default()
                });
            }
        }
        if let Some(dec) = decode_b64(body) {
            if let Some(at) = dec.find('@') {
                let (method, password) = dec[..at].split_once(':').unwrap_or(("", ""));
                let (server, port_str) = dec[at + 1..].split_once(':').unwrap_or(("", ""));
                return Some(Node {
                    r#type: "ss".into(),
                    name,
                    server: server.to_string(),
                    port: port_str.parse().unwrap_or(0),
                    method: Some(method.to_string()),
                    password: Some(password.to_string()),
                    plugin,
                    plugin_opts,
                    ..Default::default()
                });
            }
        }
        return None;
    }
    if link.starts_with("trojan://") {
        if let Ok(u) = Url::parse(link) {
            let host = u.host_str().unwrap_or("").to_string();
            return Some(Node {
                r#type: "trojan".into(),
                name: name_from_hash(&u),
                server: host.clone(),
                port: u.port().unwrap_or(443),
                password: Some(u.username().to_string()),
                sni: u
                    .query_pairs()
                    .find(|(k, _)| k == "sni")
                    .map(|(_, v)| v.into_owned())
                    .or_else(|| Some(host.clone())),
                network: u
                    .query_pairs()
                    .find(|(k, _)| k == "type")
                    .map(|(_, v)| v.into_owned())
                    .or_else(|| Some("tcp".into())),
                host: u
                    .query_pairs()
                    .find(|(k, _)| k == "host")
                    .map(|(_, v)| v.into_owned()),
                path: u
                    .query_pairs()
                    .find(|(k, _)| k == "path")
                    .map(|(_, v)| v.into_owned()),
                tls: Some("tls".into()),
                ..Default::default()
            });
        }
    }
    if link.starts_with("vless://") {
        if let Ok(u) = Url::parse(link) {
            let host = u.host_str().unwrap_or("").to_string();
            let q = |k: &str| {
                u.query_pairs()
                    .find(|(x, _)| x == k)
                    .map(|(_, v)| v.into_owned())
            };
            return Some(Node {
                r#type: "vless".into(),
                name: name_from_hash(&u),
                server: host.clone(),
                port: u.port().unwrap_or(443),
                uuid: Some(u.username().to_string()),
                flow: q("flow"),
                network: q("type").or_else(|| Some("tcp".into())),
                security: q("security").or_else(|| Some("none".into())),
                sni: q("sni").or_else(|| Some(host.clone())),
                host: q("host"),
                path: q("path"),
                reality_pbk: q("pbk"),
                reality_sid: q("sid"),
                reality_fp: q("fp").or_else(|| Some("chrome".into())),
                ..Default::default()
            });
        }
    }
    if link.starts_with("hy2://") || link.starts_with("hysteria2://") {
        if let Ok(u) = Url::parse(
            &link
                .replacen("hy2://", "https://", 1)
                .replacen("hysteria2://", "https://", 1),
        ) {
            let host = u.host_str().unwrap_or("").to_string();
            let q = |k: &str| {
                u.query_pairs()
                    .find(|(x, _)| x == k)
                    .map(|(_, v)| v.into_owned())
            };
            return Some(Node {
                r#type: "hysteria2".into(),
                name: name_from_hash(&u),
                server: host.clone(),
                port: u.port().unwrap_or(443),
                password: Some(u.username().to_string()),
                sni: q("sni").or_else(|| Some(host.clone())),
                obfs: q("obfs"),
                obfs_password: q("obfs-password"),
                insecure: q("insecure").map(|v| v == "1" || v == "true"),
                ..Default::default()
            });
        }
    }
    // hysteria1：hysteria://auth@host:port?upmbps=100&downmbps=100&obfs=xplus&obfsParam=xxx
    if link.starts_with("hysteria://") {
        if let Ok(u) = Url::parse(link) {
            let host = u.host_str().unwrap_or("").to_string();
            let q = |k: &str| {
                u.query_pairs()
                    .find(|(x, _)| x == k)
                    .map(|(_, v)| v.into_owned())
            };
            return Some(Node {
                r#type: "hysteria".into(),
                name: name_from_hash(&u),
                server: host.clone(),
                port: u.port().unwrap_or(443),
                // hysteria1 的 auth 串（可能带 Base64 前缀或明文）
                password: Some(u.username().to_string()),
                sni: q("sni").or_else(|| Some(host.clone())),
                obfs: q("obfs"),
                obfs_password: q("obfsParam").or_else(|| q("obfs-password")),
                insecure: q("insecure").map(|v| v == "1" || v == "true"),
                up: q("upmbps").and_then(|v| v.parse().ok()),
                down: q("downmbps").and_then(|v| v.parse().ok()),
                ..Default::default()
            });
        }
    }
    if link.starts_with("socks5://") || link.starts_with("socks://") {
        if let Ok(u) = Url::parse(
            &link
                .replacen("socks5://", "http://", 1)
                .replacen("socks://", "http://", 1),
        ) {
            return Some(Node {
                r#type: "socks".into(),
                name: name_from_hash(&u),
                server: u.host_str().unwrap_or("").to_string(),
                port: u.port().unwrap_or(1080),
                username: (!u.username().is_empty()).then(|| u.username().to_string()),
                password: u.password().map(String::from),
                ..Default::default()
            });
        }
    }
    if link.starts_with("http://") || link.starts_with("https://") {
        if let Ok(u) = Url::parse(link) {
            return Some(Node {
                r#type: "http".into(),
                name: name_from_hash(&u),
                server: u.host_str().unwrap_or("").to_string(),
                port: u.port().unwrap_or(8080),
                username: (!u.username().is_empty()).then(|| u.username().to_string()),
                password: u.password().map(String::from),
                ..Default::default()
            });
        }
    }
    None
}

// YAML 字符串安全引用（与 core_mgr::yaml_str 相同策略）
fn yaml_safe(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_/".contains(c))
    {
        s.to_string()
    } else {
        serde_json::to_string(s).unwrap_or_else(|_| format!("\"{}\"", s))
    }
}

// ss:// SIP002 plugin 参数解析：plugin=obfs-local;obfs=http;obfs-host=x 或 v2ray-plugin;mode=websocket;...
// 返回 (mihomo plugin 名, plugin-opts 的 YAML 子行，已按 6 空格缩进供 plugin-opts: 下使用)
fn parse_ss_plugin(query: &str) -> Option<(String, String)> {
    let plugin_val = query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k == "plugin" {
            Some(v)
        } else {
            None
        }
    })?;
    let spec = percent_decode_str(plugin_val);
    let mut parts = spec.split(';');
    let name = parts.next()?;
    let mut opts: Vec<(String, String)> = parts
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (
                k.to_string(),
                if v.is_empty() {
                    "true".to_string()
                } else {
                    v.to_string()
                },
            ),
            None => (p.to_string(), "true".to_string()), // 无值 flag（如 tls）
        })
        .collect();
    let (plugin, lines): (&str, Vec<String>) = match name {
        "obfs-local" | "simple-obfs" => {
            let mut mode = "http".to_string();
            let mut host = String::new();
            let mut uri = String::new();
            for (k, v) in opts.drain(..) {
                match k.as_str() {
                    "obfs" => mode = v,
                    "obfs-host" => host = v,
                    "obfs-uri" => uri = v,
                    _ => {}
                }
            }
            let mut ls = vec![format!("      mode: {}", yaml_safe(&mode))];
            if !host.is_empty() {
                ls.push(format!("      host: {}", yaml_safe(&host)));
            }
            if !uri.is_empty() {
                ls.push(format!("      uri: {}", yaml_safe(&uri)));
            }
            ("obfs", ls)
        }
        "v2ray-plugin" => {
            let mut mode = "websocket".to_string();
            let mut host = String::new();
            let mut path = String::new();
            let mut tls = false;
            for (k, v) in opts.drain(..) {
                match k.as_str() {
                    "mode" => mode = v,
                    "host" => host = v,
                    "path" => path = v,
                    "tls" => tls = v == "true" || v == "1",
                    _ => {}
                }
            }
            let mut ls = vec![format!("      mode: {}", yaml_safe(&mode))];
            if !host.is_empty() {
                ls.push(format!("      host: {}", yaml_safe(&host)));
            }
            if !path.is_empty() {
                ls.push(format!("      path: {}", yaml_safe(&path)));
            }
            if tls {
                ls.push("      tls: true".into());
            }
            ("v2ray-plugin", ls)
        }
        _ => return None,
    };
    Some((plugin.to_string(), lines.join("\n")))
}

// Clash YAML：提取 proxies 段
fn parse_clash_proxies(text: &str) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut in_proxies = false;
    let mut cur: Option<Node> = None;
    let flush = |cur: &mut Option<Node>, nodes: &mut Vec<Node>| {
        if let Some(n) = cur.take() {
            if !n.server.is_empty() && n.port > 0 {
                nodes.push(n);
            }
        }
    };
    for line in text.lines() {
        let l = line.trim_end();
        if !in_proxies {
            if l.trim_start().starts_with("proxies:") {
                in_proxies = true;
            }
            continue;
        }
        // 离开 proxies 段：遇到无缩进的顶层键（如 proxy-groups: / rules:）
        if !l.starts_with(' ') && !l.starts_with('\t') && !l.starts_with('-') && l.contains(':') {
            flush(&mut cur, &mut nodes);
            break;
        }
        let trimmed = l.trim_start();
        // 单行对象：  - {name: x, type: ss, ...}
        if let Some(m) = trimmed.strip_prefix("- {") {
            flush(&mut cur, &mut nodes);
            if let Some(end) = m.rfind('}') {
                let inner = &m[..end];
                let mut o = std::collections::HashMap::new();
                for kv in split_top_level(inner) {
                    if let Some(idx) = kv.find(':') {
                        let k = kv[..idx].trim().to_string();
                        let v = kv[idx + 1..].trim().trim_matches('"').to_string();
                        o.insert(k, v);
                    }
                }
                if let Some(n) = clash_to_node(&o) {
                    nodes.push(n);
                }
            }
            continue;
        }
        // 新节点：  - name: x
        if let Some(m) = trimmed.strip_prefix("- name:") {
            flush(&mut cur, &mut nodes);
            cur = Some(Node {
                name: m.trim().trim_matches('"').to_string(),
                ..Default::default()
            });
            continue;
        }
        // 字段行：    key: value（多行节点属性，无 - 前缀）
        if let Some(n) = cur.as_mut() {
            let field = trimmed.strip_prefix('-').map(str::trim).unwrap_or(trimmed);
            if let Some(idx) = field.find(':') {
                let k = field[..idx].trim().to_string();
                let v = field[idx + 1..].trim().trim_matches('"').to_string();
                match k.as_str() {
                    "name" => n.name = v,
                    "type" => n.r#type = v,
                    "server" => n.server = v,
                    "port" => n.port = v.parse().unwrap_or(0),
                    "uuid" => n.uuid = Some(v),
                    "alterId" => n.aid = v.parse().ok(),
                    "password" => n.password = Some(v),
                    "cipher" | "method" => n.method = Some(v),
                    "flow" => n.flow = Some(v),
                    "network" => n.network = Some(v),
                    "servername" | "sni" => n.sni = Some(v),
                    "tls" => n.security = Some(if v == "true" { "tls" } else { "none" }.into()),
                    "ws-path" => n.path = Some(v),
                    "username" => n.username = Some(v),
                    "plugin" => n.plugin = Some(v),
                    _ => {}
                }
            }
        }
    }
    flush(&mut cur, &mut nodes);
    nodes
}

fn split_top_level(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '"' => {
                depth ^= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                parts.push(cur.clone());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts
}

fn clash_to_node(o: &std::collections::HashMap<String, String>) -> Option<Node> {
    let t = o.get("type")?.to_string();
    let server = o.get("server").cloned().unwrap_or_default();
    let port: u16 = o.get("port").and_then(|v| v.parse().ok())?;
    if server.is_empty() || port == 0 {
        return None;
    }
    let name = o.get("name").cloned().unwrap_or_default();
    let get = |k: &str| o.get(k).cloned();
    let mut n = Node {
        r#type: t.clone(),
        name,
        server,
        port,
        ..Default::default()
    };
    match t.as_str() {
        "vmess" => {
            n.uuid = get("uuid");
            n.aid = get("alterId").and_then(|v| v.parse().ok());
            n.security = get("cipher").or_else(|| Some("auto".into()));
            n.network = get("network").or_else(|| Some("tcp".into()));
            n.tls = Some(
                if get("tls").as_deref() == Some("true") {
                    "tls"
                } else {
                    ""
                }
                .to_string(),
            );
            n.sni = get("servername").or_else(|| get("sni"));
            n.host = get("host");
            n.path = get("path").or_else(|| get("ws-path"));
        }
        "ss" => {
            n.method = get("cipher").or_else(|| get("method"));
            n.password = get("password");
        }
        "trojan" => {
            n.password = get("password");
            n.sni = get("sni")
                .or_else(|| get("servername"))
                .or_else(|| Some(n.server.clone()));
            n.tls = Some("tls".into());
            n.network = get("network").or_else(|| Some("tcp".into()));
        }
        "vless" => {
            n.uuid = get("uuid");
            n.flow = get("flow");
            n.security = Some(
                if get("tls").as_deref() == Some("true") {
                    "tls"
                } else {
                    "none"
                }
                .to_string(),
            );
            n.sni = get("servername").or_else(|| get("sni"));
            n.network = get("network").or_else(|| Some("tcp".into()));
            n.host = get("host");
            n.path = get("path").or_else(|| get("ws-path"));
        }
        "socks5" => {
            n.username = get("username");
            n.password = get("password");
        }
        "http" => {
            n.username = get("username");
            n.password = get("password");
        }
        _ => return None,
    }
    Some(n)
}

// 拉取订阅 URL
pub fn fetch_url_pub(url: &str) -> Result<String, String> {
    fetch_url(url)
}

fn fetch_url(url: &str) -> Result<String, String> {
    match fetch_url_direct(url) {
        Ok(body) => Ok(body),
        Err(direct_err) => {
            // 直连失败：若内核已运行，经本地代理重试（订阅源可能被墙）
            match crate::core_mgr::running_proxy_port() {
                Some(port) => match fetch_url_via_proxy(url, port) {
                    Ok(body) => Ok(body),
                    Err(proxy_err) => Err(format!(
                        "直连失败（{}）；经本地代理重试也失败（{}）",
                        direct_err, proxy_err
                    )),
                },
                None => Err(format!(
                    "{}（可尝试把订阅内容直接粘贴到输入框）",
                    direct_err
                )),
            }
        }
    }
}

fn fetch_url_direct(url: &str) -> Result<String, String> {
    let resp = ureq::get(url)
        .timeout(Duration::from_secs(20))
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .call()
        .map_err(|e| format!("拉取订阅失败: {}", e))?;
    resp.into_string()
        .map_err(|e| format!("读取响应失败: {}", e))
}

// 经本地 mihomo 代理拉取（解决订阅源被墙的鸡生蛋问题）
fn fetch_url_via_proxy(url: &str, port: u16) -> Result<String, String> {
    let proxy = ureq::Proxy::new(format!("http://127.0.0.1:{}", port))
        .map_err(|e| format!("代理配置无效: {}", e))?;
    let agent = ureq::AgentBuilder::new()
        .proxy(proxy)
        .timeout(Duration::from_secs(20))
        .build();
    let resp = agent
        .get(url)
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .call()
        .map_err(|e| format!("经代理拉取订阅失败: {}", e))?;
    resp.into_string()
        .map_err(|e| format!("读取响应失败: {}", e))
}

pub fn parse_subscription(text: &str) -> Vec<Node> {
    let source = try_b64_decode(text).unwrap_or_else(|| text.to_string());
    // Clash YAML
    if source.lines().any(|l| l.starts_with("proxies:")) {
        let nodes = parse_clash_proxies(&source);
        if !nodes.is_empty() {
            return dedupe_nodes(nodes);
        }
    }
    // 逐行 share link
    let mut nodes = Vec::new();
    for line in source.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some(n) = parse_share_link(l) {
            nodes.push(n);
        }
    }
    dedupe_nodes(nodes)
}

// 按 (类型, 服务器, 端口, 凭证) 去重，保留首次出现（避免重复节点进 url-test 组浪费测速）
fn dedupe_nodes(nodes: Vec<Node>) -> Vec<Node> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for n in nodes {
        let cred = n
            .uuid
            .clone()
            .or_else(|| n.password.clone())
            .or_else(|| n.username.clone())
            .unwrap_or_default();
        let key = format!("{}|{}|{}|{}", n.r#type, n.server, n.port, cred);
        if seen.insert(key) {
            out.push(n);
        }
    }
    out
}

#[tauri::command]
pub fn parse_sub(subscription: String) -> Result<serde_json::Value, String> {
    let text = if subscription.starts_with("http://") || subscription.starts_with("https://") {
        fetch_url(&subscription)?
    } else {
        subscription
    };
    let nodes = parse_subscription(&text);
    if nodes.is_empty() {
        return Err("解析不到任何节点，请检查订阅链接".into());
    }
    let names: Vec<String> = nodes
        .iter()
        .map(|n| {
            if n.name.is_empty() {
                n.server.clone()
            } else {
                n.name.clone()
            }
        })
        .collect();
    Ok(json!({ "count": nodes.len(), "names": names }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_base64_sub() {
        let sub = B64.encode("ss://YWVzLTI1Ni1nY206cGFzc3dvcmQxMjM@1.2.3.4:8388#测试节点");
        let dec = try_b64_decode(&sub);
        println!("try_b64_decode: {:?}", dec);
        let share = parse_share_link("ss://YWVzLTI1Ni1nY206cGFzc3dvcmQxMjM@1.2.3.4:8388#测试节点");
        println!("parse_share_link direct: {:?}", share);
        let nodes = parse_subscription(&sub);
        println!("nodes: {:?}", nodes);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].r#type, "ss");
        assert_eq!(nodes[0].server, "1.2.3.4");
        assert_eq!(nodes[0].name, "测试节点");
    }

    #[test]
    fn parse_multi_share_links() {
        let vmesstag = serde_json::json!({"ps":"V2节点","add":"5.6.7.8","port":443,"id":"uuid-1234","aid":0,"net":"ws","host":"h.com","path":"/p","tls":"tls","scy":"auto"});
        let vmess = format!("vmess://{}", B64.encode(vmesstag.to_string().as_bytes()));
        let vless = "vless://uuid-5678@9.9.9.9:443?security=reality&sni=real.com&pbk=pubkey&sid=sid123&fp=chrome&type=tcp#RL节点";
        let sub = format!("{}\n{}", vmess, vless);
        let nodes = parse_subscription(&sub);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].r#type, "vmess");
        assert_eq!(nodes[1].r#type, "vless");
        assert_eq!(nodes[1].reality_pbk.as_deref(), Some("pubkey"));
    }

    #[test]
    fn parse_clash_yaml() {
        let yaml = "proxies:\n  - name: Y节点\n    type: ss\n    server: 8.8.8.8\n    port: 8443\n    cipher: aes-256-gcm\n    password: pass\n  - {name: \"单行节点\", type: trojan, server: 1.1.1.1, port: 443, password: pw, sni: s.com}";
        let nodes = parse_subscription(yaml);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].r#type, "ss");
        assert_eq!(nodes[0].name, "Y节点");
        assert_eq!(nodes[1].r#type, "trojan");
    }

    #[test]
    fn parse_ss_sip002_plugin() {
        let link = "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQxMjM@1.2.3.4:8388/?plugin=obfs-local%3Bobfs%3Dhttp%3Bobfs-host%3Dcdn.example.com#插件节点";
        let n = parse_share_link(link).expect("SIP002 plugin 链接应解析成功");
        assert_eq!(n.r#type, "ss");
        assert_eq!(n.server, "1.2.3.4");
        assert_eq!(n.port, 8388);
        assert_eq!(n.plugin.as_deref(), Some("obfs"));
        let opts = n.plugin_opts.as_deref().unwrap_or("");
        assert!(opts.contains("mode: http"), "opts={}", opts);
        assert!(opts.contains("host: cdn.example.com"), "opts={}", opts);
    }

    #[test]
    fn parse_ss_sip002_plugin_v2ray() {
        let link = "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQxMjM@1.2.3.4:443/?plugin=v2ray-plugin%3Bmode%3Dwebsocket%3Bhost%3Dh.com%3Bpath%3D%2Fws%3Btls#V2";
        let n = parse_share_link(link).expect("v2ray-plugin 应解析成功");
        assert_eq!(n.plugin.as_deref(), Some("v2ray-plugin"));
        let opts = n.plugin_opts.as_deref().unwrap_or("");
        assert!(opts.contains("mode: websocket"), "opts={}", opts);
        assert!(opts.contains("path: /ws"), "opts={}", opts);
        assert!(opts.contains("tls: true"), "opts={}", opts);
    }

    #[test]
    fn dedupe_removes_duplicates() {
        let base = "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQxMjM@";
        let sub = format!(
            "{}1.2.3.4:8388#A\n{}1.2.3.4:8388#B\n{}5.6.7.8:8388#C",
            base, base, base
        );
        let nodes = parse_subscription(&sub);
        assert_eq!(nodes.len(), 2, "同服务器同凭证去重，不同服务器保留");
        assert_eq!(nodes[0].name, "A");
        assert_eq!(nodes[1].name, "C");
    }

    #[test]
    fn parse_hysteria2_links() {
        // 完整版：password@host + sni/obfs/insecure 参数
        let hy2 = "hy2://passw0rd@1.2.3.4:443?sni=example.com&obfs=salamander&obfs-password=obfs-secret&insecure=1#HY2节点";
        let n = parse_share_link(hy2).expect("hy2:// 应解析成功");
        assert_eq!(n.r#type, "hysteria2");
        assert_eq!(n.server, "1.2.3.4");
        assert_eq!(n.port, 443);
        assert_eq!(n.password.as_deref(), Some("passw0rd"));
        assert_eq!(n.sni.as_deref(), Some("example.com"));
        assert_eq!(n.obfs.as_deref(), Some("salamander"));
        assert_eq!(n.obfs_password.as_deref(), Some("obfs-secret"));
        assert_eq!(n.insecure, Some(true));
        assert_eq!(n.name, "HY2节点");

        // 别名 hysteria2:// + 缺省（无 sni → 用 host）
        let h2 = "hysteria2://pw@9.9.9.9:8443#别名";
        let m = parse_share_link(h2).expect("hysteria2:// 应解析成功");
        assert_eq!(m.r#type, "hysteria2");
        assert_eq!(m.port, 8443);
        assert_eq!(m.sni.as_deref(), Some("9.9.9.9"));
        assert_eq!(m.insecure, None);
    }

    #[test]
    fn parse_hysteria1_links() {
        // 完整版：auth@host + upmbps/downmbps/obfs/obfsParam/sni/insecure 参数
        let h1 = "hysteria://auth-secret@1.2.3.4:443?upmbps=100&downmbps=100&obfs=xplus&obfsParam=obfs-key&sni=example.com&insecure=1#HY1节点";
        let n = parse_share_link(h1).expect("hysteria:// 应解析成功");
        assert_eq!(n.r#type, "hysteria");
        assert_eq!(n.server, "1.2.3.4");
        assert_eq!(n.port, 443);
        assert_eq!(n.password.as_deref(), Some("auth-secret"));
        assert_eq!(n.up, Some(100));
        assert_eq!(n.down, Some(100));
        assert_eq!(n.obfs.as_deref(), Some("xplus"));
        assert_eq!(n.obfs_password.as_deref(), Some("obfs-key"));
        assert_eq!(n.sni.as_deref(), Some("example.com"));
        assert_eq!(n.insecure, Some(true));
        assert_eq!(n.name, "HY1节点");

        // 缺省：无 sni → host；无带宽 → None；无 obfs → None
        let m = parse_share_link("hysteria://auth@9.9.9.9:8443#简版").expect("hysteria:// 简版应解析成功");
        assert_eq!(m.r#type, "hysteria");
        assert_eq!(m.sni.as_deref(), Some("9.9.9.9"));
        assert_eq!(m.up, None);
        assert_eq!(m.down, None);
        assert_eq!(m.obfs, None);
        assert_eq!(m.insecure, None);
    }

    #[test]
    fn parse_http_and_socks_links() {
        let http = "http://user:pass@1.2.3.4:8080#HTTP节点";
        let h = parse_share_link(http).expect("http:// 应解析成功");
        assert_eq!(h.r#type, "http");
        assert_eq!(h.username.as_deref(), Some("user"));
        assert_eq!(h.password.as_deref(), Some("pass"));

        let socks = "socks5://1.2.3.4:1080#SOCKS节点";
        let s = parse_share_link(socks).expect("socks5:// 应解析成功");
        assert_eq!(s.r#type, "socks");
        assert_eq!(s.server, "1.2.3.4");
        assert_eq!(s.port, 1080);
    }
}
