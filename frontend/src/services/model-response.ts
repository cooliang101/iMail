import type { MailNotification, MessageStats } from '../app-model';
import type { Account, Contact, DeveloperToken, Draft, MailWorkItemView, Message, MessageAttachment, OutboxItem } from '../types';
import { responseObject, responseObjectArray } from './response-shape';

type RecordValue = Record<string, unknown>;
const record = (value: unknown): value is RecordValue => Boolean(value && typeof value === 'object' && !Array.isArray(value));
const string = (value: unknown): value is string => typeof value === 'string';
const number = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);
const boolean = (value: unknown): value is boolean => typeof value === 'boolean';
const optionalString = (value: unknown) => value === undefined || string(value);
const stringArray = (value: unknown): value is string[] => Array.isArray(value) && value.every(string);
const dateString = (value: unknown): value is string => string(value) && !Number.isNaN(Date.parse(value));

const participant = (value: unknown) => record(value) && string(value.name) && string(value.address);
const logo = (value: unknown) => record(value) && string(value.url);
const attachment = (value: unknown): value is MessageAttachment => record(value)
  && string(value.filename) && string(value.contentType) && number(value.size) && number(value.index);

export function isAccount(value: unknown): value is Account {
  return record(value)
    && string(value.id) && string(value.provider) && string(value.email) && string(value.displayName)
    && string(value.group) && string(value.groupIcon) && string(value.color) && string(value.status)
    && Array.isArray(value.mailboxes) && value.mailboxes.every((mailbox) => record(mailbox) && string(mailbox.path) && string(mailbox.name));
}

export function isMessage(value: unknown): value is Message {
  return record(value)
    && string(value.id) && string(value.accountId) && string(value.mailbox) && string(value.mailboxRole)
    && record(value.from) && participant(value.from) && logo(value.from.logo)
    && Array.isArray(value.to) && value.to.every(participant)
    && string(value.subject) && string(value.preview) && dateString(value.date)
    && boolean(value.unread) && boolean(value.flagged) && boolean(value.hasAttachments)
    && Array.isArray(value.attachments) && value.attachments.every(attachment)
    && stringArray(value.labels) && optionalString(value.text) && optionalString(value.html);
}

export function isContact(value: unknown): value is Contact {
  return record(value) && string(value.address) && string(value.name) && number(value.messageCount)
    && dateString(value.lastContactAt) && logo(value.logo);
}

export function isDraft(value: unknown): value is Draft {
  return record(value) && string(value.id) && string(value.accountId) && stringArray(value.to) && stringArray(value.cc)
    && string(value.subject) && string(value.text) && string(value.html) && dateString(value.createdAt) && dateString(value.updatedAt)
    && Array.isArray(value.attachments) && value.attachments.every((item) => record(item) && string(item.id) && string(item.filename) && string(item.contentType) && number(item.size) && string(item.data));
}

export function isDeveloperToken(value: unknown): value is DeveloperToken {
  return record(value) && string(value.id) && string(value.name) && string(value.prefix)
    && stringArray(value.scopes) && stringArray(value.mailboxes) && dateString(value.createdAt) && dateString(value.expiresAt)
    && optionalString(value.lastUsedAt);
}

export function isOutboxItem(value: unknown): value is OutboxItem {
  return record(value) && string(value.id) && string(value.accountId) && stringArray(value.to) && stringArray(value.cc)
    && stringArray(value.bcc) && string(value.subject) && string(value.scheduledAt) && string(value.status)
    && number(value.attempts) && dateString(value.createdAt) && dateString(value.updatedAt);
}

function isWorkItemView(value: unknown): value is MailWorkItemView {
  if (!record(value) || !record(value.item) || !isMessage(value.message)) return false;
  const item = value.item;
  return string(item.id) && string(item.messageId) && string(item.accountId) && string(item.status)
    && string(item.note) && string(item.createdAt) && string(item.updatedAt) && optionalString(item.dueAt) && optionalString(item.draftId);
}

export function isMailNotification(value: unknown): value is MailNotification {
  return record(value) && string(value.id) && string(value.kind) && string(value.title) && string(value.detail)
    && dateString(value.date) && string(value.accountId) && optionalString(value.messageId);
}

const accountStat = (value: unknown): value is MessageStats['byAccount'][number] => record(value) && string(value.accountId) && number(value.total) && number(value.unread);
const groupStat = (value: unknown): value is MessageStats['byGroup'][number] => record(value) && string(value.group) && number(value.total) && number(value.unread);

export const accountsFromResponse = (value: unknown, field = 'accounts') => responseObjectArray<Account>(value, field, isAccount);
export const messagesFromResponse = (value: unknown, field = 'messages') => responseObjectArray<Message>(value, field, isMessage);
export const contactsFromResponse = (value: unknown, field = 'contacts') => responseObjectArray<Contact>(value, field, isContact);
export const draftsFromResponse = (value: unknown, field = 'drafts') => responseObjectArray<Draft>(value, field, isDraft);
export const tokensFromResponse = (value: unknown, field = 'tokens') => responseObjectArray<DeveloperToken>(value, field, isDeveloperToken);
export const outboxFromResponse = (value: unknown, field = 'items') => responseObjectArray<OutboxItem>(value, field, isOutboxItem);
export const workItemsFromResponse = (value: unknown, field = 'items') => responseObjectArray<MailWorkItemView>(value, field, isWorkItemView);
export const notificationsFromResponse = (value: unknown, field = 'notifications') => responseObjectArray<MailNotification>(value, field, isMailNotification);
export const messageFromResponse = (value: unknown, field = 'message') => responseObject<Message>(value, field, isMessage);
export const accountStatsFromResponse = (value: unknown, field = 'byAccount') => responseObjectArray<MessageStats['byAccount'][number]>(value, field, accountStat);
export const groupStatsFromResponse = (value: unknown, field = 'byGroup') => responseObjectArray<MessageStats['byGroup'][number]>(value, field, groupStat);
