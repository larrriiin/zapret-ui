// Shared with local tests; this step only reads GitHub state.
module.exports = async function resolveRelease({ github, context, core, tag, version }) {
  if (tag !== `v${version}`) {
    throw new Error(`Release tag must match the checked-out version: expected v${version}, received ${tag}.`);
  }
  let release;
  try {
    release = (await github.rest.repos.getReleaseByTag({ ...context.repo, tag })).data;
  } catch (error) {
    if (error.status !== 404) throw error;
    if (context.eventName === 'workflow_dispatch') {
      throw new Error(`Release ${tag} does not exist. Manual rebuild requires an existing release or draft.`);
    }
  }
  core.setOutput('release_id', release ? String(release.id) : '');
  core.setOutput('draft', release ? String(release.draft) : 'true');
  core.setOutput('prerelease', release ? String(release.prerelease) : 'false');
  core.info(release ? `Rebuilding existing release ${tag} (${release.id}) from the selected workflow revision.` : `Creating a new draft release for ${tag}.`);
};
