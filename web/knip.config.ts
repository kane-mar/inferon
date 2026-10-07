import type { KnipConfig } from 'knip';

const config: KnipConfig = {
  // Files to exclude from Knip analysis
  ignore: [
    'src/components/ui/*',
    'src/libs/I18n.ts',
  ],
  // Dependencies to ignore during analysis
  ignoreDependencies: [
    '@swc/helpers', // Avoid error in CI: "`npm ci` can only install packages when your package.json and package-lock.json or npm-shrinkwrap.json are in sync."
    'lefthook', // Used via lefthook.yml git hooks, not imported
  ],
  // The API seam exports its row types for the 042d pages; they are used in-file today.
  ignoreExportsUsedInFile: true,
  // Include custom Playwright test file suffixes
  playwright: {
    entry: ['tests/**/*.@(integ|e2e).ts'],
  },
  compilers: {
    css: (text: string) => [...text.matchAll(/(?<=@)import[^;]+/g)].join('\n'),
  },
  treatConfigHintsAsErrors: true,
};

export default config;
