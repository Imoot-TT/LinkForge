# LinkForge Desktop

Rust + Tauri + React/TypeScript 实现的 Windows 桌面 MVP。

## 当前功能

- 针对用户自己的 VPS/Cloudflare 域名配置测速 URL；
- 内置 Cloudflare IPv4/IPv6 候选网段；
- 使用内置 Rust 引擎生成候选 IP、检测延迟/丢包并执行下载测速；
- 搜索、排序、勾选并导出优选结果 CSV；
- 识别 Base64 原始订阅并提供不含凭据的节点预览；
- 本地转换 VMess、VLESS、Trojan 节点；
- 保留原始 Host/SNI，支持 IPv4/IPv6 优选入口；
- 可选调用兼容 CloudflareSub 的 `/api/generate`；
- 发布服务地址和接口路径可配置；
- 第三方发送前确认，本地生成不上传节点。
- SQLite 保存并恢复测速历史；
- 3X-UI Bearer Token 只读获取分享链接，并可选择保存到 Windows Credential Manager；
- 在线订阅二维码与脱敏诊断包导出。

## 开发命令

```powershell
npm install
npm run build
cargo test --manifest-path src-tauri\Cargo.toml
npm run tauri dev
npm run tauri build
```

## 安全说明

- 测速不启动任何外部可执行文件；
- API Key 只保存在当前应用会话内存，不写入 localStorage；
- 在线生成会把完整节点提交到用户配置的服务；
- 默认服务不是本项目控制的线上实例；
- 正式版仍需接入 Windows Credential Manager、代码签名和完整历史数据库。

## 已知未完成项

- 3X-UI API 自动读取；
- SQLite 历史记录；
- Clash/Surge 完整本地导出；
- 系统凭据库；
- 二维码和客户端协议唤起；
- 固定订阅 URL 更新与撤销。
