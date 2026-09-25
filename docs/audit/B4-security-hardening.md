# B4 生产安全/加固审计报告

> 审计角色：只读审计子代理 B（生产安全/加固）
> 日期：2026-09-26　证据基准：HEAD=10f0247（`git log -1`）。
> 只读约束：未修改任何源文件，未执行 commit；仅写入本报告文件。
> 范围：src/api.rs、src/web.rs、src/config.rs、src/prod_guard.rs、src/errors.rs、src/upstream.rs、src/free_proxy.rs、src/proxy_pool.rs、Dockerfile、docker-compose.yml、.github/workflows/*、.github/scripts/deploy.sh、config.example.json、docs/（SERVER_DEPLOYMENT.md / API_CONTRACT.md / NGINX_DEPLOY.md / DOCKER.md 等）。
> 验证方式：代码逐行审读、git 历史检索（60 commits，含 config.json 全部版本 diff）、cargo audit --no-fetch（本地 advisory-db 1269 条；213 条 yanked 在线校验因网络超时未能完成，结果标注）、cargo check 通过、gitleaks 形态扫描（工作区）、HAR 抓包文件仅含 8 组空 cookie（无真实凭据）。

## 结论摘要

- BLOCKER：4
- MAJOR：7
- MINOR：9
- INFO：7
- 依赖漏洞：`cargo audit --no-fetch` 本地库 243 依赖 0 已知漏洞（found=false）；在线 yanked 校验因网络超时未完成，结果附条件。
- 工作区（含 git 全部历史）未发现真实 API Key/密码/Token 落库：docs 中的 `<SERVER_IP>`/`<YOUR_API_KEY>`/`<YOUR_UI_PASSWORD>`/`<API_KEY>` 均为占位符；HAR 的 cookies 数组均为 `[]`。

---

## BLOCKER（必须优先修复）

### B1. `/metrics` 无鉴权暴露运营面
- 位置：`src/api.rs:58-59`（路由）、`src/api.rs:152-162`（handle_metrics）；`src/config.rs:81` `metrics_enabled` 默认 true；`docs/SERVER_DEPLOYMENT.md:41` 明确“metrics…无需鉴权”，并确认公网可访问。
- 问题：未配置任何认证/IP 限制，公网任意人都可读取代理池水位、请求吞吐、熔断状态、活跃会话数。
- 可利用性：无需任何凭据；通过周期性采样可观测业务量、触发熔断后影响、上游健康，辅助滥用规避与竞争对手情报；也暴露“当前上游半死”信号。
- 修复建议：给 /metrics 加与 /api/* 一致的 API key 鉴权（或独立 SCRAPE_TOKEN + Basic Auth/IP 白名单），仅允许 Prometheus 抓取；SG 或防火墙限制源 IP。

### B2. 通过带内授权 Shodan/扫描器可对公网面板/API 做组合探测→低配 VM DoS
- 位置：`docs/SERVER_DEPLOYMENT.md`（Azure 2核 952MB，公网放行 47831 + nginx https 反代）、`src/api.rs:34-38`、`src/api.rs:893-987`（try_rounds 指数退避 + 直连兜底）。
- 问题：服务本身无一等公民的连接级限流、无连接数上限；鉴权失败也走完整 JSON 解析+日志路径。16MB 请求体+并发慢请求（代理池 acquire 阻塞等待 gate、上游 read_timeout 120s）可被少量并发流式请求占满弱 VM。公网 47831 直连 + nginx 443 双入口进一步放大。
- 可利用性：低代价脚本即可（未认证请求也消耗 CPU/日志/内存）；无需 key。
- 修复建议：nginx 层 `limit_req`/`limit_conn`；程序加 Tower 层 per-IP/全局 in-flight 限制；对 `/v1/chat/completions` 等未认证请求立即返回（先鉴权再解析 body 无法用 axum Json 提取器时用 `Bytes` 提取器先查 header）；公网只保留 HTTPS 443 入口，47831 改由防火墙限定。

### B3. 上游匿名直连的“你帮我消耗免费额度”放大面：直连兜底被匿名滥用打爆本机 IP 配额
- 位置：`src/api.rs:953-976`（direct_fallback 直连兜底）、`src/upstream.rs:96-133`（/api/open 匿名请求）、`config.example.json` `direct_fallback: true`。
- 问题：api_keys 配置后仍允许所有持 key 用户无差别用直连兜底，消耗服务器公网 IP 每小时仅约 20 次的匿名配额；全部 key 共享同一配额，任一用户滥用即导致全服务 429（DoS by quota exhaustion）。`hourly_per_ip=20` 是上游硬约束，不是本服务可扩展资源。
- 可利用性：一个 key 高频请求即可占满直连配额；代理池耗尽时兜底成为唯一出口。
- 修复建议：为直连兜底增加独立并发/配额控制（如全局限流 15/h 且到期优先走代理）；把 direct_fallback 改为配置默认 false 并结合住宅代理；文档明示“免费上游匿名配额不可扩展”的容量边界。

### B4. `clear` 的权限判定语义反转：校验通过后才 clear，普通客户持任一 key 可清空全部 key→自锁 + DoS 公共面
- 位置：`src/api.rs:1641-1673`（handle_config_api_key，尤其 1670-1671 `keys.clear()`）。
- 问题：`check_api_key` 只要“任一有效 key”即通过；`clear` 会清空整个 in-memory 列表。任何获发 key 的下游用户（或撞到 key）都能执行 `POST /api/config/api-key {"action":"clear"}`，清空后重新进入“api_keys 空 → 本机放行任意 key”模式（`src/api.rs:70`），等价于关掉认证，且每次 /ui 会话重新注入 key 前所有人都可免鉴权调用（包括面板 /api/proxies、/api/guide 读取）。注释自称“高风险操作”，但没有任何管理级权限区分或确认机制。
- 可利用性：与 B1/B2 组合，匿名者在扫码后无需口令就能炸掉整个网关鉴权；无日志分级告警。
- 修复建议：把生成/删除/清空移到“管理口令”（ui_password 或独立管理 key）+ 二次确认 token；clear 后保留至少一个配置级 key（config api_keys 不可被运行时清空，只清运行时新增）；审计日志记录操作者。

---

## MAJOR（高优先）

### M1. 面板 XSS：innerHTML 直接拼接上游可控字段（模型 id/label）与代理数据
- 位置：`src/web.rs:250-252`（模型表格 `tr.innerHTML = \`<td>${id}</td><td>${m.label ...}\`...`）、`src/web.rs:198-205`（代理表 innerHTML 拼接 host_port/source/health_score/latency_ms）、`src/web.rs:254`（错误信息拼进 innerHTML）。
- 数据源：`parse_catalog_chunk`（`src/upstream.rs:226-263`）解析的上游 JS chunk 里的 id/label 原样入库；`/api/proxies` 数据来自住宅代理文件 + 免费源，host_port 也原样。面板只受 Basic Auth 保护（有时常为普通文本口令），且 `/` 与 `/ui` 同源可被利用链放大（API key 注入在页面 JS 变量 API_KEYS 中，`src/api.rs:130-137`）。
- 可利用性：上游目录/代理文件一旦被污染（或恶意免费代理列表携带 `<img onerror>`），登录面板者执行任意 JS；进一步可读取 API_KEYS 并调用 /api/config/api-key（无 CSRF 防护时跨站也可调用，见 M3）。
- 修复建议：全部改用 `createElement`+`textContent`（或 DOMPurify）；上游 label/id 入库前做字符白名单；面板 HTML 增加 CSP（`default-src 'self'`）+ `X-Content-Type-Options`。

### M2. `/healthz` 与错误响应反射上游错误文本，泄露上游内部状态/格式
- 位置：`src/api.rs:141-148`（healthz 返回 upstream）、`src/errors.rs` openai_json/anthropic_json 直接回显 `message()`；`src/api.rs:982-987`、`963-970` 把上游响应体截断文本拼进给客户端的错误 message。
- 问题：`upstream-429: ...`/HTTP 4xx 原文直接回吐给未认证/普通客户端，帮助对手指纹上游协议与限流语义；healthz 反射 upstream_base_url 便于确认部署拓扑。
- 可利用性：低，属于信息泄露面放大。
- 修复建议：对外错误统一本地化文案（分类码 + 不携带上游原文），仅 debug 日志保留原始文本（并受 redact_logs 控制）。

### M3. 面板后端 API 无 CSRF 防护；Basic Auth 凭证会随浏览器自动/可被脚本提交
- 位置：`src/api.rs:104-125`（Basic Auth 校验只在 / 与 /ui）、`src/api.rs:1591-1615`（/api/proxies/refresh-free、/api/catalog/refresh 仅 API key 鉴权）、`src/web.rs:163-166`。
- 问题：面板自身通过 `x-api-key` 头访问 API，跨站表单/图片请求无法带自定义头（有一定天然 CSRF 缓解）；但 `/api/config/api-key`、`refresh-free`、`catalog/refresh` 若被其它同源页面/注入 JS 调用即可触发（POST + JSON body 需要 fetch，跨站 OAuth flow 仍可能通过简单表单 + text/plain 组合触发一部分）。同时 Basic Auth 凭据在无 SSL 的 47831 直连（HTTP）下明文传播，任何中间人/同网段可截获 ui_password 与 API key（见 M6）。
- 可利用性：中（需攻击者能注入页面或构造跨站请求；HTTP 明文下直接嗅探）。
- 修复建议：面板经 HTTPS 反代访问；管理 API 增加 SameSite/Origin 校验 + 一次性 CSRF token；UI_PASSWORD 改哈希存储或强制随机高强度口令。

### M4. 限流按 API key（可无限自增），无 per-IP/匿名层；`x-api-key` 可伪造绕过
- 位置：`src/prod_guard.rs:24-98`（RateLimiter 固定窗口、in-memory、key 为请求头原值）、`src/api.rs:165-183`（request_key 取 Bearer 或 x-api-key）、`src/api.rs:710-731`。
- 问题：(a) 限流 key 完全由客户端可控（先 Bearer 后 x-api-key），每个 key 独立配额 → 攻击者自行生成无数 key（/api/config/api-key 无需口令即可 generate，见 B4）或直接轮换 x-api-key 头规避 60/h 限流；(b) `api_keys.is_empty()` 时全部匿名请求通过检查，但限流只在“有 key”时生效 → 未配置 key 的公网部署完全没有限流；(c) 限流在鉴权之后，未认证暴力尝试不计入；`sweep` 每 256 次清理一次，恶意制造大量唯一 key 可使 map 无界增长至 rams 耗尽（每个 key 一条）。
- 可利用性：高（配合 B4 可制造任意 key/无界 map）。
- 修复建议：鉴权前先查 X-Forwarded-For/直连 IP 做 per-IP+全局漏斗；key 数量上限制约内存；拒绝自动生成超过阈值；清空/降级场景重算。

### M5. SSRF 校验的 IPv6 与保留网段覆盖不完整
- 位置：`src/free_proxy.rs:115-132`（is_valid_public_ip 对 V6 只排除 loopback/multicast/unspecified）、`src/proxy_pool.rs:145-179`（sanitize_proxy_url 同样 V6 只排三项）。
- 问题：IPv6 的链路本地 `fe80::/10`、ULA `fc00::/7`、`::ffff:` 映射 IPv4、`::1` 已排除，但链路本地/ULA 段未排除；同时所有解析路径都要求“host 必须是字面 IP”（拒绝域名），所以 DNS rebinding 类绕不开；proxy 值经 `reqwest::Proxy::all(p)` 后仅作为代理转发目标，不是服务端打开 URL 的被代理地址——SSRF 的数据通道风险主要在于“把内网代理当跳板”以及“/api/proxies”把内网地址漏给客户端（见 M7）。住宅文件/免费源是第三方可控，若 10.x IP 被误放行（如 `192.0.2.1` TEST-NET 已被 v4 排除、但 `10.255.255.1` 属 private 已被排除——见 M7 实测），仍属纵深不足。
- 可利用性：低-中（依赖第三方代理列表污染 + 内网可达性）。
- 修复建议：V6 补充 `is_unspecified`/link-local/ULA/mapped-v4 检查；对代理 URL 的 host 强制公网单播；对住宅文件增加来源校验。

### M6. 公网 47831 直连 HTTP 明文：凭证/错误文本/代理列表全部明文
- 位置：`docs/SERVER_DEPLOYMENT.md:35-41,84-87`（“生产用 HTTP（Azure 公网）”+ 公网直连验证）、`docs/NGINX_DEPLOY.md`（HTTPS 反代已上线，但 47831 直连放行仍在）、`docker-compose.yml:9-12`（端口 47831 全端口映射）。
- 问题：BASIC 认证（ui_password 明文 base64）、API key（Bearer/x-api-key）、代理列表 host:port、html 注入的 key 全部由公网 HTTP 明文传输；与 imagefree 共用证书但证书/HTTPS 配置仅在 nginx。
- 可利用性：高（同网段/路径上 MITM，或运营商/代理链路可嗅探）。
- 修复建议：Azure NSG 移除 22/47831 公网入口（SSH 改 key-only + bastion；47831 仅 nginx 本机回环）；全部流量强制 HTTPS 反代；面板加 HSTS（nginx add_header）并考虑在应用内合并。

### M7. `/api/proxies` 快照把“来源”与 host_port 展示给所有持 key 用户；住宅文件样本含内网/文档地址会被展示（防御纵深缺口）
- 位置：`src/proxy_pool.rs:275-290`（snapshot() 返回 items 含 source/host_port/health 等）、`src/api.rs:1583-1589`（仅 api key 校验）、`data/proxies.txt`（实测内容 `http://10.255.255.1:8080`、`http://192.0.2.1:3128` —— 分别是 RFC1918 私网与 TEST-NET-1 文档段）。
- 问题：当前 `sanitize_proxy_url` 会把这两行都过滤掉（10.x=private、192.0.2=documentation），这是对的；但一旦过滤逻辑退化（版本回滚）或来源为免费代理列表，内网/元数据地址可能进入池并被快照展示；且任何持 key 用户可读取全部出口 IP 画像（配合容量字段做资源规划）。这是配置随仓库提交（config.json 被 60 个 commit 跟踪）之外的第二个泄露面。
- 可利用性：低（当前过滤生效），但暴露面存在。
- 修复建议：快照默认只返回计数字段，items 需显式打开（管理口）；代理条目脱敏为 IP 网段（如 `10.x.x.x`）；data/proxies.txt 从仓库/镜像独占（含 .dockerignore 已做），服务器端改为仅 root 可读。

---

## MINOR

1. **MINOR-1** 代理池全局并发 gate 阻塞语义：`acquire()` 在 `gate.acquire_owned().await` 阻塞等待许可（`src/proxy_pool.rs:340-344`），配合 120s 读超时可在配额耗尽时把请求挂起最多 ~120s 而非快速 503；建议超时门控+队列上限。
2. **MINOR-2** `try_rounds` 指数退避 `2^attempt` 秒在 max_attempts 内串行 sleep，单个失败链最坏 ~14s+3 次代理尝试才返回；无 per-request deadline，攻击者可用慢失效代理堆高并发睡死连接。
3. **MINOR-3** `collect_nonstream`（`src/api.rs:1022-1068`）无输出长度上限：非流式响应可被上游无限增长累积到内存；16MB 只限制入 1 个 16MB 请求，但一个响应可远大于请求体。
4. **MINOR-4** redact 正则在 `src/api.rs:230-236`：仅对 `detail` 前 300 字符做 `sk-[a-z0-9]{8,}`/`bearer` 剥离；`x-api-key: <sk-to-...>` 形态（sk-to 前缀带连字符）不匹配 `sk-[a-z0-9]`；且 300 字符后不做任何检查；`redact_logs=false` 时完全无脱敏（文档明示，但生产中排障期容易误开）。
5. **MINOR-5** `log_request` 把 model 与 status 直接打印；model 由客户端传入（`src/api.rs:690-700`），可注入换行/伪造日志（日志注入）；无结构化字段转义。
6. **MINOR-6** `check_api_key` 为常数时间不安全（`==` 字符串比较，`src/api.rs:84`）；只是本地网关、影响低，但仍属可命名缺陷。
7. **MINOR-7** `CircuitBreaker::allow` 在 HALF_OPEN 时拒绝所有在途请求且无并发控制（`src/prod_guard.rs:117-150`），探测期间 30s 内其它请求全 503——可被单次失败触发全站拒绝，建议 HALF_OPEN 并发窗口=1 的语义下仍允许少量请求排队。
8. **MINOR-8** `docker-compose.yml` 无镜像 digest/pin、无 healthcheck、`restart: unless-stopped` 但不限制资源（`docker-compose.yml:9-19`）；容器内 `appuser` 对挂载目录写权限依赖宿主机权限（DOCKER.md 建议 `chmod -R 777 data`），权限过宽。
9. **MINOR-9** `.github/workflows/cd-deploy.yml`：SSH 密码登录（root 密码存 GitHub Secret）、`AutoAddPolicy()` 接受任意 host key、`exec_command` 无超时（timeout=950s 是为规避但仍是长阻塞）、服务器 `/root` 下明文解压源码。建议改 SSH key + known_hosts 固定 + 专用 deploy 用户。

---

## INFO

1. **INFO-1** `handle_config_api_key` 的 generate/set 不写盘（只改内存），进程重启即丢；文档与行为一致（API_CONTRACT.md），但运维若依赖持久化 key 会踩坑。
2. **INFO-2** `src/config.rs:222-268` 环境变量覆盖：`API_KEYS`/`UI_PASSWORD` 支持 env 注入，但 `config.json` 本身被 git 跟踪（60 commits），历史上未出现真实凭据（全量 diff 已核对）；建议 config.json 移出仓库（仅保留 config.example.json）。
3. **INFO-3** HAR 文件（`源代码、网络数据包/www.tryingopen.com.har`，268KB）已入库，8 组 cookies 均为 `[]`，无凭据；但该文件属“参考数据”，建议加入 .gitignore 以减小仓库面。
4. **INFO-4** `docs/SERVER_DEPLOYMENT.md` 使用 `<SERVER_IP>`/`<YOUR_API_KEY>`/`<YOUR_UI_PASSWORD>` 占位符，无真实值；建议在文档头部加“禁止填写真实值”警示。
5. **INFO-5** `/ui` 与 `/` 各自独立 Basic Auth（`src/api.rs:96-125`），但 `/healthz`、`/metrics`、`/v1/models` 等不在面板鉴权范围内——认知上“面板有密码=全部受保护”是错的，需在文档强调。
6. **INFO-6** 日志级别默认 `info,tryingopen2api=debug`（`src/main.rs:14-17`），debug 级会打印 `PROXY_OK ... proxy=direct` 等出口信息；生产应降为 info 且检查日志脱敏。
7. **INFO-7** `free_proxy_enabled` 默认 true（`config.rs` default），预检真实 HTTP 到 `http://www.gstatic.com/generate_204`（`src/free_proxy.rs:203-206`）每 30 分钟触发 50 并发出站——在受限网络/审计环境下属外联行为，需在网络策略中显式允许。

---

## 证据与验证记录

- 依赖：`cargo audit --no-fetch --json` → `{"vulnerabilities":{"found":false,"count":0,"list":[]},"lockfile":{"dependency-count":243}}`；在线 yanked 校验请求 `could not be completed in the allotted timeframe`（网络受限），结论基于本地 advisory-db 1269 条。
- 关键版本：axum 0.8.9、reqwest 0.12.28（rustls-tls）、tokio 1.53.1、hyper 1.11.1、rustls 0.23.45、ring 0.17.14、libsqlite3-sys 0.30.1（bundled）。
- 编译：`cargo check` exit 0。
- 密钥扫描：全仓库（含 git 历史 60 commits、docs、workflows）未发现真实凭据；仅占位符。
- SSRF 现状：住宅/免费代理注入均过 `sanitize_proxy_url`/`is_valid_public_ip`（私网/回环/链路本地/组播/未指定/广播/文档地址被拒）；V6 覆盖不足见 M5。
- 代理文件实测：`data/proxies.txt` 含 `http://10.255.255.1:8080` 与 `http://192.0.2.1:3128`（当前会被过滤，作为 M7 证据）；该文件已被 .gitignore/.dockerignore 排除。

## 总体加固建议（优先级排序）

1. 立即：limits/鉴权前置、/metrics 加认证、清空 key 路径加管理口令（封堵 B1/B4 组合）。
2. 立即：Azure NSG 只留 443（HTTPS），47831/22 公网关闭；SSH key-only（B3/M6）。
3. 短期：面板 XSS 全面 textContent + CSP；限流加 per-IP + 鉴权前门控 + key 数量上限（M1/M4）。
4. 短期：IPv6 SSRF 校验补全、直连兜底配额收紧、非流式输出上限（M5/MINOR-3/B3）。
5. 中期：config.json 移出 git、HAR/参考文件移出仓库、deploy 改非 root + key auth（INFO-2/3/9）。
