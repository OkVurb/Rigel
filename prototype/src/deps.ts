/**
 * Tools an agent reaches for but a fresh machine may not have. Commands are the
 * real installers (winget on Windows, brew elsewhere); every entry links to the
 * upstream docs rather than a mirror.
 */

export type DepCategory = "runtime" | "search" | "data" | "media" | "browser" | "network" | "terminal" | "build";

export interface Dep {
  id: string;
  name: string;
  category: DepCategory;
  why: string;
  provides: string[];
  commands: { label: string; command: string }[];
  docs: string;
  size: string;
  agentUse: string;
}

export const CATEGORY_LABEL: Record<DepCategory, string> = {
  runtime: "Runtimes",
  search: "Search & navigation",
  data: "Data & documents",
  media: "Media",
  browser: "Browser automation",
  network: "Network & crawling",
  terminal: "Terminals & shells",
  build: "Build tooling",
};

export const deps: Dep[] = [
  {
    id: "ripgrep",
    name: "ripgrep",
    category: "search",
    why: "Every serious agent greps a codebase before reading it. ripgrep is what Claude Code, Codex and Cursor already shell out to.",
    provides: ["rg"],
    commands: [
      { label: "Windows", command: "winget install --id BurntSushi.ripgrep.MSVC -e" },
      { label: "macOS", command: "brew install ripgrep" },
      { label: "Debian", command: "sudo apt install ripgrep" },
    ],
    docs: "https://github.com/BurntSushi/ripgrep",
    size: "~5 MB",
    agentUse: "rg -n --hidden -g '!node_modules' 'pattern' .",
  },
  {
    id: "fd",
    name: "fd",
    category: "search",
    why: "Faster, sane-defaults replacement for find. Agents use it to enumerate files without tripping over .gitignore.",
    provides: ["fd", "fdfind"],
    commands: [
      { label: "Windows", command: "winget install --id sharkdp.fd -e" },
      { label: "macOS", command: "brew install fd" },
    ],
    docs: "https://github.com/sharkdp/fd",
    size: "~3 MB",
    agentUse: "fd -e ts -x wc -l",
  },
  {
    id: "jq",
    name: "jq",
    category: "data",
    why: "Structured data without writing a script. Most agents fall back to Python for JSON mangling when jq is missing.",
    provides: ["jq"],
    commands: [
      { label: "Windows", command: "winget install --id jqlang.jq -e" },
      { label: "macOS", command: "brew install jq" },
    ],
    docs: "https://jqlang.github.io/jq/",
    size: "~2 MB",
    agentUse: "cat package.json | jq '.scripts'",
  },
  {
    id: "uv",
    name: "uv",
    category: "runtime",
    why: "Installs and runs Python tools in seconds without polluting the system interpreter. This is how crawl4ai should be installed.",
    provides: ["uv", "uvx", "python"],
    commands: [
      { label: "Windows", command: "winget install --id astral-sh.uv -e" },
      { label: "macOS", command: "brew install uv" },
    ],
    docs: "https://docs.astral.sh/uv/",
    size: "~30 MB",
    agentUse: "uvx --from crawl4ai crawl4ai-doctor",
  },
  {
    id: "crawl4ai",
    name: "crawl4ai",
    category: "network",
    why: "Open-source crawler that turns pages into clean Markdown with a local browser. Replaces paid scraping APIs for research runs like the competitor teardown.",
    provides: ["crawl4ai-setup", "crwl"],
    commands: [
      { label: "Install", command: "pip install -U crawl4ai && crawl4ai-setup" },
      { label: "Verify", command: "crawl4ai-doctor" },
      { label: "One page", command: "crwl https://docs.crawl4ai.com -o markdown" },
    ],
    docs: "https://docs.crawl4ai.com",
    size: "~400 MB with browser",
    agentUse: "crwl <url> -o markdown --bypass-cache",
  },
  {
    id: "groq",
    name: "groq",
    category: "network",
    why: "Very fast inference for cheap classification passes — dedupe candidates, tag skills, summarise pages before the expensive model sees them.",
    provides: ["groq-sdk", "GROQ_API_KEY"],
    commands: [
      { label: "Python", command: "pip install groq" },
      { label: "Node", command: "npm i groq-sdk" },
      { label: "Key", command: "export GROQ_API_KEY=...   # console.groq.com/keys" },
    ],
    docs: "https://groq.com",
    size: "SDK only",
    agentUse: "Classify 500 candidate skills into categories before ranking them.",
  },
  {
    id: "playwright",
    name: "playwright",
    category: "browser",
    why: "Headless browser an agent can drive directly, including screenshots and network inspection. The fallback when a site blocks plain HTTP.",
    provides: ["playwright", "npx playwright"],
    commands: [
      { label: "Project", command: "npm i -D playwright && npx playwright install" },
      { label: "Python", command: "pip install playwright && playwright install chromium" },
    ],
    docs: "https://playwright.dev",
    size: "~150 MB per browser",
    agentUse: "npx playwright screenshot --full-page <url> out.png",
  },
  {
    id: "ffmpeg",
    name: "ffmpeg",
    category: "media",
    why: "Any audio or video task eventually needs it: extract frames, trim, transcode, mux a voiceover onto a screen recording.",
    provides: ["ffmpeg", "ffprobe"],
    commands: [
      { label: "Windows", command: "winget install --id Gyan.FFmpeg -e" },
      { label: "macOS", command: "brew install ffmpeg" },
    ],
    docs: "https://ffmpeg.org/documentation.html",
    size: "~120 MB",
    agentUse: "ffprobe -v error -show_entries format=duration -of csv=p=0 file.mp4",
  },
  {
    id: "imagemagick",
    name: "ImageMagick",
    category: "media",
    why: "Batch resize, crop, convert and composite without opening an editor. Agents use it to normalise screenshots before vision models read them.",
    provides: ["magick", "convert"],
    commands: [
      { label: "Windows", command: "winget install --id ImageMagick.ImageMagick -e" },
      { label: "macOS", command: "brew install imagemagick" },
    ],
    docs: "https://imagemagick.org",
    size: "~60 MB",
    agentUse: "magick input.png -resize 1200x -quality 85 output.jpg",
  },
  {
    id: "tesseract",
    name: "tesseract",
    category: "media",
    why: "OCR for screenshots, PDFs and scanned documents when a vision model is overkill or the text needs to be searchable.",
    provides: ["tesseract"],
    commands: [
      { label: "Windows", command: "winget install --id UB-Mannheim.TesseractOCR -e" },
      { label: "macOS", command: "brew install tesseract" },
    ],
    docs: "https://tesseract-ocr.github.io",
    size: "~40 MB + language data",
    agentUse: "tesseract shot.png stdout -l eng",
  },
  {
    id: "pandoc",
    name: "pandoc",
    category: "data",
    why: "Converts between docx, pdf, html, epub and Markdown. The shortest path from a user's document to prompt-ready text.",
    provides: ["pandoc"],
    commands: [
      { label: "Windows", command: "winget install --id JohnMacFarlane.Pandoc -e" },
      { label: "macOS", command: "brew install pandoc" },
    ],
    docs: "https://pandoc.org",
    size: "~140 MB",
    agentUse: "pandoc brief.docx -t markdown -o brief.md",
  },
  {
    id: "sqlite",
    name: "SQLite",
    category: "data",
    why: "The index this app is prototyping becomes a single SQLite file. A CLI makes it inspectable without writing a query layer.",
    provides: ["sqlite3"],
    commands: [
      { label: "Windows", command: "winget install --id SQLite.SQLite -e" },
      { label: "macOS", command: "brew install sqlite" },
    ],
    docs: "https://sqlite.org/cli.html",
    size: "~4 MB",
    agentUse: "sqlite3 index.db 'select name, grade from skills limit 20;'",
  },
  {
    id: "gh",
    name: "GitHub CLI",
    category: "network",
    why: "Authenticated GitHub calls from the shell — 5,000 requests/hour instead of 60, plus issues, PRs and releases.",
    provides: ["gh"],
    commands: [
      { label: "Windows", command: "winget install --id GitHub.cli -e" },
      { label: "macOS", command: "brew install gh" },
      { label: "Auth", command: "gh auth login" },
    ],
    docs: "https://cli.github.com",
    size: "~40 MB",
    agentUse: "gh api repos/owner/repo/git/trees/main?recursive=1",
  },
  {
    id: "git",
    name: "Git",
    category: "build",
    why: "Source control, but also the cheapest way to read history: who changed a line, when, and why.",
    provides: ["git", "bash"],
    commands: [
      { label: "Windows", command: "winget install --id Git.Git -e" },
      { label: "macOS", command: "brew install git" },
    ],
    docs: "https://git-scm.com/doc",
    size: "~250 MB",
    agentUse: "git log -L 10,20:src/app.ts",
  },
  {
    id: "node",
    name: "Node.js",
    category: "runtime",
    why: "Runs MCP servers, the Vite dev server and most JavaScript skills. LTS is the safe default.",
    provides: ["node", "npm", "npx"],
    commands: [
      { label: "Windows", command: "winget install --id OpenJS.NodeJS.LTS -e" },
      { label: "macOS", command: "brew install node@22" },
    ],
    docs: "https://nodejs.org/en/docs",
    size: "~80 MB",
    agentUse: "npx -y some-mcp-server",
  },
  {
    id: "pwsh",
    name: "PowerShell 7",
    category: "terminal",
    why: "Cross-platform shell with structured output. Agents that emit bash syntax break on Windows without a predictable shell.",
    provides: ["pwsh"],
    commands: [
      { label: "Windows", command: "winget install --id Microsoft.PowerShell -e" },
      { label: "macOS", command: "brew install powershell" },
    ],
    docs: "https://learn.microsoft.com/powershell/",
    size: "~200 MB",
    agentUse: "pwsh -NoProfile -Command 'Get-ChildItem | ConvertTo-Json'",
  },
  {
    id: "wt",
    name: "Windows Terminal",
    category: "terminal",
    why: "One window for PowerShell, Git Bash, WSL and SSH profiles. Separate terminal profiles are how an agent gets a real shell per task.",
    provides: ["wt"],
    commands: [
      { label: "Windows", command: "winget install --id Microsoft.WindowsTerminal -e" },
      { label: "Open profile", command: "wt -p \"Git Bash\" -d ." },
    ],
    docs: "https://learn.microsoft.com/windows/terminal/",
    size: "~30 MB",
    agentUse: "wt -p \"Ubuntu\" --title build -d .",
  },
  {
    id: "wsl",
    name: "WSL 2",
    category: "terminal",
    why: "A real Linux userland on Windows. Half the tooling in this list assumes POSIX, and WSL is cheaper than porting it.",
    provides: ["wsl", "bash", "apt"],
    commands: [
      { label: "Install", command: "wsl --install -d Ubuntu" },
      { label: "List", command: "wsl -l -v" },
      { label: "Run", command: "wsl -d Ubuntu -- bash -lc 'uname -a'" },
    ],
    docs: "https://learn.microsoft.com/windows/wsl/",
    size: "~1 GB",
    agentUse: "wsl -d Ubuntu -- bash -lc 'rg --version'",
  },
  {
    id: "docker",
    name: "Docker",
    category: "build",
    why: "Reproducible environments for skills that need services — a database, a crawler, a model server — without installing them globally.",
    provides: ["docker", "docker compose"],
    commands: [
      { label: "Windows", command: "winget install --id Docker.DockerDesktop -e" },
      { label: "macOS", command: "brew install --cask docker" },
    ],
    docs: "https://docs.docker.com",
    size: "~1.5 GB",
    agentUse: "docker run --rm -v ${PWD}:/w -w /w node:22 npm test",
  },
  {
    id: "rustup",
    name: "Rust",
    category: "build",
    why: "Phase 1 of this app is a Tauri shell. That needs rustup plus the MSVC C++ build tools; neither is installed on this machine yet.",
    provides: ["rustup", "cargo", "rustc"],
    commands: [
      { label: "Windows", command: "winget install --id Rustlang.Rustup -e" },
      { label: "Build tools", command: "winget install --id Microsoft.VisualStudio.2022.BuildTools -e" },
      { label: "Verify", command: "cargo --version" },
    ],
    docs: "https://rustup.rs",
    size: "~1.2 GB",
    agentUse: "cargo tauri dev",
  },
];

export const depsByCategory = () => {
  const map = new Map<DepCategory, Dep[]>();
  for (const d of deps) {
    const list = map.get(d.category) ?? [];
    list.push(d);
    map.set(d.category, list);
  }
  return [...map.entries()];
};
