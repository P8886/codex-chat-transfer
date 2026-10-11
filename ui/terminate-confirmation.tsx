import React, { useEffect, useRef } from 'react';
import { LoaderCircle, Square, X } from 'lucide-react';
import type { RuntimeProcess } from './runtime-diagnostics';

export function TerminateConfirmation({ processes, busy, error, onCancel, onConfirm }: { processes: RuntimeProcess[]; busy: boolean; error: string; onCancel: () => void; onConfirm: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => { dialog.current?.showModal(); cancel.current?.focus(); }, []);
  return <dialog ref={dialog} className="delete-dialog" aria-label="确认结束进程" onCancel={event => { if (busy) event.preventDefault(); else onCancel(); }}>
    <header className="conversation-heading"><h2>结束 {processes.length} 个阻塞进程？</h2><button className="icon-button" title="关闭确认" aria-label="关闭进程确认" disabled={busy} onClick={onCancel}><X size={18} /></button></header>
    <div className="delete-body"><p>请先保存工作并停止运行中的任务。强制结束可能中断聊天数据写入、关闭 Codex 窗口。</p><p>仅结束下列进程，不结束 VS Code / Cursor；扩展可能重新启动后台进程。此操作不会自动导入或删除聊天。</p><ul>{processes.map(p => <li key={p.pid}>{p.source}<span className="mono">{p.name} · PID {p.pid}</span><span className="mono">{p.executable}</span></li>)}</ul>{error && <div className="alert error" role="alert">{error}</div>}</div>
    <footer className="delete-footer"><button ref={cancel} className="button" disabled={busy} onClick={onCancel}>取消</button><button className="button danger" disabled={busy} onClick={onConfirm}>{busy ? <LoaderCircle size={16} className="spin" /> : <Square size={16} />}确认结束进程</button></footer>
  </dialog>;
}
