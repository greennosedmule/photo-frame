/** Run `fn` over `items` with at most `limit` in flight; resolves when all settle. */
export async function pool<T>(
  items: readonly T[],
  limit: number,
  fn: (item: T) => Promise<void>,
  onProgress?: (done: number) => void,
): Promise<{ failed: T[]; error?: unknown }> {
  const failed: T[] = [];
  let firstError: unknown;
  let next = 0;
  let done = 0;
  const worker = async () => {
    while (next < items.length) {
      const item = items[next++]!;
      try {
        await fn(item);
      } catch (e) {
        failed.push(item);
        firstError ??= e;
      }
      onProgress?.(++done);
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return { failed, error: firstError };
}
