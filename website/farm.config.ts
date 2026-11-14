import { defineConfig } from "@farm.js/core";
import { devtools } from "@farm.js/devtools";

export default defineConfig({
  plugins: [devtools()],
  theme: {
    default: "dark",
  },
  // the installers (copied from the repository root into public/ by scripts/copy-installers.mjs)
  // as text, so `irm ... | iex` gets a string and a browser shows the script
  routeRules: {
    "/install.sh": { headers: { "content-type": "text/plain; charset=utf-8" } },
    "/install.ps1": { headers: { "content-type": "text/plain; charset=utf-8" } },
  },
  // `pnpm build` writes Vercel's prebuilt output (.vercel/output); `pnpm run deploy` uploads it
  deploy: {
    target: "vercel",
  },
});
