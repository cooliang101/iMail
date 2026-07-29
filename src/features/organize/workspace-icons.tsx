import type { IconProps } from '@phosphor-icons/react';
import { Briefcase, Buildings, Code, FolderSimple, Heart, House, Star, UsersThree } from '@phosphor-icons/react';
import type { WorkspaceIconId } from '../../types';

export const workspaceIconOptions: Array<{ id: WorkspaceIconId; label: string }> = [
  { id: 'folder', label: '文件夹' },
  { id: 'briefcase', label: '工作' },
  { id: 'building', label: '组织' },
  { id: 'home', label: '个人' },
  { id: 'users', label: '团队' },
  { id: 'code', label: '开发' },
  { id: 'heart', label: '关注' },
  { id: 'star', label: '收藏' },
];

export function WorkspaceIcon({ icon = 'folder', ...props }: IconProps & { icon?: WorkspaceIconId }) {
  const icons = { folder: FolderSimple, briefcase: Briefcase, building: Buildings, home: House, users: UsersThree, code: Code, heart: Heart, star: Star };
  const Icon = icons[icon];
  return <Icon {...props} />;
}
