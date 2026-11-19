<div align="center">
  <img src="assets/linkforge-brand-icon.png" alt="LinkForge" width="144">
  <h1>LinkForge</h1>
  <p>A Windows desktop utility for Cloudflare edge IP selection, 3X-UI node composition, and subscription generation.</p>
  <p><strong>English</strong> · <a href="README.zh-CN.md">中文</a></p>
  <p>
    <img alt="Version" src="https://img.shields.io/badge/version-0.1.2-1688f0">
    <img alt="Platform" src="https://img.shields.io/badge/platform-Windows%20x64-0078d4">
    <img alt="Protocol" src="https://img.shields.io/badge/protocol-Rust%20%2B%20Tauri-22a06b">
    <img alt="License" src="https://img.shields.io/badge/license-GPL--3.0-22a06b">
  </p>
</div>

---

## Download

[Download LinkForge 0.1.2 for Windows x64](https://github.com/Imoot-TT/LinkForge/releases/tag/v0.1.2)

## Features

- Extract IPv4 / IPv6 candidates from addresses and CIDR ranges.
- Measure TCP 443 latency, packet loss, and CFST-compatible download speed.
- Filter results, export CSV, and keep local test history.
- Sign in to 3X-UI, save credentials securely, and read clients and linked nodes.
- Combine VMess / VLESS / Trojan nodes with selected edge IPs.
- Generate subscriptions locally or publish through a configurable goodip-compatible service.
- Export Auto, Clash, V2rayN, Shadowrocket, and Surge formats.

## Run from Source

```powershell
cd app
npm ci
npm run build
npm run tauri -- dev
```

Build Windows installers:

```powershell
cd app
npm run tauri -- build
```

## Security

3X-UI credentials are stored in the local Windows Credential Manager. Node links, panel tokens, and subscription URLs are sensitive and must not be committed or posted in public issues. Review the destination before using an online subscription publisher.

## Documentation

See [`docs/README.md`](docs/README.md) for the product, architecture, testing, and release documentation.

## License

LinkForge is licensed under the [GNU General Public License v3.0](LICENSE). If you distribute a binary build, provide users with access to the corresponding source code under the same license.
