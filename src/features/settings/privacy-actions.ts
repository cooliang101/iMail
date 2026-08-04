export const CLEAR_USER_DATA_CONFIRMATION = '清除我的邮箱数据';

export function clearUserDataReady(currentPassword: string, confirmation: string) {
  return currentPassword.length >= 8 && confirmation === CLEAR_USER_DATA_CONFIRMATION;
}

export function authorizationExportPasswordError(password: string, repeated: string) {
  if (password.length < 12) return '导出文件密码至少需要 12 个字符';
  if (password !== repeated) return '两次输入的导出文件密码不一致';
  return '';
}
