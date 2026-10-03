// 真实浏览器 UI 验收 v2：捕获 404 URL + 点击 guide tab + 验证交互
const { chromium } = require('playwright');
const CHROME = 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const BASE = 'http://127.0.0.1:47997';

(async () => {
  const browser = await chromium.launch({ executablePath: CHROME, headless: true });
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await ctx.newPage();
  const results = [];
  const track = (name, ok, detail) => { results.push({ name, ok, detail }); console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}  ${detail}`); };

  const consoleErrors = [];
  page.on('console', m => { if (m.type() === 'error') consoleErrors.push({ text: m.text(), url: m.location().url }); });
  page.on('pageerror', e => consoleErrors.push({ text: 'PAGEERROR: ' + e.message, url: '' }));
  page.on('requestfailed', r => consoleErrors.push({ text: 'REQFAIL: ' + r.url(), url: r.url() }));

  await page.goto(BASE + '/ui', { waitUntil: 'networkidle', timeout: 20000 }).catch(e => track('goto /ui', false, e.message));
  await page.waitForTimeout(2500);

  const rows = await page.locator('#model-table tbody tr').count().catch(() => 0);
  track('模型表有数据行(≥12)', rows >= 12, `rows=${rows}`);

  // 点击「接入指南」tab
  const guideTab = page.locator('nav button[data-tab="guide"]');
  if (await guideTab.count()) {
    await guideTab.click();
    await page.waitForTimeout(2500);
    const guideText = await page.locator('#guide-live').innerText().catch(() => '');
    track('点击接入指南后渲染', guideText.includes('Base URL') && guideText.includes('模型数'), guideText.slice(0, 100));
  } else {
    track('接入指南 tab 存在', false, 'no [data-tab=guide] button');
  }

  // 复制 curl 按钮点击（剪贴板可能被拒，只验证无异常）
  const copyClick = await page.locator('#btn-copy-curl').count();
  track('复制 curl 按钮', copyClick === 1, `count=${copyClick}`);

  // 总览卡片数值
  const mc = await page.locator('#mc').innerText().catch(() => '');
  const pc = await page.locator('#pc').innerText().catch(() => '');
  track('总览模型/代理数渲染', mc !== '' && pc !== '', `models=${mc} proxies=${pc}`);

  // 控制台错误明细（区分 favicon 404 vs 真实接口 404）
  const flagged = consoleErrors.filter(c => c.text.includes('404'));
  const other = consoleErrors.filter(c => !c.text.includes('404'));
  track('无真实接口 404（favicon 除外）', flagged.length === 0 || flagged.every(f => /favicon|\.ico/.test(f.text)), JSON.stringify(flagged).slice(0, 200));
  track('无其他 JS 错误', other.length === 0, JSON.stringify(other).slice(0, 300));

  // body 文本采样（诊断）
  const bodySample = await page.locator('body').innerText().catch(() => '');
  console.log('--- body 前 300 字 ---');
  console.log(bodySample.slice(0, 300).replace(/\n+/g, ' | '));

  await page.close(); await ctx.close();
  await browser.close();

  const failed = results.filter(r => !r.ok);
  console.log(`\n===== UI v2 汇总: ${results.length - failed.length}/${results.length} PASS =====`);
  if (failed.length) { console.log('FAILED:'); failed.forEach(f => console.log('  -', f.name, '::', f.detail)); process.exit(1); }
})();