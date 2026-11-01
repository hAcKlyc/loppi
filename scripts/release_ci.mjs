// The main branch owns official releases. dev acceptance precedes merging here.
import { readFileSync, appendFileSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { releaseMetadata, releaseNotes } from './release_metadata.mjs';
import { githubClient, prepareRelease, uploadDraft, publishRelease, requireClean } from './publish_update.mjs';

export function releasePlan(metadata, release, tagSource) {
  if (release && !release.draft) return false; // Never rebuild a published version.
  if (tagSource && tagSource !== metadata.source) throw new Error('Existing tag belongs to another commit; use a new version');
  if (release && !tagSource && release.target_commitish !== metadata.source) throw new Error('Existing draft is not bound to this source commit');
  return true;
}

export function preflight(root, client, ref) {
  if (ref !== 'refs/heads/main') throw new Error('Official releases run only from main; accept changes on dev first');
  const version = JSON.parse(readFileSync(join(root,'package.json'))).version;
  const metadata = releaseMetadata(root, `v${version}`);
  const shouldRelease = releasePlan(metadata,client.maybeRelease(metadata.tag),client.maybeSource(metadata.tag));
  if (shouldRelease) {
    if (!client.isPublic()) throw new Error('Open-source/public repository required for anonymous updater distribution; visibility was not changed');
    releaseNotes(readFileSync(join(root,'CHANGELOG.md'),'utf8'),version);
  }
  return { ...metadata, shouldRelease };
}

export async function finalize(root, client, ref, notesFile, manifests, request = fetch) {
  if (ref !== 'refs/heads/main') throw new Error('Official releases run only from main');
  requireClean(root);
  const prepared = prepareRelease(root,notesFile,manifests);
  if (!client.isPublic()) throw new Error('Public repository required; visibility was not changed');
  const tagSource = client.maybeSource(prepared.tag);
  if (tagSource && tagSource !== prepared.source) throw new Error('Release tag belongs to another source commit');
  const existing = client.maybeRelease(prepared.tag);
  if (existing && !existing.draft) {
    // A retry after an unknown publish result verifies without republishing.
    await publishRelease(prepared,client,request);
    return prepared;
  }
  if (existing && !tagSource && existing.target_commitish !== prepared.source) throw new Error('Draft source commit mismatch');
  if (!tagSource) client.createTag(prepared.tag,prepared.source,prepared.manifest.notes);
  if (!existing) client.createDraft(prepared.tag,prepared.source,notesFile);
  uploadDraft(prepared,client);
  await publishRelease(prepared,client,request);
  return prepared;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
    const [mode,...args]=process.argv.slice(2);const client=githubClient(root);
    if (mode==='preflight') {
      const plan=preflight(root,client,process.env.GITHUB_REF);
      if(plan.shouldRelease)await client.waitForVerification(plan.source);
      if(process.env.GITHUB_OUTPUT)appendFileSync(process.env.GITHUB_OUTPUT,`version=${plan.version}\nsource=${plan.source}\nshould_release=${plan.shouldRelease}\n`);
      console.log(JSON.stringify(plan));
    } else if(mode==='publish') {
      const [notesFile,...manifests]=args;
      if(!notesFile||!manifests.length)throw new Error('publish requires notes file and platform manifests');
      const prepared=await finalize(root,client,process.env.GITHUB_REF,notesFile,manifests);
      const summary=`Published ${prepared.tag}: https://github.com/hAcKlyc/loppi/releases/tag/${prepared.tag}\nUpdater verified: https://github.com/hAcKlyc/loppi/releases/latest/download/latest.json\n`;
      if(process.env.GITHUB_STEP_SUMMARY)appendFileSync(process.env.GITHUB_STEP_SUMMARY,summary);
      console.log(summary);
    } else throw new Error('Usage: node scripts/release_ci.mjs preflight|publish [notes.txt manifests...]');
  } catch(error){ console.error(error.message);process.exitCode=1; }
}
