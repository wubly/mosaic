# Moving from Electron to the native app

The native rewrite lives in `native/` and the Electron app is still in the repo root, untouched. When
you are happy with the native build, promote it to the root with the steps below. Nothing here has been run.

## What can be removed

- `electron/`
- `app/` (the Next.js app directory)
- `components/`
- `next.config.mjs`
- `jsconfig.json`
- `package.json`
- `package-lock.json`
- `build/`, except the icons (`build/icon.*` are the only files in it today, so it can stay as it is)

Keep `docs/` (screenshots and the rewrite spec), `native/DECISIONS.md` and `.github/workflows/native.yml`.

## Command sequence

From the repository root, on a clean working tree:

```sh
git checkout native

# 1. remove the Electron/Next.js app
git rm -r electron app components next.config.mjs jsconfig.json package.json package-lock.json

# 2. promote the workspace (its `app/` crate would clash with the old `app/` directory, hence step 1)
git mv native/Cargo.toml native/Cargo.lock native/rust-toolchain.toml native/DECISIONS.md .
git mv native/crates native/app native/assets native/packaging .
git mv native/README.md docs/native.md
git rm -r --cached native 2>/dev/null; rm -rf native

# 3. ignore build output
printf 'target/\n' >> .gitignore

# 4. point CI at the root: delete the `defaults: run: working-directory: native` block and change
#    `workspaces: native` to `workspaces: .` and `path: native/target/...` to `path: target/...`
$EDITOR .github/workflows/native.yml

# 5. check it still builds
cargo build --release && cargo test

git add -A && git commit -m "Promote the native app to the repository root"
```

The packaging manifests (`packaging/flatpak`, `packaging/gentoo`) reference `native/` paths; replace the
`native/` prefix with nothing in `packaging/flatpak/app.mosaic.player.yml` and the `S=` line of the ebuild.
