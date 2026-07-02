# Pebesen Frontend

This vendored SvelteKit app is developed with the FlexNetOS workspace-managed Bun toolchain.

## Working commands in this workspace

```sh
# install dependencies
meta exec --include teri -- bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri/pebesen/frontend && bun install'

# start the dev server
meta exec --include teri -- bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri/pebesen/frontend && bun run dev'

# build the app
meta exec --include teri -- bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri/pebesen/frontend && bun run build'

# preview the production build
meta exec --include teri -- bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri/pebesen/frontend && bun run preview'
```

Do not use the older `npm` or `pnpm` scaffold examples in this checkout.
