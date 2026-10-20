import antfu from '@antfu/eslint-config'

export default antfu(
  {
    react: true,
    typescript: true,
    ignores: ['dist', 'coverage', 'src/app/routeTree.gen.ts', 'src/shared/components/ui/**'],
  },
  {
    // TanStack Router route files export the `Route` object next to their page component.
    files: ['src/app/routes/**/*.tsx'],
    rules: {
      'react-refresh/only-export-components': 'off',
    },
  },
)
