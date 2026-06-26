import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import eslintConfigPrettier from 'eslint-config-prettier';
import svelte from 'eslint-plugin-svelte';

export default [
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...svelte.configs['flat/recommended'],
  eslintConfigPrettier,
  {
    files: ['**/*.ts', '**/*.js'],
    languageOptions: {
      ecmaVersion: 2024,
      sourceType: 'module',
    },
    rules: {
      '@typescript-eslint/no-require-imports': 'off',
      'no-undef': 'off',
    },
  },
  {
    files: ['**/*.svelte'],
    languageOptions: {
      parser: svelte.parser,
    },
    rules: {
      '@typescript-eslint/no-require-imports': 'off',
      'no-undef': 'off',
    },
  },
  {
    ignores: ['build/', '.svelte-kit/', 'dist/', 'node_modules/'],
  },
];
