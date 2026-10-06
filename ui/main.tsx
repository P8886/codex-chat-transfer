import React, { useEffect, useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ArrowRight, Check, CheckCircle2, ChevronDown, Download, FileArchive, Folder, FolderOpen, Layers, LoaderCircle, LockKeyhole, MessageSquare, RefreshCw, Search, Settings2, ShieldCheck, Upload, X } from 'lucide-react';
import { ProjectMappings, type MappingDraft, type SourceProject } from './project-mappings';
import { RuntimeDiagnostics, type RuntimeProcess } from './runtime-diagnostics';
import './style.css';

type Project = { id: string; name: string; roots: string[]; nativeId: string | null };
type Thread = { id: string; title: string; cwd: string; projectId: string | null; archived: boolean; updatedAt: number; exportError?: string | null };
type Catalog = { home: string; projects: Project[]; threads: Thread[]; warnings: string[] };
type PackedThread = { id: string; title: string; sourceCwd: string; sourceProjectId?: string; archived: boolean; bytes: number; expectedMessageIds: string[] };
type Manifest = { version?: number; packageId: string; exportedAt: string; sourceProjects: SourceProject[]; threads: PackedThread[]; assets: unknown[]; warnings: string[] };
type Bootstrap = { home: string; codexBinary: string | null; token: string; runtimeClosed: boolean; runtimeError: string | null; blockingProcesses?: RuntimeProcess[]; version: string };
type Preview = { manifest: Manifest; conflicts: string[] };
type Report = { status: 'verified' | 'alreadyImported'; imported: number; project: Project; projects?: Project[]; backup: string | null; verifiedMessageCount: number; warnings: string[] };

function size(bytes: number) { return bytes > 1048576 ? `${(bytes / 1048576).toFixed(1)} MB` : `${Math.ceil(bytes / 1024)} KB`; }
function name(path: string) { return path.split(/[\\/]/).at(-1) || path; }
function date(timestamp: number) { return timestamp ? new Date(timestamp * 1000).toLocaleDateString('zh-CN') : '-'; }

function App() {
  const [bootstrap, setBootstrap] = useState<Bootstrap | null>(null);
  const [home, setHome] = useState('');
  const [binary, setBinary] = useState('');
  const [catalog, setCatalog] = useState<Catalog>({ home: '', projects: [], threads: [], warnings: [] });
  const [tab, setTab] = useState<'export' | 'import'>('export');
  const [settings, setSettings] = useState(false);
  const [projectFilter, setProjectFilter] = useState('*');
  const [query, setQuery] = useState('');
  const [archived, setArchived] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [output, setOutput] = useState('');
  const [packagePath, setPackagePath] = useState('');
  const [preview, setPreview] = useState<Preview | null>(null);
  const [target, setTarget] = useState('');
  const [targetDir, setTargetDir] = useState('');
  const [projectName, setProjectName] = useState('');
  const [preserveGroups, setPreserveGroups] = useState(true);
  const [mappings, setMappings] = useState<Record<string, MappingDraft>>({});
  const [replace, setReplace] = useState(false);
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  const [exported, setExported] = useState<{ output: string; manifest: Manifest } | null>(null);
  const [report, setReport] = useState<Report | null>(null);

  async function api<T>(endpoint: string, body: unknown, token = bootstrap?.token): Promise<T> {
    const response = await fetch(`/api/${endpoint}`, { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Transfer-Token': token || '' }, body: JSON.stringify(body) });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error || '请求失败');
    return data;
  }

  async function perform(label: string, operation: () => Promise<void>) {
    setError(''); setBusy(label);
    try { await operation(); } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(''); }
  }

  async function loadCatalog(directory = home, token = bootstrap?.token) {
    const data = await api<Catalog>('catalog', { home: directory }, token);
    setCatalog(data); setSelected(new Set()); setPreview(null); setReport(null);
    setTarget(old => data.projects.some(p => p.id === old) ? old : data.projects[0]?.id || '__new');
  }

  useEffect(() => {
    let cancelled = false;
    fetch('/api/bootstrap').then(r => r.json()).then(async (data: Bootstrap) => {
      if (cancelled) return;
      setBootstrap(data); setHome(data.home); setBinary(data.codexBinary || '');
      setBusy('扫描聊天');
      try {
        const c = await api<Catalog>('catalog', { home: data.home }, data.token);
        if (!cancelled) { setCatalog(c); setTarget(c.projects[0]?.id || '__new'); }
      } catch (e) { if (!cancelled) setError(String(e)); }
      finally { if (!cancelled) setBusy(''); }
    }).catch(e => { if (!cancelled) setError(String(e)); });
    const timer = setInterval(() => {
      fetch('/api/bootstrap').then(r => r.json()).then(data => { if (!cancelled) setBootstrap(data); }).catch(() => {});
    }, 5000);
    return () => { cancelled = true; clearInterval(timer); };
  }, []);

  const filtered = useMemo(() => catalog.threads.filter(t =>
    (archived || !t.archived) && (projectFilter === '*' || (projectFilter === '-' ? !t.projectId : t.projectId === projectFilter)) &&
    `${t.title} ${t.id} ${t.cwd}`.toLowerCase().includes(query.toLowerCase())), [catalog, archived, projectFilter, query]);
  const targetProject = catalog.projects.find(p => p.id === target);
  const targetLabel = target === '__new' ? projectName || name(targetDir) || '新项目' : targetProject?.name || '';
  const groups = preview?.manifest.sourceProjects || [];
  const groupName = (id?: string) => groups.find(g => g.id === id)?.name || '未分组聊天';
  const mappingLabel = (id?: string) => {
    const draft = id ? mappings[id] : null;
    return draft?.target === '__new' ? draft.name || groupName(id) : catalog.projects.find(p => p.id === draft?.target)?.name || '未映射';
  };
  const mappingsComplete = groups.length > 0 && groups.every(g => {
    const draft = mappings[g.id];
    return draft && (draft.target === '__new' ? !!draft.directory && !!draft.name.trim() : catalog.projects.some(p => p.id === draft.target));
  });
  function changeMapping(id: string, patch: Partial<MappingDraft>) { setMappings(old => ({ ...old, [id]: { ...old[id], ...patch } })); setReport(null); }
  const count = (id: string) => catalog.threads.filter(t => (archived || !t.archived) && (id === '*' || (id === '-' ? !t.projectId : t.projectId === id))).length;
  const selectable = filtered.filter(t => !t.exportError);
  const allSelected = selectable.length > 0 && selectable.every(t => selected.has(t.id));
  function toggle(id: string) { setSelected(old => { const next = new Set(old); next.has(id) ? next.delete(id) : next.add(id); return next; }); }
  function selectVisible() { setSelected(old => { const next = new Set(old); selectable.forEach(t => allSelected ? next.delete(t.id) : next.add(t.id)); return next; }); }

  async function choose(kind: string, setter: (path: string) => void) {
    await perform('选择文件', async () => {
      const data = await api<{ path: string | null }>('dialog', { kind });
      if (data.path) setter(data.path);
    });
  }

  async function inspect(path = packagePath) {
    await perform('校验导出包', async () => {
      setReport(null);
      const data = await api<Preview>('preview', { home, package: path });
      setPreview(data); setReplace(false);
      const used = new Set<string>();
      setMappings(Object.fromEntries((data.manifest.sourceProjects || []).map(group => {
        const matches = catalog.projects.filter(p => p.name === group.name);
        const match = matches.length === 1 && !used.has(matches[0].id) ? matches[0] : null;
        if (match) used.add(match.id);
        return [group.id, { target: match?.id || '__new', directory: '', name: group.name }];
      })));
    });
  }

  async function importPackage() {
    await perform('保存备份、重建历史并验证项目归属', async () => {
      setReport(null);
      const projectMappings = preserveGroups ? groups.map(group => {
        const draft = mappings[group.id];
        return { sourceProjectId: group.id, projectId: draft.target === '__new' ? null : draft.target, targetDir: draft.target === '__new' ? draft.directory : null, projectName: draft.target === '__new' ? draft.name : null };
      }) : null;
      const result = await api<Report>('import', { home, package: packagePath, projectId: preserveGroups || target === '__new' ? null : target, targetDir: !preserveGroups && target === '__new' ? targetDir : null, projectName: !preserveGroups && target === '__new' ? projectName : null, replace, codexBinary: binary, projectMappings });
      setReport(result);
      const updated = await api<Catalog>('catalog', { home });
      setCatalog(updated); setTarget(result.project.id);
      setMappings(old => Object.fromEntries(Object.entries(old).map(([id, draft]) => {
        const found = updated.projects.find(p => p.roots.includes(draft.directory));
        return [id, draft.target === '__new' && found ? { ...draft, target: found.id } : draft];
      })));
    });
  }

  return <div className="app">
    <header className="header">
      <div className="brand"><MessageSquare size={22} /><h1>Codex Chat Transfer</h1><span className="version">v{bootstrap?.version || '0.3.1'}</span></div>
      <div className="header-actions">
        <span className={`runtime ${bootstrap?.runtimeClosed ? 'ready' : 'blocked'}`}><span className="status-dot" />{bootstrap ? bootstrap.runtimeClosed ? '导入可用' : '数据写入进程未退出' : '正在检测进程'}</span>
        <button className="icon-button" title="设置" aria-label="设置" onClick={() => setSettings(!settings)}><Settings2 size={18} /></button>
      </div>
    </header>

    {settings && <section className="settings">
      <label>Codex 数据目录<div className="path-input"><input value={home} onChange={e => setHome(e.target.value)} /><button className="icon-button" title="选择数据目录" aria-label="选择数据目录" disabled={!!busy} onClick={() => choose('folder', setHome)}><FolderOpen size={18} /></button></div></label>
      <label>Codex 可执行文件<div className="path-input"><input value={binary} onChange={e => setBinary(e.target.value)} /><button className="icon-button" title="选择 Codex 可执行文件" aria-label="选择 Codex 可执行文件" disabled={!!busy} onClick={() => choose('binary', setBinary)}><FolderOpen size={18} /></button></div></label>
      <button className="button" disabled={!!busy} onClick={() => perform('扫描聊天', () => loadCatalog())}><RefreshCw size={16} />重新扫描</button>
    </section>}

    <div className="workspace">
      <aside className="sidebar">
        <div className="sidebar-title">本机项目<button className="icon-button" title="刷新项目" aria-label="刷新项目" disabled={!!busy} onClick={() => perform('扫描聊天', () => loadCatalog())}><RefreshCw size={15} /></button></div>
        {[{ id: '*', name: '全部聊天', roots: [] }, ...catalog.projects, { id: '-', name: '无项目', roots: [] }].map(p =>
          <button key={p.id} className={`project-row ${projectFilter === p.id ? 'active' : ''}`} title={p.roots.join('\n')} onClick={() => { setProjectFilter(p.id); setTab('export'); }}>
            {p.id === '*' ? <MessageSquare size={17} /> : <Folder size={17} />}<span>{p.name}</span><small>{count(p.id)}</small>
          </button>)}
        <div className="sidebar-bottom"><span className="eyebrow">数据目录</span><span className="mono home-path" title={home}>{home || '-'}</span></div>
      </aside>

      <main className="main">
        <nav className="tabs" aria-label="传输方向"><button className={tab === 'export' ? 'active' : ''} onClick={() => setTab('export')}><Download size={17} />导出聊天</button><button className={tab === 'import' ? 'active' : ''} onClick={() => setTab('import')}><Upload size={17} />导入到本机</button></nav>
        {error && <div className="alert error" role="alert"><X size={18} /><span>{error}</span><button className="icon-button" aria-label="关闭错误" onClick={() => setError('')}><X size={15} /></button></div>}
        {busy && <div className="activity" role="status"><LoaderCircle className="spin" size={16} />{busy}</div>}

        {tab === 'export' ? <>
          <div className="section-heading"><div><h2>{projectFilter === '*' ? '全部聊天' : projectFilter === '-' ? '无项目' : catalog.projects.find(p => p.id === projectFilter)?.name}</h2><span>{filtered.length} 条聊天</span></div><label className="check-label"><input type="checkbox" checked={archived} onChange={e => setArchived(e.target.checked)} />包含归档</label></div>
          <div className="search"><Search size={17} /><input aria-label="搜索聊天" placeholder="搜索标题、目录或会话 ID" value={query} onChange={e => setQuery(e.target.value)} />{query && <button className="icon-button" title="清除搜索" aria-label="清除搜索" onClick={() => setQuery('')}><X size={15} /></button>}</div>
          <div className="table-wrap"><table><thead><tr><th className="checkbox-column"><input type="checkbox" aria-label="选择当前列表全部聊天" checked={allSelected} onChange={selectVisible} /></th><th>聊天</th><th className="directory-column">源目录</th><th className="date-column">更新日期</th></tr></thead><tbody>
            {filtered.map(t => <tr key={t.id} className={selected.has(t.id) ? 'selected' : ''} onClick={() => !t.exportError && toggle(t.id)}><td><input aria-label={`选择 ${t.title}`} type="checkbox" disabled={!!t.exportError} checked={selected.has(t.id)} onChange={() => toggle(t.id)} onClick={e => e.stopPropagation()} /></td><td><div className="thread-title">{t.title}{t.archived && <span className="tag">归档</span>}{t.exportError && <span className="tag conflict" title={t.exportError}>文件冲突</span>}</div><div className="thread-id mono">{t.id}</div></td><td className="directory-column"><span className="path-cell" title={t.cwd}>{t.cwd}</span></td><td className="date-column">{date(t.updatedAt)}</td></tr>)}
            {!filtered.length && <tr><td colSpan={4} className="empty">{busy ? '正在读取聊天' : '没有匹配的聊天'}</td></tr>}
          </tbody></table></div>
          <div className="export-bar"><span><strong>{selected.size}</strong> 条已选</span><div className="path-input"><input aria-label="导出文件路径" placeholder="导出文件路径" value={output} onChange={e => setOutput(e.target.value)} /><button className="icon-button" title="选择导出位置" aria-label="选择导出位置" disabled={!!busy} onClick={() => choose('save', setOutput)}><FolderOpen size={18} /></button></div><button className="button primary" disabled={!!busy || !selected.size || !output} onClick={() => perform('打包聊天与历史依赖', async () => { setExported(null); const result = await api<{ output: string; manifest: Manifest }>('export', { home, threads: [...selected], output }); setExported(result); })}><Download size={17} />导出 {selected.size || ''}</button></div>
          {exported && <div className="result" role="status"><CheckCircle2 size={22} /><div><strong>已导出 {exported.manifest.threads.length} 条聊天</strong><p className="mono">{exported.output}</p><span>{exported.manifest.sourceProjects?.length || 0} 个项目分组 · {exported.manifest.assets.length} 个图片引用 · 历史依赖已合并 · 文件校验已完成</span>{exported.manifest.warnings.length > 0 && <details><summary>{exported.manifest.warnings.length} 条附件警告</summary>{exported.manifest.warnings.map((w, i) => <p key={i}>{w}</p>)}</details>}</div></div>}
        </> : <>
          <div className="section-heading"><div><h2>导入到本机项目</h2><span>{preview ? `${preview.manifest.threads.length} 条聊天待导入` : '尚未选择导出包'}</span></div><ShieldCheck size={23} className="muted" /></div>
          <section className="import-fields">
            <label className="field-label">导出包<div className="path-input"><FileArchive size={18} /><input aria-label="导出包路径" placeholder="选择 .cct.zip 导出包" value={packagePath} onChange={e => { setPackagePath(e.target.value); setPreview(null); setReport(null); }} /><button className="icon-button" title="选择导出包" aria-label="选择导出包" disabled={!!busy} onClick={() => choose('package', p => { setPackagePath(p); setPreview(null); setReport(null); })}><FolderOpen size={18} /></button></div></label>
            <button className="button" disabled={!!busy || !packagePath} onClick={() => inspect()}><ShieldCheck size={17} />校验包</button>
            <div className="mode-tabs target-field" role="group" aria-label="导入方式"><button disabled={!!busy} aria-pressed={preserveGroups} className={preserveGroups ? 'active' : ''} onClick={() => { setPreserveGroups(true); setReport(null); }}><Layers size={16} />保留项目分组</button><button disabled={!!busy} aria-pressed={!preserveGroups} className={!preserveGroups ? 'active' : ''} onClick={() => { setPreserveGroups(false); setReport(null); }}><MessageSquare size={16} />合并到一个项目</button></div>
            {!preserveGroups && <><label className="field-label target-field">目标项目<div className="select"><Folder size={18} /><select aria-label="目标项目" disabled={!!busy} value={target} onChange={e => { setTarget(e.target.value); setReport(null); }}>{catalog.projects.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}<option value="__new">新建本机项目</option></select><ChevronDown size={15} /></div></label>
            {target === '__new' ? <><label className="field-label">项目文件夹<div className="path-input"><input aria-label="目标项目文件夹" placeholder="B 电脑上的项目绝对路径" value={targetDir} onChange={e => setTargetDir(e.target.value)} /><button className="icon-button" title="选择项目文件夹" aria-label="选择项目文件夹" disabled={!!busy} onClick={() => choose('folder', p => { setTargetDir(p); if (!projectName) setProjectName(name(p)); })}><FolderOpen size={18} /></button></div></label><label className="field-label">项目名称<input className="plain-input" aria-label="新项目名称" value={projectName} onChange={e => setProjectName(e.target.value)} /></label></> : <div className="target-path mono">{targetProject?.roots.join(' · ')}</div>}</>}
          </section>
          {preserveGroups && preview && <ProjectMappings groups={groups} projects={catalog.projects} drafts={mappings} disabled={!!busy} onChange={changeMapping} onFolder={id => choose('folder', directory => changeMapping(id, { directory }))} />}
          {preview?.manifest.version === 1 && preserveGroups && <p className="legacy-note">旧包未记录项目名称，当前分组按 A 电脑的源目录推导。</p>}
          {!bootstrap?.runtimeClosed && <RuntimeDiagnostics processes={bootstrap?.blockingProcesses || []} message={bootstrap?.runtimeError || '正在检查进程；退出对应程序后状态会自动更新。'} onRefresh={() => perform('检测进程', async () => setBootstrap(await fetch('/api/bootstrap').then(r => r.json())))} />}
          {preview && <>
            <div className="package-meta"><span><CheckCircle2 size={16} />{preview.manifest.threads.length} 条会话校验通过</span><span>{size(preview.manifest.threads.reduce((n, t) => n + t.bytes, 0))}</span><span>{new Date(preview.manifest.exportedAt).toLocaleString('zh-CN')}</span></div>
            <div className="table-wrap preview-table"><table><thead><tr><th>聊天</th><th>源项目 / 目录</th><th className="target-column">导入目标</th></tr></thead><tbody>{preview.manifest.threads.map(t => <tr key={t.id}><td><div className="thread-title">{t.title}{preview.conflicts.includes(t.id) && <span className="tag conflict">同 ID</span>}</div><div className="thread-id mono">{t.id}</div></td><td><strong className="source-group-name">{groupName(t.sourceProjectId)}</strong><span className="path-cell" title={t.sourceCwd}>{t.sourceCwd}</span></td><td><span className="destination"><ArrowRight size={15} /><Folder size={16} />{preserveGroups ? mappingLabel(t.sourceProjectId) : targetLabel}</span></td></tr>)}</tbody></table></div>
            {preview.conflicts.length > 0 && <label className="replace-option"><input type="checkbox" checked={replace} onChange={e => setReplace(e.target.checked)} /><span>替换 {preview.conflicts.length} 条同 ID 会话，原记录自动备份</span></label>}
            {preview.manifest.warnings.length > 0 && <details className="warnings"><summary>{preview.manifest.warnings.length} 条原始附件警告</summary>{preview.manifest.warnings.map((w, i) => <p key={i}>{w}</p>)}</details>}
            <div className="import-bar"><span><ShieldCheck size={16} />{preserveGroups ? `${groups.length} 个项目分组 · 整包备份与校验` : '完整校验后保存项目归属'}</span><button className="button primary" disabled={!!busy || !bootstrap?.runtimeClosed || (preview.conflicts.length > 0 && !replace) || (preserveGroups ? !mappingsComplete : target === '__new' ? !targetDir : !targetProject) || !binary} onClick={importPackage}><Upload size={17} />导入 {preview.manifest.threads.length} 条</button></div>
          </>}
          {report && <div className="result import-result" role="status"><CheckCircle2 size={24} /><div><strong>{report.status === 'alreadyImported' ? '此包已导入，项目归属已再次核对，未重复写入' : (report.projects?.length || 1) > 1 ? `${report.imported} 条聊天已按 ${report.projects!.length} 个项目分组导入` : `${report.imported} 条聊天已验证并归入 ${report.project.name}`}</strong>{(report.projects?.length || 1) > 1 && <div className="imported-projects">{report.projects!.map(p => <span key={p.id}><Folder size={15} />{p.name}</span>)}</div>}<div className="verification"><span><Check size={15} />原生项目 ID</span><span><Check size={15} />侧栏项目归属</span><span><Check size={15} />已解除无项目标记</span><span><Check size={15} />聊天正文索引</span></div>{report.backup && <p className="mono">备份：{report.backup}</p>}<span>{report.status === 'verified' ? `${report.verifiedMessageCount} 条消息可读取。重新打开 Codex 后查看项目。` : '项目归属与目录已再次核对。'}</span></div></div>}
        </>}
        {report && report.warnings.length > 0 && <details className="warnings" open><summary>{report.warnings.length} 条导入提醒</summary>{report.warnings.map((w, i) => <p key={i}>{w}</p>)}</details>}
        {catalog.warnings.length > 0 && <details className="warnings"><summary>{catalog.warnings.length} 条扫描警告</summary>{catalog.warnings.map((w, i) => <p key={i}>{w}</p>)}</details>}
      </main>
    </div>
  </div>;
}

createRoot(document.getElementById('root')!).render(<App />);
