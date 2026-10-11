import React, { useEffect, useRef, useState } from 'react';
import { ImageOff, LoaderCircle, X } from 'lucide-react';
import type { ConversationTarget } from './conversation-preview';

export function PreviewImage({ target, token, messageIndex, imageIndex, label }: { target: ConversationTarget; token: string; messageIndex: number; imageIndex: number; label: string }) {
  const button = useRef<HTMLButtonElement>(null);
  const [visible,setVisible]=useState(false);
  const [source,setSource]=useState('');
  const [error,setError]=useState('');
  const [expanded,setExpanded]=useState(false);
  useEffect(()=>{
    const element=button.current;
    if(!element)return;
    const observer=new IntersectionObserver(entries=>{if(entries.some(entry=>entry.isIntersecting)){setVisible(true);observer.disconnect();}},{root:element.closest('.conversation-body'),rootMargin:'100px'});
    observer.observe(element);return()=>observer.disconnect();
  },[]);
  useEffect(()=>{
    if(!visible)return;
    const controller=new AbortController();
    fetch('/api/conversation-image',{method:'POST',headers:{'Content-Type':'application/json','X-Transfer-Token':token},body:JSON.stringify({...target,messageIndex,imageIndex}),signal:controller.signal})
      .then(async response=>{const data=await response.json();if(!response.ok)throw new Error(data.error||'图片不可用');if(!controller.signal.aborted)setSource(data.dataUrl);})
      .catch(e=>{if(!controller.signal.aborted)setError(e instanceof Error?e.message:String(e));});
    return()=>controller.abort();
  },[visible,target,token,messageIndex,imageIndex]);
  return <><button ref={button} className="preview-thumbnail" aria-label={`查看图片 ${label}`} disabled={!source||!!error} title={error||label} onClick={()=>setExpanded(true)}>
    {error ? <span className="image-unavailable"><ImageOff size={22}/><span>图片不可用</span><small>{error}</small></span> : source ? <img src={source} alt={label} onError={()=>setError('图片无法解码')}/> : <LoaderCircle className="spin" size={20}/>}
    <span className="image-caption">{label}</span>
  </button>{expanded&&<ImageViewer source={source} label={label} onClose={()=>setExpanded(false)}/>}</>;
}

function ImageViewer({source,label,onClose}:{source:string;label:string;onClose:()=>void}){
  const dialog=useRef<HTMLDialogElement>(null);
  useEffect(()=>{dialog.current?.showModal();},[]);
  return <dialog ref={dialog} className="image-viewer" aria-label="图片查看" onCancel={event=>{event.stopPropagation();onClose();}}><header><span>{label}</span><button className="icon-button" title="关闭图片" aria-label="关闭图片" onClick={onClose}><X size={20}/></button></header><img src={source} alt={label}/></dialog>;
}
