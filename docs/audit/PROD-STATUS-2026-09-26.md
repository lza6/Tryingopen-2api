# PROD-STATUS-2026-09-26.md — 发布与生产状态复核（Y 代理）

> 复核时间：2026-09-26（Asia/Shanghai）
> 仓库：`C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api` → GitHub `lza6/Tryingopen-2api`
> 性质：只读复核 + 公网健康检查。未提交、未推送、未修改任何生产资源。
> 工具：git 本地快照、gh CLI v2.91.0、Invoke-WebRequest（`NO_PROXY=*` 直连公网）。

## 1. Release v0.1.11 状态

| 项 | 值 |
|---|---|
| 版本 | v0.1.11（TryingOpen2API，非 draft、非 prerelease） |
| URL | https://github.com/lza6/Tryingopen-2api/releases/tag/v0.1.11 |
| created | 2026-09-25T20:44:02Z |
| published | 2026-09-25T20:49:28Z |
| target_commitish | main |
| 资产 | `tryingopen2api.exe`（8,683,520 B），`tryingopen2api.exe.sha256`（86 B） |
| exe SHA-256（GitHub 声明） | `fd254932a3911e2189e681c59f23accd13fe469a16ead7bf9da770e6eb8f7c77` |
| sha256 资产内容摘要 | `f645759019797176cf51550eab31a98da1f1011a9b130c191b3c21bbc0f77c0b` |
| 发布备注 | “CI 已验证：fmt / clippy / test / audit / gitleaks”，校验提示 `sha256 -l`/`Get-FileHash` |

资产数量：2，均未标记为 draft（发布已公开）。未对本地/远端二进制做重算比对（只读复核；如需一致性独立核验，可下载资产后 `Get-FileHash`）。

## 2. 最近 CI/CD/Release 工作流（gh run list --limit 10）

全部 10 条均 `status=completed` 且 `conclusion=success`。无失败、无未完成、无取消。

| 时间(UTC) | 工作流 | 事件/分支 | headSha | 结论 |
|---|---|---|---|---|
| 2026-09-25T21:39:38Z | CI | push/main | `2a21027…` | success |
| 2026-09-25T21:39:37Z | CD-Deploy | push/main | `2a21027…` | success |
| 2026-09-25T20:56:35Z | CI | push/main | `57ae7cc…` | success |
| 2026-09-25T20:56:35Z | CD-Deploy | push/main | `57ae7cc…` | success |
| 2026-09-25T20:45:47Z | release | push/v0.1.11 | `7fecdd7…` | success |
| 2026-09-25T20:45:18Z | CD-Deploy | push/main | `7fecdd7…` | success |
| 2026-09-25T20:45:18Z | CI | push/main | `7fecdd7…` | success |
| 2026-09-25T17:22:39Z | CI | push/main | `3a6b99c…` | success |
| 2026-09-25T17:22:39Z | CD-Deploy | push/main | `3a6b99c…` | success |
| 2026-09-25T16:25:29Z | CI | push/main | `10f0247…` | success |

最近 3 条 main push 均触发 CI+CD-Deploy 且双绿；release 工作流（v0.1.11 分支推送）亦为 success。

## 3. 公网 healthz（NO_PROXY 直连 https://try.hwhcie.bond/healthz）

```
HTTP 200
{"app":"tryingopen2api","models":24,"ok":true,"proxies":4500,"upstream":"https://www.tryingopen.com","version":"0.1.11"}
```

| 字段 | 值 | 说明 |
|---|---|---|
| app | tryingopen2api | 与仓库项目一致 |
| ok | true | 健康 |
| version | 0.1.11 | 与发布 tag 一致 |
| models | 24 | 模型数 |
| proxies | 4500 | 代理池规模 |
| upstream | https://www.tryingopen.com | 上游源站 |

## 4. 版本一致性对比

| 对照项 | 版本/SHA | 与公网一致？ |
|---|---|---|
| 公网 healthz | 0.1.11 | — |
| Release tag v0.1.11 | tag 提交 `7fecdd7`（fix: 发布前复查…） | ✅ 版本字符串一致 |
| main HEAD（origin/main=本地 HEAD） | `2a21027`（docs: 本地完整带key E2E…） | ⚠️ 见下 |

- **tag v0.1.11 与 main HEAD 不一致**：`v0.1.11..origin/main` 多出 2 个提交：
  1. `57ae7cc` docs: E2E公网真实验收报告(v0.1.11)——无key探活+安全拒绝PASS
  2. `2a21027` docs: 本地完整带key E2E(11PASS+2EXPECTED)与Release资产核对报告
  - 两提交均为 **docs/报告类，非源码变更**；tag 指向的 `7fecdd7` 仍是发布源码基线。
- **本地工作区未提交变更（2 个文件）**：
  - `src/web.rs`：面板前端文案一行（`sk-local（未配置 api_keys 时任意）` → `面板已自动注入（空配置时生成会话级 key）`），非已发布代码。
  - `docs/ARCHITECTURE.md`：文档修订（5 插入/3 删除）。
  - 这两个变更尚未提交/推送/CD 未部署，因此不会影响当前公网版本判定。
- **公网运行版本** = release tag 语义版本（0.1.11）✅；**源码基线** = tag 提交 `7fecdd7`（生产 CD 最后一次部署对应 `7fecdd7` 的 CD-Deploy，success）。main HEAD 仅多 2 个 docs 提交，不影响产物。

## 5. 结论

- **生产健康**：healthz HTTP 200，`ok=true`，`version=0.1.11`，24 模型 / 4500 代理，上游正常。
- **发布正常**：v0.1.11 已公开（非 draft/prerelease），2 个资产齐全；最近 10 条 CI/CD/release 工作流全部 success，无失败/未完成。
- **版本口径**：公网 0.1.11 与 release tag v0.1.11 一致；main HEAD 与 tag 存在 2 个 docs-only 提交差异，**不影响**发布产物/运行版本一致性。
- **注意（非阻塞）**：本地有未提交的 `src/web.rs` + `docs/ARCHITECTURE.md` 变更；若后续希望面板注入提示与已部署行为一致，需提交推送并触发 CD 后重新验收。

## 证据来源
- `gh release view v0.1.11` + `gh api .../releases/tags/v0.1.11`
- `gh run list --limit 10`（含 JSON 明细 databaseId/headSha/status/conclusion）
- `git ls-remote origin refs/heads/main refs/tags/v0.1.11`、`git log v0.1.11..origin/main`
- `Invoke-WebRequest https://try.hwhcie.bond/healthz`（NO_PROXY 直连，HTTP 200）
