import React, { useEffect, useRef } from 'react';
import { LoaderCircle, Trash2, X } from 'lucide-react';

export type DeleteTarget = { home: string; threads: { id: string; title: string }[] };

export function DeleteConfirmation({ target, busy, error, onCancel, onConfirm }: { target: DeleteTarget; busy: boolean; error: string; onCancel: () => void; onConfirm: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => { dialog.current?.showModal(); cancel.current?.focus(); }, []);
  return <dialog ref={dialog} className="delete-dialog" aria-label="确认删除会话" onCancel={event => { if (busy) event.preventDefault(); else onCancel(); }}>
    <header className="conversation-heading"><h2>删除 {target.threads.length} 条本机会话？</h2><button className="icon-button" aria-label="关闭删除提示" disabled={busy} onClick={onCancel}><X size={18} /></button></header>
    <div className="delete-body"><p>会话及本机索引将被删除，原始记录会先备份。项目文件不会删除。</p><ul>{target.threads.map(thread => <li key={thread.id}>{thread.title}<span className="mono">{thread.id}</span></li>)}</ul>{error && <div className="alert error" role="alert">{error}</div>}</div>
    <footer className="delete-footer"><button ref={cancel} className="button" disabled={busy} onClick={onCancel}>取消</button><button className="button danger" disabled={busy} onClick={onConfirm}>{busy ? <LoaderCircle size={16} className="spin" /> : <Trash2 size={16} />}确认删除 {target.threads.length} 条</button></footer>
  </dialog>;
}
