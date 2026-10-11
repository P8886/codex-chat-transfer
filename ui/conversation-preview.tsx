import React, { useEffect, useRef, useState } from 'react';
import { ChevronLeft, ChevronRight, LoaderCircle, X } from 'lucide-react';
import { PreviewImage } from './preview-image';

export type ConversationTarget = { threadId: string; title: string; home?: string; package?: string };
type Page = { title: string; total: number; offset: number; warnings?: string[]; messages: { role: string; text: string; timestamp: string | null; truncated: boolean; images?: {label:string}[] }[] };

export function ConversationPreview({ target, token, onClose }: { target: ConversationTarget; token: string; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<Page | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);

  useEffect(() => { dialog.current?.showModal(); }, []);
  useEffect(() => {
    const controller = new AbortController();
    setLoading(true); setError(''); setPage(null);
    fetch('/api/conversation', {
      method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Transfer-Token': token },
      body: JSON.stringify({ ...target, offset }), signal: controller.signal,
    }).then(async response => {
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || '读取会话失败');
      if (!controller.signal.aborted) { setPage(result); scroll.current?.scrollTo(0, 0); }
    }).catch(e => { if (!controller.signal.aborted) setError(e instanceof Error ? e.message : String(e)); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [target, token, offset]);

  return <dialog ref={dialog} className="conversation-dialog" aria-label="会话预览" onCancel={onClose}>
    <header className="conversation-heading"><div><h2>{target.title}</h2><span>{target.package ? '导出包' : '本机聊天'} · {page ? `${page.total} 条消息` : '会话预览'}</span></div><button className="icon-button" aria-label="关闭预览" title="关闭预览" onClick={onClose}><X size={20} /></button></header>
    <div ref={scroll} className="conversation-body" aria-busy={loading}>
      {loading && <div className="conversation-empty"><LoaderCircle size={20} className="spin" />正在读取会话</div>}
      {error && <div className="alert error" role="alert">{error}</div>}
      {page?.warnings?.map((warning, index) => <div className="alert warning" role="status" key={index}>{warning}</div>)}
      {page && page.messages.length === 0 && <p className="conversation-empty">没有可预览的正文消息</p>}
      {page?.messages.map((message, index) => <article className={`conversation-message ${message.role}`} key={offset + index}><div className="message-meta"><strong>{message.role === 'user' ? '用户' : '助手'}</strong>{message.timestamp && <time>{new Date(message.timestamp).toLocaleString('zh-CN')}</time>}</div><div className="message-content"><div className="message-text">{message.text}</div>{message.images&&message.images.length>0&&<div className="message-images">{message.images.map((image,imageIndex)=><PreviewImage key={imageIndex} target={target} token={token} messageIndex={offset+index} imageIndex={imageIndex} label={image.label}/>)}</div>}{message.truncated && <p className="message-truncated">这条消息过长，仅显示前 131072 个字符。</p>}</div></article>)}
    </div>
    <footer className="conversation-footer"><span>{page && page.total > 0 ? `${page.offset + 1}–${page.offset + page.messages.length} / ${page.total}` : ''}</span><button className="icon-button" aria-label="上一页消息" title="上一页消息" disabled={loading || offset === 0} onClick={() => setOffset(Math.max(0, offset - 50))}><ChevronLeft size={20} /></button><button className="icon-button" aria-label="下一页消息" title="下一页消息" disabled={loading || !page || offset + page.messages.length >= page.total} onClick={() => setOffset(offset + 50)}><ChevronRight size={20} /></button></footer>
  </dialog>;
}
