import test from 'node:test';
import assert from 'node:assert/strict';
import resolveRelease from './resolve-release.cjs';

function fixture(eventName, release, status = 404) {
  const outputs = {};
  return {
    outputs,
    args: {
      tag: 'v26.9.4', version: '26.9.4', context: { eventName, repo: { owner: 'owner', repo: 'zapret-ui' } },
      core: { setOutput: (key, value) => { outputs[key] = value; }, info() {} },
      github: { rest: { repos: { async getReleaseByTag({ tag }) {
        assert.equal(tag, 'v26.9.4');
        if (release) return { data: release };
        throw Object.assign(new Error('GitHub API error'), { status });
      } } } },
    },
  };
}
test('manual rebuild targets the same release and preserves published/draft status', async () => {
  for (const draft of [true, false]) {
    const { args, outputs } = fixture('workflow_dispatch', { id: 42, draft, prerelease: false });
    await resolveRelease(args);
    assert.deepEqual(outputs, { release_id: '42', draft: String(draft), prerelease: 'false' });
  }
});
test('tag push can create a draft, but manual rebuild cannot accidentally create a release', async () => {
  const { args, outputs } = fixture('push');
  await resolveRelease(args);
  assert.equal(outputs.draft, 'true');
  assert.equal(outputs.release_id, '');
  await assert.rejects(resolveRelease(fixture('workflow_dispatch').args), /requires an existing release/);
});
test('rejects version mismatch and propagates authentication/network errors', async () => {
  await assert.rejects(resolveRelease({ ...fixture('workflow_dispatch').args, tag: 'v26.9.3' }), /must match/);
  await assert.rejects(resolveRelease(fixture('push', null, 403).args), /GitHub API error/);
});
