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
td.num { text-align:right; }
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
button.danger { background:#7f1d1d; color:#fecaca; }
button.sm { padding:3px 10px; font-size:12px; min-height:28px; }
.guide-box { white-space:pre-wrap; word-break:break-word; }
input,textarea,select { background:var(--bg); border:1px solid var(--border); color:var(--text); border-radius:6px; padding:8px 10px; font-size:13px; width:100%; font-family:inherit; }
input:focus,textarea:focus,select:focus { border-color:var(--accent); }
label { display:block; color:var(--muted); font-size:12px; margin:8px 0 4px; }
.row { display:flex; gap:8px; align-items:center; flex-wrap:wrap; }
.empty { color:var(--muted); font-size:13px; padding:12px 4px; }
.statbar { display:flex; gap:18px; flex-wrap:wrap; font-size:13px; padding:10px 12px; margin-top:12px; border:1px dashed var(--border); border-radius:8px; color:var(--muted); }
.statbar b { color:var(--text); }
.icon-dot { display:inline-block; width:6px; height:6px; border-radius:50%; background:var(--ok); margin-right:6px; vertical-align:middle; animation:blink 1.4s infinite; }
@keyframes blink { 0%,100%{opacity:1} 50%{opacity:.25} }
#toast { position:fixed; bottom:24px; left:50%; transform:translateX(-50%); background:var(--card); border:1px solid var(--accent); padding:10px 20px; border-radius:10px; display:none; z-index:100; font-size:13px; box-shadow:0 8px 24px rgba(0,0,0,.5); }
.tblwrap { overflow-x:auto; }
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
      <div class="card"><div class="lbl">剩余可用次数</div><div class="num" id="c-capacity">-</div></div>
    </div>
    <div class="panel"><h2>说明</h2><p style="line-height:1.8;color:var(--muted);font-size:13px">
      TryingOpen2API 把 <b>tryingopen.com</b> 的免费开源模型（内置 12 个 + 动态目录）逆向为 OpenAI / Anthropic 兼容本地网关。
      完全匿名：<b>不需要 Cookie / 登录 / API Key</b>。站点按「每 IP 每日约 20 次」限流，
      网关自动用代理池轮换出口 IP（住宅代理文件 + 免费代理源），429 时自动换出口并退避重试，
      容量 = 可用出口 IP × 每 IP 每日 20 次 − 已用次数，
      全部失败后直连兜底。
    </p></div>
  </section>

  <section id="tab-proxies" style="display:none">
    <div class="panel">
      <div class="row" style="margin-bottom:12px">
        <button class="ghost sm" onclick="refreshProxies()">刷新</button>
        <button class="ghost sm" id="btn-refreshfree" onclick="refreshFree()">手动抓免费代理</button>
        <span style="color:var(--muted);font-size:12px">住宅代理文件在 config.json 的 proxy_file 字段（每行一个 http://user:pass@host:port）</span>
      </div>
      <div class="statbar">
        <span><span class="icon-dot"></span>15s 自动刷新</span>
        <span>上次刷新 <b id="proxy-last">-</b></span>
        <span>免费 <b id="proxy-free">-</b></span>
        <span>住宅 <b id="proxy-res">-</b></span>
        <span>总量 <b id="proxy-total">-</b></span>
        <span>可用出口 <b id="proxy-avail">-</b></span>
        <span>剩余次数 <b id="proxy-cap">-</b></span>
      </div>
      <div class="empty" id="proxy-empty">暂无代理数据</div>
      <div class="tblwrap"><table id="proxy-table" style="display:none">
        <thead><tr><th>出口</th><th>来源</th><th>延迟</th><th>今日次数</th><th>容量剩余</th><th>健康分</th><th>冷却</th><th>连续失败</th></tr></thead>
        <tbody><tr><td colspan="6" class="empty">加载中…</td></tr></tbody>
      </table></div>
    </div>
  </section>

  <section id="tab-models" style="display:none">
    <div class="panel">
      <div class="row" style="margin-bottom:12px">
        <button class="ghost sm" id="btn-refreshcat" onclick="refreshCatalog()">同步上游目录</button>
        <span style="color:var(--muted);font-size:12px">启动时自动抓取；失败保留内置静态目录</span>
      </div>
      <div class="tblwrap"><table id="model-table">
        <thead><tr><th>模型 ID</th><th>名称</th><th>上下文</th><th>价格/M</th><th>工具</th><th>视觉</th></tr></thead>
        <tbody><tr><td colspan="6" class="empty">加载中…</td></tr></tbody>
      </table></div>
    </div>
  </section>

  <section id="tab-guide" style="display:none">
    <div class="panel"><h2>API Key</h2>
      <div class="row">
        <button class="ghost sm" id="btn-genkey">生成 Key</button>
        <button class="danger sm" id="btn-clearkeys">清空全部动态 Key</button>
        <span style="color:var(--muted);font-size:12px">动态 Key 仅存内存，重启后失效；config.json 的 api_keys 是持久 Key。</span>
      </div>
      <div class="row" style="margin-top:8px">
        <input id="new-key" readonly placeholder="生成后显示在这里（仅当前页面可见）" autocomplete="off" spellcheck="false">
      </div>
    </div>
    <div class="panel"><h2>接入信息（实时）</h2>
      <div id="guide-live" class="guide-box">加载中…</div>
    </div>
    <div class="panel"><h2>默认 effort（思考程度）</h2>
      <div class="guide-box">客户端在 chat/completions 请求体传 <b>effort</b> 字段：&#10;- balanced：默认，均衡思考&#10;- deep：深度思考（更慢更稳）&#10;- low：低思考（更快更省）&#10;当前主要支持 reasoning 类模型（如 qwen/qwen3.8-27b）；不支持时模型会忽略该字段。模型支持情况以 /v1/models 为准。</div>
    </div>
    <div class="panel"><h2>OpenAI 兼容</h2>
      <div id="guide-openai" class="guide-box">…</div>
    </div>
    <div class="panel"><h2>Anthropic 兼容（Claude Code）</h2>
      <div id="guide-anthropic" class="guide-box">…</div>
    </div>
    <div class="panel"><h2>Python OpenAI SDK</h2>
      <div id="guide-python" class="guide-box">…</div>
    </div>
  </section>
</main>
<div id="toast"></div>
<script>
const API = "";
// 后端渲染面板时注入当前 API key（面板已过 Basic Auth / 会话级 key 自举）
const API_KEYS = __API_KEYS_JSON__;
function esc(v) {
  return String(v ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
}
async function j(path, opts) {
  opts = opts || {};
  const headers = new Headers(opts.headers || {});
  if (Array.isArray(API_KEYS) && API_KEYS.length > 0) headers.set('x-api-key', API_KEYS[0]);
  headers.set('Accept', 'application/json');
  opts.headers = headers;
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), 10000);
  try {
    const r = await fetch(API + path, Object.assign({}, opts, { signal: ctl.signal }));
    const d = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(d?.error?.message || (r.status + " " + r.statusText));
    return d;
  } finally { clearTimeout(timer); }
}
let toastTimer = null;
function toast(msg) {
  const t = document.getElementById('toast');
  t.textContent = msg; t.style.display = 'block';
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.style.display = 'none'; }, 2600);
}
function setBtnBusy(id, busy, labelOn) {
  const b = document.getElementById(id);
  if (!b) return;
  if (busy) { b.dataset.orig = b.textContent; b.textContent = labelOn || '处理中…'; b.disabled = true; }
  else { b.textContent = b.dataset.orig || b.textContent; b.disabled = false; }
}
document.querySelectorAll('nav button').forEach(b => b.onclick = () => {
  document.querySelectorAll('nav button').forEach(x => { x.classList.remove('active'); x.setAttribute('aria-selected','false'); });
  b.classList.add('active'); b.setAttribute('aria-selected','true');
  document.querySelectorAll('main section').forEach(s => s.style.display = 'none');
  document.getElementById('tab-' + b.dataset.tab).style.display = '';
  if (b.dataset.tab === 'guide') loadGuide();
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
    const capRem = snap.capacity?.capacity_remaining;
    document.getElementById('c-capacity').textContent = (typeof capRem === 'number' && isFinite(capRem)) ? capRem : '-';
  } catch (e) { document.getElementById('dot').className = 'dot err'; toast('加载失败: ' + e.message); }
}
async function refreshProxies() {
  const tb = document.querySelector('#proxy-table tbody');
  try {
    const snap = await j('/api/proxies');
    const items = snap.items || [];
    document.getElementById('proxy-empty').style.display = items.length ? 'none' : '';
    document.getElementById('proxy-table').style.display = items.length ? '' : 'none';
    const fmtNum = (v) => (typeof v === 'number' && isFinite(v)) ? v : '-';
    const rows = items.map(it => {
      const hs = fmtNum(it.health_score);
      const hsCls = (hs === '-') ? 'dim' : (hs >= .8 ? 'ok' : hs >= .4 ? 'warn' : 'err');
      const cool = it.cooling ? (it.cooldown_seconds != null ? it.cooldown_seconds + 's' : '冷却中') : '✓';
      const src = esc(it.source || '-');
      const hp = esc(it.host_port || '-');
      const lat = (typeof it.latency_ms === 'number' && isFinite(it.latency_ms)) ? it.latency_ms + ' ms' : '-';
      const du = (typeof it.daily_uses === 'number') ? it.daily_uses : 0;
      const cap = fmtNum(it.capacity_remaining);
      const fails = (typeof it.fails === 'number') ? it.fails : 0;
      return `<tr><td>${hp}</td><td>${src}</td><td class="num">${lat}</td><td class="num">${du}</td><td class="num">${cap}</td><td><span class="badge ${hsCls}">${hs}</span></td><td>${cool}</td><td class="num">${fails}</td></tr>`;
    });
    tb.innerHTML = rows.join('') || '<tr><td colspan="8" class="empty">暂无代理数据</td></tr>';
    const now = new Date();
    document.getElementById('proxy-last').textContent = new Intl.DateTimeFormat('zh-CN',{hour12:false,month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',second:'2-digit'}).format(now);
    document.getElementById('proxy-free').textContent = snap.free ?? '-';
    document.getElementById('proxy-res').textContent = snap.residential ?? '-';
    document.getElementById('proxy-total').textContent = snap.total ?? '-';
    document.getElementById('proxy-avail').textContent = snap.available ?? '-';
    document.getElementById('proxy-cap').textContent = fmtNum(snap.capacity?.capacity_remaining);
  } catch (e) {
    // 失败保留旧数据，仅 toast
    document.getElementById('proxy-table').style.display = '';
    document.getElementById('proxy-empty').style.display = 'none';
    toast('代理池读取失败: ' + e.message);
  }
}
async function refreshFree() {
  setBtnBusy('btn-refreshfree', true, '抓取中…');
  try {
    const d = await j('/api/proxies/refresh-free', { method: 'POST' });
    toast('免费代理注入 ' + (d.injected ?? 0) + ' 个');
    refreshProxies();
  } catch (e) { toast(e.message); }
  finally { setBtnBusy('btn-refreshfree', false); }
}
async function refreshCatalog() {
  setBtnBusy('btn-refreshcat', true, '同步中…');
  try {
    const d = await j('/api/catalog/refresh', { method: 'POST' });
    toast('模型目录已同步: ' + (d.models ?? 0) + ' 个');
    loadModels();
  } catch (e) { toast('目录同步失败: ' + e.message); }
  finally { setBtnBusy('btn-refreshcat', false); }
}
async function loadModels() {
  const tb = document.querySelector('#model-table tbody');
  try {
    const list = await j('/v1/models');
    const metas = {};
    for (const m of list.data || []) metas[m.id] = m;
    let guideIds = [];
    try { const g = await j('/api/guide'); guideIds = g.models || []; } catch (e) {}
    for (const id of guideIds) { if (!metas[id]) metas[id] = { id }; }
    const ids = Object.keys(metas);
    if (ids.length === 0) { tb.innerHTML = '<tr><td colspan="6" class="empty">暂无模型</td></tr>'; return; }
    const rows = ids.map(id => {
      const m = metas[id] || {};
      const ctx = m.context || (m.context_window ? (m.context_window/1000) + 'k' : '-');
      const pm = m.price_per_mtok;
      const price = (typeof pm === 'number' && pm > 0) ? '$' + pm.toFixed(2) : (typeof pm === 'number' && pm === 0 ? '免费' : (typeof pm === 'string' ? esc(pm) : '-'));
      const tools = (m.tools === true) ? '<span class="badge ok">工具</span>' : (m.tools === false ? '-' : '<span class="badge warn" title="后端未返回能力字段">未知</span>');
      const vision = (m.vision === true) ? '<span class="badge ok">视觉</span>' : (m.vision === false ? '-' : '<span class="badge warn" title="后端未返回能力字段">未知</span>');
      return `<tr><td>${esc(id)}</td><td>${esc(m.label || m.owned_by || '-')}</td><td>${esc(ctx)}</td><td class="num">${price}</td><td>${tools}</td><td>${vision}</td></tr>`;
    });
    tb.innerHTML = rows.join('');
  } catch (e) { tb.innerHTML = '<tr><td colspan="6" class="empty">模型读取失败: ' + esc(e.message) + '</td></tr>'; toast('模型读取失败: ' + e.message); }
}
async function loadGuide() {
  const live = document.getElementById('guide-live');
  const oa = document.getElementById('guide-openai');
  const anth = document.getElementById('guide-anthropic');
  const py = document.getElementById('guide-python');
  try {
    const g = await j('/api/guide');
    const base = g.base_url || (location.protocol + '//' + location.host + '/v1');
    const anthBase = base.replace(/\/v1$/, '');
    const key = (Array.isArray(API_KEYS) && API_KEYS.length > 0) ? API_KEYS[0] : (g.api_keys_configured ? '<需要有效 key>' : '面板已自动注入（空配置时生成会话级 key）');
    const models = (g.models && g.models.length) ? g.models.join('、') : '（目录为空，点击同步）';
    live.innerHTML = `监听: ${esc(g.listen_addr || '-')}\nBase URL: ${esc(base)}\n模型数: ${esc(g.models ? g.models.length : '-')}（${esc(models)}）\n代理池: ${esc(g.proxy_count ?? '-')}\n上游: ${esc(g.upstream || '-')}\n密钥已配置: ${esc(g.api_keys_configured ? '是' : '否（建议先配置）')}`;
    oa.innerHTML = `Base URL: ${esc(base)}\nAPI Key: ${esc(key)}\n模型: ${esc(models)}`;
    anth.innerHTML = `ANTHROPIC_BASE_URL=${esc(anthBase)}\nANTHROPIC_API_KEY=${esc(key)}`;
    py.innerHTML = `from openai import OpenAI\nclient = OpenAI(base_url="${esc(base)}", api_key="${esc(key)}")\nmodel = "${esc(g.models?.[0] || 'qwen/qwen3.8-27b')}"`;
  } catch (e) {
    live.innerHTML = '接入信息加载失败: ' + esc(e.message);
    oa.textContent = '加载失败';
    anth.textContent = '加载失败';
    py.textContent = '加载失败';
  }
}
async function genKey() {
  setBtnBusy('btn-genkey', true);
  try {
    const d = await j('/api/config/api-key', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ action: 'generate' }) });
    const inp = document.getElementById('new-key');
    inp.value = d.key || '';
    inp.select();
    toast('已生成，请复制保存（仅内存，重启失效）');
  } catch (e) { toast('生成失败: ' + e.message); }
  finally { setBtnBusy('btn-genkey', false); }
}
async function clearKeys() {
  if (!window.confirm('确定清空全部动态 Key？此操作会关闭鉴权（空 key 放行模式），且 config.json 的静态 key 不受影响。继续请输入确认。')) return;
  try {
    await j('/api/config/api-key', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ action: 'clear', admin_confirm: true }) });
    toast('已清空动态 Key，页面刷新后生效');
    document.getElementById('new-key').value = '';
  } catch (e) { toast('清空失败: ' + e.message); }
}
document.getElementById('btn-genkey').onclick = genKey;
document.getElementById('btn-clearkeys').onclick = clearKeys;
loadOverview(); refreshProxies(); loadModels();
setInterval(() => { loadOverview(); refreshProxies(); }, 15000);
</script>
</body>
</html>
"##;
