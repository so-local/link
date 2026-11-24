---
name: typescript
description: Build, test, lint and write idiomatic TypeScript or JavaScript, picking npm, pnpm or yarn from the lockfile.
---

# TypeScript and JavaScript

## Detect

The lockfile picks the package manager: `pnpm-lock.yaml` is pnpm,
`yarn.lock` is yarn, `package-lock.json` is npm (`bun.lockb` is bun). Use
only that one. `package.json` `scripts` names the real commands (`build`,
`test`, `lint`, `typecheck`) and `packageManager`/`engines` pins versions.
`tsconfig.json` configures TypeScript; `vitest.config.*`, `jest.config.*`
or `node --test` run tests; `eslint.config.*`/`.eslintrc*`, `.prettierrc`,
`biome.json` set style. A monorepo has `workspaces` or `pnpm-workspace.yaml`.
Check `node --version`.

## Commands

```sh
npm ci            # pnpm install --frozen-lockfile / yarn install --immutable
npm run build
npm test          # one file: npx vitest run path -t "name"; npx jest path -t "name"
npm run lint
npx tsc --noEmit                 # type check
npx prettier --check .           # or the project's format script
```

Use `ci`/frozen installs to avoid changing the lockfile. In a monorepo,
filter to the package (`pnpm --filter pkg test`, `npm -w pkg test`).

## Idioms

- Keep `strict` on. Avoid `any`; use `unknown` and narrow it; model
  variants with discriminated unions; validate external data at the boundary
  (zod or similar if present) since types vanish at runtime.
- `const` by default; `===`; optional chaining; `async`/`await` with
  errors handled, no floating promises, `Promise.all` for independent work.
- Respect module format (`"type": "module"`, `.js` extensions in ESM
  imports, `import` vs `require`).
- Do not use `eval`, `innerHTML` on untrusted text, or `child_process.exec`
  with built strings.

## Pitfalls

Mixing package managers rewrites lockfiles. Type checks pass while the
tests fail, and vice versa; run both. Snapshot tests: review before
updating. Timers and network in tests: use fake timers and mocks.
`node_modules` and `dist` stay out of commits. Browser versus Node APIs
differ.
