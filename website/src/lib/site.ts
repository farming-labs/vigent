/** Facts the site shows in more than one place. */
export const VERSION = "0.9.7";
export const INSTALL = "npm i -g @vigent/cli";
/** The standalone binary, no Node: the installers are the repo-root files, served by the site (scripts/copy-installers.mjs). */
export const INSTALL_SH = "curl -fsSL https://vigent.farming-labs.dev/install.sh | sh";
export const INSTALL_PS1 = "irm https://vigent.farming-labs.dev/install.ps1 | iex";
export const REPO = "https://github.com/farming-labs/vigent";

export const LOGO = [
  "██╗   ██╗██╗ ██████╗ ███████╗███╗   ██╗████████╗",
  "██║   ██║██║██╔════╝ ██╔════╝████╗  ██║╚══██╔══╝",
  "██║   ██║██║██║  ███╗█████╗  ██╔██╗ ██║   ██║",
  "╚██╗ ██╔╝██║██║   ██║██╔══╝  ██║╚██╗██║   ██║",
  " ╚████╔╝ ██║╚██████╔╝███████╗██║ ╚████║   ██║",
  "  ╚═══╝  ╚═╝ ╚═════╝ ╚══════╝╚═╝  ╚═══╝   ╚═╝",
].join("\n");
