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
