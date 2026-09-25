# C4 前端/契约/UX 审计报告（只读子代理 C）

> 审计日期：2026-09-26
> 范围：src/web.rs（嵌入 HTML+JS 控制面板）、src/api.rs（路由/响应）、docs/API_CONTRACT.md、docs/PROTOCOL.md、README.md
> 方法：只读审计，未修改任何业务文件，未运行测试、未做 git 操作；全部行号按审计时快照（web.rs 261 行 / api.rs 1688 行）逐条对照真实代码，不臆造。
> 分级：BLOCKER（真实失效/契约破坏）→ MAJOR（明显缺陷/契约误导）→ MINOR（健壮性/体验）→ INFO（文档一致性提示）

## 0. 总体结论（摘要）

- BLOCKER：0
- MAJOR：6
- MINOR：9
- INFO：6

前后端契约整体**一致**：前端 fetch 的字段名/结构与后端响应逐项比对通过（/v1/models、/api/proxies、/api/guide、/api/config/api-key、/healthz 均可对上），用户路径基本完整，无点了没反应的假按钮/假数据。主要问题集中在：默认配置（api_keys 为空）下面板首次打开必然 401、fetch 无超时且刷新无 loading、接入指南全部为字面量占位（PORT 不可用、/api/models 死引用）、README/面板对免费源数量矛盾、模型表价格/健康分/colspan 兜底缺失、a11y 无 ARIA 语义。

## 1. BLOCKER

无。

说明：默认配置下面板首次打开数据拉取会 401（MAJOR-1），但属 MAJOR 级体验问题，不构成启动/核心 API 失效；核心 API 与文档契约逐条一致，故不升级为 BLOCKER。

## 2. MAJOR

### MAJOR-1 面板无 API Key 管理 UI：文档/日志声称"在 /ui 生成 key"，实际 /ui 做不到（假功能 + 文档过度声称）
- 位置：docs/API_CONTRACT.md:10（"运行时可在 /ui 或 POST /api/config/api-key 生成 key"）、src/main.rs:90（日志"面板可一键生成"）、src/web.rs（全文 4 个 tab 无任何 API Key 生成/查看/清除 UI，也无 /api/config/api-key 调用）、src/api.rs:1641-1678（端点真实存在但前端未接）。
- 问题：三处声称"面板可生成 key"，但面板内只有 总览/代理池/模型/接入指南 四个 tab，唯一能 POST 的是 refresh-free 与 catalog/refresh；生成/设置/清除 API Key 只能手动 curl 或改 config.json 后重启。这是"看起来有其实不能用"的反例（文档/UI 层面）。
- 附带（真实 401 场景）：默认双空放行（src/api.rs:69-70），面板首次打开可正常加载；一旦管理员通过 curl 生成 key，已打开的面板仍持有加载时的旧空 key 快照（src/web.rs:153），后续 /api/proxies、/v1/models 全部 401，必须刷新页面重新注入才恢复——无自动恢复提示。
- 建议：面板加"API Key 管理"小组件（生成/复制/清除，调 /api/config/api-key 并提示刷新生效）；否则删除文档与日志中的"面板可生成"字样。

### MAJOR-2 前端 fetch 无超时 + 刷新动作无 loading 状态（代理池反复空渲染）
- 位置：src/web.rs:154-162（j() 无超时）、src/web.rs:256-257（页载 + 15s setInterval）、src/web.rs:190-194（刷新先清空 tbody）。
- 问题：fetch 无 AbortController/超时；refreshProxies 每次进入都先 tb.innerHTML=''，成功前表格为空且无 loading 指示；上游/代理源慢时用户无法区分"加载中"与"已失败"。loadModels 同理。
- 建议：j() 加 10s 超时；刷新失败保留旧数据并显示"上次成功 HH:mm:ss（失败）"。

### MAJOR-3 接入指南 tab 为字面量占位：PORT 不可用 + /api/models 死引用，未使用后端 /api/guide
- 位置：src/web.rs:150（const PORT = location.port || "47831"，从未用于模板）、src/web.rs:138（Base URL 字面 :PORT）、src/web.rs:141（ANTHROPIC_BASE_URL 字面 :PORT）、src/web.rs:144（Python SDK 示例 :PORT）、src/web.rs:135（"以 /api/models（后端补 meta 后）为准"）。
- 问题：
  - 面板内所有指南示例硬编码 PORT，复制即错（除非手改成监听端口）；README 则写死 47831，与 config.example.json 的 0.0.0.0:47831 口径不一。
  - 后端没有 /api/models 路由（api.rs:40-59 仅有 /v1/models）——这是"看起来有其实不能用"的死引用；且"补 meta 后"暗示 meta 缺失，而 /v1/models 实际已返回 label/context_window/tools/vision（models.rs:325-346）。
  - 指南从不渲染后端 /api/guide 返回的真实 listen_addr/base_url/models/api_keys_configured（api.rs:1617-1638）；api_keys 已配置时仍写"sk-local（未配置 api_keys 时任意）"会误导。
- 建议：指南区改调 /api/guide 渲染真实地址与模型；/api/models 改 /v1/models；删"补 meta 后"。

### MAJOR-4 模型表价格/能力渲染与真实后端形态脱节
- 位置：src/web.rs:244-248。
- 问题：price = (typeof m.price_per_mtok === 'number') ? '$'+v : (m.price_per_mtok || '-')——上游解析对缺失价格 unwrap_or(0.0)（upstream.rs:218），价格为 0（免费模型）时显示 $0，非 number 的 truthy 字符串会原样显示且不带 $；且 tools/vision 的"未知"降级分支在当前后端（永远返回布尔，models.rs:341,343）下不可达，代码注释与真实行为不一致。
- 建议：价格统一 $x.xx / "免费" 按 isFinite 判空；删除不可达降级说明。

### MAJOR-5 README/面板对免费代理源与静态模型数量描述互相矛盾（13 vs 44、13 vs 12）
- 位置：README.md:117（"免费代理 13 个公开源"）、README.md:5（"13+" 模型）、面板 src/web.rs:88（"13 个"）、实际 src/free_proxy.rs:44（FREE_PROXY_SOURCES 共 44 个）、src/models.rs:41-171（静态 12 个 m!()）。
- 问题：源码 44 源、静态 12 模型，文档/面板仍写 13；README"13 个公开源"会让运维误判免费抓取规模。N5 审计实测面板 4497 代理/24 模型，也证明"13"与运行态不符。
- 建议：README 改 44 源；面板改"12 个静态 + 动态刷新"。

### MAJOR-6 代理表渲染兜底缺失：health_score 无 Number 兜底、colspan 与列数不符
- 位置：src/web.rs:202（it.health_score >= .8，undefined/NaN 走 >= 为 false 并渲染 undefined 文本）、src/web.rs:114（代理表 8 个 th）vs src/web.rs:113 初始行 colspan="6"、src/web.rs:241 模型表 colspan="6"（模型表是 6 列，正确；代理表应为 8）。
- 影响：首屏"加载中…"只占 6 列视觉错位；代理 health_score 字段缺失时显示 undefined。
- 建议：health_score 加 Number 兜底显示 -；代理表所有 colspan 6 改 8。

## 3. MINOR

### MINOR-1 面板 nav 无 ARIA tab 语义（role/aria-selected/aria-controls 缺失，仅 onclick 切换）
- 位置：src/web.rs:23-26、src/web.rs:165-169。已有 :focus-visible（web.rs:27）。建议补 tablist/tab/tabpanel + 方向键。

### MINOR-2 全局错误兜底缺失：loadModels 内 /api/guide 失败被吞、refreshFree/refreshCatalog 无禁用/防并发
- 位置：src/web.rs:238（catch 吞错）、src/web.rs:216-229（点击期间无 disabled，双击并发 POST）、src/web.rs:256-257（页面加载+轮询并行累积）。
- 建议：按钮 disabled + "抓取中…"；全局 unhandledrejection 打 toast。

### MINOR-3 时间格式不统一：toLocaleTimeString() 跨浏览器 12h/24h 混排
- 位置：src/web.rs:207。建议固定 Intl.DateTimeFormat('zh-CN',{hour12:false})。

### MINOR-4 toast 无 id/队列、2.6s 自动消失；连续失败互相覆盖
- 位置：src/web.rs:164。建议加 role="status"（aria-live）+ 队列。

### MINOR-5 空态/加载态/失败态共用 innerHTML 重写，刷新动作间表格闪空白
- 位置：src/web.rs:190-194、src/web.rs:234。建议保留旧数据 + 行级过渡；失败标"上次成功时间"。

### MINOR-6 模型 ID/label/host_port 直接拼进 innerHTML，无 HTML 转义（XSS 纵深）
- 位置：src/web.rs:198-203、src/web.rs:250-251。当前数据源可信，建议统一 escapeHtml 或 textContent。

### MINOR-7 代理表来源/冷却无兜底：空 source 显示空白、cooling=true 但 cooldown_seconds 缺失显示 undefineds
- 位置：src/web.rs:198、src/web.rs:203。建议 - 兜底。

### MINOR-8 无 CSP meta（单文件 HTML），公网转发后面板 XSS 面扩大
- 位置：src/web.rs:5-9。默认 127.0.0.1 风险低，公网暴露时应加 CSP + host 白名单。

### MINOR-9 触控目标偏小：.sm 按钮约 22px 高，低于 WCAG 2.5.8 24x24
- 位置：src/web.rs:47、src/web.rs:36-37。建议 min-height 28px、移动端行 padding 加大。

## 4. INFO

### INFO-1 API_CONTRACT.md 版本示例过期（写 0.1.1，实际 Cargo.toml:3 为 0.1.10；healthz 输出 env!("CARGO_PKG_VERSION")，api.rs:143）
- 位置：docs/API_CONTRACT.md:2,86。

### INFO-2 API_CONTRACT.md "/ui（Basic Auth 保护）"与"GET /ui"重复小节，且未列 / 路由（api.rs:44-45 中 / 与 /ui 同一 handler）
- 位置：docs/API_CONTRACT.md:104-112。

### INFO-3 限流窗口口径四处不一："每 IP 每日约 20 次"（README.md:7,36,170）、"每 IP 每小时约 20 次"（docs/PROTOCOL.md:73、web.rs:88）、"每 IP 每日约 20 次"（docs/API_CONTRACT.md:117）；配置字段 hourly_per_ip 与"每日"冲突
- 影响：排障口径混乱。建议统一为上游实测"每小时"并说明本地按 hourly_per_ip 落地。

### INFO-4 API_CONTRACT.md 指标文案与实现基本一致但未说明 status_class 聚合语义（2xx/4xx/429/5xx）
- 位置：docs/API_CONTRACT.md:36-46 vs src/prod_guard.rs:219-220,250-264。

### INFO-5 面板文本"完全匿名：不需要 Cookie / 登录 / API Key"（web.rs:88）渲染在需 Basic Auth + x-api-key 的面板内，文案与面板自身受控性有轻微张力（上游匿名成立）

### INFO-6 data/proxies.txt 为占位文件（10.255.255.1、192.0.2.1 均为私有/文档地址），load_file 会全部被 sanitize_proxy_url 过滤得 0（proxy_pool.rs:145-175）
- 影响：面板提示"住宅代理文件"实际 0 注入，易误判。建议占位文件加注释或清空。

### INFO-7（承接 N5 快照核对）N5-ui-ux-audit.md:16 称"后端字段未就绪时显示黄色 TODO badge"；当前 web.rs:246 注释明确"不再显示假 TODO"且改为"未知"徽章——N5 快照已过期；且该降级分支在后端永远返回布尔时不可达（关联 MAJOR-4）

## 5. 契约核对表（前端 fetch ↔ 后端响应，逐项）

| 前端使用 | 后端端点 | 结果 |
|---|---|---|
| j('/healthz') → upstream/models/proxies | handle_healthz（api.rs:141-149） | 一致 |
| j('/api/proxies') → items[].host_port/source/latency_ms/daily_uses/capacity_remaining/health_score/cooling/cooldown_seconds/fails + free/residential/total/available/capacity.capacity_remaining | handle_proxies → pool.snapshot（proxy_pool.rs:598-641） | 一致（ProxySnapshot 字段全存在，数值序列化为 JSON number） |
| POST /api/proxies/refresh-free → injected | handle_refresh_free（api.rs:1591-1603） | 一致（free_proxy_enabled=false 时 400 且 toast 显示后端 message） |
| POST /api/catalog/refresh → models | handle_catalog_refresh（api.rs:1604-1616） | 一致（失败返回 502） |
| GET /v1/models → data[].id/label/owned_by/context_window/context/price_per_mtok/tools/vision | handle_v1_models（api.rs:267-276）+ OpenAIModelObject（models.rs:325-346） | 一致 |
| GET /api/guide → models[] 合并进模型表 | handle_guide（api.rs:1617-1638） | 一致（仅当 /v1/models 缺某 id 时补一行） |
| x-api-key 头注入（面板） | handle_dashboard 注入 keys_json（api.rs:124-131）+ check_api_key 认 x-api-key（api.rs:73,83） | 一致（key 为加载时快照，生成后需刷新页面，见 MAJOR-1） |
| 错误读取 d.error.message | ApiError::openai_json（errors.rs:74-84） | 一致 |
| Basic Auth 面板 | handle_dashboard 401 + WWW-Authenticate（api.rs:96-122） | 一致 |

**用户路径完整性**：面板加载（默认开放，正常）→ 总览统计 → 代理池（真实快照+刷新/手动抓取）→ 模型列表（合并 guide）→ 接入指南（字面 PORT 不可用，MAJOR-3）→ 错误 toast → 空态（proxy-empty）→ loading（初始"加载中…"，刷新中缺失 MAJOR-2）。无"点了没反应"按钮；可恢复性：Basic Auth 401 走浏览器弹窗、fetch 401 走 toast，下轮自刷恢复。

## 6. 修复优先级建议

1. 【P0】MAJOR-1：给面板补 API Key 生成/清除 UI（调 /api/config/api-key），并处理生成后需刷新页面的 401 恢复。
2. 【P0】MAJOR-2：j() 加 AbortController；refreshProxies 失败时不先清空 tbody。
3. 【P0】MAJOR-3：指南区调 /api/guide 渲染真实 listen_addr/base_url/models；改 /api/models 为 /v1/models。
4. 【P1】MAJOR-4/5/6：价格与 health_score 兜底、colspan 8、README 13→44、面板 13→12+动态。
5. 【P1】MINOR-1/2/4/6：ARIA tab、全局错误捕获、toast 队列、escapeHtml。
6. 【P2】INFO 项文档对齐：版本号、/ui 小节合并、每小时/每日口径、proxies.txt 占位。
