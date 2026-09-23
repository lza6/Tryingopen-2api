//! 内置 Web 控制面板（总览 / 代理池 / 模型 / 接入指南）
//!
//! 轻量无构建：单 HTML + 原生 JS + CSS，Rust 直接内嵌。

pub const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>TryingOpen2API 控制台</title>
<style>
:root { --bg:#0b0f14; --card:#131a23; --border:#24303d; --text:#e6edf3; --muted:#8b98a5; --accent:#38bdf8; --ok:#34d399; --warn:#fbbf24; --err:#f87171; }
* { box-sizing:border-box; margin:0; padding:0; }
body { background:var(--bg); color:var(--text); font-family:-apple-system,'Segoe UI',Roboto,'Microsoft YaHei',sans-serif; min-height:100vh; }
@media (prefers-reduced-motion: reduce) { * { animation:none !important; transition:none !important; } }
header { display:flex; align-items:center; justify-content:space-between; padding:14px 24px; border-bottom:1px solid var(--border); background:var(--card); position:sticky; top:0; z-index:10; }
header h1 { font-size:16px; font-weight:600; }
header .dot { display:inline-block; width:8px; height:8px; border-radius:50%; background:var(--muted); margin-right:8px; vertical-align:middle; }
header .dot.ok { background:var(--ok); } header .dot.err { background:var(--err); }
.hstat { display:flex; gap:16px; font-size:12px; color:var(--muted); }
.hstat b { color:var(--text); }
main { max-width:1180px; margin:0 auto; padding:20px 24px 60px; }
nav { display:flex; gap:2px; margin-bottom:20px; border-bottom:1px solid var(--border); flex-wrap:wrap; }
nav button { background:transparent; border:none; color:var(--muted); padding:10px 16px; cursor:pointer; font-size:14px; border-bottom:2px solid transparent; }
nav button.active { color:var(--text); border-bottom-color:var(--accent); }
nav button:hover { color:var(--text); }
button:focus-visible, input:focus-visible, textarea:focus-visible, select:focus-visible, a:focus-visible { outline:2px solid #7dd3fc; outline-offset:2px; }
.cards { display:grid; grid-template-columns:repeat(auto-fit,minmax(170px,1fr)); gap:14px; margin-bottom:20px; }
.card { background:var(--card); border:1px solid var(--border); border-radius:10px; padding:14px 16px; }
.card .num { font-size:24px; font-weight:700; margin-top:4px; }
.card .lbl { color:var(--muted); font-size:13px; }
.panel { background:var(--card); border:1px solid var(--border); border-radius:10px; padding:16px; margin-bottom:20px; }
.panel h2 { font-size:15px; margin-bottom:12px; color:var(--muted); font-weight:600; }
table { width:100%; border-collapse:collapse; font-size:13px; }
th,td { text-align:left; padding:8px 10px; border-bottom:1px solid var(--border); }
th { color:var(--muted); font-weight:500; }
.badge { display:inline-block; padding:2px 8px; border-radius:20px; font-size:12px; }
.badge.ok { background:#0c3b2e; color:var(--ok); } .badge.warn { background:#3d2f0a; color:var(--warn); }
.badge.err { background:#3b0f0f; color:var(--err); } .badge.dim { background:#1c2530; color:var(--muted); }
.chip { display:inline-block; background:#1c2530; border:1px solid var(--border); border-radius:6px; padding:2px 8px; margin:2px; font-size:12px; }
button { background:var(--accent); color:#04121a; border:none; border-radius:6px; padding:6px 14px; cursor:pointer; font-size:13px; font-weight:600; }
button:hover { filter:brightness(1.12); }
button:disabled { opacity:.5; cursor:not-allowed; }
button.ghost { background:transparent; border:1px solid var(--border); color:var(--text); }
button.ghost:hover { border-color:var(--accent); color:var(--accent); }
button.sm { padding:3px 10px; font-size:12px; }
input,textarea,select { background:var(--bg); border:1px solid var(--border); color:var(--text); border-radius:6px; padding:8px 10px; font-size:13px; width:100%; font-family:inherit; }
input:focus,textarea:focus,select:focus { border-color:var(--accent); }
label { display:block; color:var(--muted); font-size:12px; margin:8px 0 4px; }
.row { display:flex; gap:8px; align-items:center; flex-wrap:wrap; }
.empty { color:var(--muted); font-size:13px; padding:12px 4px; }
#toast { position:fixed; bottom:24px; left:50%; transform:translateX(-50%); background:var(--card); border:1px solid var(--accent); padding:10px 20px; border-radius:10px; display:none; z-index:100; font-size:13px; box-shadow:0 8px 24px rgba(0,0,0,.5); }
.guide-box { background:var(--bg); border:1px solid var(--border); border-radius:8px; padding:10px 12px; font-family:ui-monospace,Consolas,monospace; font-size:12px; white-space:pre-wrap; word-break:break-all; }
</style>
</head>
<body>
<header>
  <h1><span class="dot" id="dot"></span>TryingOpen2API 控制台</h1>
  <div class="hstat">
    <span>上游 <b id="up"></b></span>
    <span>模型 <b id="mc">-</b></span>
    <span>代理 <b id="pc">-</b></span>
  </div>
</header>
<main>
  <nav>
    <button data-tab="overview" class="active">总览</button>
    <button data-tab="proxies">代理池</button>
    <button data-tab="models">模型</button>
    <button data-tab="guide">接入指南</button>
  </nav>

  <section id="tab-overview">
    <div class="cards">
      <div class="card"><div class="lbl">模型数</div><div class="num" id="c-models">-</div></div>
      <div class="card"><div class="lbl">代理总数</div><div class="num" id="c-proxies">-</div></div>
      <div class="card"><div class="lbl">免费代理</div><div class="num" id="c-free">-</div></div>
      <div class="card"><div class="lbl">可用代理</div><div class="num" id="c-avail">-</div></div>
    </div>
    <div class="panel"><h2>说明</h2><p style="line-height:1.8;color:var(--muted);font-size:13px">
      TryingOpen2API 把 <b>tryingopen.com</b> 的免费开源模型（13 个）逆向为 OpenAI / Anthropic 兼容本地网关。
      完全匿名：<b>不需要 Cookie / 登录 / API Key</b>。站点按「每 IP 每小时约 20 次」限流，
      网关自动用代理池轮换出口 IP（住宅代理文件 + 免费代理源），429 时自动换出口并退避重试，
      全部失败后直连兜底。
    </p></div>
  </section>

  <section id="tab-proxies" style="display:none">
    <div class="panel">
      <div class="row" style="margin-bottom:12px">
        <button class="ghost sm" onclick="refreshProxies()">刷新</button>
        <button class="ghost sm" onclick="refreshFree()">手动抓免费代理</button>
        <span style="color:var(--muted);font-size:12px">住宅代理文件在 config.json 的 proxy_file 字段（每行一个 http://user:pass@host:port）</span>
      </div>
      <div class="empty" id="proxy-empty">暂无代理数据</div>
      <table id="proxy-table" style="display:none">
        <thead><tr><th>出口</th><th>来源</th><th>今日次数</th><th>健康分</th><th>冷却</th><th>连续失败</th></tr></thead>
        <tbody></tbody>
      </table>
    </div>
  </section>

  <section id="tab-models" style="display:none">
    <div class="panel">
      <div class="row" style="margin-bottom:12px">
        <button class="ghost sm" onclick="refreshCatalog()">同步上游目录</button>
        <span style="color:var(--muted);font-size:12px">启动时自动抓取；失败保留内置静态目录</span>
      </div>
      <table id="model-table">
        <thead><tr><th>模型 ID</th><th>名称</th><th>上下文</th><th>价格/M</th><th>工具</th><th>视觉</th></tr></thead>
        <tbody></tbody>
      </table>
    </div>
  </section>

  <section id="tab-guide" style="display:none">
    <div class="panel"><h2>OpenAI 兼容</h2>
      <div class="guide-box">Base URL: http://127.0.0.1:PORT/v1&#10;API Key: sk-local（未配置 api_keys 时任意）&#10;模型: qwen/qwen3.8-27b</div>
    </div>
    <div class="panel"><h2>Anthropic 兼容（Claude Code）</h2>
      <div class="guide-box">ANTHROPIC_BASE_URL=http://127.0.0.1:PORT&#10;ANTHROPIC_API_KEY=sk-local</div>
    </div>
    <div class="panel"><h2>Python OpenAI SDK</h2>
      <div class="guide-box">from openai import OpenAI&#10;client = OpenAI(base_url="http://127.0.0.1:PORT/v1", api_key="sk-local")&#10;resp = client.chat.completions.create(model="qwen/qwen3.8-27b", messages=[{"role":"user","content":"你好"}])&#10;print(resp.choices[0].message.content)</div>
    </div>
  </section>
</main>
<div id="toast"></div>
<script>
const PORT = location.port || "47831";
const API = "";
async function j(path, opts) {
  const r = await fetch(API + path, opts);
  const d = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(d?.error?.message || (r.status + " " + r.statusText));
  return d;
}
function toast(msg) { const t = document.getElementById('toast'); t.textContent = msg; t.style.display = 'block'; setTimeout(() => t.style.display = 'none', 2600); }
document.querySelectorAll('nav button').forEach(b => b.onclick = () => {
  document.querySelectorAll('nav button').forEach(x => x.classList.remove('active'));
  b.classList.add('active');
  document.querySelectorAll('main section').forEach(s => s.style.display = 'none');
  document.getElementById('tab-' + b.dataset.tab).style.display = '';
});
async function loadOverview() {
  try {
    const h = await j('/healthz');
    document.getElementById('dot').className = 'dot ok';
    document.getElementById('up').textContent = h.upstream || '-';
    document.getElementById('mc').textContent = h.models;
    document.getElementById('pc').textContent = h.proxies;
    document.getElementById('c-models').textContent = h.models;
    document.getElementById('c-proxies').textContent = h.proxies;
    const snap = await j('/api/proxies');
    document.getElementById('c-free').textContent = snap.free ?? 0;
    document.getElementById('c-avail').textContent = snap.available ?? 0;
  } catch (e) { document.getElementById('dot').className = 'dot err'; toast('加载失败: ' + e.message); }
}
async function refreshProxies() {
  try {
    const snap = await j('/api/proxies');
    const tb = document.querySelector('#proxy-table tbody');
    tb.innerHTML = '';
    const items = snap.items || [];
    document.getElementById('proxy-empty').style.display = items.length ? 'none' : '';
    document.getElementById('proxy-table').style.display = items.length ? '' : 'none';
    for (const it of items) {
      const tr = document.createElement('tr');
      tr.innerHTML = `<td>${it.host_port || '-'}</td><td>${it.source || ''}</td><td>${it.daily_uses ?? 0}</td>` +
        `<td><span class="badge ${it.health_score >= .8 ? 'ok' : it.health_score >= .4 ? 'warn' : 'err'}">${it.health_score}</span></td>` +
        `<td>${it.cooling ? it.cooldown_seconds + 's' : '✓'}</td><td>${it.fails ?? 0}</td>`;
      tb.appendChild(tr);
    }
    loadOverview();
  } catch (e) { toast('代理池读取失败: ' + e.message); }
}
async function refreshFree() {
  try {
    const d = await j('/api/proxies/refresh-free', { method: 'POST' });
    toast('免费代理注入 ' + (d.injected ?? 0) + ' 个');
    refreshProxies();
  } catch (e) { toast(e.message); }
}
async function refreshCatalog() {
  try {
    const d = await j('/api/catalog/refresh', { method: 'POST' });
    toast('模型目录已同步: ' + (d.models ?? 0) + ' 个');
    loadModels();
  } catch (e) { toast('目录同步失败: ' + e.message); }
}
async function loadModels() {
  try {
    const list = await j('/v1/models');
    const tb = document.querySelector('#model-table tbody');
    tb.innerHTML = '';
    const metas = {};
    for (const m of list.data || []) metas[m.id] = m;
    const ids = Object.keys(metas);
    if (ids.length === 0) { tb.innerHTML = '<tr><td colspan="6" class="empty">暂无模型</td></tr>'; return; }
    for (const id of ids) {
      const tr = document.createElement('tr');
      tr.innerHTML = `<td>${id}</td><td>${metas[id].owned_by || ''}</td><td>-</td><td>-</td><td>-</td><td>-</td>`;
      tb.appendChild(tr);
    }
  } catch (e) { toast('模型读取失败: ' + e.message); }
}
loadOverview(); refreshProxies(); loadModels();
</script>
</body>
</html>
"##;
