export function applySettledResult<T>(
  result: PromiseSettledResult<unknown>,
  parse: (value: unknown) => T,
  apply: (value: T) => void,
) {
  if (result.status === 'rejected') return result.reason;
  try {
    apply(parse(result.value));
    return undefined;
  } catch (error) {
    return error;
  }
}
