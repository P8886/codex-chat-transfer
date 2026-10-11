import React, { useId, useState } from 'react';
import { createPortal } from 'react-dom';

export function ActionButton({ reason, ...props }: React.ButtonHTMLAttributes<HTMLButtonElement> & { reason?: string | null }) {
  const id = useId();
  const [position, setPosition] = useState<{top:number;left:number} | null>(null);
  function show(element: HTMLElement) {
    if (!reason) return;
    const rect=element.getBoundingClientRect();
    setPosition({top:rect.bottom+110 < innerHeight ? rect.bottom+8 : Math.max(8,rect.top-100),left:Math.max(8,Math.min(rect.right-300,innerWidth-308))});
  }
  return <span className="action-button" tabIndex={reason ? 0 : undefined} aria-describedby={reason ? id : undefined} onClick={e=>{if(reason)e.stopPropagation();}} onMouseEnter={e=>show(e.currentTarget)} onMouseLeave={()=>setPosition(null)} onFocus={e=>show(e.currentTarget)} onBlur={()=>setPosition(null)}>
    <button {...props} disabled={!!reason || props.disabled} aria-describedby={reason ? id : undefined} />
    {reason && position && createPortal(<span id={id} className="tooltip-text" role="tooltip" style={{position:'fixed',display:'block',top:position.top,left:position.left,bottom:'auto',right:'auto',width:300}}>{reason}</span>,document.body)}
  </span>;
}
