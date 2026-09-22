/**
 * Where the Apple Messages reader's source and license live for the Build a
 * person is running. The desktop installer ships the reader, a separate
 * program under the GNU GPL v3, and the GPL asks that whoever receives it can
 * find the license and the source that matches their copy. The links point at
 * the release tag for this Build's Product Version, so an old install keeps
 * pointing at its own source after `main` moves on.
 */

import { productVersionOf } from "./buildFormat";

const REPO = "https://github.com/bitrealm-io/message-vault";
const READER_PATH = "crates/helpers/imessage-reader";

/** The reader's folder at the release tag for this Build. */
export function readerSourceUrl(build: string): string {
  return `${REPO}/tree/v${productVersionOf(build)}/${READER_PATH}`;
}

/** The GPL text in the reader's folder at the release tag for this Build. */
export function readerLicenseUrl(build: string): string {
  return `${REPO}/blob/v${productVersionOf(build)}/${READER_PATH}/LICENSE`;
}
