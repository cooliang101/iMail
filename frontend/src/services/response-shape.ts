type JsonRecord = Record<string, unknown>;

function invalidResponse(field?: string): never {
  throw new Error(field ? `iMail 收到的数据格式不正确：缺少 ${field}` : 'iMail 收到的数据格式不正确');
}

export function responseRecord(value: unknown): JsonRecord {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return invalidResponse();
  return value as JsonRecord;
}

export function responseArray<T>(value: unknown, field: string): T[] {
  const result = responseRecord(value)[field];
  if (!Array.isArray(result)) return invalidResponse(field);
  return result as T[];
}

export function responseObjectArray<T extends object>(value: unknown, field: string, validate?: (item: unknown) => item is T): T[] {
  const result = responseArray<unknown>(value, field);
  if (!result.every((item) => item !== null && typeof item === 'object' && !Array.isArray(item) && (!validate || validate(item)))) return invalidResponse(field);
  return result as T[];
}

export function responseObject<T extends object>(value: unknown, field: string, validate?: (item: unknown) => item is T): T {
  const result = responseRecord(value)[field];
  if (!result || typeof result !== 'object' || Array.isArray(result) || (validate && !validate(result))) return invalidResponse(field);
  return result as T;
}

export function responseStringArray(value: unknown, field: string) {
  const result = responseArray<unknown>(value, field);
  if (!result.every((item) => typeof item === 'string')) return invalidResponse(field);
  return result;
}

export function responseNumber(value: unknown, field: string) {
  const result = responseRecord(value)[field];
  if (typeof result !== 'number' || !Number.isFinite(result)) return invalidResponse(field);
  return result;
}

export function responseBoolean(value: unknown, field: string) {
  const result = responseRecord(value)[field];
  if (typeof result !== 'boolean') return invalidResponse(field);
  return result;
}

export function responseOptionalString(value: unknown, field: string) {
  const result = responseRecord(value)[field];
  if (result === undefined || result === null) return undefined;
  if (typeof result !== 'string') return invalidResponse(field);
  return result;
}
