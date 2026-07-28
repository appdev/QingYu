function requireEnv(name) {
  const value = process.env[name]?.trim();

  if (!value) {
    throw new Error(`${name} is required.`);
  }

  return value;
}

const repository = requireEnv("GITHUB_REPOSITORY");
const releaseTag = requireEnv("RELEASE_TAG");
const isPrerelease = releaseTag.replace(/^v/u, "").includes("-");
const releasePath = isPrerelease ? "download/preview" : "latest/download";

process.stdout.write(`https://github.com/${repository}/releases/${releasePath}/latest.json\n`);
