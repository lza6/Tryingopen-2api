# CLEANUP AUDIT — TryingOpen2API 只读审计

- 日期：2026-09-26
- 分支/HEAD：main（origin/main 同步）
- 模式：只读审计；唯一写集为本报告。未执行任何 git 修改、删除或移动。
- 结论分级：P0=紧急风险；P1=高优先处理；P2=低优先/建议。

## 结论

仓库主体健康：git 已跟踪文件未发现真实高熵凭据（当前 HEAD 与工作区均未命中 `sk-`/`ghp_`/`gho_` 等真实密钥形态）；`target/`、`data/proxies.txt`、`源代码、网络数据包/`、`优化迭代计划/` 均处于 git 忽略/未跟踪状态。主要风险集中在 **git 历史中的 1 个疑似旧 api key**（`sk-to-bqsprd1mg4f07i6uywz8le3ho5tj2anx`，当前工作区已脱敏，但密钥串仍存在于 git 历史可达提交中）、**config.json 被 git 跟踪**（虽当前内容无真实凭据，但仍是密钥落库面）、以及 **target/debug 占用约 11.4 GB 巨型残留**。文档存在多处测试数/模块清单/版本向量的漂移，建议与新报告一并闭环。

---

## 1. 历史密钥/凭据扫描

| # | 发现 | 优先级 | 证据 | 状态 |
|---|------|--------|------|------|
| 1-1 | 真实疑似 API Key 已进入历史 | P1 | `git log --all -p` 命中 `sk-to-bqsprd...`，定位提交 `63226a8`（凭据脱敏提交，9/25）与 `296e391`（服务器部署记录）。当前 HEAD 工作区 `rg` 无此串；`docs/SERVER_DEPLOYMENT.md:25` 现为 `sk-to-<YOUR_API_KEY>` 占位符。 | 历史含密，当前文件已脱敏 |
| 1-2 | `SSH_PASS` 仅以 GitHub Actions Secret 引用 | P2 | `.github/workflows/cd-deploy.yml:50`、`README.md:253`；无明文 SSH 密码。 | 无泄漏 |
| 1-3 | config.json / config.example 字段均无凭据值 | P2 | `config.json api_keys=[]`；config.json 历史 3 次提交均无真实 key/password。 | 当前无凭据 |
| 1-4 | worktree 全目录真实凭据扫描 | P2 | `rg` 扫描（排除 target/.git）未命中 sk-/ghp_/gho_/AKIA/PRIVATE KEY/Bearer 真实值；HAR `cookie` 为空。 | 未发现 |

> 证据备注：真实密钥串仅出现在 git 历史中，当前工作树无明文。读取仅用于审计，未回写任何文件。

---

## 2. 残留/旧产物盘点

| # | 路径 | 大小 | 建议 | 优先级 |
|---|------|------|------|--------|
| 2-1 | `target/debug` | 约 11.39 GB / 21368 文件 | 可清理（构建缓存）；若需保留最近调试产物，仅保留相关增量。清理前需用户确认。 | P1 |
| 2-2 | `target/release` | 约 640 MB / 1830 文件 | 保留；当前发布/本地构建产物。 | P2 |
| 2-3 | `target/aarch64-unknown-linux-musl` | 约 67 MB / 275 文件 | 服务器构建替换物；若不再需要可清理。 | P2 |
| 2-4 | `target/audit-download-v0112` | 8.26 MB：`tryingopen2api.exe` + `.sha256`（2026/9/26 07:24） | 审计样本；可与 Release 核对后按需清理。 | P2 |
| 2-5 | `target/tmp` | 0 文件（0 KB） | 空目录，可安全清理。 | P2 |
| 2-6 | `data/proxies.txt` | 48 B，2 行：`http://10.255.255.1:8080` / `http://192.0.2.1:3128`（保留地址） | 文件被 `.gitignore` 忽略，属于本地配置；保留或按环境调整。 | P2 |
| 2-7 | `reference/` | 空目录 | 保留（`reference/*.txt` 已被 gitignore）。 | P2 |
| 2-8 | 根目录临时文件 | 未发现 `.tmp/.log/.bak/.orig` | 无。 | — |

> 所有删除动作均需用户显式授权；本报告仅列清单。

---

## 3. config.json 跟踪现状与影响面

| 项 | 状态 | 证据 | 建议 |
|----|------|------|------|
| git 跟踪 | 是 | `git ls-files config.json` 命中；历史 3 次提交（36ec3a5 / 94b5c46 / 2672823）、当前工作树与 HEAD 内容一致 | 移出 git 跟踪 |
| 本地覆盖 | 已支持 | `.gitignore:13` 已忽略 `config.local.json` | 安全移除 config.json 时沿用 `config.local.json` 覆盖 |
| Dockerfile | 引用 `/app/config.json`（`Dockerfile:39,41`） | 卷挂载 `/app/config.json` 仍可用 `config.example.json` 或 compose mount | 保留挂载约定 |
| docker-compose | 引用 `./config.json:/app/config.json:ro`（`docker-compose.yml:14`） | 若删除仓库内 config.json，compose 需改为 `config.local.json` 或环境变量注入 | 改 compose 挂载 |
| README | 引用 `config.json`（README:23,30,156 等） | 保留文档示例即可，需改成 `config.example.json` 复制步骤 | 文档提示 |
| .dockerignore | 已忽略 `config.json`（.dockerignore:15-16） | 镜像不携带 config | 不需改 |
| .github | 无引用 | — | — |
| scripts / start.bat / build.bat | `start.bat` 无显式 config；`build.bat` 提到 `--config config.json`（注释）。 | 预期无破坏 | — |

**建议安全移除方案（需用户确认后执行）**
1. 用 `config.example.json` 作为仓库模板；生产机器保留工作区/服务器上的 `config.json`。
2. `git rm --cached config.json`（不删工作区文件）。
3. `.gitignore` 增加 `config.json`（仅忽略根 config.json，保留 example）。
4. compose 挂载改为 `./config.local.json:/app/config.json:ro` 或环境变量；README/DEPLOYMENT 文档同步。
5. 如需彻底移除历史中的敏感 commit，再做历史改写（`git filter-repo`/`bfg`），需要单独授权且涉及强制推送（本任务禁止 git 操作，不执行）。

---

## 4. 文档漂移抽查

### 4.1 版本号
| 文档 | 声明 | 与代码 | 结论 |
|------|------|--------|------|
| `Cargo.toml` `version` | `0.1.12` | — | 基准 |
| README | `v0.1.12`（README:9,229） | 一致 | ✓ |
| workflow_status | `v0.1.12`（第 135~147 行） | 一致 | ✓ |
| RELEASE_NOTES | `v0.1.12`（行 2）+ 节“v0.1.11” | 内文章节仍叫 v0.1.11，但当前版本说明 v0.1.12 | ⚠ 发布说明头部写 v0.1.12，正文建议补 v0.1.12 changelog |

### 4.2 测试数
| 文档 | 声称 | 实际 grep（`#[test]` 属性数） | 结论 |
|------|------|------------------------------|------|
| README §八 | `44 个测试` | `#[test]` 数 = 43（9 个文件），不含 doc 测试 / 编译宏差异 | 漂移 1 个（当前未含文档/参数生成差异，运行 `cargo test` 结果以 CI 为准） |
| workflow_status | `53 tests`（行 130/147）、`46 → 53`（行 144）、行 14 `37tests` | HEAD 与 `7fc3017` 的 `#[test]` attrs 均为 43 | **漂移：53/46/37 与源码属性数不匹配** |
| ARCHITECTURE | `46 个测试`（行 67） | 43 | 漂移 |

> 说明：`cargo test` 实际通过数可能包含 doc tests 与参数化展开，但文档上的 44/46/53/37 说法彼此冲突且均偏离 43 的属性数，需统一。

### 4.3 模块/文件清单
| 文档 | 声称 | 实际 | 结论 |
|------|------|------|------|
| `docs/ARCHITECTURE.md` | 仅列出 openai_sse.rs / anthropic_sse.rs / stream.rs（缺 `mod.rs`、`openai_sse_helper.rs`；`responses.rs` 虽有但缩进错位） | 实际 `src/protocol/` 含 6 个文件：anthropic_sse.rs、mod.rs、openai_sse_helper.rs、openai_sse.rs、responses.rs、stream.rs | 漂移 |
| `docs/ARCHITECTURE.md` 模块入口少 `prod_guard.rs` | 列了 prod_guard，实际也有；但树中 `protocol/` 嵌套/缩进异常（README 同） | README/ARCHITECTURE tree 格式不一致 | 格式漂移 |
| `README.md` §九 tree | 列出 protocol 下 3 文件，未列 `mod.rs` / `openai_sse_helper.rs` / `responses.rs` | 如上 | 漂移 |
| `docs/INDEX.md` | 审计索引只列 N1/N3/N4/N5；audit/ 实际目录已有 A4/B4/C4/D4/N1/N3/N4/N5/PROD-STATUS/RELEASE-VERIFY 等 | 索引未同步新增审计报告 | 漂移 |

### 4.4 config 字段说明
| README 行 | 声称 | 实际 | 结论 |
|------|------|------|------|
| README §七 列 `direct_fallback_quota`、`rate_limit_max_keys`、`ui_password` 等 | 配置支持 | `config.json` 没写这些字段（但有默认值，`src/config.rs` 支持）；`config.example.json` 含完整字段 | 属“默认值可省略”，文档无大错但建议标注 |

---

## 风险

| 风险 | 级别 |
|------|------|
| 历史中包含疑似生产 API Key，若该 key 仍有效，任何拿到历史/日志者可能复用 | P1 |
| `target/debug` 11.4 GB，长时间占用磁盘；磁盘满时可能影响后续构建/日志 | P1 |
| `config.json` 仍是被 git 跟踪文件，未来若写入真实 key/password 会随 push 泄漏；当前 .dockerignore 只保护镜像不带，git 层没有阻止 | P1 |
| 文档测试数/模块清单漂移，会误导新人/交接审计 | P2 |

---

## 建议动作（全部需用户确认后再执行）

1. **P0/P1**：评估历史中的 `sk-to-...` key 是否仍有效；若有效，尽快在服务端轮换，并把该 commit 后续历史纳入“不可公开访问”或选择历史清洗（需额外授权，且禁用 `-f`）。
2. **P1**：将 `config.json` 移出 git 跟踪（`git rm --cached config.json` + `.gitignore` 增加 `config.json` + 同步 compose/文档），并将运行时配置改用 `config.local.json`。
3. **P1**：在获得用户授权后清理 `target/debug`（可保留 `target/release`）；`target/audit-download-v0112` 与 `target/tmp` 为核心确认即可清理。
4. **P2**：统一测试数口径：建议把 README/ARCHITECTURE/workflow_status 中的 44/46/37/53 统一为实际 `#[test]` 属性数（当前 43）＋ doc test 结构，避免跨文档冲突。
5. **P2**：补 `docs/INDEX.md` 审计索引，列出 A4/B4/C4/D4/PROD-STATUS/RELEASE-VERIFY 等已有报告。
6. **P2**：修正 README/ARCHITECTURE 的 `src/protocol/` 文件清单（补充 mod.rs、openai_sse_helper.rs、responses.rs）。
7. **P2**：RELEASE_NOTES 补 v0.1.12 的变更节，避免正文只有 v0.1.11。

---
*报告生成于只读审计；唯一新写文件为本报告。*


