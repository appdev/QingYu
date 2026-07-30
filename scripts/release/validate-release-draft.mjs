import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

function requireValue(value, name) {
  const normalized = String(value ?? "").trim();
  if (!normalized) {
    throw new Error(`${name} is required.`);
  }
  return normalized;
}

function parseBoolean(value) {
  return String(value ?? "").trim().toLowerCase() === "true";
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

function requiredUnsignedAssets(version) {
  return [
    `QingYu_${version}_android_arm64_unsigned.apk`,
    `QingYu_${version}_ios_simulator_arm64_unsigned.app.zip`,
    `QingYu_${version}_linux_arm64.AppImage`,
    `QingYu_${version}_linux_arm64.deb`,
    `QingYu_${version}_linux_arm64.rpm`,
    `QingYu_${version}_linux_x64.AppImage`,
    `QingYu_${version}_linux_x64.deb`,
    `QingYu_${version}_linux_x64.rpm`,
    `QingYu_${version}_macos_arm64.dmg`,
    `QingYu_${version}_macos_x64.dmg`,
    `QingYu_${version}_windows_x64_portable.zip`,
    `QingYu_${version}_windows_x64_setup.exe`,
  ];
}

function requiredSignedAssets(version) {
  return [
    `QingYu_${version}_macos_arm64_updater.app.tar.gz`,
    `QingYu_${version}_macos_arm64_updater.app.tar.gz.sig`,
    `QingYu_${version}_macos_x64_updater.app.tar.gz`,
    `QingYu_${version}_macos_x64_updater.app.tar.gz.sig`,
    `QingYu_${version}_linux_arm64.AppImage.sig`,
    `QingYu_${version}_linux_x64.AppImage.sig`,
    `QingYu_${version}_windows_x64_setup.exe.sig`,
    "latest.json",
  ];
}

function validateBody(body, repository, tag) {
  const normalized = requireValue(body, "Release body");
  if (/\$\{[^}]+\}|\{\{[^}]+\}\}|<current[-_ ]?tag>/iu.test(normalized)) {
    throw new Error("Release body contains a generator placeholder.");
  }

  const repositoryPattern = escapeRegExp(repository);
  const tagPattern = escapeRegExp(tag);
  const comparePattern = new RegExp(
    `https://github\\.com/${repositoryPattern}/(?:compare/[^\\s)]+\\.\\.\\.${tagPattern}|commits/${tagPattern})(?:[\\s)]|$)`,
    "u",
  );
  if (!comparePattern.test(normalized)) {
    throw new Error("Release body does not contain the complete-change link for this tag.");
  }

  return normalized;
}

function validateAssets(assets, version) {
  if (!Array.isArray(assets)) {
    throw new Error("Release assets must be an array.");
  }

  const names = new Set();
  for (const asset of assets) {
    const name = requireValue(asset?.name, "Release asset name");
    if (names.has(name)) {
      throw new Error(`Duplicate release asset: ${name}.`);
    }
    if (!Number.isFinite(asset?.size) || asset.size <= 0) {
      throw new Error(`Release asset ${name} must have a non-zero size.`);
    }
    names.add(name);
  }

  for (const requiredName of requiredUnsignedAssets(version)) {
    if (!names.has(requiredName)) {
      throw new Error(`Missing required release asset: ${requiredName}.`);
    }
  }

  const signedNames = requiredSignedAssets(version);
  const hasSignedIndicator = [...names].some(
    (name) => name === "latest.json" || name.endsWith(".sig") || name.includes("_updater."),
  );
  if (hasSignedIndicator) {
    const missingSignedNames = signedNames.filter((name) => !names.has(name));
    if (missingSignedNames.length > 0) {
      throw new Error(`Incomplete signed updater assets: missing ${missingSignedNames.join(", ")}.`);
    }
  }

  return { names: [...names].sort(), signedRelease: hasSignedIndicator };
}

export function validateReleaseDraft({
  release,
  repository,
  version,
  tag,
  resolvedTargetSha,
  allowPublishedRetry = false,
}) {
  if (!release || typeof release !== "object" || Array.isArray(release)) {
    throw new Error("Release API response must be an object.");
  }

  const normalizedRepository = requireValue(repository, "Repository");
  const normalizedVersion = requireValue(version, "Version").replace(/^v/u, "");
  const normalizedTag = requireValue(tag, "Release tag");
  const normalizedTargetSha = requireValue(resolvedTargetSha, "Resolved target SHA").toLowerCase();
  const expectedTag = `v${normalizedVersion}`;

  if (normalizedTag !== expectedTag) {
    throw new Error(`Release tag ${normalizedTag} does not match package version ${normalizedVersion}.`);
  }
  if (release.tag_name !== normalizedTag) {
    throw new Error(`Release API tag ${release.tag_name || "<missing>"} does not match ${normalizedTag}.`);
  }
  if (String(release.target_commitish || "").toLowerCase() !== normalizedTargetSha) {
    throw new Error(
      `Release target commit ${release.target_commitish || "<missing>"} does not match resolved tag ${normalizedTargetSha}.`,
    );
  }

  const alreadyPublished = release.draft === false;
  if (alreadyPublished && !allowPublishedRetry) {
    throw new Error(`Release ${normalizedTag} is not a draft.`);
  }
  if (release.draft !== true && !alreadyPublished) {
    throw new Error(`Release ${normalizedTag} has an invalid draft state.`);
  }

  const expectedPrerelease = normalizedVersion.includes("-");
  if (release.prerelease !== expectedPrerelease) {
    throw new Error(
      `Release prerelease state ${String(release.prerelease)} does not match version ${normalizedVersion}.`,
    );
  }

  const body = validateBody(release.body, normalizedRepository, normalizedTag);
  const assets = validateAssets(release.assets, normalizedVersion);

  return {
    tag: normalizedTag,
    version: normalizedVersion,
    targetSha: normalizedTargetSha,
    prerelease: expectedPrerelease,
    signedRelease: assets.signedRelease,
    alreadyPublished,
    title: String(release.name ?? ""),
    body,
    assetNames: assets.names,
  };
}

export function validateDownloadedAssets({ root, assetNames, version, signedRelease }) {
  const resolvedRoot = path.resolve(requireValue(root, "Release assets root"));
  for (const assetName of assetNames) {
    if (path.basename(assetName) !== assetName) {
      throw new Error(`Release asset name is not a basename: ${assetName}.`);
    }
    const assetPath = path.join(resolvedRoot, assetName);
    let stat;
    try {
      stat = fs.statSync(assetPath);
    } catch {
      throw new Error(`Downloaded release asset is missing: ${assetName}.`);
    }
    if (!stat.isFile() || stat.size <= 0) {
      throw new Error(`Downloaded release asset must be a non-empty file: ${assetName}.`);
    }
  }

  if (signedRelease) {
    const manifestPath = path.join(resolvedRoot, "latest.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    if (manifest.version !== version) {
      throw new Error(`Updater manifest version ${manifest.version || "<missing>"} does not match ${version}.`);
    }
    const expectedPlatforms = [
      "darwin-aarch64",
      "darwin-x86_64",
      "linux-aarch64",
      "linux-x86_64",
      "windows-x86_64",
    ];
    for (const platform of expectedPlatforms) {
      const entry = manifest.platforms?.[platform];
      if (!entry || typeof entry.url !== "string" || typeof entry.signature !== "string" || !entry.signature.trim()) {
        throw new Error(`Updater manifest is incomplete for ${platform}.`);
      }
    }
  }
}

function writeOutput(name, value) {
  const outputPath = process.env.GITHUB_OUTPUT?.trim();
  if (outputPath) {
    fs.appendFileSync(outputPath, `${name}=${value}\n`, "utf8");
  }
}

function main(env = process.env) {
  const releasePath = path.resolve(requireValue(env.RELEASE_JSON_PATH, "RELEASE_JSON_PATH"));
  const packagePath = path.resolve(env.PACKAGE_JSON_PATH?.trim() || "package.json");
  const release = JSON.parse(fs.readFileSync(releasePath, "utf8"));
  const packageJson = JSON.parse(fs.readFileSync(packagePath, "utf8"));
  const result = validateReleaseDraft({
    release,
    repository: requireValue(env.GITHUB_REPOSITORY, "GITHUB_REPOSITORY"),
    version: requireValue(packageJson.version, "package.json version"),
    tag: requireValue(env.RELEASE_TAG, "RELEASE_TAG"),
    resolvedTargetSha: requireValue(env.RESOLVED_TARGET_SHA, "RESOLVED_TARGET_SHA"),
    allowPublishedRetry: parseBoolean(env.ALLOW_PUBLISHED_RETRY),
  });

  if (parseBoolean(env.REQUIRE_PUBLISHED) && !result.alreadyPublished) {
    throw new Error(`Release ${result.tag} is still a draft after publication.`);
  }
  if (env.RELEASE_ASSETS_ROOT?.trim()) {
    validateDownloadedAssets({
      root: env.RELEASE_ASSETS_ROOT,
      assetNames: result.assetNames,
      version: result.version,
      signedRelease: result.signedRelease,
    });
  }

  writeOutput("signed_release", result.signedRelease);
  writeOutput("already_published", result.alreadyPublished);
  writeOutput("prerelease", result.prerelease);
  writeOutput("release_version", result.version);
  console.log(
    `Validated ${result.tag} at ${result.targetSha}: ${result.assetNames.length} assets, signed=${result.signedRelease}, alreadyPublished=${result.alreadyPublished}.`,
  );
}

const isCli = process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;

if (isCli) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
