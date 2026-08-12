---
name: gen-test
description: 按项目测试惯例生成测试。适用于为 subscribe/core_mgr/proxy 等模块补充单元测试。
disable-model-invocation: true
---

生成针对 $ARGUMENTS 的测试，遵循 Cat CC2 项目现有测试惯例。

## 现有测试模式（参考 src-tauri/tests/full_flow.rs 与各模块尾部 #[cfg(test)] mod tests）
- 单元测试写在模块尾部 `#[cfg(test)] mod tests`，纯函数直接构造输入、断言输出
- 集成测试用 base64 假节点订阅走 connect / save_rules / disconnect 全流程（tests/full_flow.rs）
- 网络/进程类函数（fetch_url、spawn_core、set_system_proxy_inner）不做真实调用，只测纯逻辑
- 断言用 assert! / assert_eq!，中文注释

## 要求
1. 分析目标模块的函数，找出可单测的纯逻辑（解析、配置生成、去重、校验）
2. 按上述惯例生成测试，放在对应模块的 tests 模块或 tests/ 目录
3. 不要改动被测代码；如需小重构以便测试，先说明再动手
4. 完成后运行 `cd src-tauri && CARGO_BUILD_JOBS=2 cargo test --lib -j 2` 验证全部通过
