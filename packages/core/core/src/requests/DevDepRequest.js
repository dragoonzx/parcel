// @flow strict-local
import type {ModuleSpecifier} from '@parcel/types';
import type ParcelConfig from '../ParcelConfig';
import type {
  DevDepRequest,
  ParcelOptions,
  InternalDevDepOptions,
} from '../types';
import type {RunAPI} from '../RequestTracker';
import type {ProjectPath} from '../projectPath';

import nullthrows from 'nullthrows';
import {getInvalidationHash} from '../assetUtils';
import {createBuildCache} from '../buildCache';
import {invalidateOnFileCreateToInternal} from '../utils';
import {
  fromProjectPath,
  fromProjectPathRelative,
  toProjectPath,
} from '../projectPath';

export async function createDevDependency(
  opts: InternalDevDepOptions,
  plugin: {name: ModuleSpecifier, resolveFrom: ProjectPath, ...},
  requestDevDeps: Map<string, string>,
  options: ParcelOptions,
): Promise<DevDepRequest> {
  let {moduleSpecifier, resolveFrom, invalidateParcelPlugin} = opts;
  let key = `${moduleSpecifier}:${fromProjectPathRelative(resolveFrom)}`;

  // If the request sent us a hash, we know the dev dep and all of its dependencies didn't change.
  // Reuse the same hash in the response. No need to send back invalidations as the request won't
  // be re-run anyway.
  let hash = requestDevDeps.get(key);
  if (hash != null) {
    return {
      moduleSpecifier,
      resolveFrom,
      hash,
    };
  }

  let resolveFromAbsolute = fromProjectPath(options.projectRoot, resolveFrom);

  // Ensure that the package manager has an entry for this resolution.
  await options.packageManager.resolve(moduleSpecifier, resolveFromAbsolute);
  let invalidations = options.packageManager.getInvalidations(
    moduleSpecifier,
    resolveFromAbsolute,
  );

  let invalidateOnFileChangeProject = [
    ...invalidations.invalidateOnFileChange,
  ].map(f => toProjectPath(options.projectRoot, f));

  // It is possible for a transformer to have multiple different hashes due to
  // different dependencies (e.g. conditional requires) so we must always
  // recompute the hash and compare rather than only sending a transformer
  // dev dependency once.
  hash = await getInvalidationHash(
    invalidateOnFileChangeProject.map(f => ({
      type: 'file',
      filePath: f,
    })),
    options,
  );

  let devDepRequest: DevDepRequest = {
    moduleSpecifier,
    resolveFrom,
    hash,
    invalidateOnFileCreate: invalidations.invalidateOnFileCreate.map(i =>
      invalidateOnFileCreateToInternal(options.projectRoot, i),
    ),
    invalidateOnFileChange: new Set(invalidateOnFileChangeProject),
  };

  // Optionally also invalidate the parcel plugin that is loading the config
  // when this dev dep changes (e.g. to invalidate local caches).
  if (invalidateParcelPlugin) {
    devDepRequest.additionalInvalidations = [
      {
        moduleSpecifier: plugin.name,
        resolveFrom: plugin.resolveFrom,
      },
    ];
  }

  return devDepRequest;
}

type DevDepSpecifier = {|
  moduleSpecifier: ModuleSpecifier,
  resolveFrom: ProjectPath,
|};

type DevDepRequests = {|
  devDeps: Map<string, string>,
  invalidDevDeps: Array<DevDepSpecifier>,
|};

export async function getDevDepRequests(api: RunAPI): Promise<DevDepRequests> {
  let previousDevDepRequests = new Map(
    await Promise.all(
      api
        .getSubRequests()
        .filter(req => req.type === 'dev_dep_request')
        .map(async req => [
          req.id,
          nullthrows(await api.getRequestResult<DevDepRequest>(req.id)),
        ]),
    ),
  );

  return {
    devDeps: new Map(
      [...previousDevDepRequests.entries()]
        .filter(([id]) => api.canSkipSubrequest(id))
        .map(([, req]) => [
          `${req.moduleSpecifier}:${fromProjectPathRelative(req.resolveFrom)}`,
          req.hash,
        ]),
    ),
    invalidDevDeps: await Promise.all(
      [...previousDevDepRequests.entries()]
        .filter(([id]) => !api.canSkipSubrequest(id))
        .flatMap(([, req]) => {
          return [
            {
              moduleSpecifier: req.moduleSpecifier,
              resolveFrom: req.resolveFrom,
            },
            ...(req.additionalInvalidations ?? []),
          ];
        }),
    ),
  };
}

// Tracks dev deps that have been invalidated during this build
// so we don't invalidate the require cache more than once.
const invalidatedDevDeps = createBuildCache();

export function invalidateDevDeps(
  invalidDevDeps: Array<DevDepSpecifier>,
  options: ParcelOptions,
  config: ParcelConfig,
) {
  for (let {moduleSpecifier, resolveFrom} of invalidDevDeps) {
    let key = `${moduleSpecifier}:${fromProjectPathRelative(resolveFrom)}`;
    if (!invalidatedDevDeps.has(key)) {
      config.invalidatePlugin(moduleSpecifier);
      options.packageManager.invalidate(
        moduleSpecifier,
        fromProjectPath(options.projectRoot, resolveFrom),
      );
      invalidatedDevDeps.set(key, true);
    }
  }
}

export async function runDevDepRequest(
  api: RunAPI,
  devDepRequest: DevDepRequest,
) {
  await api.runRequest<null, void>({
    id:
      'dev_dep_request:' +
      devDepRequest.moduleSpecifier +
      ':' +
      devDepRequest.hash,
    type: 'dev_dep_request',
    run: ({api}) => {
      for (let filePath of nullthrows(devDepRequest.invalidateOnFileChange)) {
        api.invalidateOnFileUpdate(filePath);
        api.invalidateOnFileDelete(filePath);
      }

      for (let invalidation of nullthrows(
        devDepRequest.invalidateOnFileCreate,
      )) {
        api.invalidateOnFileCreate(invalidation);
      }

      api.storeResult({
        moduleSpecifier: devDepRequest.moduleSpecifier,
        resolveFrom: devDepRequest.resolveFrom,
        hash: devDepRequest.hash,
        additionalInvalidations: devDepRequest.additionalInvalidations,
      });
    },
    input: null,
  });
}