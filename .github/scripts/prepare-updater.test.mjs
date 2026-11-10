import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

const script = fileURLToPath(new URL('./prepare-updater.mjs', import.meta.url))

for (const key of ['  test-public-key\n', '', '   ']) {
  test(key.trim() ? 'generates bundler config from another working directory' : `rejects empty key (${key.length} characters)`, () => {
    const directory = mkdtempSync(join(tmpdir(), 'savehub-updater-'))
    try {
      const output = join(directory, 'config.json')
      const result = spawnSync(process.execPath, [script, output], {
        cwd: directory, encoding: 'utf8',
        env: { ...process.env, SAVEHUB_UPDATER_PUBLIC_KEY: key },
      })
      if (key.trim()) {
        assert.equal(result.status, 0, result.stderr)
        const config = JSON.parse(readFileSync(output, 'utf8'))
        assert.equal(config.bundle.createUpdaterArtifacts, true)
        assert.equal(config.plugins.updater.pubkey, key.trim())
        assert.ok(!result.stdout.includes(key.trim()))
      } else {
        assert.equal(result.status, 1)
        assert.ok(!existsSync(output))
      }
    } finally {
      rmSync(directory, { recursive: true, force: true })
    }
  })
}
