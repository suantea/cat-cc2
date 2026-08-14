// Windows 系统代理开关（注册表 + InternetSetOption 刷新）
use std::process::Command;

const PROXY_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";

// wininet 刷新通知：注册表写入后必须调用，否则系统代理不生效
#[link(name = "wininet")]
extern "system" {
    fn InternetSetOptionW(
        h: *const std::ffi::c_void,
        option: u32,
        buffer: *const std::ffi::c_void,
        len: u32,
    ) -> i32;
}
const INTERNET_OPTION_SETTINGS_CHANGED: u32 = 39;
const INTERNET_OPTION_REFRESH: u32 = 37;

fn notify_sys_proxy_changed() {
    unsafe {
        InternetSetOptionW(
            std::ptr::null(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null(),
            0,
        );
        InternetSetOptionW(
            std::ptr::null(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null(),
            0,
        );
    }
}

// 内部实现（供 core_mgr 调用）：enable=true 时用 port 设置本地代理
pub fn set_system_proxy_inner(enable: bool, port: u16) {
    if std::env::consts::OS != "windows" {
        return;
    }
    let _ = Command::new("reg")
        .args([
            "add",
            PROXY_KEY,
            "/v",
            "ProxyEnable",
            "/t",
            "REG_DWORD",
            "/d",
            if enable { "1" } else { "0" },
            "/f",
        ])
        .output();
    if enable && port > 0 {
        let _ = Command::new("reg")
            .args([
                "add",
                PROXY_KEY,
                "/v",
                "ProxyServer",
                "/t",
                "REG_SZ",
                "/d",
                &format!("127.0.0.1:{}", port),
                "/f",
            ])
            .output();
    }
    // 通知系统刷新，立即生效
    notify_sys_proxy_changed();
}

#[tauri::command]
pub fn set_system_proxy(enable: bool, port: Option<u16>) -> Result<serde_json::Value, String> {
    set_system_proxy_inner(enable, port.unwrap_or(0));
    Ok(serde_json::json!({ "ok": true }))
}

// ---------- 异常退出残留代理自愈 ----------

// 解析本地代理端口：仅接受 127.0.0.1/localhost:PORT 形式；远程代理返回 None（绝不动用户手动代理）
fn parse_local_proxy_port(server: &str) -> Option<u16> {
    let (host, port_str) = server.rsplit_once(':')?;
    let host = host.trim();
    if host != "127.0.0.1" && host != "localhost" {
        return None;
    }
    port_str.trim().parse().ok().filter(|p| *p > 0)
}

// 解析 reg query 值行：形如 `    ProxyServer    REG_SZ    127.0.0.1:7890`
// 本地化无关：找含 REG_ 类型关键字的行，取最后一个 token 作为值
fn parse_reg_value_line(line: &str) -> Option<String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.iter().any(|t| t.starts_with("REG_")) {
        tokens.last().map(|v| v.to_string())
    } else {
        None
    }
}

fn reg_query_value(name: &str) -> Option<String> {
    let out = Command::new("reg")
        .args(["query", PROXY_KEY, "/v", name])
        .output()
        .ok()?;
    if !out.status.success() {
        return None; // 键或值不存在
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find_map(parse_reg_value_line)
}

// 启动自愈：系统代理指向本地端口但该端口无进程监听（上次异常退出/崩溃的残留）→ 还原代理，
// 防止网页访问异常。端口有监听的（其他代理工具或存活的孤儿内核）一律不动；远程代理不动。
pub fn restore_stale_proxy_if_dead() -> bool {
    if std::env::consts::OS != "windows" {
        return false;
    }
    let enabled = reg_query_value("ProxyEnable")
        .map(|v| v == "0x1")
        .unwrap_or(false);
    if !enabled {
        return false;
    }
    let server = reg_query_value("ProxyServer").unwrap_or_default();
    let Some(port) = parse_local_proxy_port(&server) else {
        return false; // 远程代理或格式异常 → 不动
    };
    // 端口可绑定 = 无监听 = 代理已死 → 还原；有监听则代理仍可用 → 不动
    if std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
        return false;
    }
    set_system_proxy_inner(false, 0);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_local_proxy_port_accepts_localhost() {
        assert_eq!(parse_local_proxy_port("127.0.0.1:7890"), Some(7890));
        assert_eq!(parse_local_proxy_port("localhost:8080"), Some(8080));
    }

    #[test]
    fn parse_local_proxy_port_rejects_remote_or_malformed() {
        assert_eq!(parse_local_proxy_port("10.0.0.5:8080"), None); // 远程代理不动
        assert_eq!(parse_local_proxy_port("proxy.corp.com:8080"), None);
        assert_eq!(parse_local_proxy_port("127.0.0.1:abc"), None);
        assert_eq!(parse_local_proxy_port("127.0.0.1"), None);
        assert_eq!(parse_local_proxy_port(""), None);
    }

    #[test]
    fn parse_reg_value_line_extracts_value() {
        // reg query 输出行（本地化无关）：类型关键字后最后一个 token 即值
        assert_eq!(
            parse_reg_value_line("    ProxyServer    REG_SZ    127.0.0.1:7890"),
            Some("127.0.0.1:7890".to_string())
        );
        assert_eq!(
            parse_reg_value_line("    ProxyEnable    REG_DWORD    0x1"),
            Some("0x1".to_string())
        );
        assert_eq!(
            parse_reg_value_line("    HKEY_CURRENT_USER\\Software"),
            None
        );
        assert_eq!(parse_reg_value_line(""), None);
    }
}
