// Build the Rust engine to WebAssembly and copy it into src/wasm/.
// Works the same on Windows, macOS and Linux.
//
//   node scripts/build-wasm.mjs               always rebuild
//   node scripts/build-wasm.mjs --if-missing  only when src/wasm/ has no build yet

import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const web = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const crate = resolve(web, '..', 'durak-wasm')
const pkg = join(crate, 'pkg')
const out = join(web, 'src', 'wasm')

if (process.argv.includes('--if-missing') && existsSync(join(out, 'durak_wasm_bg.wasm'))) {
  process.exit(0)
}

console.log('Building the WASM engine (wasm-pack build --target web)...')
const run = spawnSync('wasm-pack', ['build', '--target', 'web'], {
  cwd: crate,
  stdio: 'inherit',
  shell: process.platform === 'win32',
})
if (run.error || run.status !== 0) {
  console.error(
    '\nCould not build the WASM engine. Install the prerequisites and try again:\n' +
      '  rustup target add wasm32-unknown-unknown\n' +
      '  cargo install wasm-pack\n',
  )
  process.exit(run.status || 1)
}

mkdirSync(out, { recursive: true })
const files = readdirSync(pkg).filter(name => name.startsWith('durak_wasm'))
for (const name of files) copyFileSync(join(pkg, name), join(out, name))
console.log(`Copied ${files.length} files to src/wasm/`)
