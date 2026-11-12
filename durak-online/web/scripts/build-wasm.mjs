// Build the Rust engine to WebAssembly and copy it into src/wasm/.
// Works the same on Windows, macOS and Linux.
//
//   node scripts/build-wasm.mjs              always rebuild
//   node scripts/build-wasm.mjs --if-stale   only when there is no build yet, or the
//                                            Rust code changed since the last build

import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const web = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const root = resolve(web, '..')
const crate = join(root, 'durak-wasm')
const pkg = join(crate, 'pkg')
const out = join(web, 'src', 'wasm')
const built = join(out, 'durak_wasm_bg.wasm')

/** Newest modification time of any Rust source or manifest the engine is built from. */
function newestSource() {
  let newest = 0
  const visit = path => {
    if (!existsSync(path)) return
    const st = statSync(path)
    if (st.isDirectory()) {
      for (const name of readdirSync(path)) visit(join(path, name))
    } else if (/\.(rs|toml|lock)$/.test(path)) {
      newest = Math.max(newest, st.mtimeMs)
    }
  }
  for (const part of ['durak-core/src', 'durak-core/Cargo.toml', 'durak-wasm/src', 'durak-wasm/Cargo.toml', 'Cargo.toml', 'Cargo.lock', '.cargo/config.toml']) {
    visit(join(root, part))
  }
  return newest
}

if (process.argv.includes('--if-stale') && existsSync(built) && statSync(built).mtimeMs >= newestSource()) {
  process.exit(0)
}

console.log('Building the game engine (wasm-pack build --target web). The first build takes a minute or two...')
const run = spawnSync('wasm-pack', ['build', '--target', 'web'], {
  cwd: crate,
  stdio: 'inherit',
  shell: process.platform === 'win32',
})
if (run.error || run.status !== 0) {
  console.error(
    '\nCould not build the game engine.\n' +
      'If the message above says wasm-pack is not recognised or not found, install the prerequisites:\n' +
      '  rustup target add wasm32-unknown-unknown\n' +
      '  cargo install wasm-pack\n' +
      'then close this window, open a new one and try again. See INSTALL.md for help.\n',
  )
  process.exit(run.status || 1)
}

mkdirSync(out, { recursive: true })
const files = readdirSync(pkg).filter(name => name.startsWith('durak_wasm'))
for (const name of files) copyFileSync(join(pkg, name), join(out, name))
console.log(`Copied ${files.length} files to src/wasm/`)
