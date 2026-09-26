# CODE REVIEW — tryingopen-2api 面板增强（复制按钮 / 健康自检 / curl 块）

- 日期：2026-09-27
- 审查对象：工作区未提交改动 `src/web.rs`（HEAD 2191209 = v0.1.14，+42 行）；另含 `config.example.json`（+2 行，非本次审查重点）
- 模式：只读审查，未修改任何源文件
- 审查范围：复制 curl/复制 Python 按钮、健康自检按钮、curl 块、XSS、可访问性、healthz 头

---

## 结论

改动核心方向正确：`guide-curl` 用 `textContent` 渲染（无 XSS 注入面）、按钮为原生 `<button>`（可聚焦、支持键盘、有 focus-visible 样式）、复制失败路径有 toast 兜底。curl 命令的换行拼接与 JSON 转义在"正常 key + 受信模型 ID"下**可用**（`\"` 在 JSON 层是合法转义，body 能被正确解析）。

但存在 **1 个 P1 功能缺陷：新增的 curl 示例块与健康自检结果面板被写死在 `display:none`，且全文件没有任何代码把它们显示出来** —— 健康自检按钮点击后"自检中…"→ 按钮复原，结果输出到永远隐藏的 `#guide-selfcheck`，用户看不到任何结果；curl 块同样不可见。两个新功能实际不可用。

另有 1 个 P2 字符串注入/转义缺口（拦截式 key 可含任意字符 → 拼入 curl 命令后可破坏 shell 命令），以及若干 P3 可访问性/健壮性建议。无 CRITICAL（未发现可直接利用的远程 XSS 或命令执行面）。

---

## 发现

### P1 — 健康自检结果与 curl 示例块永远不可见（功能失效）

- 文件:行：`src/web.rs:155-159`（两个 `display:none` 面板）、`src/web.rs:333-334`（把 curl 文本写入隐藏的 `#guide-curl`）、`src/web.rs:346-362`（自检结果写入隐藏的 `#guide-selfcheck`）
- 详情：
  - `panel-curl`（行 155）与 `panel-selfcheck`（行 158）均带 `style="display:none"`；
  - 全文件检索 `panel-curl` / `panel-selfcheck` / `guide-curl` / `guide-selfcheck`，除 HTML 定义与写入外**零处**修改 `style.display` 或调用展示逻辑；
  - 点击"健康自检"→ `selfCheck()` 把结果 `setBox(results.join('<br>'))` 写进隐藏的 `#guide-selfcheck`，页面无任何可见变化，按钮短暂变"自检中…"后复原 → 用户以为没反应；
  - curl 示例同样：`curlEl.textContent = curlCmd` 写入隐藏面板，"curl 块"不可见（复制 curl 仍可用，因为直接复制 JS 变量而非读 DOM）。
- 影响：两个新增功能（健康自检 / curl 示例块）对用户完全不可见，按钮点击无可见反馈，属于交付功能失效。
- 修复（最小）：`selfCheck()` 开始时 `document.getElementById('panel-selfcheck').style.display = '';`；"复制 curl"按钮点击时显示 `panel-curl`，或将 curl 文本展示到已可见的接入信息面板（`guide-live`）内；自检开始时用 `aria-busy` 标记。

### P2 — curl 命令对拦截式 key 未做 shell/JSON 转义（可注入）

- 文件:行：`src/web.rs:329-332`（curl 模板拼接）、`src/api.rs:2117-2120`（`set` action 对 key 无字符校验）
- 详情：
  - `-H "Authorization: Bearer ${key}"` 直接内插；后端 `set` 端点仅 `.trim()` 判空，**未限制字符集**，key 可包含 `"`、`\`、`'`、换行、`$(...)` 等；
  - 当 key 含 `"` 时，`-H "Authorization: Bearer key"x"` 的引号对错位，curl 命令在用户 shell 中执行行为改变；若 key 含反引号/`$()` 且用户无引号包裹复制执行，可形成本地命令注入；
  - 注入者 = 能调用 `POST /api/config/api-key {action:"set"}` 的人 —— 该端点有 `check_api_key` 鉴权（`src/api.rs:2096-2099`），且 set 允许写入任意值，因此实际风险链是"有 key 写权限的人给自己/同面板用户种下恶意复制命令"。危害为本地（管理员自己执行），但属于真实的转义缺口；模型 ID 来自上游目录（受信）、base 来自配置（受信），暂不受影响。
  - 说明：`-d '{\"model\":...}'` 中的 `\"` 在 JSON 层是**合法转义**（`node` 实测渲染后 body 为 `{\"model\":\"...\"...}`，serde_json 可正确解析为 `{"model":"..."}`），因此 JSON body 本身正确，问题只在 key/特殊字符。
- 修复建议：用 `JSON.stringify` 生成 body 片段；对 key 做最少字符集校验（`^[\w-]{8,}$`）或至少拒收 `"`、`\`、换行、单引号；复制内容改为从受控渲染的 `textContent` 读取而不是重复拼串。

### P3 — 可访问性与健壮性

1. `src/web.rs:336,338` 复制按钮 handler 只在 `loadGuide()` 成功路径绑定；`/api/guide` 失败时（行 340-343）按钮保留但无 onclick → **点击静默无反应**，无 toast。
2. `src/web.rs:346-362` 自检无 `aria-live="polite"` / `aria-busy`，结果面板也不可见（受 P1 影响）；结果依赖 emoji ✅/❌ 传达成功失败，无文字辅助（屏幕阅读器读"检查标记/交叉标记"尚可，但建议补文字）。
3. `src/web.rs:349` 文案"耗时约 10s"，但 3 个检查**串行**执行，每个自带 10s abort（`j()` 内 AbortController），最坏约 30s；可并行化或放宽文案。
4. `navigator.clipboard` 仅在 secure context 可用；经 LAN IP `http://` 访问面板时走 `else` 分支提示"不支持"，无 `execCommand("copy")` 降级（可接受，注释留档即可）。
5. `src/web.rs:148-150` 新按钮无 `aria-label` —— 但按钮文本即可访问名（可接受）；`type` 未写（默认 `submit`），页面无 form，无实际影响，建议补 `type="button"` 防御未来改动。
6. `src/web.rs:352-355` `check()` 的返回值 `true/false` 未被消费（死代码），可删。

### 建议（用户指定项 4）

- `src/web.rs:357` healthz 探测经 `j()` 会带上 `x-api-key` header（`src/web.rs:186` 无条件设置）。`handle_healthz`（`src/api.rs:157`）无鉴权，多送 header 无害，但会在反代日志中留 key 痕迹。改进：为 `j()` 增加选项（如 `j(path, { auth:false })`）或 healthz 检查改用 `fetch` + 不带 x-api-key。

---

## 建议汇总

| 优先级 | 项 | 修复要点 |
|---|---|---|
| P1 | 面板显示逻辑缺失 | `selfCheck()` 与复制按钮点击时把对应 panel `display` 置空；或把 curl/自检结果并入可见面板 |
| P2 | key 无字符校验 + curl 拼接 | set 端点限制 key 字符集；curl 参数改用 JSON.stringify / 参数化渲染 |
| P3 | 复制按钮失败路径静默 | 绑定放到初始化（`btn-copy-curl`/`btn-copy-python` 静态 onclick 或 loadGuide 兜底绑定） |
| P3 | 自检可访问性 | 面板 `aria-live="polite"`、`aria-busy`；并行化 3 个探测 |
| P3 | healthz 泄漏 key header | `j()` 支持禁用 x-api-key 选项 |

## 未发现项（明确核验为安全）

- XSS：curl 命令全部经 `textContent` 输出（`src/web.rs:334`）；错误消息经 `esc()`（`src/web.rs:355,357-359`）；`setBox` 的 innerHTML 输入均为静态/esc 值 → 无反射型 XSS。
- 自检按钮重复点击：`disabled=true` 防抖（`src/web.rs:351`），异常路径也会在末尾恢复（行 361），无永久卡死。
- 复制按钮重复绑定：loadGuide 每次覆盖 handler，无累积监听。

*审查方式：git diff + 行号级静态核查 + Node 脚本模拟模板字符串渲染确认 curl 转义行为；未运行服务、未改动文件。*

