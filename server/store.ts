import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import type { StoreData } from './types.js';

const dataDir = path.resolve('.data');
const file = path.join(dataDir, 'store.json');
const initial: StoreData = { accounts: [], messages: [], tokens: [] };
let queue = Promise.resolve();

export async function readStore(): Promise<StoreData> {
  await mkdir(dataDir, { recursive: true });
  try {
    return JSON.parse(await readFile(file, 'utf8')) as StoreData;
  } catch {
    return structuredClone(initial);
  }
}

export function updateStore(mutator: (data: StoreData) => void | Promise<void>): Promise<StoreData> {
  let output: StoreData;
  const operation = queue.catch(() => undefined).then(async () => {
    const data = await readStore();
    await mutator(data);
    const temp = `${file}.${process.pid}.tmp`;
    await writeFile(temp, JSON.stringify(data, null, 2), 'utf8');
    await rename(temp, file);
    output = data;
  });
  queue = operation.then(() => undefined, () => undefined);
  return operation.then(() => output!);
}
