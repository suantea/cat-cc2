# Cat CC2

> 极简代理工具：**订阅一键连接**(Windows / macOS) —— Rust + Tauri 2 外壳 + mihomo 内核。

[Apache-2.0](#版权与许可) · Windows / macOS · [AtomGit](https://atomgit.com/asuan_suan/cat-cc2)

## ✨ 功能特性

- **订阅一键连接**：粘贴订阅链接（或节点内容）→ 自动解析节点 → 自动选最快节点（url-test）→ 自动设置系统代理
- **自动选优与故障切换**：连接后对所有节点测速，自动选用延迟最低节点；节点故障自动切换
- **轻量**：Tauri 2 + 系统 WebView2 / WKWebView，无自带 Chromium、无终端窗口，占用低、启动快
- **内核健壮**：mihomo 内核意外退出自动重启（最多 3 次，指数退避 1s/2s/4s）；配置启动前校验，非法配置拒绝启动
- **异常自愈**：异常退出/强杀后，下次启动自动还原残留的死系统代理，网页不会异常（macOS 扫描全部网络服务，切网残留也能清理）
- **规则可定制**：自定义直连/代理规则，保存后自动重启内核立即生效
- **托盘常驻**：关窗最小化到托盘，托盘退出 = 停内核 + 还原系统代理
- **订阅自动刷新**：连接期间每 6 小时自动重拉订阅，失败沿用旧节点；全节点失效自动重新拉取
- **多 Worker 故障转移**：订阅支持填多个 URL（换行/逗号分隔），失效自动轮转切换
- **DNS 防污染**：内置国内/国外双 DNS（223.5.5.5 + 8.8.8.8）防污染加速
- **CF 优选 IP**：连接时自动并发测速内置候选池（可 `config/opt_ip_pool.txt` 覆盖），选最快前 5 注入 ws+tls 节点（保留 Host 头）提速；也可界面手动填
- **WS 保活**：内核 keep-alive 15s，防 CF 空闲回收 WebSocket 连接

## 📖 使用手册

**完整使用手册见 [使用手册.md](使用手册.md)** —— 涵盖安装、订阅管理、界面说明、路由规则写法、异常处理与自愈、FAQ 等全部使用细节。

核心三步：

1. **粘贴订阅**：把订阅链接（URL）或节点内容粘贴到输入框（支持 base64 节点列表 / Clash YAML / 6 种协议节点）
2. **一键连接**：点绿色"一键连接"，程序自动完成解析 → 配置 → 校验 → 启动内核 → 设置系统代理
3. **断开**：点"断开"或托盘"退出"（= 停内核 + 还原系统代理）

> 💡 **不想走代理的域名**：在"自定义规则"里加一行 `DOMAIN-SUFFIX,域名,DIRECT` 即可，例如 `DOMAIN-SUFFIX,steamcommunity.com,DIRECT`（详见使用手册第六章）。

## 🚀 快速开始

### Windows

1. 获取程序：直接运行 `cat-cc2.exe`（release 构建产物在 `src-tauri/target/release/`），或按下方开发命令自行构建
2. 放置内核：将 `mihomo-core.exe`（Clash Meta 内核，windows-amd64-compatible）放到程序目录或项目根目录
   > ⚠️ 内核文件名**必须是 `mihomo-core.exe`**。改为 `mihomo.exe` 会被部分杀软拦截导致启动失败（EPERM）
3. 双击运行 → 粘贴订阅 → 一键连接

### macOS

1. 获取程序：下载 `Cat CC2_<版本>_<arch>.dmg`（arm64 / x64）拖入"应用程序"
2. 放置内核：将 `mihomo-core`（Clash Meta 内核，darwin 对应架构）放到程序目录或项目根目录
3. 首次打开未签名 App 会触发 Gatekeeper：右键 → "打开" → 确认；或 `xattr -dr com.apple.quarantine "/Applications/Cat CC2.app"`
4. 打开 App → 粘贴订阅 → 一键连接（首次会弹授权框输入管理员密码设置系统代理）

## 🗺️ 路由策略

| 层级 | 策略 |
|------|------|
| 用户自定义规则 | 私有 IP + 国内常用域名直连；可自由添加（写法见[使用手册](使用手册.md)第六章） |
| gfw 名单（内置 4022 条） | **全部走代理**（可用 `config/routes_box/gfwlist.txt` 覆盖） |
| 其余 | 默认直连（`MATCH,DIRECT`） |

## 🛠️ 开发

```bash
cargo check                                # 语法检查
cargo test --lib -j 2                       # 单元测试（订阅解析各协议 + 畸形输入弹药库 + 系统代理解析，30 个）
CARGO_BUILD_JOBS=2 cargo test --test full_flow -j 2   # 全流程集成测试
CARGO_BUILD_JOBS=2 cargo build --release -j 2          # release 构建（低并行防线程耗尽）
```

> 本机没有把 `~/.cargo/bin` 加进 PATH 时，直接用 `~/.cargo/bin/cargo` 调用。

产物：Windows `src-tauri/target/release/cat-cc2.exe`；macOS `cargo tauri build` 产出 `.app` / `.dmg`（需安装 tauri-cli）。

## 📁 目录结构

```
src-tauri/src/    核心 Rust 代码（subscribe 订阅解析 / core_mgr 内核管理 / proxy 系统代理）
dist/             前端界面（单页 index.html）
config/           运行时配置（rules.txt 用户规则、routes_box 覆盖名单、opt_ip*.txt 优选 IP、temp/ 临时文件）
使用手册.md       完整使用文档
```

## ⚖️ 版权与许可

**© 2026 suantea**，本仓库代码基于 **Apache License 2.0** 发布（见 [LICENSE](LICENSE)）。

> GitHub 镜像：github.com/suantea/cat-cc2（AtomGit 为主仓库，两边同步）

第三方组件说明：

- 依赖 crate（tauri / serde / base64 / url / ureq）均为宽松许可（MIT / Apache-2.0），版权归其各自作者
- `mihomo-core` / `mihomo-core.exe`：独立的 Clash Meta 内核（GPL-3.0），需自行获取，**不属于本仓库代码**（未入库）
- 内置 gfw 名单：整理自社区公开数据，商用前请自行核实其来源许可
- 窗口图标：使用 Tauri 模板默认图标（MIT 模板）

## 🧭 开发预期

**设计哲学**：功能最少、防崩溃第一、避开杀软拦截（Tauri 用系统 WebView2/WKWebView，无自带 Chromium）。

- **已实现**：订阅一键连接、自动选优与故障切换、gfw 路由、规则编辑、内核自动重启、异常退出自愈、订阅自动刷新、CF 优选 IP（自动+手动）、多 Worker 故障转移、WS 保活、macOS 支持
- **明确不做**（保持极简）：订阅分组、手动节点切换、手动测速、ip-check、Clash 导出、SSE 日志流、多 profile 管理 —— 故障切换交给 mihomo 内核的 url-test
- **后续候选**（按需）：订阅凭证改用系统钥匙串/凭据管理器存储、更完善的日志、macOS 签名公证（需 Apple Developer 账号）、TUN 模式（需 root/系统扩展）

## 📄 其他文档

- [使用手册.md](使用手册.md) — 完整使用指南（安装、订阅、规则、FAQ）
- [AGENTS.md](AGENTS.md) — 项目设计与开发约束说明
