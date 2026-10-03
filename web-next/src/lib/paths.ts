import fs from "fs";
import path from "path";
import { parse } from "smol-toml";

import { currentAccountId } from "./accountScope";

const DEFAULT_DB = "data/vault.db";
const DEFAULT_DATA_DIR = "data";
const DEFAULT_ASSETS_DIR = "assets";
const DEFAULT_ASSETS_CONVERTED_DIR = "assets_converted";

/** Repo root (parent of web-next/), detected via config/config.toml. */
export function repoRoot(): string {
  const cwd = process.cwd();
  if (fs.existsSync(path.join(cwd, "config", "config.toml"))) {
    return cwd;
  }
  const parent = path.resolve(cwd, "..");
  if (fs.existsSync(path.join(parent, "config", "config.toml"))) {
    return parent;
  }
  return parent;
}

export function configTomlPath(): string {
  return path.join(repoRoot(), "config", "config.toml");
}

function resolveConfiguredPath(
  configured: string | undefined,
  fallback: string,
): string {
  const rel = configured?.trim() || fallback;
  if (path.isAbsolute(rel)) return rel;
  return path.join(repoRoot(), rel);
}

export type AccountAssetDirs = {
  assetsDir: string;
  assetsConvertedDir: string;
};

type RawConfig = {
  paths?: {
    db?: string;
    data_dir?: string;
    assets_dir?: string;
    assets_converted_dir?: string;
  };
};

function loadRawConfig(): RawConfig {
  const configPath = configTomlPath();
  if (!fs.existsSync(configPath)) {
    return {};
  }
  try {
    const text = fs.readFileSync(configPath, "utf8");
    return parse(text) as RawConfig;
  } catch {
    return {};
  }
}

export function dbPath(): string {
  const fromEnv = process.env.VAULT_DB?.trim();
  if (fromEnv) {
    return path.isAbsolute(fromEnv) ? fromEnv : path.join(repoRoot(), fromEnv);
  }
  const cfg = loadRawConfig();
  return resolveConfiguredPath(cfg.paths?.db, DEFAULT_DB);
}

/** Parent of vault.db — better-sqlite3 fails if this directory is missing. */
export function ensureDbParentDir(): string {
  const file = dbPath();
  const dir = path.dirname(file);
  fs.mkdirSync(dir, { recursive: true });
  return file;
}

export function dataDir(): string {
  const fromEnv = process.env.VAULT_DATA_DIR?.trim();
  if (fromEnv) {
    return path.isAbsolute(fromEnv) ? fromEnv : path.join(repoRoot(), fromEnv);
  }
  const cfg = loadRawConfig();
  return resolveConfiguredPath(cfg.paths?.data_dir, DEFAULT_DATA_DIR);
}

export function accountDataDir(accountId: string): string {
  return path.join(dataDir(), accountId);
}

export function assetsDirName(): string {
  return loadRawConfig().paths?.assets_dir?.trim() || DEFAULT_ASSETS_DIR;
}

export function assetsConvertedDirName(): string {
  return (
    loadRawConfig().paths?.assets_converted_dir?.trim() ||
    DEFAULT_ASSETS_CONVERTED_DIR
  );
}

/**
 * One account's attachment folders. The server keeps one folder of originals
 * and one of previews per account, shared by every import source:
 *   data/<account_id>/<assets_dir>
 *   data/<account_id>/<assets_converted_dir>
 */
export function accountAssetDirs(accountId = currentAccountId()): AccountAssetDirs {
  const account = accountDataDir(accountId);
  return {
    assetsDir: path.join(account, assetsDirName()),
    assetsConvertedDir: path.join(account, assetsConvertedDirName()),
  };
}
