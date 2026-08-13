import eslint from '@eslint/js';
import jsxA11y from 'eslint-plugin-jsx-a11y';
import reactHooks from 'eslint-plugin-react-hooks';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  { ignores: ['dist/**', 'node_modules/**'] },
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['src/**/*.{ts,tsx}', 'vite.config.ts'],
    plugins: {
      'jsx-a11y': jsxA11y,
      'react-hooks': reactHooks,
    },
    languageOptions: {
      globals: {
        AbortController: 'readonly',
        Blob: 'readonly',
        CustomEvent: 'readonly',
        DOMParser: 'readonly',
        Element: 'readonly',
        Event: 'readonly',
        EventSource: 'readonly',
        File: 'readonly',
        FileReader: 'readonly',
        FormData: 'readonly',
        HTMLElement: 'readonly',
        HTMLInputElement: 'readonly',
        HTMLTextAreaElement: 'readonly',
        KeyboardEvent: 'readonly',
        LocalStorage: 'readonly',
        MessageEvent: 'readonly',
        MouseEvent: 'readonly',
        MutationObserver: 'readonly',
        Node: 'readonly',
        RequestInit: 'readonly',
        Response: 'readonly',
        Storage: 'readonly',
        URL: 'readonly',
        URLSearchParams: 'readonly',
        WebSocket: 'readonly',
        Window: 'readonly',
        console: 'readonly',
        document: 'readonly',
        fetch: 'readonly',
        localStorage: 'readonly',
        navigator: 'readonly',
        process: 'readonly',
        setTimeout: 'readonly',
        window: 'readonly',
      },
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-unused-vars': 'off',
      'no-undef': 'off',
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
      'jsx-a11y/alt-text': 'error',
      'jsx-a11y/anchor-has-content': 'error',
      'jsx-a11y/aria-props': 'error',
      'jsx-a11y/aria-proptypes': 'error',
      'jsx-a11y/aria-role': 'error',
      'jsx-a11y/aria-unsupported-elements': 'error',
      // AppInput/AppSelect/AppCheckbox are composite controls; axe verifies the rendered associations.
      'jsx-a11y/label-has-associated-control': 'off',
    },
  },
  {
    files: ['src/App.tsx'],
    rules: {
      // Desktop event subscriptions are intentionally registered once and use React setters/ref snapshots.
      'react-hooks/exhaustive-deps': 'off',
    },
  },
);
