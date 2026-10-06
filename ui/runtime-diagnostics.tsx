import { ClipboardCopy, LockKeyhole, RefreshCw } from 'lucide-react';

export type RuntimeProcess = { pid: number; name: string; source: string; reason: string; executable: string | null; parent: string | null };

export function RuntimeDiagnostics({ processes, message, onRefresh }: { processes: RuntimeProcess[]; message: string; onRefresh: () => void }) {
  return <section className="runtime-diagnostics">
    <div className="runtime-block"><LockKeyhole size={18} /><div><strong>后台进程可能修改聊天数据，导入已暂停</strong><span>{message}</span></div><button className="icon-button" aria-label="刷新进程检测" title="刷新进程检测" onClick={onRefresh}><RefreshCw size={16} /></button></div>
    {processes.length > 0 && <><div className="runtime-table"><table><thead><tr><th className="pid-column">PID</th><th>进程来源</th><th>原因 / 文件路径</th></tr></thead><tbody>{processes.map(process => <tr key={process.pid}><td className="mono">{process.pid}</td><td><strong>{process.source}</strong><span className="thread-id">{process.name}{process.parent ? ` · 上级：${process.parent}` : ''}</span></td><td><span>{process.reason}</span><span className="path-cell">{process.executable || '路径不可读取'}</span></td></tr>)}</tbody></table></div><button className="button diagnostics-copy" onClick={() => navigator.clipboard.writeText(JSON.stringify(processes, null, 2))}><ClipboardCopy size={15} />复制进程诊断</button></>}
  </section>;
}
