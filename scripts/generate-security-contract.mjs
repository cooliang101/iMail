import { createCipheriv, createHash, createHmac, scryptSync } from 'node:crypto';

const masterKey = Buffer.from('000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f', 'hex');
const iv = Buffer.from('202122232425262728292a2b', 'hex');
const plaintext = '{"authType":"oauth2","accessToken":"access-secret","refreshToken":"refresh-secret","scopes":["mail.read","mail.send"]}';
const cipher = createCipheriv('aes-256-gcm', masterKey, iv);
const ciphertext = Buffer.concat([cipher.update(plaintext, 'utf8'), cipher.final()]);
const encryptedPayload = [iv, cipher.getAuthTag(), ciphertext].map((value) => value.toString('base64url')).join('.');
const password = 'iMail 密码兼容 Test 123!';
const passwordSalt = Buffer.from('30313233343536373839616263646566', 'hex');
const passwordHash = scryptSync(password, passwordSalt, 64);
const rawToken = 'imail_mcp_contract-token_1234567890';
const auditSalt = '0123456789abcdef'.repeat(4);
const auditActor = 'login-ip:192.0.2.42';

console.log(JSON.stringify({
  formatVersion: 1,
  masterKeyHex: masterKey.toString('hex'),
  ivHex: iv.toString('hex'),
  plaintext,
  encryptedPayload,
  password,
  passwordSaltHex: passwordSalt.toString('hex'),
  passwordEncoded: `scrypt:${passwordSalt.toString('base64url')}:${passwordHash.toString('base64url')}`,
  rawToken,
  tokenSha256: createHash('sha256').update(rawToken).digest('hex'),
  auditSalt,
  auditActor,
  auditActorHmacSha256: createHmac('sha256', auditSalt).update(auditActor).digest('hex'),
}, null, 2));
