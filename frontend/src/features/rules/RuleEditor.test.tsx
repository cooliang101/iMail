import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'preact-render-to-string';
import { parseHTML } from 'linkedom';
import { RuleEditor } from './RuleEditor';
import { ruleInput } from './rule-model';

describe('rule editor style contract', () => {
  it('uses shared theme-controlled buttons, inputs and custom select controls', () => {
    const { document } = parseHTML(renderToStaticMarkup(<RuleEditor value={ruleInput()} accounts={[]} disabled={false} onChange={vi.fn()} />));
    expect(document.querySelector('select')).toBeNull();
    expect(document.querySelectorAll('[aria-haspopup="listbox"]').length).toBeGreaterThan(0);
    for (const button of document.querySelectorAll('button')) expect(button.className).toMatch(/app-button|app-select/);
    for (const input of document.querySelectorAll('input:not([type="hidden"])')) {
      expect(input.closest('.app-input, .app-checkbox, .app-switch')).not.toBeNull();
    }
    expect(document.querySelectorAll('.rule-editor[style], .rule-clause[style], button[style]').length).toBe(0);
  });
  it('has labels and disables controls while submitting', () => {
    const { document } = parseHTML(renderToStaticMarkup(<RuleEditor value={ruleInput()} accounts={[]} disabled onChange={vi.fn()} />));
    expect(document.querySelector('fieldset')?.hasAttribute('disabled')).toBe(true);
    expect(document.querySelector('[aria-label="条件 1"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="动作 1"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="规则名称"]')).not.toBeNull();
  });
});
