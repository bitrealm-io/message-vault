import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";
import { formatBuild } from "./src/lib/buildFormat";

/** git's trimmed answer, or null when git is missing, fails, or this is no checkout. */
function git(...args: string[]): string | null {
  try {
    return execFileSync("git", args, {
      cwd: import.meta.dirname,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  } catch {
    return null;
  }
}

/**
 * The part of the Build after the `+`. These are the rules of
 * `crates/libs/build-version`, which the vault server and the desktop app
 * follow: change the two together. `MESSAGE_VAULT_BUILD_METADATA` wins when it
 * is set, because the release Dockerfile has no `.git`; set and empty is a
 * release.
 */
function buildMetadata(): string {
  const fromEnv = process.env.MESSAGE_VAULT_BUILD_METADATA;
  if (fromEnv !== undefined) return fromEnv.trim();
  const commit = git("rev-parse", "--short=8", "HEAD");
  if (commit === null) return "unknown";
  // Tracked files only: a build leaves untracked output behind.
  if (git("status", "--porcelain", "--untracked-files=no")) return `${commit}.dirty`;
  const tagged = git("describe", "--exact-match", "--tags", "--match", "v*", "HEAD") !== null;
  return tagged ? "" : commit;
}

const productVersion: string = JSON.parse(
  readFileSync(new URL("./package.json", import.meta.url), "utf8"),
).version;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  define: {
    __APP_BUILD__: JSON.stringify(formatBuild(productVersion, buildMetadata())),
  },
  build: {
    // The entry chunk cannot get under Vite's 500 kB default: React Aria and
    // its supporting packages render the login and browse screens and alone
    // minify to about 512 kB (measured in #216). Screens and the advanced
    // search form are already lazy. The limit sits above the floor so the
    // build reports a size regression, not a condition nobody intends to fix.
    chunkSizeWarningLimit: 1000,
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      // Same-origin /v1 in browser → vault (blank server URL on login).
      "/v1": {
        target: "http://127.0.0.1:8080",
        changeOrigin: true,
      },
      // Login health light uses GET /health when the server URL field is blank.
      "/health": {
        target: "http://127.0.0.1:8080",
        changeOrigin: true,
      },
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    // Prefer // @vitest-environment jsdom in *.test.tsx when globs are unavailable.
    environmentMatchGlobs: [
      ["**/*.test.tsx", "jsdom"],
      ["**/*.spec.tsx", "jsdom"],
    ],
    setupFiles: ["src/test/setup.ts"],
  },
});
