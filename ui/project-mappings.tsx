import { ArrowRight, ChevronDown, Folder, FolderOpen } from 'lucide-react';

export type SourceProject = { id: string; name: string; roots: string[] };
export type MappingDraft = { target: string; directory: string; name: string };
type LocalProject = { id: string; name: string; roots: string[] };

export function ProjectMappings({ groups, projects, drafts, disabled, onChange, onFolder }: {
  groups: SourceProject[]; projects: LocalProject[]; drafts: Record<string, MappingDraft>; disabled: boolean;
  onChange: (id: string, patch: Partial<MappingDraft>) => void; onFolder: (id: string) => void;
}) {
  return <section className="group-mappings" aria-label="项目分组映射">
    <div className="mapping-heading"><span>源项目</span><span>B 电脑的目标项目 / 文件夹</span></div>
    {groups.map(group => {
      const draft = drafts[group.id] || { target: '__new', directory: '', name: group.name };
      const current = projects.find(p => p.id === draft.target);
      return <div className="group-mapping-row" key={group.id} data-source-project-id={group.id}>
        <div className="source-project"><div><Folder size={17} /><strong>{group.name}</strong><ArrowRight size={15} /></div><span className="mono">{group.roots.join(' · ')}</span></div>
        <div className="mapping-destination">
          <div className="select"><Folder size={17} /><select aria-label={`目标项目：${group.name}`} disabled={disabled} value={draft.target} onChange={e => onChange(group.id, { target: e.target.value })}>
            <option value="__new">新建项目：{group.name}</option>
            {projects.map(project => <option key={project.id} value={project.id}>{project.name} · {project.roots[0]}</option>)}
          </select><ChevronDown size={15} /></div>
          {draft.target === '__new' ? <div className="new-group-fields">
            <label className="field-label">B 电脑项目文件夹<div className="path-input"><input aria-label={`项目文件夹：${group.name}`} disabled={disabled} value={draft.directory} placeholder="选择 B 电脑上的项目文件夹" onChange={e => onChange(group.id, { directory: e.target.value })} /><button className="icon-button" title={`选择 ${group.name} 的文件夹`} aria-label={`选择 ${group.name} 的文件夹`} disabled={disabled} onClick={() => onFolder(group.id)}><FolderOpen size={17} /></button></div></label>
            <label className="field-label">项目名称<input className="plain-input" aria-label={`项目名称：${group.name}`} disabled={disabled} value={draft.name} onChange={e => onChange(group.id, { name: e.target.value })} /></label>
          </div> : <span className="target-path mono">{current?.roots.join(' · ')}</span>}
        </div>
      </div>;
    })}
  </section>;
}
