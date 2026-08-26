const VERIFICATION_CONTEXT = /(?:验证码|动态码|校验码|安全码|登录码|确认码|一次性(?:密码|口令)|verification\s+code|sign[ -]?in\s+code|login\s+code|security\s+code|authentication\s+code|one[ -]?time\s+(?:code|passcode|password)|passcode|\botp\b|\b2fa\b)/giu;
const NUMERIC_CODE = /(?<!\d)(\d{4,8})(?!\d)/g;

export function findVerificationCode(value: string) {
  const source = value.replace(/\s+/g, ' ').trim();
  if (!source) return undefined;
  const contexts = [...source.matchAll(VERIFICATION_CONTEXT)];
  if (contexts.length === 0) return undefined;

  return [...source.matchAll(NUMERIC_CODE)]
    .map((match) => {
      const code = match[1];
      const index = match.index ?? 0;
      const distance = Math.min(...contexts.map((context) => {
        const contextIndex = context.index ?? 0;
        const contextEnd = contextIndex + context[0].length;
        if (index >= contextIndex && index <= contextEnd) return 0;
        return index < contextIndex ? contextIndex - (index + code.length) : index - contextEnd;
      }));
      return { code, distance, lengthPenalty: code.length === 6 ? 0 : 1 };
    })
    .filter((candidate) => candidate.distance <= 80)
    .sort((left, right) => left.distance - right.distance || left.lengthPenalty - right.lengthPenalty)[0]?.code;
}
