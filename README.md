# Cat CC2

极简 Windows 代理工具：**订阅一键连接**。Rust + Tauri 2 外壳 + mihomo 内核。

## 功能

- 粘贴订阅链接一键连接（自动识别 base64 节点列表 / Clash YAML / 6 种 share link）
- 自动选择最快节点（`url-test` 代理组）+ 故障自动切换
- gfw 路由策略：私有 IP 与国内常用域名直连，gfw 名单走代理，其余默认直连
- 自定义规则编辑，保存后自动重启内核立即生效
- 内核意外退出自动重启（最多 3 次，指数退避 1s/2s/4s）
- 关闭窗口最小化到托盘，托盘退出 = 停内核 + 还原系统代理

## 快速开始

1. 将 `mihomo-core.exe`（Clash Meta 内核，windows-amd64-compatible）放到程序目录或项目根目录。
   > 注意：命名为 `mihomo.exe` 可能被部分杀软拦截导致启动失败，请保留 `mihomo-core.exe` 命名。
2. 运行 `src-tauri/target/release/cat-cc2.exe`（release 构建无终端窗口）。
3. 粘贴订阅链接（URL 或直接粘贴节点内容）→ 一键连接。

## 路由策略

- 用户自定义规则（`config/rules.txt`：私有 IP 直连 + 国内常用域名直连，可编辑）
- gfw 名单（内置 `src-tauri/routes/gfwlist.txt`，4022 条；可用 `config/routes_box/gfwlist.txt` 覆盖）→ **全部走代理**
- `MATCH,DIRECT` → **其余默认直连**

## 开发

```bash
cargo check                                # 语法检查
cargo test --lib -j 2                       # 单元测试
CARGO_BUILD_JOBS=2 cargo test --test full_flow -j 2   # 全流程集成测试（connect→保存规则重启→disconnect）
CARGO_BUILD_JOBS=2 cargo build --release -j 2          # release 构建（低并行，防线程资源耗尽 0xc0000409）
```

## 目录结构

```
src-tauri/src/
  subscribe.rs   订阅拉取/解析（share link、Clash YAML、base64、SIP002 plugin）
  core_mgr.rs    mihomo 内核管理：配置生成、启动校验、进程监控、connect/disconnect
  proxy.rs       Windows 系统代理（注册表 + InternetSetOption 刷新）
  lib.rs         Tauri 入口、托盘、窗口事件
src-tauri/routes/   内置 gfw 名单（可被外部覆盖）
dist/index.html     前端界面（单页，无构建流程）
config/             运行时配置目录（rules.txt 用户规则、routes_box 覆盖名单、temp/ 临时配置）
```

## 已知限制

- 仅支持 6 种协议：vmess / ss（含 SIP002 plugin：obfs、v2ray-plugin）/ trojan / vless / socks / http
- gfw 名单走代理，其余默认直连（`MATCH,DIRECT`）
- 订阅拉取直连失败时会经本地代理自动重试；未连接时若订阅源被墙，可把订阅内容直接粘贴到输入框
