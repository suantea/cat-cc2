# 第三方声明（Third-Party Notices）

本项目（Cat CC2）自身代码采用 **Apache License 2.0** 授权（见 [LICENSE](LICENSE)）。以下为本项目分发时涉及的第三方组件声明。

## 运行时组件

### mihomo-core.exe（Clash Meta 内核）

- **许可**：GPL-3.0（<https://www.gnu.org/licenses/gpl-3.0.html>）
- **上游源码**：<https://github.com/MetaCubeX/mihomo>
- **说明**：以独立子进程方式随发行版分发，**未修改其源代码**。按 GPL-3.0 要求，随发行版提供本声明及上游源码链接；源码亦可通过上述地址获取。
- 内核文件需自行从上游获取（本仓库不包含该二进制）。

## 依赖 crate（仅构建期引用，不随源码分发）

| crate | 许可 |
|-------|------|
| tauri / tauri-build | MIT / Apache-2.0 |
| serde / serde_json | MIT / Apache-2.0 |
| base64 | MIT / Apache-2.0 |
| url | MIT / Apache-2.0 |
| ureq | MIT / Apache-2.0 |

版权归其各自作者所有。

## 数据与资源

- **内置 gfw 名单**：整理自社区公开数据（域名列表），来源与许可请以原始列表为准。
- **窗口图标**：Tauri 模板默认图标（MIT 模板）。

## 说明

本声明仅为第三方版权与许可的合规说明，不构成对本项目自身代码许可（Apache-2.0）的修改。
