/**
 * How a Build is written and read. A Build is the Product Version plus the
 * commit it was built from, `0.9.0+343fe0d8`; `.dirty` follows the commit when
 * the source held uncommitted changes, and a release is the Product Version
 * alone.
 *
 * `vite.config.ts` imports this to embed the SPA's Build, so nothing here may
 * touch the DOM or the embedded value. The vault server and the desktop app
 * format theirs in `crates/libs/build-version`, under the same rules: change
 * the two together.
 */

/** Join a Product Version and build metadata. Empty metadata is a release. */
export function formatBuild(productVersion: string, metadata: string): string {
  return metadata ? `${productVersion}+${metadata}` : productVersion;
}

/** The Product Version a Build carries: everything before the `+`. */
export function productVersionOf(build: string): string {
  const plus = build.indexOf("+");
  return plus === -1 ? build : build.slice(0, plus);
}

/**
 * Whether two Builds come from different releases. Only the Product Version is
 * compared, so two dev builds from different commits do not differ.
 */
export function productVersionsDiffer(a: string, b: string): boolean {
  return productVersionOf(a) !== productVersionOf(b);
}
