pub mod core_mgr;
mod proxy;
mod subscribe;

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, WindowEvent,
};

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // 关闭窗口 = 最小化到托盘（隐藏），不退出程序；退出走托盘菜单
            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }
            // 托盘
            let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                });
            // 使用默认窗口图标（icon.ico）
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            let _tray = tray.build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            subscribe::parse_sub,
            core_mgr::connect,
            core_mgr::disconnect,
            core_mgr::status,
            core_mgr::get_rules,
            core_mgr::get_effective_rules,
            core_mgr::save_rules,
            proxy::set_system_proxy,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
