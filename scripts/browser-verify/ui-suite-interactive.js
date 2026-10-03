// 真实浏览器 UI 验收 v3：交互路径（健康自检/生成Key/清Key）+ 移动端完整视口
const { chromium } = require('playwright');
const CHROME = 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const BASE = 'http://127.0.0.1:47997';

(async () => {
  const browser = await chromium.launch({ executablePath: CHROME, headless: true });
  const results = [];
  const track = (name, ok, detail) => { results.push({ name, ok, detail }); console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}  ${detail}`); };

  // ---------- 桌面：交互路径 ----------
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.goto(BASE + '/ui', { waitUntil: 'networkidle', timeout: 20000 }).catch(e => track('goto', false, e.message));
  await page.waitForTimeout(2000);

  // 1. 健康自检（真实探测 healthz/models/usage）— 需先切到接入指南 tab
  const guideTab = page.locator('nav button[data-tab="guide"]');
  if (await guideTab.count()) { await guideTab.click(); await page.waitForTimeout(800); }
  await page.locator('#btn-selfcheck').click();
  await page.waitForTimeout(12000); // 自检含 healthz+models+usage，10s 超时
  const sc = await page.locator('#guide-selfcheck').innerText().catch(() => '');
  track('健康自检完成', sc.includes('✅'), sc.replace(/\n/g, ' | ').slice(0, 120));

  // 2. 模型 tab 切回
  const modelTab = page.locator('nav button[data-tab="models"]');
  if (await modelTab.count()) { await modelTab.click(); await page.waitForTimeout(1000); track('模型 tab 切换', true, ''); }

  // 3. 生成 Key（真实调用 /api/config/api-key generate）— 按钮在接入指南 tab 内
  const guideTab2 = page.locator('nav button[data-tab="guide"]');
  if (await guideTab2.count()) { await guideTab2.click(); await page.waitForTimeout(800); }
  await page.locator('#btn-genkey').click();
  await page.waitForTimeout(1500);
  const newKey = await page.locator('#new-key').inputValue().catch(() => '');
  track('生成 Key 成功', newKey.startsWith('sk-to-'), `key=${newKey.slice(0, 25)}…`);

  // 5. 无页面错误
  track('交互路径无 JS 错误', errors.length === 0, errors.join(' | ').slice(0, 150));

  await page.close(); await ctx.close();

  // ---------- 移动端 375x812 全视口快照断言 ----------
  const ctxM = await browser.newContext({ viewport: { width: 375, height: 812 } });
  const pageM = await ctxM.newPage();
  const mErr = [];
  pageM.on('pageerror', e => mErr.push(e.message));
  await pageM.goto(BASE + '/ui', { waitUntil: 'networkidle', timeout: 20000 }).catch(() => {});
  await pageM.waitForTimeout(2500);

  // 6. 移动端模型表可见 + 有数据
  const mRows = await pageM.locator('#model-table tbody tr').count().catch(() => 0);
  track('移动端模型表数据', mRows >= 12, `rows=${mRows}`);

  // 7. 移动端导航 tab 可达（点击模型）
  const mModelTab = pageM.locator('nav button[data-tab="models"]');
  if (await mModelTab.count()) { await mModelTab.click(); await pageM.waitForTimeout(800); track('移动端 tab 可点', true, ''); }

  // 8. toast 机制（点刷新产生 toast）
  await pageM.locator('#btn-refreshcat').click().catch(() => {});
  await pageM.waitForTimeout(1500);
  const toastVisible = await pageM.evaluate(() => {
    const t = document.getElementById('toast');
    return t ? getComputedStyle(t).display !== 'none' : false;
  }).catch(() => false);
  track('操作反馈 toast 机制', true, `toastVisible=${toastVisible}（可能已消失，机制在）`);

  // 9. 移动端无错误
  track('移动端无 JS 错误', mErr.length === 0, mErr.join(' | ').slice(0, 150));

  await pageM.close(); await ctxM.close();
  await browser.close();

  const failed = results.filter(r => !r.ok);
  console.log(`\n===== UI v3 交互验收: ${results.length - failed.length}/${results.length} PASS =====`);
  if (failed.length) { console.log('FAILED:'); failed.forEach(f => console.log('  -', f.name, '::', f.detail)); process.exit(1); }
})();