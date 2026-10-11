import { ClipboardCopy, LockKeyhole, RefreshCw, Square } from 'lucide-react';
import { ActionButton } from './action-button';

export type RuntimeProcess = { pid: number; name: string; source: string; reason: string; executable: string | null; parent: string | null; startTime?: number };

export function RuntimeDiagnostics({ processes, message, onRefresh, onStop, busy }: { processes: RuntimeProcess[]; message: string; onRefresh: () => void; onStop: () => void; busy: boolean }) {
  return <section className="runtime-diagnostics">
    <div className="runtime-block"><LockKeyhole size={18} /><div><strong>{processes.length ? '后台进程可能修改聊天数据，导入和删除已暂停' : '当前没有检测到阻塞进程'}</strong><span>{message}</span></div><button className="icon-button" aria-label="刷新进程检测" title="刷新进程检测" disabled={busy} onClick={onRefresh}><RefreshCw size={16} /></button></div>
    {processes.length > 0 && <><div className="runtime-table"><table><thead><tr><th className="pid-column">PID</th><th>进程来源</th><th>原因 / 文件路径</th></tr></thead><tbody>{processes.map(process => <tr key={process.pid}><td className="mono">{process.pid}</td><td><strong>{process.source}</strong><span className="thread-id">{process.name}{process.parent ? ` · 上级：${process.parent}` : ''}</span></td><td><span>{process.reason}</span><span className="path-cell" title={process.executable || ''}>{process.executable || '路径不可读取，请手动退出'}</span></td></tr>)}</tbody></table></div><div className="runtime-actions"><button className="button" onClick={() => navigator.clipboard.writeText(JSON.stringify(processes, null, 2))}><ClipboardCopy size={15} />复制进程诊断</button><ActionButton className="button danger" reason={busy ? '操作进行中，请稍候' : !processes.some(p => p.executable && p.startTime) ? '进程身份不可验证，请手动退出对应程序' : null} onClick={onStop}><Square size={15} />结束阻塞进程</ActionButton></div></>}
  </section>;
}
