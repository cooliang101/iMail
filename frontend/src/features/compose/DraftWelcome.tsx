import '../../styles/compose.css';
import { PencilSimple } from '@phosphor-icons/react';

export function DraftWelcome({ onCreate }: { onCreate: () => void }) {
  return <section className="composer-pane composer-welcome"><PencilSimple size={48} weight="duotone" /><h2>选择草稿继续编辑</h2><p>修改会自动保存，也可以直接新建一封邮件。</p><button onClick={onCreate}>新建邮件</button></section>;
}
