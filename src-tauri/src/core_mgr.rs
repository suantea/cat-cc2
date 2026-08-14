// mihomo 内核管理：配置生成 / 启动校验 / 进程监控 / connect / disconnect / status
use crate::subscribe::{parse_subscription, Node};
use serde::Serialize;
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const MAX_RETRIES: u32 = 3;
// 订阅自动刷新 / 全节点失效自动重拉
const REFRESH_INTERVAL: Duration = Duration::from_secs(6 * 3600); // 每 6h 定时刷新
const ALL_DEAD_INTERVAL: Duration = Duration::from_secs(60); // 全挂检查间隔
const ALL_DEAD_THRESHOLD: u32 = 3; // 连续 3 次全挂才触发重拉，防误判
const REFRESH_COOLDOWN: Duration = Duration::from_secs(120); // 两次实际重拉最小间隔

// 内核二进制名：Windows 为 mihomo-core.exe；macOS/Linux 为 mihomo-core
#[cfg(target_os = "windows")]
const CORE_NAME: &str = "mihomo-core.exe";
#[cfg(not(target_os = "windows"))]
const CORE_NAME: &str = "mihomo-core";

// Windows 下隐藏子进程控制台窗口；其他平台原样返回
#[cfg(target_os = "windows")]
fn hide_window(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW)
}
#[cfg(not(target_os = "windows"))]
fn hide_window(cmd: &mut Command) -> &mut Command {
    cmd
}

// 强杀进程：Windows taskkill /F；macOS/Linux kill（SIGTERM，mihomo 正常处理退出）
#[cfg(target_os = "windows")]
fn kill_process(pid: u32) {
    let _ = hide_window(Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();
}
#[cfg(not(target_os = "windows"))]
fn kill_process(pid: u32) {
    let _ = Command::new("kill")
        .args([&pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();
}

#[derive(Serialize, Default, Clone)]
pub struct CoreStatus {
    pub running: bool,
    pub node: String,
    pub latency: u64,
    pub last_error: String,
    pub retries: u32,
    pub download_total: u64, // 累计下行字节（内核启动以来）
    pub upload_total: u64,   // 累计上行字节（内核启动以来）
}

struct CoreState {
    status: CoreStatus,
    nodes: Vec<Node>,
    mixed_port: u16,
    ctrl_port: u16,
    intentional_stop: bool,
    core_pid: Option<u32>, // 当前内核进程 PID（用于 kill）
    generation: u32,       // 启动代数：重启时自增，旧监控线程据此退出
    last_probe_ms: u64,    // 上次主动测速时间戳（节流用）
    temp_dir: PathBuf,
    config_file: PathBuf,
    core_bin: PathBuf,
    proxy_enabled: bool, // 系统代理是否由本程序开启（断开/退出时只清理自己的）
    sub_source: String,  // connect 传入的原始订阅串（自动刷新用）
    refresh_stop: Option<Arc<AtomicBool>>, // 自动刷新线程停止信号
}

static STATE: OnceLock<Mutex<CoreState>> = OnceLock::new();

fn state() -> &'static Mutex<CoreState> {
    STATE.get_or_init(|| {
        Mutex::new(CoreState {
            status: CoreStatus::default(),
            nodes: Vec::new(),
            mixed_port: 7890,
            ctrl_port: 9090,
            intentional_stop: true,
            core_pid: None,
            generation: 0,
            last_probe_ms: 0,
            temp_dir: PathBuf::new(),
            config_file: PathBuf::new(),
            core_bin: PathBuf::new(),
            proxy_enabled: false,
            sub_source: String::new(),
            refresh_stop: None,
        })
    })
}

// 查找内核：exe 同目录 → src-tauri/ → 项目根（编译期路径，开发模式稳定）
fn find_core() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        exe_dir.join(CORE_NAME),
        exe_dir.join("..").join("..").join(CORE_NAME), // src-tauri/
        exe_dir
            .join("..")
            .join("..")
            .join("..")
            .join(CORE_NAME), // 项目根（main exe）
        exe_dir
            .join("..")
            .join("..")
            .join("..")
            .join("..")
            .join(CORE_NAME), // 项目根（测试 exe）
        manifest.join("..").join(CORE_NAME),           // 编译期：src-tauri/../ = 项目根
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn app_base() -> PathBuf {
    // 可写数据目录：exe 同目录（便携）；开发时放项目 config/
    if let Ok(exe) = std::env::current_exe() {
        let dir = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
        // 开发模式（target/debug/ 下无内核二进制）→ 用项目根
        if !dir.join(CORE_NAME).exists() {
            if let Some(root) = dir
                .parent()
                .and_then(|d| d.parent())
                .and_then(|d| d.parent())
            {
                return root.join("config");
            }
        }
        return dir.join("config");
    }
    PathBuf::from("config")
}

fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn pick_port(pref: u16) -> u16 {
    if port_free(pref) {
        return pref;
    }
    (20000..65535u16).find(|&p| port_free(p)).unwrap_or(pref)
}

// 检查进程是否存活（Windows tasklist 精确匹配 PID；Unix kill -0 探活）
fn pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        let out = hide_window(
            Command::new("tasklist").args(["/FI", &format!("PID eq {}", pid), "/NH"]),
        )
        .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .any(|tok| tok == pid.to_string()),
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // 信号 0 不发送任何信号，仅探测进程是否存在（同用户权限下可用）
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// 等待进程退出（100ms 轮询，最多 max_wait；查不到进程时视为已退出）
fn wait_pid_exit(pid: u32, max_wait: Duration) {
    let deadline = std::time::Instant::now() + max_wait;
    while std::time::Instant::now() < deadline {
        if !pid_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// 停止当前内核：标记主动停止 + 强杀进程（返回被杀 PID）。
// 监控线程 wait 返回后见 intentional_stop 即退出；generation 计数防旧线程误重启
fn stop_kernel(st: &mut CoreState) -> Option<u32> {
    st.intentional_stop = true;
    st.status.running = false;
    st.status.node.clear();
    st.status.latency = 0;
    let pid = st.core_pid.take();
    if let Some(p) = pid {
        kill_process(p);
    }
    pid
}

// ---------- 配置生成 ----------
fn yaml_str(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_/".contains(c))
    {
        s.to_string()
    } else {
        serde_json::to_string(s).unwrap_or_else(|_| format!("\"{}\"", s))
    }
}

fn outbound_yaml(n: &Node) -> Option<String> {
    let name = yaml_str(&n.name);
    let mut parts: Vec<String> = vec![
        format!("  - name: {}", name),
        format!("    type: {}", n.r#type),
    ];
    let push = |parts: &mut Vec<String>, line: String| {
        parts.push(line);
    };
    match n.r#type.as_str() {
        "ss" => {
            push(&mut parts, format!("    server: {}", n.server));
            push(&mut parts, format!("    port: {}", n.port));
            push(
                &mut parts,
                format!(
                    "    cipher: {}",
                    yaml_str(n.method.as_deref().unwrap_or("aes-256-gcm"))
                ),
            );
            push(
                &mut parts,
                format!(
                    "    password: {}",
                    yaml_str(n.password.as_deref().unwrap_or(""))
                ),
            );
            if let Some(p) = n.plugin.as_deref() {
                push(&mut parts, format!("    plugin: {}", yaml_str(p)));
                if let Some(o) = n.plugin_opts.as_deref() {
                    push(&mut parts, format!("    plugin-opts:\n{}", o));
                }
            }
            push(&mut parts, "    udp: true".into());
        }
        "trojan" => {
            push(&mut parts, format!("    server: {}", n.server));
            push(&mut parts, format!("    port: {}", n.port));
            push(
                &mut parts,
                format!(
                    "    password: {}",
                    yaml_str(n.password.as_deref().unwrap_or(""))
                ),
            );
            push(
                &mut parts,
                format!(
                    "    sni: {}",
                    yaml_str(n.sni.as_deref().unwrap_or(&n.server))
                ),
            );
            if n.network.as_deref() == Some("ws") {
                push(&mut parts, "    network: ws".into());
                push(
                    &mut parts,
                    format!(
                        "    ws-opts:\n      path: {}\n      headers:\n        Host: {}",
                        yaml_str(n.path.as_deref().unwrap_or("/")),
                        yaml_str(
                            n.host
                                .as_deref()
                                .unwrap_or(n.sni.as_deref().unwrap_or(&n.server))
                        )
                    ),
                );
            }
            push(&mut parts, "    udp: true".into());
        }
        "vmess" => {
            push(&mut parts, format!("    server: {}", n.server));
            push(&mut parts, format!("    port: {}", n.port));
            push(
                &mut parts,
                format!("    uuid: {}", yaml_str(n.uuid.as_deref().unwrap_or(""))),
            );
            push(&mut parts, format!("    alterId: {}", n.aid.unwrap_or(0)));
            push(
                &mut parts,
                format!(
                    "    cipher: {}",
                    yaml_str(n.security.as_deref().unwrap_or("auto"))
                ),
            );
            if n.tls.as_deref() == Some("tls") || n.security.as_deref() == Some("tls") {
                push(&mut parts, "    tls: true".into());
                push(
                    &mut parts,
                    format!(
                        "    servername: {}",
                        yaml_str(n.sni.as_deref().unwrap_or(&n.server))
                    ),
                );
            }
            if n.network.as_deref() == Some("ws") {
                push(&mut parts, "    network: ws".into());
                push(
                    &mut parts,
                    format!(
                        "    ws-opts:\n      path: {}",
                        yaml_str(n.path.as_deref().unwrap_or("/"))
                    ),
                );
                if let Some(h) = n.host.as_deref() {
                    push(
                        &mut parts,
                        format!("      headers:\n        Host: {}", yaml_str(h)),
                    );
                }
            }
            push(&mut parts, "    udp: true".into());
        }
        "vless" => {
            push(&mut parts, format!("    server: {}", n.server));
            push(&mut parts, format!("    port: {}", n.port));
            push(
                &mut parts,
                format!("    uuid: {}", yaml_str(n.uuid.as_deref().unwrap_or(""))),
            );
            if let Some(f) = n.flow.as_deref() {
                if !f.is_empty() {
                    push(&mut parts, format!("    flow: {}", yaml_str(f)));
                }
            }
            if n.security.as_deref() == Some("reality") {
                push(
                    &mut parts,
                    format!(
                        "    tls: true\n    servername: {}\n    client-fingerprint: {}",
                        yaml_str(n.sni.as_deref().unwrap_or(&n.server)),
                        yaml_str(n.reality_fp.as_deref().unwrap_or("chrome"))
                    ),
                );
                if let Some(pbk) = n.reality_pbk.as_deref() {
                    if !pbk.is_empty() {
                        push(
                            &mut parts,
                            format!(
                                "    reality-opts:\n      public-key: {}\n      short-id: {}",
                                yaml_str(pbk),
                                yaml_str(n.reality_sid.as_deref().unwrap_or(""))
                            ),
                        );
                    }
                }
            } else if n.security.as_deref() == Some("tls") || n.tls.as_deref() == Some("tls") {
                push(
                    &mut parts,
                    format!(
                        "    tls: true\n    servername: {}",
                        yaml_str(n.sni.as_deref().unwrap_or(&n.server))
                    ),
                );
            }
            if n.network.as_deref() == Some("ws") {
                push(&mut parts, "    network: ws".into());
                push(
                    &mut parts,
                    format!(
                        "    ws-opts:\n      path: {}",
                        yaml_str(n.path.as_deref().unwrap_or("/"))
                    ),
                );
                if let Some(h) = n.host.as_deref() {
                    push(
                        &mut parts,
                        format!("      headers:\n        Host: {}", yaml_str(h)),
                    );
                }
            }
            push(&mut parts, "    udp: true".into());
        }
        "socks" => {
            push(&mut parts, format!("    server: {}", n.server));
            push(&mut parts, format!("    port: {}", n.port));
            if let Some(u) = n.username.as_deref() {
                if !u.is_empty() {
                    push(
                        &mut parts,
                        format!(
                            "    username: {}\n    password: {}",
                            yaml_str(u),
                            yaml_str(n.password.as_deref().unwrap_or(""))
                        ),
                    );
                }
            }
        }
        "http" => {
            push(&mut parts, format!("    server: {}", n.server));
            push(&mut parts, format!("    port: {}", n.port));
            if let Some(u) = n.username.as_deref() {
                if !u.is_empty() {
                    push(
                        &mut parts,
                        format!(
                            "    username: {}\n    password: {}",
                            yaml_str(u),
                            yaml_str(n.password.as_deref().unwrap_or(""))
                        ),
                    );
                }
            }
        }
        _ => return None,
    }
    Some(parts.join("\n"))
}

// 用户自定义追加规则（可编辑，追加在 gfw 自动规则之后、MATCH 之前）
const DEFAULT_RULES: &str = "\
# 私有地址直连
IP-CIDR,127.0.0.0/8,DIRECT
IP-CIDR,10.0.0.0/8,DIRECT
IP-CIDR,172.16.0.0/12,DIRECT
IP-CIDR,192.168.0.0/16,DIRECT
# 国内常用服务直连
DOMAIN-SUFFIX,baidu.com,DIRECT
DOMAIN-SUFFIX,bdstatic.com,DIRECT
DOMAIN-SUFFIX,qq.com,DIRECT
DOMAIN-SUFFIX,gtimg.com,DIRECT
DOMAIN-SUFFIX,weixin.qq.com,DIRECT
DOMAIN-SUFFIX,taobao.com,DIRECT
DOMAIN-SUFFIX,tmall.com,DIRECT
DOMAIN-SUFFIX,alicdn.com,DIRECT
DOMAIN-SUFFIX,jd.com,DIRECT
DOMAIN-SUFFIX,360buyimg.com,DIRECT
DOMAIN-SUFFIX,bilibili.com,DIRECT
DOMAIN-SUFFIX,hdslb.com,DIRECT
DOMAIN-SUFFIX,weibo.com,DIRECT
DOMAIN-SUFFIX,zhihu.com,DIRECT
DOMAIN-SUFFIX,douyin.com,DIRECT
DOMAIN-SUFFIX,bytedance.com,DIRECT
DOMAIN-SUFFIX,163.com,DIRECT
DOMAIN-SUFFIX,126.com,DIRECT
DOMAIN-SUFFIX,youku.com,DIRECT
DOMAIN-SUFFIX,iqiyi.com,DIRECT
DOMAIN-SUFFIX,sohu.com,DIRECT
DOMAIN-SUFFIX,ctrip.com,DIRECT
DOMAIN-SUFFIX,mi.com,DIRECT
DOMAIN-SUFFIX,xiaomi.com,DIRECT
DOMAIN-SUFFIX,huawei.com,DIRECT
DOMAIN-SUFFIX,tencent.com,DIRECT
DOMAIN-SUFFIX,aliyun.com,DIRECT
DOMAIN-SUFFIX,aliyuncs.com,DIRECT
# 国际 CDN：国内可直连，走 gfw 代理反而 4-12s（实测），必须直连
DOMAIN-SUFFIX,jsdelivr.net,DIRECT";

// 内置 gfw 名单（被墙域名，走代理）；允许用外部 config/routes_box/gfwlist.txt 覆盖
const BUILTIN_GFWLIST: &str = include_str!("../routes/gfwlist.txt");

// 规则文件路径：config/rules.txt（用户自定义追加规则）
fn rules_file() -> PathBuf {
    app_base().join("rules.txt")
}

// 读取 gfw 名单：优先外部 config/routes_box/gfwlist.txt；外部不存在或为空时用内置
fn load_gfwlist() -> Vec<String> {
    let ext = app_base().join("routes_box").join("gfwlist.txt");
    let content = match std::fs::read_to_string(&ext) {
        Ok(s) if !s.trim().is_empty() => s,
        _ => BUILTIN_GFWLIST.to_string(),
    };
    content
        .lines()
        .map(|l| l.trim())
        .filter(|l| {
            !l.is_empty() && !l.starts_with('#') && !l.starts_with('!') && !l.starts_with('[')
        })
        .map(|l| l.to_string())
        .collect()
}

// 加载用户自定义追加规则：优先读取 rules.txt；不存在则写入默认规则
fn load_user_rules() -> String {
    let fp = rules_file();
    if let Ok(content) = std::fs::read_to_string(&fp) {
        if !content.trim().is_empty() {
            return content.trim().to_string();
        }
    }
    let _ = std::fs::create_dir_all(app_base());
    let _ = std::fs::write(&fp, DEFAULT_RULES);
    DEFAULT_RULES.to_string()
}

// ---------- CF 优选 IP ----------
// 优选 IP 文件路径：config/opt_ip.txt
fn opt_ips_file() -> PathBuf {
    app_base().join("opt_ip.txt")
}

// 纯函数：逐行 trim、跳过空行和 # 注释、支持逗号分隔多 IP、
// 仅保留合法 IP、去重、最多保留 5 个
fn parse_opt_ips(content: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in content.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        for part in l.split(',') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            if p.parse::<std::net::IpAddr>().is_ok() && seen.insert(p.to_string()) {
                out.push(p.to_string());
                if out.len() >= 5 {
                    return out;
                }
            }
        }
    }
    out
}

// 读取优选 IP 列表（不存在或读取失败时返回空）
#[tauri::command]
pub fn load_opt_ips() -> Vec<String> {
    match std::fs::read_to_string(opt_ips_file()) {
        Ok(c) => parse_opt_ips(&c),
        Err(_) => Vec::new(),
    }
}

// 保存优选 IP 列表
#[tauri::command]
pub fn save_opt_ips(content: String) -> Result<serde_json::Value, String> {
    let _ = std::fs::create_dir_all(app_base());
    std::fs::write(opt_ips_file(), content).map_err(|e| format!("写 opt_ip.txt 失败: {}", e))?;
    Ok(json!({ "ok": true }))
}

// 为可优选节点（ws + tls 的 vless/vmess/trojan）按每个 IP 克隆一份变体；
// 其余节点（ss/socks/http、非 ws、ws 无 tls）原样保留
fn apply_opt_ips(nodes: Vec<Node>, ips: &[String]) -> Vec<Node> {
    if ips.is_empty() {
        return nodes;
    }
    let mut out = Vec::new();
    for n in nodes {
        let is_ws = n.network.as_deref() == Some("ws");
        let has_tls = n.security.as_deref() == Some("tls") || n.tls.as_deref() == Some("tls");
        let optable = is_ws
            && has_tls
            && (n.r#type == "vless" || n.r#type == "vmess" || n.r#type == "trojan");
        if optable {
            for ip in ips {
                let mut m = n.clone();
                m.server = ip.clone();
                m.name = format!("{}@{}", n.name, ip);
                out.push(m);
            }
        } else {
            out.push(n);
        }
    }
    out
}

// 生成完整路由规则（旧项目 gfw 策略）：私有/国内直连 + gfw 名单走代理 + 其余直连
fn build_rules() -> String {
    build_rules_from(&load_user_rules(), &load_gfwlist())
}

// 纯函数（便于单测）：用户规则（跳过 MATCH 行，由系统统一生成）+ gfw 走代理 + 其余直连
fn build_rules_from(user_rules: &str, gfw: &[String]) -> String {
    let mut rules: Vec<String> = Vec::new();
    // 1) 用户自定义追加规则（含私有地址直连 + 国内直连）；跳过 MATCH 行
    for line in user_rules.lines() {
        let l = line.trim();
        if !l.is_empty() && !l.starts_with('#') && !l.starts_with("MATCH") {
            rules.push(l.to_string());
        }
    }
    // 2) gfw 名单 → 走代理（mihomo 的 DOMAIN-SUFFIX 不支持单条多域名，必须一条一域名）
    for d in gfw {
        rules.push(format!("DOMAIN-SUFFIX,{},auto", d));
    }
    // 3) 其余全部直连
    rules.push("MATCH,DIRECT".to_string());
    rules.join("\n")
}

fn build_config(nodes: &[Node], mixed_port: u16, ctrl_port: u16) -> Option<String> {
    let mut outbounds = Vec::new();
    let mut names = Vec::new();
    for n in nodes {
        if let Some(y) = outbound_yaml(n) {
            outbounds.push(y);
            names.push(yaml_str(&n.name));
        }
    }
    if outbounds.is_empty() {
        return None;
    }
    let rules_yaml = build_rules()
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| format!("  - {}", l))
        .collect::<Vec<_>>()
        .join("\n");
    let cfg = format!(
        "mixed-port: {}\nallow-lan: false\nmode: rule\nlog-level: warning\nlog-file: history.log\nexternal-controller: 127.0.0.1:{}\ndns:\n  enable: true\n  enhanced-mode: redir-host\n  nameserver:\n    - 223.5.5.5\n    - 119.29.29.29\n  fallback:\n    - 8.8.8.8\n    - 1.1.1.1\n  fallback-filter:\n    geoip: false\nproxies:\n{}\nproxy-groups:\n  - name: auto\n    type: url-test\n    url: http://www.gstatic.com/generate_204\n    interval: 60\n    tolerance: 300\n    proxies:\n{}\nrules:\n{}",
        mixed_port, ctrl_port,
        outbounds.join("\n"),
        names.iter().map(|n| format!("      - {}", n)).collect::<Vec<_>>().join("\n"),
        rules_yaml,
    );
    Some(cfg)
}

// ---------- 校验与启动 ----------
// mihomo -t 校验配置；本机线程资源紧张时 mihomo 可能挂起而非崩溃，加超时防止连接永久卡住
const VALIDATE_TIMEOUT: Duration = Duration::from_secs(10);

fn validate_config(core: &Path, temp: &Path, config: &Path) -> bool {
    let Ok(mut child) = hide_window(
        Command::new(core).args([
            "-t",
            "-d",
            temp.to_str().unwrap_or(""),
            "-f",
            config.to_str().unwrap_or(""),
        ]),
    )
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    else {
        return false;
    };
    let deadline = std::time::Instant::now() + VALIDATE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    // 超时：按校验失败处理（拒绝启动），并杀掉挂起的 mihomo -t
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return false,
        }
    }
}

fn spawn_core(st: &CoreState) -> Result<Child, String> {
    hide_window(
        Command::new(&st.core_bin).args([
            "-d",
            st.temp_dir.to_str().unwrap_or(""),
            "-f",
            st.config_file.to_str().unwrap_or(""),
        ]),
    )
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| format!("启动内核失败: {}", e))
}

fn start_core_locked(st: &mut CoreState) -> Result<(), String> {
    if !st.core_bin.exists() {
        return Err(format!("未找到内核: {}", st.core_bin.display()));
    }
    // 生成配置
    let cfg = match build_config(&st.nodes, st.mixed_port, st.ctrl_port) {
        Some(c) => c,
        None => {
            st.status.last_error = "无有效节点".into();
            return Err("无有效节点".into());
        }
    };
    std::fs::create_dir_all(&st.temp_dir).map_err(|e| e.to_string())?;
    let mut f = std::fs::File::create(&st.config_file).map_err(|e| e.to_string())?;
    f.write_all(cfg.as_bytes()).map_err(|e| e.to_string())?;
    // 校验
    if !validate_config(&st.core_bin, &st.temp_dir, &st.config_file) {
        st.status.last_error = "配置校验失败，已拒绝启动".into();
        return Err("配置校验失败，已拒绝启动".into());
    }
    // 启动
    let child = spawn_core(st)?;
    st.intentional_stop = false;
    st.status.running = true;
    st.status.retries = 0;
    st.status.last_error.clear();
    st.core_pid = Some(child.id());
    st.generation += 1;
    let gen = st.generation;
    // 监控线程：等待退出 → 自动重启
    std::thread::spawn(move || {
        let _ = wait_child(child);
        // 退出后检查是否需要重启
        let mut s = state().lock().unwrap();
        // 主动停止 或 已被新一轮启动接管（generation 变化）→ 直接退出
        if s.intentional_stop || s.generation != gen {
            // 仅当仍是当前代（主动停止且未被新内核接管）才清运行状态；
            // generation 已变 = 新内核已接管，绝不能改 running（否则会误报停止，
            // 旧监控线程醒来时会把新内核的运行状态清掉）
            if s.generation == gen {
                s.status.running = false;
                s.core_pid = None;
            }
            return;
        }
        s.status.running = false;
        s.core_pid = None;
        if s.status.retries < MAX_RETRIES {
            s.status.retries += 1;
            let delay_ms = 1000u64 * (1u64 << (s.status.retries - 1)); // 1s, 2s, 4s
            drop(s);
            std::thread::sleep(Duration::from_millis(delay_ms));
            let mut s2 = state().lock().unwrap();
            if !s2.intentional_stop && s2.generation == gen {
                s2.status.last_error =
                    format!("内核意外退出，自动重启（第 {} 次）", s2.status.retries);
                let _ = start_core_locked(&mut s2);
            }
        } else {
            s.status.last_error = "内核连续退出，已停止".into();
            // 放弃重启：内核已不再服务，还原系统代理，避免残留死代理导致网页访问异常
            if s.proxy_enabled {
                crate::proxy::set_system_proxy_inner(false, 0);
                s.proxy_enabled = false;
            }
        }
    });
    Ok(())
}

fn wait_child(mut child: Child) -> std::io::Result<std::process::ExitStatus> {
    child.wait()
}

// 查询当前节点 + 延迟（external-controller）
// history 为空（url-test 定时测速未产出数据）时主动触发一次测速，让延迟能显示
fn query_node_status() {
    let (running, ctrl_port) = {
        let s = state().lock().unwrap();
        (s.status.running, s.ctrl_port)
    };
    if !running {
        return;
    }
    let base = format!("http://127.0.0.1:{}", ctrl_port);
    let auto_url = format!("{}/proxies/auto", base);
    if let Ok(resp) = ureq::get(&auto_url).timeout(Duration::from_secs(2)).call() {
        if let Ok(text) = resp.into_string() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                let mut s = state().lock().unwrap();
                s.status.node = v
                    .get("now")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let has_hist = v
                    .get("history")
                    .and_then(|x| x.as_array())
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);
                if has_hist {
                    if let Some(last) = v
                        .get("history")
                        .and_then(|x| x.as_array())
                        .and_then(|a| a.last())
                    {
                        s.status.latency = last.get("delay").and_then(|x| x.as_u64()).unwrap_or(0);
                    }
                } else {
                    s.status.latency = 0;
                }
                drop(s);
                // history 为空 → 主动触发一次测速（节流：距上次至少 5s）
                if !has_hist {
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let last_probe = state().lock().unwrap().last_probe_ms;
                    if now_ms.saturating_sub(last_probe) >= 5000 {
                        state().lock().unwrap().last_probe_ms = now_ms;
                        let delay_url = format!("{}/proxies/auto/delay?url=http://www.gstatic.com/generate_204&timeout=5000", base);
                        if let Ok(dresp) =
                            ureq::get(&delay_url).timeout(Duration::from_secs(7)).call()
                        {
                            if let Ok(dtext) = dresp.into_string() {
                                if let Ok(dv) = serde_json::from_str::<serde_json::Value>(&dtext) {
                                    let mut st = state().lock().unwrap();
                                    if let Some(d) = dv.get("delay").and_then(|x| x.as_u64()) {
                                        st.status.latency = d;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // 累计流量（内核启动以来）：/connections 返回 downloadTotal/uploadTotal
    let conn_url = format!("{}/connections", base);
    if let Ok(resp) = ureq::get(&conn_url).timeout(Duration::from_secs(2)).call() {
        if let Ok(text) = resp.into_string() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                let mut s = state().lock().unwrap();
                s.status.download_total =
                    v.get("downloadTotal").and_then(|x| x.as_u64()).unwrap_or(0);
                s.status.upload_total = v.get("uploadTotal").and_then(|x| x.as_u64()).unwrap_or(0);
            }
        }
    }
}

// ---------- 命令 ----------
#[tauri::command]
pub fn connect(subscription: String) -> Result<serde_json::Value, String> {
    // 拉取订阅
    let text = if subscription.starts_with("http://") || subscription.starts_with("https://") {
        crate::subscribe::fetch_url_pub(&subscription)?
    } else {
        subscription.clone()
    };
    let nodes = apply_opt_ips(parse_subscription(&text), &load_opt_ips());
    if nodes.is_empty() {
        return Err("解析不到任何节点，请检查订阅链接".into());
    }
    let mut st = state().lock().unwrap();
    st.sub_source = subscription.clone();
    // 停掉旧内核（若有），避免旧进程泄漏继续占用端口
    if let Some(pid) = stop_kernel(&mut st) {
        wait_pid_exit(pid, Duration::from_secs(3));
    }
    st.nodes = nodes;
    st.mixed_port = pick_port(7890);
    st.ctrl_port = pick_port(9090);
    st.temp_dir = app_base().join("temp");
    st.config_file = st.temp_dir.join("config.yaml");
    st.core_bin = find_core()
        .ok_or(format!("未找到 {}，请将其放在程序目录", CORE_NAME))?;
    let count = st.nodes.len();
    let mixed = st.mixed_port;
    start_core_locked(&mut st)?;
    // 开启系统代理
    crate::proxy::set_system_proxy_inner(true, mixed);
    st.proxy_enabled = true;
    // 启动订阅自动刷新后台线程（disconnect 时置停）
    let stop = Arc::new(AtomicBool::new(false));
    st.refresh_stop = Some(stop.clone());
    drop(st);
    std::thread::spawn(move || refresh_loop(stop));
    Ok(json!({ "ok": true, "nodes": count, "mixedPort": mixed }))
}

#[tauri::command]
pub fn disconnect() -> Result<serde_json::Value, String> {
    let mut st = state().lock().unwrap();
    // 停止订阅自动刷新线程
    if let Some(s) = st.refresh_stop.take() {
        s.store(true, Ordering::Relaxed);
    }
    // 真正杀掉内核进程（taskkill /F /PID），监控线程 wait 返回后见 intentional_stop 即退出
    stop_kernel(&mut st);
    // 只还原本程序开启的系统代理（避免误关其他代理工具的设置）
    if st.proxy_enabled {
        crate::proxy::set_system_proxy_inner(false, 0);
        st.proxy_enabled = false;
    }
    Ok(json!({ "ok": true }))
}

// 全节点失效检测：/proxies/auto 的 all 对象非空且所有延迟均为 0（超时）才算全挂；
// 任何请求/解析错误一律返回 false，避免瞬时故障误触发
fn all_nodes_dead() -> bool {
    let (running, ctrl_port) = {
        let s = state().lock().unwrap();
        (s.status.running, s.ctrl_port)
    };
    if !running {
        return false;
    }
    let url = format!("http://127.0.0.1:{}/proxies/auto", ctrl_port);
    let Ok(resp) = ureq::get(&url).timeout(Duration::from_secs(2)).call() else {
        return false;
    };
    let Ok(text) = resp.into_string() else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let all = match v.get("all").and_then(|x| x.as_object()) {
        Some(o) if !o.is_empty() => o,
        _ => return false,
    };
    all.values().all(|x| x.as_u64().unwrap_or(1) == 0)
}

// 订阅自动刷新：重拉订阅并按当前优选 IP 重建节点后重启内核（仿 save_rules 重启路径）
fn do_refresh() {
    // 持锁快照，避免拉取期间长期占用锁
    let (sub_source, _mixed_port, _ctrl_port, _temp_dir, _config_file, _core_bin) = {
        let s = state().lock().unwrap();
        (
            s.sub_source.clone(),
            s.mixed_port,
            s.ctrl_port,
            s.temp_dir.clone(),
            s.config_file.clone(),
            s.core_bin.clone(),
        )
    };
    // 自动刷新只对 URL 订阅有意义
    if sub_source.is_empty()
        || !(sub_source.starts_with("http://") || sub_source.starts_with("https://"))
    {
        return;
    }
    let text = match crate::subscribe::fetch_url_pub(&sub_source) {
        Ok(t) => t,
        Err(_) => {
            state().lock().unwrap().status.last_error = "订阅自动刷新失败，沿用旧节点".into();
            return;
        }
    };
    let mut nodes = parse_subscription(&text);
    if nodes.is_empty() {
        state().lock().unwrap().status.last_error = "订阅刷新：解析不到节点，沿用旧配置".into();
        return;
    }
    nodes = apply_opt_ips(nodes, &load_opt_ips());
    // 重新持锁：期间可能已断开，直接返回
    let mut st = state().lock().unwrap();
    if !st.status.running || st.intentional_stop {
        return;
    }
    if let Some(pid) = stop_kernel(&mut st) {
        wait_pid_exit(pid, Duration::from_secs(3));
    }
    st.nodes = nodes;
    if let Err(e) = start_core_locked(&mut st) {
        st.status.last_error = e;
        return;
    }
    crate::proxy::set_system_proxy_inner(true, st.mixed_port);
    st.proxy_enabled = true;
}

// 后台线程：周期检查全挂与定时刷新；disconnect 置 stop 后退出
fn refresh_loop(stop: Arc<AtomicBool>) {
    let mut last_refresh = std::time::Instant::now();
    let mut all_dead_count: u32 = 0;
    loop {
        std::thread::sleep(ALL_DEAD_INTERVAL);
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if all_nodes_dead() {
            all_dead_count += 1;
            if all_dead_count >= ALL_DEAD_THRESHOLD {
                all_dead_count = 0;
                if last_refresh.elapsed() >= REFRESH_COOLDOWN {
                    last_refresh = std::time::Instant::now();
                    do_refresh();
                }
            }
        } else {
            all_dead_count = 0;
        }
        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            last_refresh = std::time::Instant::now();
            do_refresh();
        }
    }
}

// 当前运行中内核的 mixed 端口（供订阅拉取经本地代理重试）
pub fn running_proxy_port() -> Option<u16> {
    let s = state().lock().unwrap();
    if s.status.running {
        Some(s.mixed_port)
    } else {
        None
    }
}

#[tauri::command]
pub fn status() -> Result<CoreStatus, String> {
    query_node_status();
    let s = state().lock().unwrap();
    Ok(s.status.clone())
}

#[tauri::command]
pub fn get_rules() -> Result<String, String> {
    Ok(load_user_rules())
}

// 读取内核日志尾部（mihomo 的 log-file：config/temp/history.log）；limit 为最大行数
#[tauri::command]
pub fn get_logs(limit: Option<usize>) -> Result<String, String> {
    let log_file = {
        let st = state().lock().unwrap();
        st.temp_dir.join("history.log")
    };
    let limit = limit.unwrap_or(100);
    let content = match std::fs::read_to_string(&log_file) {
        Ok(c) => c,
        Err(_) => return Ok(String::new()), // 未连接或无日志
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(limit);
    Ok(lines[start..].join("\n"))
}

// 返回完整生效规则（用户自定义 + gfw 代理清单 + MATCH 兜底），供界面只读展示
#[tauri::command]
pub fn get_effective_rules() -> Result<serde_json::Value, String> {
    let all = build_rules();
    let gfw_count = load_gfwlist().len();
    Ok(json!({ "rules": all, "gfw_count": gfw_count }))
}

// 保存规则到 rules.txt；restart=true 且内核运行中时重启内核使新规则生效
#[tauri::command]
pub fn save_rules(content: String, restart: bool) -> Result<serde_json::Value, String> {
    let _ = std::fs::create_dir_all(app_base());
    std::fs::write(rules_file(), content).map_err(|e| format!("写规则文件失败: {}", e))?;
    let mut st = state().lock().unwrap();
    let was_running = st.status.running;
    if restart && was_running {
        // 停旧内核（让监控线程退出），等进程真正退出后再用新规则重启
        if let Some(pid) = stop_kernel(&mut st) {
            wait_pid_exit(pid, Duration::from_secs(3));
        }
        drop(st);
        let mut st = state().lock().unwrap();
        start_core_locked(&mut st)?;
        crate::proxy::set_system_proxy_inner(true, st.mixed_port);
        st.proxy_enabled = true;
    }
    Ok(json!({ "ok": true, "restarted": restart && was_running }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ss_node() -> Node {
        Node {
            r#type: "ss".into(),
            name: "s1".into(),
            server: "1.2.3.4".into(),
            port: 8388,
            method: Some("aes-256-gcm".into()),
            password: Some("pw".into()),
            ..Default::default()
        }
    }

    #[test]
    fn yaml_str_plain_vs_quoted() {
        assert_eq!(yaml_str("abc-123.def"), "abc-123.def");
        assert_eq!(yaml_str("含中文:节点"), "\"含中文:节点\"");
        assert_eq!(yaml_str("a b"), "\"a b\"");
    }

    #[test]
    fn outbound_ss_plain() {
        let y = outbound_yaml(&ss_node()).expect("ss 应生成");
        assert!(y.contains("type: ss"));
        assert!(y.contains("server: 1.2.3.4"));
        assert!(y.contains("port: 8388"));
        assert!(y.contains("cipher: aes-256-gcm"));
        assert!(!y.contains("plugin"));
    }

    #[test]
    fn outbound_ss_with_plugin() {
        let mut n = ss_node();
        n.plugin = Some("obfs".into());
        n.plugin_opts = Some("      mode: http\n      host: cdn.example.com".into());
        let y = outbound_yaml(&n).expect("应生成");
        assert!(y.contains("plugin: obfs"));
        assert!(y.contains("plugin-opts:"));
        assert!(y.contains("mode: http"));
        assert!(y.contains("host: cdn.example.com"));
    }

    #[test]
    fn outbound_vmess_ws_tls() {
        let n = Node {
            r#type: "vmess".into(),
            name: "v1".into(),
            server: "5.6.7.8".into(),
            port: 443,
            uuid: Some("u1".into()),
            aid: Some(0),
            security: Some("auto".into()),
            tls: Some("tls".into()),
            network: Some("ws".into()),
            host: Some("h.com".into()),
            path: Some("/p".into()),
            sni: Some("h.com".into()),
            ..Default::default()
        };
        let y = outbound_yaml(&n).expect("应生成");
        assert!(y.contains("tls: true"));
        assert!(y.contains("network: ws"));
        assert!(y.contains("ws-opts:"));
        assert!(y.contains("Host: h.com"));
    }

    #[test]
    fn outbound_vless_reality() {
        let n = Node {
            r#type: "vless".into(),
            name: "r1".into(),
            server: "9.9.9.9".into(),
            port: 443,
            uuid: Some("u2".into()),
            security: Some("reality".into()),
            sni: Some("real.com".into()),
            reality_pbk: Some("pbk".into()),
            reality_sid: Some("sid".into()),
            reality_fp: Some("chrome".into()),
            ..Default::default()
        };
        let y = outbound_yaml(&n).expect("应生成");
        assert!(y.contains("reality-opts:"));
        assert!(y.contains("public-key: pbk"));
        assert!(y.contains("short-id: sid"));
        assert!(y.contains("client-fingerprint: chrome"));
    }

    #[test]
    fn outbound_trojan_ws() {
        let n = Node {
            r#type: "trojan".into(),
            name: "t1".into(),
            server: "t.com".into(),
            port: 443,
            password: Some("pw".into()),
            network: Some("ws".into()),
            path: Some("/w".into()),
            host: Some("h.com".into()),
            ..Default::default()
        };
        let y = outbound_yaml(&n).expect("应生成");
        assert!(y.contains("network: ws"));
        assert!(y.contains("path: /w"));
    }

    #[test]
    fn outbound_socks_auth() {
        let n = Node {
            r#type: "socks".into(),
            name: "k1".into(),
            server: "3.3.3.3".into(),
            port: 1080,
            username: Some("u".into()),
            password: Some("p".into()),
            ..Default::default()
        };
        let y = outbound_yaml(&n).expect("应生成");
        assert!(y.contains("username: u"));
        assert!(y.contains("password: p"));
    }

    #[test]
    fn outbound_unknown_type_rejected() {
        let n = Node {
            r#type: "unknown".into(),
            ..Default::default()
        };
        assert!(outbound_yaml(&n).is_none());
    }

    #[test]
    fn build_rules_structure() {
        let user =
            "# 注释\nIP-CIDR,127.0.0.0/8,DIRECT\nMATCH,PROXY\nDOMAIN-SUFFIX,baidu.com,DIRECT";
        let gfw = vec!["example.com".to_string(), "blocked.org".to_string()];
        let rules = build_rules_from(user, &gfw);
        let lines: Vec<&str> = rules.lines().collect();
        assert!(lines.contains(&"IP-CIDR,127.0.0.0/8,DIRECT"));
        assert!(lines.contains(&"DOMAIN-SUFFIX,baidu.com,DIRECT"));
        assert!(
            !rules.contains("MATCH,PROXY"),
            "用户 MATCH 行应被跳过（系统统一生成）"
        );
        assert!(lines.contains(&"DOMAIN-SUFFIX,example.com,auto"));
        assert!(lines.contains(&"DOMAIN-SUFFIX,blocked.org,auto"));
        assert_eq!(
            lines.last(),
            Some(&"MATCH,DIRECT"),
            "最后一行应为默认直连兜底"
        );
    }

    #[test]
    fn gfwlist_builtin_fallback_nonempty() {
        // 外部覆盖不存在时回退内置名单（内置 4022 条）
        let g = load_gfwlist();
        assert!(g.len() >= 4000, "内置 gfw 名单应完整，实际 {}", g.len());
        assert!(g.iter().all(|d| !d.starts_with("DOMAIN")), "名单应为裸域名");
    }

    #[test]
    fn apply_opt_ips_expands_ws_tls_only() {
        // ws+tls 的 vless 节点配 2 个 IP 应生成 2 个 @ip 后缀变体；ss 节点保持不变
        let ips = vec!["1.1.1.1".to_string(), "2.2.2.2".to_string()];
        let vless = Node {
            r#type: "vless".into(),
            name: "v1".into(),
            server: "orig.example.com".into(),
            port: 443,
            uuid: Some("u1".into()),
            security: Some("tls".into()),
            network: Some("ws".into()),
            ..Default::default()
        };
        let out = apply_opt_ips(vec![vless.clone(), ss_node()], &ips);
        // vless 展开为 2 个 + ss 原样 1 个
        assert_eq!(out.len(), 3);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"v1@1.1.1.1"));
        assert!(names.contains(&"v1@2.2.2.2"));
        assert!(names.contains(&"s1"));
        // server 被替换为优选 IP；ss 节点保持原样
        let v_ip: Vec<&str> = out
            .iter()
            .filter(|n| n.name.starts_with("v1@"))
            .map(|n| n.server.as_str())
            .collect();
        assert!(v_ip.contains(&"1.1.1.1"));
        assert!(v_ip.contains(&"2.2.2.2"));
        let ss = out.iter().find(|n| n.name == "s1").unwrap();
        assert_eq!(ss.server, "1.2.3.4");
        // ws 无 tls / 非 ws 不展开
        let ws_no_tls = Node {
            r#type: "vmess".into(),
            name: "nt".into(),
            server: "x.com".into(),
            port: 443,
            uuid: Some("u2".into()),
            network: Some("ws".into()),
            security: Some("auto".into()),
            ..Default::default()
        };
        let no_ws = Node {
            r#type: "trojan".into(),
            name: "nw".into(),
            server: "y.com".into(),
            port: 443,
            password: Some("pw".into()),
            security: Some("tls".into()),
            ..Default::default()
        };
        let out2 = apply_opt_ips(vec![ws_no_tls, no_ws], &ips);
        assert_eq!(out2.len(), 2);
        assert!(out2.iter().all(|n| !n.name.contains('@')));
        // ips 为空 → 原样返回
        let out3 = apply_opt_ips(vec![vless], &[]);
        assert_eq!(out3.len(), 1);
        assert_eq!(out3[0].name, "v1");
        assert_eq!(out3[0].server, "orig.example.com");
    }

    #[test]
    fn parse_opt_ips_filters_invalid() {
        // 合法 IP 保留、非法/注释/空行剔除、去重、超 5 个截断、逗号分隔解析
        let content = "# 注释\n8.8.8.8\n\n999.1.1.1\n\n1.1.1.1\n\n3.3.3.3, 4.4.4.4\n\nnot-an-ip\n\n5.5.5.5\n\n6.6.6.6\n";
        let ips = parse_opt_ips(content);
        assert_eq!(
            ips,
            vec!["8.8.8.8", "1.1.1.1", "3.3.3.3", "4.4.4.4", "5.5.5.5"]
        );
        assert_eq!(ips.len(), 5);
        // 空内容 / 纯注释
        assert!(parse_opt_ips("").is_empty());
        assert!(parse_opt_ips("# 只有注释\n\n").is_empty());
    }
}
