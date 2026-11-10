// Generate bundler configuration with the same public key embedded by Rust.
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const publicKey = process.env.SAVEHUB_UPDATER_PUBLIC_KEY?.trim()
if (!publicKey) {
  console.error('Set SAVEHUB_UPDATER_PUBLIC_KEY before building updater artifacts.')
  process.exit(1)
}

const root = fileURLToPath(new URL('../../', import.meta.url))
const config = JSON.parse(readFileSync(resolve(root, 'tauri.updater.conf.json'), 'utf8'))
config.plugins = { ...config.plugins, updater: { ...config.plugins?.updater, pubkey: publicKey } }
const output = process.argv[2] ? resolve(process.argv[2]) : resolve(root, 'target/updater/tauri.updater.conf.json')
mkdirSync(dirname(output), { recursive: true })
writeFileSync(output, JSON.stringify(config, null, 2) + '\n')
console.log(`Updater configuration: ${output}`)
