import { Folder, PencilSimple, Plus } from '../../components/icons';
import { AppButton } from '../../components/AppButton';
import type { SmartFolder } from '../../app-model';
import './search.css';

export function SmartFoldersNav({ folders, activeId, loading, error, onRetry, onSelect, onEdit, onCreate }: {
  folders: SmartFolder[]; activeId: string | null; loading: boolean; error: string;
  onRetry: () => void; onSelect: (folder: SmartFolder) => void; onEdit: (folder: SmartFolder) => void; onCreate: () => void;
}) {
  return <section className="smart-folders-nav" aria-label="智能文件夹"><div className="section-label"><span>智能文件夹</span><button title="新增智能文件夹" aria-label="新增智能文件夹" onClick={onCreate}><Plus size={16} /></button></div>
    {loading && <p role="status">正在加载…</p>}{error && <div role="alert"><p>{error}</p><AppButton appearance="subtle" onClick={onRetry}>重试加载智能文件夹</AppButton></div>}
    {!loading && !error && folders.length === 0 && <AppButton appearance="subtle" className="smart-folder-empty" onClick={onCreate}>保存常用搜索</AppButton>}
    <nav className="nav-block">{folders.map((folder) => <div className="smart-folder-row" key={folder.id}><button title={folder.name} className={folder.id === activeId ? 'active' : ''} onClick={() => onSelect(folder)}><Folder size={17} /><span>{folder.name}</span></button><button aria-label={`编辑智能文件夹 ${folder.name}`} onClick={() => onEdit(folder)}><PencilSimple size={14} /></button></div>)}</nav>
  </section>;
}
