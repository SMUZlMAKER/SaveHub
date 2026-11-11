// Exercise the exact inline publisher. The privileged job must not load PR scripts.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const workflow = readFileSync(new URL('../workflows/windows.yml', import.meta.url), 'utf8')
const source = workflow.split('      - name: Publish installer\n')[1].split('          script: |\n')[1]
  .split('\n').map(line => line.replace(/^ {12}/, '')).join('\n')
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor
const publish = new AsyncFunction('require', 'github', 'context', 'core', 'process', source)
const sha = '0123456789abcdef0123456789abcdef01234567'

function fixture({ preview = false, previous, existing = false, signature = 'signed', lookupError, installer = 'SaveHub_0.2.2_x64-setup.exe' } = {}) {
  const calls = []
  const missing = () => { throw Object.assign(new Error('Not found'), { status: 404 }) }
  const github = { rest: { repos: {
    getReleaseByTag: async args => {
      calls.push(['lookup', args])
      if (lookupError) throw Object.assign(new Error('Lookup failed'), { status: lookupError })
      return existing ? { data: { id: 9, draft: true } } : missing()
    },
    getLatestRelease: async () => previous ? { data: { assets: [{ name: `SaveHub_${previous}_x64-setup.exe` }], tag_name: 'build-legacy' } } : missing(),
    createRelease: async args => { calls.push(['create', args]); return { data: { id: 1 } } },
    uploadReleaseAsset: async args => { calls.push(['upload', args]) },
    updateRelease: async args => { calls.push(['update', args]); return { data: { html_url: 'https://example.test/release' } } },
  } } }
  const fs = {
    readdir: async () => [installer],
    readFile: async path => {
      if (path.endsWith('.sig')) { assert.ok(!preview, 'previews must not need signing material'); return signature }
      return Buffer.from('installer contents')
    },
  }
  const context = { repo: { owner: 'owner', repo: 'repo' }, serverUrl: 'https://github.com', runId: 12345 }
  const core = { summary: { addLink: () => ({ write: async () => {} }) } }
  const run = () => publish(name => name === 'node:fs/promises' ? fs : require(name), github, context, core, {
    env: { SOURCE_SHA: sha, PRERELEASE: String(preview) },
  })
  return { calls, run }
}

test('stable release uses installer version and signs a matching versioned download URL', async () => {
  const { calls, run } = fixture({ previous: '0.2.1' })
  await run()
  const created = calls.find(([kind]) => kind === 'create')[1]
  assert.equal(created.name, 'SaveHub 0.2.2')
  assert.equal(created.tag_name, 'v0.2.2')
  assert.equal(created.target_commitish, sha)
  assert.equal(created.draft, true)
  const uploads = calls.filter(([kind]) => kind === 'upload').map(([, args]) => args)
  assert.deepEqual(uploads.map(a => a.name), ['SaveHub_0.2.2_x64-setup.exe', 'SaveHub_0.2.2_x64-setup.exe.sig', 'latest.json'])
  const manifest = JSON.parse(uploads[2].data.toString())
  assert.equal(manifest.version, '0.2.2')
  assert.equal(manifest.platforms['windows-x86_64'].signature, 'signed')
  assert.equal(manifest.platforms['windows-x86_64'].url, 'https://github.com/owner/repo/releases/download/v0.2.2/SaveHub_0.2.2_x64-setup.exe')
  assert.equal(calls.at(-1)[1].make_latest, 'true')
})

test('preview has a version title and commit tag without build numbers or updater assets', async () => {
  const { calls, run } = fixture({ preview: true })
  await run()
  const created = calls.find(([kind]) => kind === 'create')[1]
  assert.equal(created.name, 'SaveHub 0.2.2')
  assert.equal(created.tag_name, `v0.2.2-preview.${sha.slice(0, 12)}`)
  assert.equal(created.prerelease, true)
  assert.equal(calls.filter(([kind]) => kind === 'upload').length, 1)
  assert.equal(calls.at(-1)[1].make_latest, 'false')
})

for (const [name, options, message] of [
  ['existing tag', { existing: true }, /already exists/],
  ['same stable version', { previous: '0.2.2' }, /Increase Cargo/],
  ['older stable version', { previous: '0.3.0' }, /Increase Cargo/],
  ['empty signature', { signature: '' }, /Missing updater signature/],
  ['invalid installer version', { installer: 'SaveHub_latest_x64-setup.exe' }, /version expected/],
  ['release lookup failure', { lookupError: 403 }, /Lookup failed/],
]) {
  test(`rejects ${name} before creating or replacing releases`, async () => {
    const { calls, run } = fixture(options)
    await assert.rejects(run(), message)
    assert.ok(calls.every(([kind]) => kind === 'lookup'))
  })
}
