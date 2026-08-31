import type { MailRuleInput, RuleAction, RuleCondition } from '../../app-model';

export const conditionOptions = [
  { value: 'sender', label: '发件人是' }, { value: 'senderDomain', label: '发件人域名是' },
  { value: 'recipient', label: '收件人 / 抄送人是' }, { value: 'subjectContains', label: '主题包含' },
  { value: 'bodyContains', label: '正文包含' }, { value: 'hasAttachments', label: '包含附件' },
  { value: 'unread', label: '未读' }, { value: 'flagged', label: '已加星标' },
  { value: 'mailboxRole', label: '邮件文件夹' }, { value: 'label', label: '已有标签' },
];
export const actionOptions = [
  { value: 'addLabel', label: '添加标签' }, { value: 'removeLabel', label: '移除标签' },
  { value: 'markRead', label: '设为已读' }, { value: 'flag', label: '设置星标' },
  { value: 'mute', label: '静音通知' }, { value: 'archive', label: '归档到邮箱服务器' },
];
export const mailboxOptions = [
  { value: 'inbox', label: '收件箱' }, { value: 'sent', label: '已发送' }, { value: 'archive', label: '归档' },
  { value: 'drafts', label: '草稿' }, { value: 'trash', label: '已删除' }, { value: 'junk', label: '垃圾邮件' }, { value: 'custom', label: '自定义文件夹' },
];
export function newCondition(field: string): RuleCondition {
  if (field === 'hasAttachments' || field === 'unread' || field === 'flagged') return { field, value: true };
  return { field: field as Extract<RuleCondition, { value: string }>['field'], value: field === 'mailboxRole' ? 'inbox' : '' };
}
export function newAction(type: string): RuleAction {
  if (type === 'archive') return { type };
  if (type === 'addLabel' || type === 'removeLabel') return { type, value: '' };
  return { type: type as 'markRead' | 'flag' | 'mute', value: true };
}
export function ruleInput(rule?: MailRuleInput): MailRuleInput {
  return rule ? { name: rule.name, enabled: rule.enabled, priority: rule.priority, accountIds: [...rule.accountIds], matchMode: rule.matchMode, conditions: rule.conditions.map(c => ({ ...c })), actions: rule.actions.map(a => ({ ...a })), stopProcessing: rule.stopProcessing } : {
    name: '', enabled: true, priority: 100, accountIds: [], matchMode: 'all', conditions: [{ field: 'subjectContains', value: '' }], actions: [{ type: 'addLabel', value: '' }], stopProcessing: false,
  };
}
export function validateRule(rule: MailRuleInput): string | null {
  const valid = (value: string, max: number) => value.trim().length > 0 && value.length <= max && !Array.from(value).some(char => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127);
  if (!valid(rule.name, 80)) return '请填写 1–80 个字符的规则名称';
  if (!Number.isInteger(rule.priority) || rule.priority < 0 || rule.priority > 10000) return '优先级需要 0–10000 的整数，数字越小越先执行';
  if (!rule.conditions.length || rule.conditions.length > 20 || !rule.actions.length || rule.actions.length > 20) return '规则需要 1–20 个条件和动作';
  for (const condition of rule.conditions) {
    if (typeof condition.value === 'string' && !valid(condition.value, condition.field === 'sender' || condition.field === 'recipient' ? 320 : 200)) return '请完整填写匹配条件';
    if ((condition.field === 'sender' || condition.field === 'recipient') && !/^[^\s@]+@[^\s@]+$/.test(condition.value)) return '发件人和收件人需要完整邮箱地址';
    if (condition.field === 'senderDomain' && !/^[a-z\d](?:[a-z\d.-]*[a-z\d])?\.[a-z\d-]+$/i.test(condition.value)) return '请输入完整域名，例如 github.com（不含 @）';
  }
  for (const action of rule.actions) if ('value' in action && typeof action.value === 'string' && !valid(action.value, 80)) return '请填写 1–80 个字符的标签';
  if (rule.actions.some((action, index) => action.type === 'archive' && index !== rule.actions.length - 1)) return '归档只能放在最后一个动作';
  return null;
}
