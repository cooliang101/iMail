import { api } from './api';
import type { MailRule, MailRuleInput, RulePreview, RuleRun } from '../app-model';

export const mailRulesService = {
  list: () => api<{ rules: MailRule[] }>('/api/mail-rules'),
  save: (input: MailRuleInput, id?: string) => api<{ rule: MailRule }>(id ? `/api/mail-rules/${encodeURIComponent(id)}` : '/api/mail-rules', { method: id ? 'PUT' : 'POST', body: JSON.stringify(input) }),
  remove: (id: string) => api(`/api/mail-rules/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  preview: (input: MailRuleInput, ruleId?: string) => api<RulePreview>('/api/mail-rules/preview', { method: 'POST', body: JSON.stringify({ input, ruleId }) }),
  apply: (token: string) => api<{ queued: number }>('/api/mail-rules/apply', { method: 'POST', body: JSON.stringify({ token, confirmed: true }) }),
  runs: () => api<{ runs: RuleRun[] }>('/api/mail-rule-runs'),
  retry: (id: string) => api(`/api/mail-rule-runs/${encodeURIComponent(id)}/retry`, { method: 'POST' }),
};
