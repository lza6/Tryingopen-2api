# Release 资产核对报告 v0.1.11（2026-09-26）

- 核对代理：发布资产核对代理（B），只读/下载核对，未运行 exe，未改代码。
- 仓库：lza6/Tryingopen-2api（本地 C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api）
- 执行日期：2026-09-26（本地时区 Asia/Shanghai）

## 1) GitHub Release 信息

命令：`gh release view v0.1.11 --repo lza6/Tryingopen-2api`

真实输出：

```
title:	TryingOpen2API v0.1.11
tag:	v0.1.11
draft:	false
prerelease:	false
immutable:	false
author:	github-actions[bot]
created:	2026-09-25T20:44:02Z
published:	2026-09-25T20:49:28Z
url:	https://github.com/lza6/Tryingopen-2api/releases/tag/v0.1.11
asset:	tryingopen2api.exe
asset:	tryingopen2api.exe.sha256
```

Release 说明（节选）："TryingOpen2API v0.1.11（Rust axum 免费模型 OpenAI/Anthropic 兼容网关）……CI 已验证：fmt / clippy / test / audit / gitleaks。校验：`sha256 -l tryingopen2api.exe`（或 `Get-FileHash`）。"

## 2) Release 资产下载

命令：`gh release download v0.1.11 --repo lza6/Tryingopen-2api -D <临时目录> --pattern "tryingopen2api.exe*"`

临时目录：`%TEMP%\release-v0.1.11-verify-e292e40b`

下载结果：

| 文件 | 大小（字节） |
|---|---|
| tryingopen2api.exe | 8,683,520 |
| tryingopen2api.exe.sha256 | 86 |

## 3) SHA256 哈希比对（真实输出）

命令：`Get-FileHash -Algorithm SHA256 tryingopen2api.exe` 与 `.sha256` 文件内容比对。

- EXE 实际哈希：`FD254932A3911E2189E681C59F23ACCD13FE469A16EAD7BF9DA770E6EB8F7C77`
- .sha256 文件原始内容：`fd254932a3911e2189e681c59f23accd13fe469a16ead7bf9da770e6eb8f7c77  tryingopen2api.exe`
- 比对方式：`.sha256` 为 `hash  filename` 格式，取首字段后与 exe 哈希做大小写不敏感比较
- 结果：**一致（MATCH=True）**

## 4) GitHub Actions 工作流状态（最新 8 条）

命令：`gh run list --repo lza6/Tryingopen-2api --limit 8`

| 结论 | 状态 | 工作流 | 分支 | 事件 | Run ID | 时长 | 触发时间 (UTC) |
|---|---|---|---|---|---|---|---|
| success | completed | CD-Deploy | main | push | 36188730489 | 3m45s | 2026-09-25T20:56:35Z |
| success | completed | CI | main | push | 36188730452 | 3m31s | 2026-09-25T20:56:35Z |
| success | completed | release | v0.1.11 | push | 36187695234 | 4m9s | 2026-09-25T20:45:47Z |
| success | completed | CD-Deploy | main | push | 36187645344 | 4m7s | 2026-09-25T20:45:18Z |
| success | completed | CI | main | push | 36187645340 | 4m40s | 2026-09-25T20:45:18Z |
| success | completed | CI | main | push | 36166732343 | 3m8s | 2026-09-25T17:22:39Z |
| success | completed | CD-Deploy | main | push | 36166732248 | 3m45s | 2026-09-25T17:22:39Z |
| success | completed | CI | main | push | 36160662327 | 3m19s | 2026-09-25T16:25:29Z |

关键结论：
- v0.1.11 release 工作流（run 36187695234）：success，4m9s。
- v0.1.11 触发的 CD-Deploy（run 36187645344）：success，4m7s。
- 发布后 main 分支 CD-Deploy（run 36188730489，E2E 公网验收报告 push）：success，3m45s。

## 5) 公网 healthz 核对

目标：https://try.hwhcie.bond/healthz

- 本机直连结果（curl `--noproxy '*'` + Invoke-WebRequest `-NoProxy`）：**TCP 443 可达（TcpTestSucceeded=True，RemoteAddress=20.204.27.154），但 TLS 握手被远端重置（schannel: Recv failure: Connection was reset / CRYPT_E_REVOCATION_OFFLINE）**，判定为本机 TLS 栈与边缘节点握手兼容性问题，非服务端不可达。
- 独立网络栈（fetch 工具）直连成功，真实响应体：

```json
{"app":"tryingopen2api","models":24,"ok":true,"proxies":4500,"upstream":"https://www.tryingopen.com","version":"0.1.11"}
```

- version：**0.1.11**，ok: true，models: 24，proxies: 4500，upstream: https://www.tryingopen.com。

## 6) 结论

| 核对项 | 结果 |
|---|---|
| GitHub Release v0.1.11（title/tag/draft/published/资产） | 正常：已发布（published 2026-09-25T20:49:28Z），非 draft/prerelease，含 tryingopen2api.exe + .sha256 |
| exe SHA256 与 .sha256 比对 | 一致（FD254932A3911E2189E681C59F23ACCD13FE469A16EAD7BF9DA770E6EB8F7C77） |
| CI/CD-Deploy/release 工作流 | 全部 success（release v0.1.11 4m9s；CD-Deploy 4m7s；main 最新 CD-Deploy 3m45s） |
| 公网 https://try.hwhcie.bond/healthz version | 0.1.11（独立网络栈真实响应；本机 curl/IWR 直连 TLS 握手被重置，详见第 5 节） |

综合结论：**v0.1.11 发布资产、哈希、CI/CD 工作流与公网部署版本全部核对一致，核对通过。**

备注：本次核对为只读/下载核对；下载的 tryingopen2api.exe 仅做哈希比对，未运行。
