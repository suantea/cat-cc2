# AGENTS.md — Cat CC2

## Project
极简 Windows 代理工具：**订阅一键连接**。Rust + Tauri 外壳 + mihomo 内核。
设计目标：功能最少、防崩溃第一、避开杀软对 Electron 的拦截（Tauri 用系统 WebView2，无自带 Chromium）。

## 核心流程（一键连接）
1. 用户粘贴订阅链接 → `subscribe.rs` 拉取/解析 → 自动识别格式：
   - base64 节点列表（vmess:// ss:// trojan:// vless:// socks:// http://）→ 逐行解析
   - Clash YAML 订阅 → 提取 `proxies:` 节点列表
2. `core_mgr.rs` 生成 mihomo 配置：mixed 端口 + `url-test` 代理组（自动选最快节点+故障切换）
3. 启动前 `mihomo -t` 校验配置，非法配置拒绝启动
4. 启动内核（`mihomo-core.exe`）→ `proxy.rs` 设置 Windows 系统代理（注册表 + InternetSetOption 刷新）→ 托盘变绿
5. 断开：停内核 + 还原系统代理

## 路由策略（移植自旧 Cat CC 的 gfw 模式）
- 用户自定义规则（`config/rules.txt`，含私有 IP 直连 + 国内常用域名直连）
- gfw 名单（内置 `src-tauri/routes/gfwlist.txt`，4022 条被墙域名，可用 `config/routes_box/gfwlist.txt` 覆盖）→ **全部走代理**
- `MATCH,DIRECT` → **其余默认直连**
- 注意：mihomo 的 `DOMAIN-SUFFIX` **不支持单条多域名**（旧 sing-box 的 domain_suffix 数组语法不兼容），必须一条一域名；规则里禁止用户 MATCH 行（系统统一生成）

## 防崩溃与异常恢复设计
1. **配置极小化**：无超大规则列表（旧 Cat CC 崩溃根因是 4000+ domain_suffix 塞进 sing-box 单条 rule）
2. **启动前校验**：`mihomo -t` 验配置，失败不启动
3. **运行时监控**：mihomo 意外退出 → 自动重启（最多 3 次，指数退避 1s/2s/4s）→ 仍失败则报错，不无限重试；重启竞态用 `generation` 计数防旧监控线程误重启
4. **残留自愈**：启动时检测系统代理指向本地端口但无监听（异常退出/强杀残留）→ 自动还原；断开/崩溃放弃重启时只清理本程序开启的代理（`proxy_enabled` 标记，不误关其他代理工具）

## Tech
- Rust（edition 2021），依赖：tauri 2、serde/serde_json、base64、url、ureq
- Tauri 2 外壳：系统 WebView2 渲染 `dist/index.html`，`withGlobalTauri: true` 提供 `window.__TAURI__.core.invoke`
- `mihomo-core.exe` — Clash Meta 内核（windows-amd64-compatible，置于项目根目录；命名为 mihomo.exe 会被部分杀软拦截 spawn 报 EPERM，必须用 mihomo-core.exe）
- 配置目录 `config/`（运行时生成），临时配置 `config/temp/`

## Run / Build
```bash
cargo check                        # 语法检查
cargo test --release --test full_flow   # 全流程集成测试（connect→保存规则自动重启→disconnect）
CARGO_BUILD_JOBS=2 cargo build --release -j 2   # 构建 release（低并行避免线程资源耗尽 0xc0000409）
```
- 产物：`src-tauri/target/release/cat-cc2.exe`（release 无终端窗口；debug 版会弹终端，正常）
- 端口：mihomo mixed 默认 `7890`（被占则随机 20000-65535）；external-controller 默认 `9090`
- 关窗最小化到托盘；托盘退出 = 停内核 + 还原系统代理

## Tauri Commands（前端 invoke）
| Command | 用途 |
|---------|------|
| `parse_sub` | 解析订阅，返回节点数与名称 |
| `connect` | 拉取/解析 → 生成配置 → 校验 → 启动内核 → 开系统代理 |
| `disconnect` | 停内核（taskkill）+ 还原系统代理 |
| `status` | `{ running, node, latency, last_error, retries }` |
| `get_rules` | 读取用户自定义规则（rules.txt） |
| `save_rules` | 保存规则；`restart=true` 且连接中时自动重启内核立即生效 |

## 关键实现要点
- 订阅解析：`parse_share_link` 支持 6 协议；Clash YAML 用 `proxies:` 段行解析；base64 解码兼容标准/URL-safe 两种
- 节点状态查询：external-controller `GET /proxies/auto` 拿当前节点 + 延迟
- 系统代理：写注册表后必须调 `InternetSetOption`（SETTINGS_CHANGED + REFRESH）通知系统刷新，否则不生效
- `gfwlist` 外部覆盖文件存在但为空时回退内置名单（防代理全失效）
- 子进程 spawn 用 `Stdio::null()` 隔离 stdout，避免挂起调用方
- 残留代理自愈：`proxy.rs::restore_stale_proxy_if_dead` 启动时检查，本地代理端口无监听 → 还原；远程代理 / 有监听的端口一律不动

## 明确砍掉（相对旧 Cat CC）
订阅分组、手动节点切换、手动测速、ip-check、Clash 导出、SSE 日志流、多 profile 管理。failover 交给 mihomo url-test。
