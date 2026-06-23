export default {
  plugins: ['prettier-plugin-svelte'],
  overrides: [
    {
      files: '*.svelte',
      options: {
        parser: 'svelte',
      },
    },
  ],
  tabWidth: 2,
  useTabs: false,
  singleQuote: true,
  trailingComma: 'es5',
  semi: true,
  printWidth: 100,
};
