// Copies vigent's installers from the repository root into public/, so the site
// serves them at https://vigent.farming-labs.dev/install.sh and /install.ps1.
// The root files are the only copies: the ones in public/ are ignored by git and
// written again by every dev, build and preview run (see package.json).
//   curl -fsSL https://vigent.farming-labs.dev/install.sh | sh
//   irm https://vigent.farming-labs.dev/install.ps1 | iex
// Each downloads the release archives from GitHub releases, not from the site.
import * as fs from "node:fs";

const FILES = ["install.sh", "install.ps1"];
const root = new URL("../../", import.meta.url);
const pub = new URL("../public/", import.meta.url);

fs.mkdirSync(pub, { recursive: true });
for (const f of FILES) {
  const from = new URL(f, root);
  if (!fs.existsSync(from)) {
    console.error(`copy-installers: ${f} isn't at the repository root; the site's /${f} would be missing`);
    process.exit(1);
  }
  fs.copyFileSync(from, new URL(f, pub));
}
console.log(`copied ${FILES.join(", ")} into public/`);
