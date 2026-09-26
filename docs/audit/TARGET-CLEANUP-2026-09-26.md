# target/ 构建残留清理评估（只读审计）

- 日期：2026-09-26 19:06（Asia/Shanghai）
- 范围：`C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target\`
- 模式：只读审计。未删除/移动任何文件，未执行任何 git 操作。
- 报告路径：`docs/audit/TARGET-CLEANUP-2026-09-26.md`
- 结论状态：**待用户确认后执行**（本报告不执行清理）

---

## 一、结论

1. `target/` 整体是 Cargo 构建产物，且已被 `.gitignore` 忽略（`target/`，第 1 行），git 无任何 target 跟踪文件（`git ls-files target` = 0 行）。删除 `target/` **不会**影响 git 跟踪；CI/CD（GitHub Actions）独立 checkout + 独立构建，不依赖本机 target。
2. 实测占用：`debug` ≈ **13,292 MB / 24,839 文件**（约占全 target 的 94%），`release` ≈ **853 MB / 2,967 文件**，`aarch64-unknown-linux-musl` ≈ **67 MB / 275 文件**，`audit-download-v0112` ≈ **8.3 MB / 2 文件**，`tmp` = 空。
3. 当前有**活跃构建进程**占用 `target/` 文件（`cargo.exe` PID 12684 + 多个 `rustc.exe` 子进程，19:02 启动，构建仍在进行，见 §三）。**在构建完成前删除 target 会导致该构建失败/损坏**，必须先停构建或等其完成。
4. 建议：`debug`、`aarch64-unknown-linux-musl`、`audit-download-v0112`、`tmp` 可删；`release/` **建议保留最新产物**（`tryingopen2api.exe` 8.77 MB，SHA256 `2F7FE91E…`）。可选全删（含 release），但会失去本机现成 release 二进制，需重新 `cargo build --release`。
5. 预计释放：保守方案 ≈ **13.3 GB**；激进方案（含 release）≈ **14.2 GB**。删除后 `cargo build` / `cargo build --release` 会重建全部产物（首次需全量编译）。

---

## 二、各目录判定表

| 子目录 | 实测大小 | 文件数 | 用途判定 | 分类 | 理由 |
|---|---|---|---|---|---|
| `debug` | 13,292 MB | 24,839 | 本地开发调试构建产物（incremental 7,913 MB + deps 5,082 MB + build 112 MB） | **可安全删除** | Cargo debug 产物，可再生；`target/` 已在 `.gitignore` 第 1 行，git status 与 target 无关 |
| `release` | 853 MB | 2,967 | 正式 release 构建产物（含 `tryingopen2api.exe` 8,772,144 B，SHA256 `2F7FE91E2DDFA6141E03E0ABB30F7A5AC09D114B363D4DBAC9A042936B67AEA2`） | **需确认（默认保留）** | 保留最新产物可继续运行/分发；删除后仅需 `cargo build --release` 重建 |
| `aarch64-unknown-linux-musl` | 67 MB | 275 | 交叉编译 Linux aarch64 产物（deps 库） | **可安全删除** | 可再生交叉编译产物；CI/CD 在 GitHub Actions 独立构建（见 ci.yml / cd-deploy.yml），本机产物仅为开发用 |
| `audit-download-v0112` | 8.3 MB | 2 | 旧 release 核对样本（`tryingopen2api.exe` 8,188,? B + `.sha256` 校验文件） | **需确认** | 样本哈希已核对一致（`FB84F6DF…` 与 sidecar 匹配）；用途完成、体积很小，删除与否均不影响构建，建议归档到 `reference/` 后删除或直接保留 |
| `tmp` | 0 MB | 0 | Cargo 临时目录 | **可安全删除** | 当前为空 |
| 根级文件（`.rustc_info.json`、`CACHEDIR.TAG`、`review-config-2026-09-26.json`、`review-gateway.*.log`） | ~0 MB | 4 | 构建元信息 / 网关审查日志 | **可安全删除**（随 target 一起） | 可再生元数据；`review-*` 为 2026-09-26 网关审查日志，内容无保留价值 |

> 注：`git status --porcelain` 仅显示 `Cargo.toml`、`src/upstream.rs` 两个已修改文件，与 target 无关；`git ls-files target` 为 0，`git check-ignore -v` 确认 `target/`、`target/debug`、`target/release` 均命中 `.gitignore:1:target/`。

---

## 三、进程占用检查

实测时间 2026-09-26 19:05-19:06（Win32_Process 快照，随后复查仍在）：

- **`cargo.exe` PID 12684**（父 PID 30764，19:02:20 启动），命令行 `cargo build --release`
- **`rustc.exe` 多个子进程**：19:03-19:05 期间有 11+ 个，快照时 PID 18916/48260/46196/25956/6876/6052/23320/40172/13776/24488/13608/48540/40836 等，全部由 cargo 12684 派生
- 父级 `pwsh.exe` PID 45932（19:02:16 启动）运行的是「修改 Cargo.toml release profile → `cargo build --release` + 计时」脚本
- 未发现 `tryingopen2api.exe` 运行实例

**风险：删除 target 会命中正在写入的 `target\release\deps` 文件（rustc 构建输出目录），导致当前构建失败/文件损坏。必须先停掉 PID 12684（及其 rustc 子进程/父 pwsh 45932）或等待构建完成，再执行删除。**

---

## 四、清理方案（待用户确认，不执行）

> 命令均为 PowerShell；执行前请先确认构建进程已退出。以下按「推荐 / 激进」两档。

### 推荐（保留最新 release 产物）

```powershell
# 0) 前置检查：无 cargo/rustc 进程
Get-CimInstance Win32_Process | Where-Object { $_.Name -in 'cargo.exe','rustc.exe','build-script-build.exe','tryingopen2api.exe' }

# 1) 删除 debug 构建产物（释放 ≈ 13.3 GB）
Remove-Item -LiteralPath 'C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target\debug' -Recurse -Force

# 2) 删除交叉编译产物（≈ 67 MB）
Remove-Item -LiteralPath 'C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target\aarch64-unknown-linux-musl' -Recurse -Force

# 3) 删除空 tmp（0 MB）与旧核对样本（≈ 8.3 MB；如需留档请先复制到 reference/）
Remove-Item -LiteralPath 'C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target\tmp' -Recurse -Force
Remove-Item -LiteralPath 'C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target\audit-download-v0112' -Recurse -Force

# 4) 删除 target 根级杂项（.rustc_info.json / CACHEDIR.TAG / review-* 日志，≈ 0 MB）
Remove-Item -LiteralPath 'C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target\.rustc_info.json','C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target\CACHEDIR.TAG','C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target\review-config-2026-09-26.json','C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target\review-gateway.stderr.log','C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target\review-gateway.stdout.log' -Force
```

预计释放：**≈ 13.3 GB**（C 盘当前剩余约 66.6 GB）。

### 激进（连 release 一起删，需重新全量构建）

```powershell
Remove-Item -LiteralPath 'C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target' -Recurse -Force
```

预计释放：**≈ 14.2 GB**。之后重新构建：

```powershell
cargo build            # 重建 debug（约 13 GB 空间需求）
cargo build --release  # 重建 release 二进制
```

---

## 五、风险

1. **活跃构建**：当前 `cargo build --release`（PID 12684）正在写 `target\release\deps`。此时删除 target 会破坏该构建或导致 rustc 报错，必须先停进程/等完成。
2. **增量缓存丢失**：删除 debug/release 会丢 incremental 缓存，下次构建变全量编译，耗时与磁盘需求上升（release 全量构建在 fat-LTO + codegen-units=1 配置下更慢）。可接受：CI/CD 均在 GitHub Actions 独立构建，不依赖本机缓存。
3. **audit-download-v0112 为唯一旧版核对样本**：删除后无法再对照旧二进制；如需留档请先复制（如 `reference/`）。
4. **不做 git 操作、不误删其他内容**：`target/` 之外（`src/`、`data/`、`docs/` 等）不在本次删除范围；`.gitignore` 已忽略 target，删除不影响跟踪。
5. **构建工具/目标差异**：aarch64-unknown-linux-musl 本机重建需对应 target 已安装；CI（release.yml/cd-deploy.yml）在 ubuntu runner/服务器独立构建，不受本机删除影响。

---

## 六、验证建议（执行后）

```powershell
# 确认释放量（应为 ~0 或仅 release 残留）
(Get-ChildItem 'C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tryingopen-2api\target' -Recurse -File | Measure-Object Length -Sum).Sum / 1GB

# 确认构建可重建
cargo build --release --locked
```