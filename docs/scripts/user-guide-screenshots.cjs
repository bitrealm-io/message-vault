// Captures the User Guide's screenshots into docs/src/assets/user-guide/.
//
// It needs two servers already running from the release image:
//   DEMO_URL  - a new Message Crate, which starts with the Demo Account (steps 2 and 3)
//   EMPTY_URL - a second new Message Crate on its own volume (step 4)
// The empty server must be unclaimed, so its volume is deleted before each run.
//
//   npm install --no-save playwright && npx playwright install chromium
//   DEMO_URL=http://localhost:18080 EMPTY_URL=http://localhost:18081 \
//     node scripts/user-guide-screenshots.cjs
//
// PLAYWRIGHT names the library's folder when it is installed somewhere else.
// --verbose prints the text of each captured screen, for checking labels.
//
// The import run screens are not captured here, because a browser cannot
// start an import. Those are taken by hand: see issue #944.
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT || 'playwright');

const DEMO = process.env.DEMO_URL || 'http://localhost:8080';
const EMPTY = process.env.EMPTY_URL || 'http://localhost:8081';
const OUT = path.join(__dirname, '..', 'src', 'assets', 'user-guide');

async function capture(browser) {
  const seen = {};
  const open = async (base) => {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 });
    const p = await context.newPage();
    await p.goto(`${base}/`);
    await p.waitForTimeout(1200);
    return p;
  };
  const shot = async (p, name) => {
    await p.waitForTimeout(600);
    await p.screenshot({ path: `${OUT}/${name}.png` });
    seen[name] = (await p.locator('body').innerText()).slice(0, 700);
  };
  const logOut = async (p) => {
    await p.getByRole('button', { name: 'Account menu' }).click();
    await p.waitForTimeout(300);
    seen.accountMenu = await p.locator('[role=menu]').innerText();
    await p.getByRole('menuitem', { name: /log ?out/i }).click();
    await p.waitForTimeout(1000);
  };

  // Steps 2 and 3: the demo Message Crate.
  const demo = await open(DEMO);
  await shot(demo, 'login');
  await demo.getByRole('button', { name: 'Explore Demo Account' }).click();
  await demo.waitForTimeout(2500);
  await shot(demo, 'demo-messages');
  await demo.getByRole('button', { name: /^Carolyn Jones/ }).first().click();
  await demo.waitForTimeout(2000);
  await demo.getByPlaceholder('Search messages').fill('attachment:any');
  await demo.keyboard.press('Enter');
  await demo.waitForTimeout(2500);
  await shot(demo, 'demo-search');
  await demo.context().close();

  // Step 4: an empty Message Crate.
  const own = await open(EMPTY);
  await shot(own, 'create-owner');
  await own.getByLabel('Username').fill('owner');
  await own.getByLabel('Password', { exact: true }).fill('a long owner password');
  await own.getByLabel('Confirm Password').fill('a long owner password');
  await own.getByRole('button', { name: 'Create Owner' }).click();
  await own.waitForTimeout(2000);
  await own.getByRole('button', { name: 'Add account' }).click();
  await own.waitForTimeout(600);
  await own.getByLabel('Username').fill('alex');
  await own.getByLabel('Password', { exact: true }).fill('a long account password');
  await own.getByLabel('Confirm password').fill('a long account password');
  await shot(own, 'add-account');
  await own.getByRole('button', { name: 'Create', exact: true }).click();
  await own.waitForTimeout(1500);
  await logOut(own);
  await own.getByLabel('Username').fill('alex');
  await own.getByLabel('Password', { exact: true }).fill('a long account password');
  await own.getByRole('button', { name: 'Log in' }).click();
  await own.waitForTimeout(2000);
  await shot(own, 'profile-setup');
  await own.getByLabel('Display Name').fill('Alex Example');
  await own.getByLabel('Account 1 value').fill('+14155550100');
  await own.getByRole('button', { name: 'Continue to Message Crate' }).click();
  await own.waitForTimeout(2000);
  const state = await own.context().storageState();
  await own.context().close();

  // Step 7: the Import form. The desktop app is what shows it, so the page is
  // told it is the desktop app. Nothing behind the form works in a browser.
  const context = await browser.newContext({
    viewport: { width: 1280, height: 1100 },
    deviceScaleFactor: 2,
    storageState: state,
  });
  await context.addInitScript(() => {
    window.__TAURI_INTERNALS__ = {
      invoke: () => Promise.reject(new Error('not the desktop app')),
      transformCallback: () => 0,
      metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
    };
  });
  const form = await context.newPage();
  await form.goto(`${EMPTY}/#/import`);
  await form.waitForTimeout(2500);
  await shot(form, 'import-form');
  await context.close();
  return seen;
}

(async () => {
  const browser = await chromium.launch();
  try {
    const seen = await capture(browser);
    if (process.argv.includes('--verbose')) console.log(JSON.stringify(seen, null, 2));
    console.log(`Wrote ${Object.keys(seen).length} screens to ${OUT}`);
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
