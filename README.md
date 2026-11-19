# LinkForge

LinkForge（链路工坊）是面向自建 Xray / 3X-UI 用户的 Windows 桌面工具：按 CFST 的默认模型测试本机到 Cloudflare 边缘 IP 的延迟、丢包和下载速度，再把优选 IP 与 3X-UI 节点组合并生成订阅。

## 下载

Windows 10/11 x64 安装包见 [Releases](https://github.com/Imoot-TT/LinkForge/releases)。

当前版本：`0.1.10`

## 主要能力

- IPv4 / IPv6 地址或 CIDR 候选抽取；
- TCP 443 延迟、丢包和 CFST 默认下载测速；
- 结果筛选、CSV 导出和本地测速历史；
- 3X-UI 账号登录、凭据安全保存、客户端和关联节点读取；
- VMess / VLESS / Trojan 节点与优选 IP 组合；
- 本地订阅生成，以及可配置的兼容 goodip 服务发布；
- 自动识别生成格式：Auto、Clash、V2rayN、Shadowrocket、Surge。

## 本地开发

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

产物位于 `app/src-tauri/target/release/bundle/`。发布流程参考 [`VidoGo-Basic`](https://github.com/Imoot-TT/VidoGo-Basic) 的 tag + GitHub Release 模式。

## 安全说明

3X-UI 登录凭据只保存在本机 Windows 凭据管理器中；节点链接、面板 Token 和订阅地址属于敏感信息，不应提交到仓库或公开 Issue。调用第三方在线订阅服务前，请确认其服务地址和隐私风险。

## 文档

完整产品、架构、测试和发布审计文档见 [`docs/README.md`](docs/README.md)。

