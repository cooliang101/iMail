import { describe, expect, it } from 'vitest';
import { defaultExternalAccessSettings, externalAccessSettingsUpdateSchema } from './external-access.js';

describe('external access settings', () => {
  it('keeps the Gateway and MCP disabled by default', () => {
    expect(defaultExternalAccessSettings).toEqual({ gatewayEnabled: false, mcpEnabled: false });
  });

  it('accepts only explicit boolean changes', () => {
    expect(externalAccessSettingsUpdateSchema.parse({ gatewayEnabled: true })).toEqual({ gatewayEnabled: true });
    expect(() => externalAccessSettingsUpdateSchema.parse({})).toThrow();
    expect(() => externalAccessSettingsUpdateSchema.parse({ mcpEnabled: 'true' })).toThrow();
  });
});
