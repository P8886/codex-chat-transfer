import { test, expect } from '@playwright/test';

const threads = [
  { id: '01a0f156-3371-77d2-a57b-758094b1c140', title: '子任务勾选状态更新', cwd: 'D:\\Company\\renewlet', projectId: 'source-project', archived: false, updatedAt: 1790816400 },
  { id: '01a0ddad-da32-70c2-bf9e-147d1193aef7', title: '样式更改与日历统计', cwd: 'D:\\Company\\renewlet', projectId: 'source-project', archived: false, updatedAt: 1790816300 },
];
const project = { id: 'target-project', name: 'todo', roots: ['D:\\Personal\\todo'], nativeId: 'native-project' };
const manifest = { version: 2, packageId: 'test-package', exportedAt: '2026-10-01T01:00:00Z', sourceProjects: [{id:'source-project', name:'renewlet', roots:['D:\\Company\\renewlet']}], threads: threads.map(t => ({ ...t, sourceCwd: t.cwd, sourceProjectId:'source-project', bytes: 2048, expectedMessageIds: ['one', 'two'] })), assets: [], warnings: [] };

test.beforeEach(async ({ page }) => {
  let imported = false;
  await page.route('**/api/**', async route => {
    const endpoint = new URL(route.request().url()).pathname;
    if (endpoint === '/api/import') imported = true;
    const replies: Record<string, unknown> = {
      '/api/bootstrap': { home: 'C:\\Users\\test\\.codex', codexBinary: 'C:\\Codex\\codex.exe', token: 'test-token', runtimeClosed: true, version: '0.3.0' },
      '/api/catalog': { home: 'C:\\Users\\test\\.codex', projects: [project, { ...project, id: 'source-project', name: 'renewlet', roots: ['D:\\Company\\renewlet'] }], threads: imported ? threads.map(t => ({...t, projectId: project.id, cwd: project.roots[0]})) : threads, warnings: [] },
      '/api/export': { output: 'D:\\transfer\\chats.cct.zip', manifest },
      '/api/preview': { manifest, conflicts: [] },
      '/api/import': { status: 'verified', imported: 2, project, backup: 'C:\\Users\\test\\.codex\\chat-transfer-backups\\test', verifiedMessageCount: 4, warnings: [] },
    };
    await route.fulfill({ json: replies[endpoint] ?? { error: 'Unexpected endpoint' } });
  });
});

test('selection, export and target project import', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('子任务勾选状态更新', { exact: true })).toBeVisible();
  await page.getByRole('checkbox', { name: '选择当前列表全部聊天' }).check();
  await page.getByRole('textbox', { name: '导出文件路径' }).fill('D:\\transfer\\chats.cct.zip');
  await page.getByRole('button', { name: '导出 2', exact: true }).click();
  await expect(page.getByText('已导出 2 条聊天')).toBeVisible();
  await page.getByRole('button', { name: '导入到本机', exact: true }).click();
  await page.getByRole('textbox', { name: '导出包路径' }).fill('D:\\transfer\\chats.cct.zip');
  await page.getByRole('button', { name: '校验包', exact: true }).click();
  await page.getByRole('combobox', { name: '目标项目' }).selectOption('target-project');
  await page.getByRole('button', { name: '导入 2 条', exact: true }).click();
  await expect(page.getByText('2 条聊天已验证并归入 todo')).toBeVisible();
  await expect(page.getByText('已解除无项目标记', { exact: true })).toBeVisible();
  await page.screenshot({ path: 'test-results/desktop-import.png', fullPage: true });
});

test('running Codex disables import and mobile layout does not overflow', async ({ page }) => {
  await page.route('**/api/bootstrap', route => route.fulfill({ json: { home: 'C:\\Users\\test\\.codex', codexBinary: 'C:\\Codex\\codex.exe', token: 'test-token', runtimeClosed: false, version: '0.3.0' } }));
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  await page.getByRole('button', { name: '导入到本机', exact: true }).click();
  await page.getByRole('textbox', { name: '导出包路径' }).fill('D:\\transfer\\chats.cct.zip');
  await page.getByRole('button', { name: '校验包', exact: true }).click();
  await expect(page.getByRole('button', { name: '导入 2 条', exact: true })).toBeDisabled();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
  await page.screenshot({ path: 'test-results/mobile-import.png', fullPage: true });
});

test('keeps two source projects, requires both folders and submits separate mappings', async ({ page }) => {
  const sources=[{id:'group-todo',name:'待办项目',roots:['D:\\Company\\todo']},{id:'group-notes',name:'笔记项目',roots:['D:\\Company\\notes']}];
  const grouped={...manifest,sourceProjects:sources,threads:manifest.threads.map((t,i)=>({...t,sourceProjectId:sources[i].id,sourceCwd:sources[i].roots[0]}))};
  const importedProjects=sources.map((p,i)=>({id:`imported-${i}`,name:p.name,roots:[i===0?'D:\\Imported\\todo':'D:\\Imported\\notes'],nativeId:`native-${i}`}));
  let imported=false;
  await page.route('**/api/preview', route=>route.fulfill({json:{manifest:grouped,conflicts:[]}}));
  await page.route('**/api/catalog', route=>route.fulfill({json:{home:'C:\\Users\\test\\.codex',projects:imported?importedProjects:[],threads:imported?threads.map((t,i)=>({...t,projectId:importedProjects[i].id,cwd:importedProjects[i].roots[0]})):[],warnings:[]}}));
  await page.route('**/api/import', async route=>{
    const body=route.request().postDataJSON();
    expect(body.projectId).toBeNull();
    expect(body.projectMappings).toEqual([
      {sourceProjectId:'group-todo',projectId:null,targetDir:'D:\\Imported\\todo',projectName:'待办项目'},
      {sourceProjectId:'group-notes',projectId:null,targetDir:'D:\\Imported\\notes',projectName:'笔记项目'},
    ]);
    imported=true;
    await route.fulfill({json:{status:'verified',imported:2,project:importedProjects[0],projects:importedProjects,backup:'C:\\backup',verifiedMessageCount:4,warnings:[]}});
  });
  await page.goto('/');
  await page.getByRole('button',{name:'导入到本机',exact:true}).click();
  await page.getByRole('textbox',{name:'导出包路径'}).fill('D:\\grouped.cct.zip');
  await page.getByRole('button',{name:'校验包',exact:true}).click();
  await expect(page.getByRole('button',{name:'保留项目分组',exact:true})).toHaveAttribute('aria-pressed','true');
  const commit=page.getByRole('button',{name:'导入 2 条',exact:true});await expect(commit).toBeDisabled();
  await page.getByRole('textbox',{name:'项目文件夹：待办项目',exact:true}).fill('D:\\Imported\\todo');
  await expect(commit).toBeDisabled();
  await page.getByRole('textbox',{name:'项目文件夹：笔记项目',exact:true}).fill('D:\\Imported\\notes');
  await expect(commit).toBeEnabled();await commit.click();
  await expect(page.getByText('2 条聊天已按 2 个项目分组导入',{exact:true})).toBeVisible();
  await page.screenshot({path:'test-results/grouped-import.png',fullPage:true});
});

test('shows the editor process that actually blocks import',async({page})=>{
  await page.route('**/api/bootstrap',route=>route.fulfill({json:{home:'C:\\Users\\test\\.codex',codexBinary:'C:\\codex.exe',token:'test-token',runtimeClosed:false,runtimeError:'请退出列表中的数据写入进程',version:'0.3.1',blockingProcesses:[{pid:1234,name:'codex.exe',source:'编辑器的 Codex 扩展后台',reason:'会话服务仍在运行，可能写入聊天数据库',executable:'C:\\Users\\test\\.vscode\\extensions\\codex.exe',parent:'Code.exe'}]}}));
  await page.goto('/');await page.getByRole('button',{name:'导入到本机',exact:true}).click();
  await expect(page.getByText('编辑器的 Codex 扩展后台',{exact:true})).toBeVisible();
  await expect(page.getByText('1234',{exact:true})).toBeVisible();
  await expect(page.getByRole('button',{name:'复制进程诊断',exact:true})).toBeVisible();
});
