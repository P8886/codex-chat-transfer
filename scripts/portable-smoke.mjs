import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { chromium } from '@playwright/test';

const root = process.cwd();
const home = path.join(root, '.runtime', 'portable-smoke-home');
const destination = path.join(root, '.runtime', 'portable-smoke-project');
const output = path.join(root, '.runtime', `portable-smoke-${Date.now()}.cct.zip`);

if (process.argv.includes('--prepare')) {
  fs.mkdirSync(path.join(home, 'sessions', '2026', '10', '02'), { recursive: true });
  fs.mkdirSync(destination, { recursive: true });
  const id = randomUUID();
  const turn = randomUUID();
  const rows = [
    { type: 'session_meta', payload: { id, timestamp: '2026-10-02T01:00:00Z', cwd: destination, source: 'vscode', originator: 'Codex Desktop', model_provider: 'openai', history_mode: 'paginated', cli_version: '0.159.2' } },
    { type: 'event_msg', payload: { type: 'task_started', turn_id: turn, started_at: 1790902800, collaboration_mode_kind: 'default' } },
    { type: 'event_msg', payload: { type: 'item_completed', thread_id: id, turn_id: turn, item: { type: 'UserMessage', id: randomUUID(), content: [{ type: 'text', text: 'Portable application test', text_elements: [] }] } } },
    { type: 'event_msg', payload: { type: 'item_completed', thread_id: id, turn_id: turn, item: { type: 'AgentMessage', id: randomUUID(), content: [{ type: 'Text', text: 'Portable application test reply' }], phase: 'final_answer' } } },
    { type: 'event_msg', payload: { type: 'task_complete', turn_id: turn, started_at: 1790902800, completed_at: 1790902802, duration_ms: 2000 } },
  ];
  fs.writeFileSync(path.join(home, 'sessions', '2026', '10', '02', `rollout-2026-10-02T01-00-00-${id}.jsonl`), rows.map((r, ordinal) => JSON.stringify({ ...r, ordinal, timestamp: '2026-10-02T01:00:00Z' })).join('\n') + '\n');
  fs.writeFileSync(path.join(home, '.codex-global-state.json'), JSON.stringify({
    'local-projects': { fixture: { id: 'fixture', name: '便携窗口测试项目', rootPaths: [destination] } },
    'thread-project-assignments': { [id]: { projectKind: 'local', projectId: 'fixture' } },
  }));
  fs.writeFileSync(path.join(home, 'session_index.jsonl'), JSON.stringify({ id, thread_name: '便携窗口导出测试', updated_at: '2026-10-02T01:00:00Z' }) + '\n');
  console.log(home);
  process.exit(0);
}

const endpoint = 'http://127.0.0.1:47834';
let browser;
for (let attempt = 0; attempt < 30; attempt++) {
  try { browser = await chromium.connectOverCDP(endpoint); break; }
  catch { await new Promise(resolve => setTimeout(resolve, 500)); }
}
if (!browser) throw new Error('The native WebView2 debugging endpoint did not become available');
let page;
for (let attempt = 0; attempt < 30; attempt++) {
  page = browser.contexts().flatMap(c => c.pages()).find(p => /^http:\/\/127\.0\.0\.1:\d+\/$/.test(p.url()));
  if (page) break;
  await new Promise(resolve => setTimeout(resolve, 500));
}
if (!page) throw new Error('The native application did not load its embedded UI');
await page.getByText('便携窗口导出测试', { exact: true }).waitFor();
await page.getByRole('checkbox', { name: '选择当前列表全部聊天' }).check();
await page.getByRole('textbox', { name: '导出文件路径' }).fill(output);
await page.getByRole('button', { name: '导出 1', exact: true }).click();
await page.getByText('已导出 1 条聊天', { exact: true }).waitFor();
if (!fs.existsSync(output)) throw new Error('The native export did not create its package');
const actual = await page.evaluate(() => ({ width: innerWidth, height: innerHeight, overflow: document.documentElement.scrollWidth > innerWidth }));
if (actual.overflow) throw new Error('The native application layout overflows horizontally');
fs.mkdirSync(path.join(root, 'test-results'), { recursive: true });
await page.screenshot({ path: path.join(root, 'test-results', 'portable-window.png'), fullPage: true });
await page.getByRole('button', { name: '导入到本机', exact: true }).click();
await page.getByRole('textbox', { name: '导出包路径' }).fill(output);
await page.getByRole('button', { name: '校验包', exact: true }).click();
await page.getByText('1 条会话校验通过', { exact: true }).waitFor();
console.log(JSON.stringify({ nativePage: page.url(), exportedPackage: output, packageBytes: fs.statSync(output).size, viewport: actual, previewVerified: true }));
await browser.close();
