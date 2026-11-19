<div align="center">
  <img src="assets/linkforge-brand-icon.png" alt="LinkForge" width="144">
  <h1>LinkForge</h1>
  <p>面向自建 Xray / 3X-UI 用户的 Cloudflare IP 优选与订阅生成桌面工具。</p>
  <p><a href="README.md">English</a> · <strong>中文</strong></p>
  <p>
    <img alt="版本" src="https://img.shields.io/badge/version-0.1.2-1688f0">
    <img alt="平台" src="https://img.shields.io/badge/platform-Windows%20x64-0078d4">
    <img alt="技术栈" src="https://img.shields.io/badge/protocol-Rust%20%2B%20Tauri-22a06b">
    <img alt="许可证" src="https://img.shields.io/badge/license-GPL--3.0-22a06b">
  </p>
</div>

---

## 下载

[下载 LinkForge 0.1.2 Windows x64 安装包](https://github.com/Imoot-TT/LinkForge/releases/tag/v0.1.2)

## 主要功能

- 从 IPv4 / IPv6 地址或 CIDR 网段抽取候选 IP；
- 测量 TCP 443 延迟、丢包和与 CFST 一致的下载速度；
- 筛选结果、导出 CSV，并保存本地测速历史；
- 登录 3X-UI，安全保存凭据，读取客户端和关联节点；
- 将 VMess / VLESS / Trojan 节点与优选 IP 组合；
- 本地生成订阅，也可调用可配置的 goodip 兼容服务；
- 输出 Auto、Clash、V2rayN、Shadowrocket、Surge 格式。

## 从源码运行

```powershell
cd app
npm ci
npm run build
npm run tauri -- dev
```

生成 Windows 安装包：

```powershell
cd app
npm run tauri -- build
```

## 安全说明

3X-UI 登录凭据只保存在本机 Windows 凭据管理器中；节点链接、面板 Token 和订阅地址属于敏感信息，不应提交到仓库或公开 Issue。调用第三方在线订阅服务前，请确认其服务地址和隐私风险。

## 文档

完整产品、架构、测试和发布审计文档见 [`docs/README.md`](docs/README.md)。

## 许可证

LinkForge 使用 [GNU GPLv3](LICENSE) 发布。分发二进制安装包时，应同时向用户提供对应源码，并继续使用相同许可证。
