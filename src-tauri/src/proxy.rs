// 系统代理开关：Windows 用注册表 + InternetSetOption 刷新；macOS 用 networksetup（失败自动降级 osascript 提权）

// ---------- Windows 实现（注册表 + wininet 刷新） ----------

#[cfg(target_os = "windows")]
mod win {
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

    pub fn set(enable: bool, port: u16) {
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

    // 解析本地代理端口：仅接受 127.0.0.1/localhost:PORT 形式；远程代理返回 None（绝不动用户手动代理）
    pub fn parse_local_proxy_port(server: &str) -> Option<u16> {
        let (host, port_str) = server.rsplit_once(':')?;
        let host = host.trim();
        if host != "127.0.0.1" && host != "localhost" {
            return None;
        }
        port_str.trim().parse().ok().filter(|p| *p > 0)
    }

    // 解析 reg query 值行：形如 `    ProxyServer    REG_SZ    127.0.0.1:7890`
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

    // 启动自愈：系统代理指向本地端口但该端口无进程监听（上次异常退出/崩溃的残留）→ 还原代理
    pub fn restore() -> bool {
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
        set(false, 0);
        true
    }
}

// ---------- macOS 实现（networksetup，set 命令需 admin → 自动降级 osascript 提权） ----------

#[cfg(target_os = "macos")]
mod mac {
    use std::process::Command;

    const NETWORKSETUP: &str = "/usr/sbin/networksetup";
    const OSASCRIPT: &str = "/usr/bin/osascript";

    // 探测活动网络服务：遍历服务列表（跳过禁用 `*` 项），取第一个有 IPv4 地址的
    fn active_service() -> Option<String> {
        let out = Command::new(NETWORKSETUP).arg("-listallnetworkservices").output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let svc = line.trim();
            if svc.is_empty()
                || svc.starts_with('*')
                || svc.starts_with("An asterisk")
            {
                continue;
            }
            if let Ok(info) = Command::new(NETWORKSETUP).args(["-getinfo", svc]).output() {
                let info = String::from_utf8_lossy(&info.stdout);
                if info.contains("IP address:") && !info.contains("IP address: none") {
                    return Some(svc.to_string());
                }
            }
        }
        None
    }

    // shell 单引号包裹（服务名可能含空格），内部单引号转义
    fn sq(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    // 提权执行一条 shell 脚本（系统弹授权框，短期记住免重复输入）
    fn run_admin(script: &str) -> bool {
        let full = format!(
            "do shell script \"{}\" with administrator privileges",
            script.replace('"', "\\\"")
        );
        Command::new(OSASCRIPT)
            .args(["-e", &full])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn proxy_on(port: u16) -> bool {
        let Some(svc) = active_service() else {
            return false;
        };
        let q = sq(&svc);
        let script = format!(
            "networksetup -setwebproxy {q} 127.0.0.1 {port} && networksetup -setsecurewebproxy {q} 127.0.0.1 {port} && networksetup -setsocksfirewallproxy {q} 127.0.0.1 {port}",
            q = q,
            port = port
        );
        run_admin(&script)
    }

    fn proxy_off() -> bool {
        let Some(svc) = active_service() else {
            return false;
        };
        let q = sq(&svc);
        let script = format!(
            "networksetup -setwebproxystate {q} off && networksetup -setsecurewebproxystate {q} off && networksetup -setsocksfirewallproxystate {q} off",
            q = q
        );
        run_admin(&script)
    }

    pub fn set(enable: bool, port: u16) {
        if enable && port > 0 {
            proxy_on(port);
        } else {
            proxy_off();
        }
    }

    // 读取活动服务的 web 代理现状：(enabled, server, port)
    fn current_proxy() -> Option<(bool, String, u16)> {
        let svc = active_service()?;
        let out = Command::new(NETWORKSETUP)
            .args(["-getwebproxy", &svc])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut enabled = false;
        let mut server = String::new();
        let mut port = 0u16;
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("Enabled:") {
                enabled = v.trim() == "Yes";
            } else if let Some(v) = line.strip_prefix("Server:") {
                server = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("Port:") {
                port = v.trim().parse().unwrap_or(0);
            }
        }
        Some((enabled, server, port))
    }

    // 启动自愈：系统代理指向本地端口但该端口无进程监听（上次异常退出/崩溃的残留）→ 还原代理
    pub fn restore() -> bool {
        let Some((enabled, server, port)) = current_proxy() else {
            return false;
        };
        if !enabled {
            return false;
        }
        if server != "127.0.0.1" && server != "localhost" {
            return false; // 远程代理 → 不动
        }
        if port == 0 {
            return false;
        }
        // 端口可绑定 = 无监听 = 代理已死 → 还原；有监听则代理仍可用 → 不动
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
            return false;
        }
        proxy_off();
        true
    }

    // macOS 侧无需对外导出（Linux 构建时此模块不存在，避免死代码警告在部分工具链出现）
    #[allow(dead_code)]
    pub fn _unused_guard() {}
}

// ---------- 平台无关入口 ----------

pub fn set_system_proxy_inner(enable: bool, port: u16) {
    #[cfg(target_os = "windows")]
    win::set(enable, port);
    #[cfg(target_os = "macos")]
    mac::set(enable, port);
    // 其他平台（linux 等）：静默无操作
}

#[tauri::command]
pub fn set_system_proxy(enable: bool, port: Option<u16>) -> Result<serde_json::Value, String> {
    set_system_proxy_inner(enable, port.unwrap_or(0));
    Ok(serde_json::json!({ "ok": true }))
}

// 启动自愈：上次异常退出/崩溃残留的死系统代理 → 还原
pub fn restore_stale_proxy_if_dead() -> bool {
    #[cfg(target_os = "windows")]
    {
        win::restore()
    }
    #[cfg(target_os = "macos")]
    {
        mac::restore()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        false
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::win::parse_local_proxy_port;

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
