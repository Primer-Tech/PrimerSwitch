import { createHash } from 'node:crypto';
import {
  readFile,
  writeFile,
  mkdir,
  readdir,
  stat,
  lstat,
  rm,
} from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Observe the real production build: its normal config, plugins and defaults are retained.
const desktop = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const repository = path.resolve(desktop, '../..');
const inventoryFile = path.join(
  repository,
  '.artifacts/frontend-bundle-inventory.json',
);
// Remove old evidence before importing build tooling or validating any inputs.
await rm(inventoryFile, { force: true });
const { build } = await import('vite');
const lock = JSON.parse(
  await readFile(path.join(desktop, 'package-lock.json'), 'utf8'),
);
const slash = (value) => value.replaceAll('\\', '/');
const sha256 = (value) => createHash('sha256').update(value).digest('hex');
const relative = (value) => {
  const result = slash(path.relative(repository, value));
  if (result.startsWith('../') || path.isAbsolute(result))
    throw new Error('Inventory input outside repository');
  return result;
};
const cleanId = (id) => id.split('?')[0];
const safeId = (id) => {
  if (id.startsWith('\0')) {
    if (id === '\0vite/modulepreload-polyfill.js')
      return 'virtual:vite/modulepreload-polyfill.js';
    throw new Error(`Unexpected virtual module: ${id.replaceAll('\0', '')}`);
  }
  if (!path.isAbsolute(cleanId(id)))
    throw new Error('Unexpected non-file module');
  return relative(cleanId(id)) + id.slice(cleanId(id).length);
};
const inputs = new Set([
  path.join(desktop, 'package-lock.json'),
  path.join(desktop, 'package.json'),
  fileURLToPath(import.meta.url),
]);
const brandManifestPath = path.join(
  repository,
  'docs/legal/brand/manifest.json',
);
const brandManifest = JSON.parse(await readFile(brandManifestPath, 'utf8'));
if (
  brandManifest.formatVersion !== 1 ||
  brandManifest.license.spdx !== 'OFL-1.1' ||
  brandManifest.logo.attributionKind !== 'project-owner-branding'
)
  throw new Error('Unreviewed brand asset manifest');
inputs.add(brandManifestPath);
const brandAssets = [];
for (const [identity, item, expectedPath] of [
  [
    'Primer official logo',
    brandManifest.logo,
    'apps/desktop/src/assets/primer-logo.png',
  ],
  [
    'Comfortaa SemiBold',
    brandManifest.font,
    'apps/desktop/src/assets/fonts/comfortaa-600.subset.woff2',
  ],
]) {
  if (item.file !== expectedPath || !/^[a-f0-9]{64}$/.test(item.sha256))
    throw new Error('Unexpected brand asset identity');
  const source = path.join(repository, item.file);
  if (
    !(await lstat(source)).isFile() ||
    sha256(await readFile(source)) !== item.sha256
  )
    throw new Error(`Brand source differs from pinned manifest: ${identity}`);
  inputs.add(source);
  brandAssets.push({ identity, ...item });
}
const fontLicensePath = path.join(repository, brandManifest.license.file);
relative(fontLicensePath);
if (
  !(await lstat(fontLicensePath)).isFile() ||
  sha256(await readFile(fontLicensePath)) !== brandManifest.license.sha256
)
  throw new Error('Brand font license differs from pinned manifest');
inputs.add(fontLicensePath);
const packages = new Map();
let resolved;
let moduleIds = [];
let outputs;
async function packageFor(id) {
  if (id.startsWith('\0')) {
    if (id !== '\0vite/modulepreload-polyfill.js')
      throw new Error('Unattributed virtual module');
    inputs.add(
      path.join(desktop, 'node_modules/vite/dist/node/chunks/config.js'),
    );
    inputs.add(path.join(desktop, 'node_modules/vite/LICENSE.md'));
    return packageFor(path.join(desktop, 'node_modules/vite/package.json'));
  }
  const normalized = slash(path.relative(desktop, cleanId(id)));
  const parts = normalized.split('/');
  const offset = parts.lastIndexOf('node_modules');
  if (offset < 0) return null;
  const count = parts[offset + 1]?.startsWith('@') ? 3 : 2;
  const location = parts.slice(0, offset + count).join('/');
  if (location.startsWith('../'))
    throw new Error('Dependency outside desktop package lock');
  const entry = lock.packages[location];
  if (!entry || entry.link || !entry.version || !entry.integrity)
    throw new Error(`Unpinned dependency: ${location}`);
  const manifestPath = path.join(desktop, location, 'package.json');
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  const name = parts.slice(offset + 1, offset + count).join('/');
  if (manifest.name !== name || manifest.version !== entry.version)
    throw new Error(`Installed dependency differs from lock: ${location}`);
  inputs.add(manifestPath);
  const key = location;
  if (!packages.has(key))
    packages.set(key, {
      name,
      version: entry.version,
      location,
      lockPath: location,
      lockFile: 'apps/desktop/package-lock.json',
      integrity: entry.integrity,
      declaredLicense: manifest.license,
      modules: [],
    });
  return packages.get(key);
}
const observer = {
  name: 'primerswitch-production-inventory',
  enforce: 'post',
  configResolved(config) {
    resolved = config;
    if (
      config.command !== 'build' ||
      !config.isProduction ||
      config.mode !== 'production' ||
      config.build.watch
    )
      throw new Error('Inventory requires a production build');
    if (config.build.sourcemap || config.build.lib || config.build.ssr)
      throw new Error('Unsupported production output mode');
    for (const file of config.configFileDependencies) inputs.add(file);
    if (config.configFile) inputs.add(config.configFile);
  },
  generateBundle: {
    order: 'post',
    handler(_options, bundle) {
      moduleIds = [...this.getModuleIds()];
      for (const id of moduleIds) {
        if (this.getModuleInfo(id)?.isExternal)
          throw new Error(`Unexpected external dependency: ${safeId(id)}`);
        if (!id.startsWith('\0')) inputs.add(cleanId(id));
      }
      outputs = Object.values(bundle);
    },
  },
};
process.chdir(desktop);
await build({ plugins: [observer] });
if (!outputs || !resolved)
  throw new Error('Production build did not emit an inventory');
const outDir = path.resolve(resolved.root, resolved.build.outDir);
relative(outDir);
const outputNames = new Set(outputs.map((output) => output.fileName));
const chunks = [];
const assets = [];
const brandOutputs = new Map();
for (const output of outputs.filter((item) => item.type === 'asset')) {
  const item = brandAssets.find(
    (asset) => sha256(output.source) === asset.sha256,
  );
  if (!item) continue;
  const extension = path.extname(item.file);
  const basename = path.basename(item.file, extension);
  if (
    !output.fileName.startsWith(`assets/${basename}-`) ||
    !output.fileName.endsWith(extension) ||
    [...brandOutputs.values()].some((asset) => asset.file === item.file)
  )
    throw new Error('Unexpected or duplicate brand asset output');
  brandOutputs.set(output.fileName, item);
}
if (brandOutputs.size !== brandAssets.length)
  throw new Error(
    'Brand assets must be emitted unchanged; inlined or missing assets are not accepted',
  );
const cssInputs = moduleIds
  .filter((id) => /\.css(?:\?|$)|[?&]type=style(?:&|$)/.test(id))
  .map(safeId)
  .sort();
for (const output of outputs) {
  if (output.fileName.includes('..') || path.isAbsolute(output.fileName))
    throw new Error('Unsafe emitted path');
  const bytes = await readFile(path.join(outDir, output.fileName));
  const expected = output.type === 'chunk' ? output.code : output.source;
  if (sha256(bytes) !== sha256(expected))
    throw new Error(`Emitted output changed: ${output.fileName}`);
  const common = {
    file: output.fileName,
    sha256: sha256(bytes),
    bytes: bytes.length,
  };
  if (output.type === 'chunk') {
    for (const imported of [
      ...output.imports,
      ...output.dynamicImports,
      ...output.referencedFiles,
    ]) {
      if (!outputNames.has(imported))
        throw new Error(`Unexpected external output reference: ${imported}`);
    }
    const modules = [];
    for (const [id, module] of Object.entries(output.modules)) {
      // Rollup includes tree-shaken modules in moduleIds; only positive rendered code belongs here.
      if (module.renderedLength <= 0) continue;
      const normalized = safeId(id);
      const pkg = await packageFor(id);
      if (pkg && !pkg.modules.includes(normalized))
        pkg.modules.push(normalized);
      modules.push({
        id: normalized,
        renderedLength: module.renderedLength,
        ...(id.startsWith('\0')
          ? {
              provenance:
                'generated runtime helper from the pinned Vite module-preload plugin',
              sourceInput:
                'apps/desktop/node_modules/vite/dist/node/chunks/config.js',
            }
          : {}),
        ...(pkg
          ? {
              packageName: pkg.name,
              packageVersion: pkg.version,
              lockPath: pkg.lockPath,
            }
          : {}),
      });
    }
    chunks.push({
      ...common,
      modules: modules.sort((a, b) => a.id.localeCompare(b.id)),
      imports: output.imports,
      dynamicImports: output.dynamicImports,
    });
  } else if (output.fileName === 'index.html') {
    const htmlInput = path.join(desktop, 'index.html');
    inputs.add(htmlInput);
    if (
      /\b(?:src|href)\s*=\s*["'](?:https?:|\/\/|data:)/i.test(
        bytes.toString('utf8'),
      )
    )
      throw new Error('Unexpected external HTML asset');
    assets.push({
      ...common,
      kind: 'html',
      sourceInputs: [relative(htmlInput)],
    });
  } else if (output.fileName.endsWith('.css')) {
    if (!cssInputs.length)
      throw new Error('CSS emitted without identifiable source inputs');
    const css = bytes.toString('utf8');
    if (/@import\b/i.test(css)) throw new Error('Unexpected CSS import');
    const cssReferences = [];
    const remainder = css.replace(
      /url\s*\(\s*(?:"([^"]*)"|'([^']*)'|([^)]*))\s*\)/gi,
      (_match, quoted, single, bare) => {
        const value = (quoted ?? single ?? bare).trim();
        if (/^(?:[a-z]+:|\/\/)/i.test(value) || /[?#\\]/.test(value))
          throw new Error('Unexpected CSS asset reference');
        const reference = value.startsWith('/')
          ? value.slice(1)
          : path.posix.join(path.posix.dirname(output.fileName), value);
        const asset = brandOutputs.get(reference);
        if (!asset || asset.file !== brandManifest.font.file)
          throw new Error(
            'CSS reference requires exact known font attribution',
          );
        cssReferences.push(reference);
        return '';
      },
    );
    if (/url\s*\(/i.test(remainder))
      throw new Error('Unparsed CSS asset reference');
    // Vite extracts CSS separately: its contributing inputs are not rendered JS modules.
    for (const id of moduleIds.filter((id) =>
      /\.css(?:\?|$)|[?&]type=style(?:&|$)/.test(id),
    )) {
      if (await packageFor(id))
        throw new Error(
          'Third-party CSS requires explicit per-asset package attribution',
        );
    }
    assets.push({
      ...common,
      kind: 'css',
      referencedAssets: [...new Set(cssReferences)].sort(),
      sourceInputs: cssInputs,
      provenance:
        'aggregate CSS inputs from this build; not rendered JavaScript modules',
    });
  } else if (brandOutputs.has(output.fileName)) {
    const asset = brandOutputs.get(output.fileName);
    if (sha256(bytes) !== asset.sha256)
      throw new Error('Brand emitted bytes differ from pinned source');
    assets.push({
      ...common,
      kind:
        asset.file === brandManifest.font.file ? 'font' : 'project-branding',
      identity: asset.identity,
      sourceInputs: [asset.file],
      sourceSha256: asset.sha256,
      provenance: asset.sourceUrl,
      attributionManifest: relative(brandManifestPath),
      ...(asset.file === brandManifest.font.file
        ? { license: 'OFL-1.1', licenseFile: brandManifest.license.file }
        : { rightsStatement: asset.rightsStatement }),
    });
  } else
    throw new Error(
      `Unexpected emitted asset requires explicit attribution: ${output.fileName}`,
    );
}
async function listFiles(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isSymbolicLink()) throw new Error('Symlink in emitted output');
    if (entry.isDirectory()) files.push(...(await listFiles(full)));
    else files.push(slash(path.relative(outDir, full)));
  }
  return files;
}
for (const file of await listFiles(outDir)) {
  if (!outputNames.has(file))
    throw new Error(`Unexpected copied/public output asset: ${file}`);
}
const inputHashes = [];
for (const file of [...inputs].sort()) {
  if (!(await stat(file)).isFile())
    throw new Error('Non-file production input');
  inputHashes.push({
    path: relative(file),
    sha256: sha256(await readFile(file)),
  });
}
for (const asset of brandAssets) {
  if (
    sha256(await readFile(path.join(repository, asset.file))) !== asset.sha256
  )
    throw new Error('Brand source changed during production build');
}
const npmPackages = [...packages.values()].sort((a, b) =>
  a.lockPath.localeCompare(b.lockPath),
);
for (const pkg of npmPackages) pkg.modules.sort();
const inventory = {
  formatVersion: 1,
  packageLockSha256: sha256(
    await readFile(path.join(desktop, 'package-lock.json')),
  ),
  packageManifestSha256: sha256(
    await readFile(path.join(desktop, 'package.json')),
  ),
  build: {
    mode: resolved.mode,
    config: relative(resolved.configFile),
    target: resolved.build.target,
    outDir: relative(outDir),
    viteVersion: JSON.parse(
      await readFile(
        path.join(desktop, 'node_modules/vite/package.json'),
        'utf8',
      ),
    ).version,
    rollupVersion: JSON.parse(
      await readFile(
        path.join(desktop, 'node_modules/rollup/package.json'),
        'utf8',
      ),
    ).version,
  },
  inputs: inputHashes,
  chunks: chunks.sort((a, b) => a.file.localeCompare(b.file)),
  assets: assets.sort((a, b) => a.file.localeCompare(b.file)),
  packages: npmPackages,
};
await mkdir(path.dirname(inventoryFile), { recursive: true });
await writeFile(inventoryFile, JSON.stringify(inventory, null, 2) + '\n');
console.log(`Production bundle inventory: ${relative(inventoryFile)}`);
console.log(
  `Bundled npm packages: ${npmPackages.map((pkg) => `${pkg.name}@${pkg.version}`).join(', ')}`,
);
