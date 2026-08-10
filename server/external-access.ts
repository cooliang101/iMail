import { z } from 'zod';
import { getMetadata, setMetadata } from './store.js';

const metadataKey = 'external_access_v1';

export const externalAccessSettingsSchema = z.object({
  gatewayEnabled: z.boolean(),
  mcpEnabled: z.boolean(),
});

export const externalAccessSettingsUpdateSchema = externalAccessSettingsSchema.partial()
  .refine((value) => Object.keys(value).length > 0, '至少提供一个要更新的外部接入设置');

export type ExternalAccessSettings = z.infer<typeof externalAccessSettingsSchema>;

export const defaultExternalAccessSettings: ExternalAccessSettings = {
  gatewayEnabled: false,
  mcpEnabled: false,
};

export async function readExternalAccessSettings(): Promise<ExternalAccessSettings> {
  const stored = await getMetadata(metadataKey);
  if (!stored) return structuredClone(defaultExternalAccessSettings);
  try {
    return externalAccessSettingsSchema.parse({ ...defaultExternalAccessSettings, ...JSON.parse(stored) });
  } catch {
    return structuredClone(defaultExternalAccessSettings);
  }
}

export async function updateExternalAccessSettings(changes: z.infer<typeof externalAccessSettingsUpdateSchema>) {
  const next = externalAccessSettingsSchema.parse({ ...await readExternalAccessSettings(), ...changes });
  await setMetadata(metadataKey, JSON.stringify(next));
  return next;
}
