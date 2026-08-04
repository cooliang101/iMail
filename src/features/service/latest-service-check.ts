export type LatestCheckHandlers<T> = {
  onSuccess: (value: T) => void;
  onError: (reason: unknown) => void;
  onSettled: () => void;
};

export function createLatestServiceCheckRunner() {
  let generation = 0;
  return {
    cancel() {
      generation += 1;
    },
    async run<T>(operation: () => Promise<T>, handlers: LatestCheckHandlers<T>) {
      const current = ++generation;
      try {
        const value = await operation();
        if (current !== generation) return;
        handlers.onSuccess(value);
      } catch (reason) {
        if (current !== generation) return;
        handlers.onError(reason);
      } finally {
        if (current === generation) handlers.onSettled();
      }
    },
  };
}
