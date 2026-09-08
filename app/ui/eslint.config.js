/**
 * The UI linter.
 *
 * Type checked rules rather than the syntax only set, because the bug this is
 * here to catch is a promise nobody awaited: the shell fires RPC calls from
 * event handlers, and a rejected one that no `void` or `catch` marks is a
 * failure the user never sees. `no-floating-promises` needs the type checker to
 * know a call returns a promise at all, so `projectService` is on.
 *
 * `eslint-config-prettier` comes last and switches off every rule that has an
 * opinion about layout. Prettier owns that, and two tools disagreeing over the
 * same line is a fight with no winner.
 */

import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import prettier from 'eslint-config-prettier';

export default tseslint.config(
  {
    ignores: [
      'dist',
      'node_modules',
      'test-results',
      'playwright-report',
      // The driver's own config belongs to `tests/tsconfig.json`, and the
      // project service reads the root one. Linting it against the browser
      // project would report every `process.env` as an unknown global, which
      // says something about the tsconfig split rather than about the file.
      'playwright.config.ts',
      // Local overrides of it (see BN-160) belong to no tsconfig either.
      'playwright.*.config.ts',
    ],
  },
  js.configs.recommended,
  // The type checked set is for TypeScript alone. This file is plain JavaScript
  // and no tsconfig covers it, so a rule that asks the type checker about it
  // fails to load rather than reporting anything.
  ...tseslint.configs.recommendedTypeChecked.map((c) => ({
    ...c,
    files: ['src/**/*.ts', 'tests/**/*.ts', '*.config.ts'],
  })),
  {
    files: ['src/**/*.ts', 'tests/**/*.ts', '*.config.ts'],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      '@typescript-eslint/no-floating-promises': 'error',
    },
  },
  prettier,
);
