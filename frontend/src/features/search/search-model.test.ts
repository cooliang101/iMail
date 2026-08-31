import { describe, expect, it } from 'vitest';
import { buildMessageQuery } from '../../app/selectors';
import { filtersFromQuery, localDateInput, validateFilters } from './search-model';
import { embeddedDomainCall } from '../../services/mail';

describe('advanced search and smart-folder contracts', () => {
  it('captures the complete current scope when saving an ordinary search', () => {
    const filters = filtersFromQuery('accountId=a1&group=Work&mailboxRole=inbox&q=budget&sender=s%40example.test&label=finance&unread=true');
    expect(filters).toEqual({ accountIds:['a1'],group:'Work',mailboxRole:'inbox',q:'budget',sender:'s@example.test',labels:['finance'],unread:true,snoozed:false });
  });
  it('does not impose inbox scope on an all-mail smart folder, and preserves false conditions', () => {
    const query = buildMessageQuery({ view:'search', search:'最新', searchFilters:{ body:'预算',unread:false,accountIds:['a1','a2'] }, accountFilter:'all',groupFilter:null,mailFilter:'all',activeLabel:null,activeMailbox:null });
    expect(new URLSearchParams(query).has('mailboxRole')).toBe(false);
    expect(filtersFromQuery(query)).toEqual({ body:'预算',unread:false,accountIds:['a1','a2'],q:'最新' });
  });
  it('round-trips local time and validates intervals and addresses', () => {
    const instant = '2026-08-31T09:20:00.000Z';
    expect(new Date(localDateInput(instant)).toISOString()).toBe(instant);
    expect(validateFilters({since:instant,before:instant})).toBeTruthy();
    expect(validateFilters({recipient:'invalid'})).toBeTruthy();
    expect(validateFilters({recipient:'a@example.test'})).toBeNull();
  });
  it('maps all smart-folder mutations to typed local desktop operations', () => {
    expect(embeddedDomainCall('/api/smart-folders')).toEqual({operation:'smartFoldersList'});
    const input = {name:'预算',filters:{body:'预算',unread:false}};
    expect(embeddedDomainCall('/api/smart-folders',{method:'POST',body:JSON.stringify(input)})).toEqual({operation:'smartFolderCreate',input});
    expect(embeddedDomainCall('/api/smart-folders/id',{method:'PUT',body:JSON.stringify(input)})).toEqual({operation:'smartFolderUpdate',folderId:'id',input});
    expect(embeddedDomainCall('/api/smart-folders/id',{method:'DELETE'})).toEqual({operation:'smartFolderDelete',folderId:'id'});
  });
});
